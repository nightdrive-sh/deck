//! Searches: what is searched, the one-day cache, the budget and the 429 block.

use super::*;
use crate::{
    catalog::RateLimited,
    curate::search::{CachedSearch, NOT_FOUND_MAX_AGE, SEARCH_MAX_AGE},
};

/// A search function for tests: `results` answers the searches in order, and
/// `calls` records the albums searched.
fn fake_search<'a>(
    calls: &'a mut Vec<String>,
    mut results: Vec<Result<Vec<FoundAlbum>>>,
) -> impl AsyncFnMut(&str, &str) -> Result<Vec<FoundAlbum>> + 'a {
    results.reverse();
    async move |_artist: &str, album: &str| {
        calls.push(album.to_owned());
        results.pop().expect("unexpected search")
    }
}

#[tokio::test]
async fn known_candidates_are_not_searched() {
    let candidates = [
        candidate("Gene Clark", "No Other"),
        candidate("Ultra Bra", "Kroketti"),
    ];
    let mut cache = SearchCache::default();
    let mut calls = Vec::new();
    let search = fake_search(&mut calls, vec![Ok(no_other())]);
    let (hits, searched) = lookup(
        &candidates,
        &[false, true],
        &mut cache,
        NOW,
        Duration::ZERO,
        search,
    )
    .await
    .unwrap();
    assert_eq!(calls, ["No Other"]);
    assert_eq!(searched, 1);
    assert_eq!(hits, [Some(no_other()), None]);

    let listened = [TopAlbum {
        artist: "Ultra Bra".to_owned(),
        album: "Kroketti".to_owned(),
        plays: 5,
    }];
    let (report, _) = check(&candidates, &hits, &[], &[], &listened);
    assert_eq!(report.accepted.len(), 1);
    assert_eq!(reasons(&report), [Rejection::Listened]);
}

#[tokio::test]
async fn repeated_dry_runs_search_only_new_candidates() {
    let mut cache = SearchCache::default();
    let mut calls = Vec::new();
    let first = [candidate("Gene Clark", "No Other")];
    lookup(
        &first,
        &[false],
        &mut cache,
        NOW,
        Duration::ZERO,
        fake_search(&mut calls, vec![Ok(no_other())]),
    )
    .await
    .unwrap();

    // The next round an hour later: the same candidate comes from the cache.
    let second = [
        candidate("Gene Clark", "No Other"),
        candidate("Gene Clark", "White Light"),
    ];
    let (hits, searched) = lookup(
        &second,
        &[false, false],
        &mut cache,
        NOW + 3600,
        Duration::ZERO,
        fake_search(&mut calls, vec![Ok(Vec::new())]),
    )
    .await
    .unwrap();
    assert_eq!(calls, ["No Other", "White Light"]);
    assert_eq!(searched, 1);
    assert_eq!(hits, [Some(no_other()), Some(Vec::new())]);
    assert_eq!(cache.searches_left(), SEARCH_BUDGET - 2);
}

#[tokio::test]
async fn budget_leaves_the_rest_unchecked() {
    let candidates: Vec<Candidate> = (0..SEARCH_BUDGET + 2)
        .map(|i| candidate(&format!("Artist {i}"), &format!("Album {i}")))
        .collect();
    let mut cache = SearchCache::default();
    let mut calls = Vec::new();
    let results = (0..SEARCH_BUDGET).map(|_| Ok(Vec::new())).collect();
    let (hits, searched) = lookup(
        &candidates,
        &vec![false; candidates.len()],
        &mut cache,
        NOW,
        Duration::ZERO,
        fake_search(&mut calls, results),
    )
    .await
    .unwrap();
    assert_eq!(searched, SEARCH_BUDGET);
    assert_eq!(calls.len(), SEARCH_BUDGET);
    assert_eq!(cache.searches_left(), 0);
    assert_eq!(hits[SEARCH_BUDGET..], [None, None]);

    let (report, _) = check(&candidates, &hits, &[], &[], &[]);
    assert_eq!(
        reasons(&report)[SEARCH_BUDGET..],
        [Rejection::NotChecked, Rejection::NotChecked]
    );
}

#[tokio::test]
async fn not_found_is_remembered_for_half_a_year() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("curate-searches.json");
    let mut cache = SearchCache::default();
    cache.remember_not_found("Hum", "You'd Prefer an Astronaut", NOW);
    cache.remember_not_found("Hum", "You'd Prefer An Astronaut", NOW + 60);
    assert_eq!(cache.not_found.len(), 1);
    cache.save(&path).unwrap();

    // A week later it is not searched, costs nothing and is rejected as not found.
    let mut cache = SearchCache::load(&path, NOW + 7 * 24 * 60 * 60);
    let candidates = [candidate("Hum", "You'd Prefer an Astronaut")];
    let mut calls = Vec::new();
    let (hits, searched) = lookup(
        &candidates,
        &[false],
        &mut cache,
        NOW,
        Duration::ZERO,
        fake_search(&mut calls, Vec::new()),
    )
    .await
    .unwrap();
    assert!(calls.is_empty());
    assert_eq!(searched, 0);
    assert_eq!(cache.searches_left(), SEARCH_BUDGET);
    let (report, _) = check(&candidates, &hits, &[], &[], &[]);
    assert_eq!(reasons(&report), [Rejection::NotFound]);

    let cache = SearchCache::load(&path, NOW + 60 + NOT_FOUND_MAX_AGE);
    assert_eq!(cache.get("Hum", "You'd Prefer an Astronaut"), None);
}

#[test]
fn cache_forgets_searches_after_a_day() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("curate-searches.json");
    let mut cache = SearchCache::default();
    cache.searches.push(CachedSearch {
        artist: "Gene Clark".to_owned(),
        album: "No Other".to_owned(),
        fetched: NOW,
        hits: no_other(),
    });
    cache.save(&path).unwrap();

    let cache = SearchCache::load(&path, NOW + SEARCH_MAX_AGE - 1);
    assert_eq!(cache.get("Gene Clark", "No Other"), Some(&no_other()[..]));
    assert_eq!(cache.searches_left(), SEARCH_BUDGET - 1);
    let cache = SearchCache::load(&path, NOW + SEARCH_MAX_AGE);
    assert_eq!(cache.get("Gene Clark", "No Other"), None);
    assert_eq!(cache.searches_left(), SEARCH_BUDGET);
}

#[tokio::test]
async fn rate_limit_stops_the_run_and_blocks_until_retry_after() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("curate-searches.json");
    let candidates = [
        candidate("Gene Clark", "No Other"),
        candidate("Gene Clark", "White Light"),
        candidate("Gene Clark", "Roadmaster"),
    ];
    let skip = [false; 3];
    let mut cache = SearchCache::load(&path, NOW);
    let mut calls = Vec::new();
    let limited = RateLimited {
        retry_after: Some(34560),
    };
    let error = lookup(
        &candidates,
        &skip,
        &mut cache,
        NOW,
        Duration::ZERO,
        fake_search(&mut calls, vec![Ok(no_other()), Err(limited.into())]),
    )
    .await
    .unwrap_err();
    cache.save(&path).unwrap();

    // The first 429 stops the run: the third is not searched, the second not retried.
    assert_eq!(calls, ["No Other", "White Light"]);
    let message = error.to_string();
    assert!(
        message.starts_with("Spotify is rate limiting Deck (429, retry-after 34560 s, until "),
        "{message}"
    );
    assert!(message.ends_with("Stop this run: Deck makes no Spotify searches before that."));

    // The next run searches nothing until the block is over, not even cache misses.
    let mut cache = SearchCache::load(&path, NOW + 60);
    assert_eq!(cache.rate_limit.blocked_until(NOW + 60), Some(NOW + 34560));
    assert!(
        cache
            .ensure_open(NOW + 60)
            .unwrap_err()
            .to_string()
            .contains("retry-after 34500 s")
    );
    let mut calls = Vec::new();
    let error = lookup(
        &candidates,
        &skip,
        &mut cache,
        NOW + 60,
        Duration::ZERO,
        fake_search(&mut calls, Vec::new()),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("429"));
    assert!(calls.is_empty());

    // After the block searching resumes, and what was searched before it is cached.
    let mut cache = SearchCache::load(&path, NOW + 34560);
    assert!(cache.ensure_open(NOW + 34560).is_ok());
    let mut calls = Vec::new();
    let results = vec![Ok(Vec::new()), Ok(Vec::new())];
    let (_, searched) = lookup(
        &candidates,
        &skip,
        &mut cache,
        NOW + 34560,
        Duration::ZERO,
        fake_search(&mut calls, results),
    )
    .await
    .unwrap();
    assert_eq!(calls, ["White Light", "Roadmaster"]);
    assert_eq!(searched, 2);
}

#[tokio::test]
async fn block_does_not_stop_a_run_that_needs_no_search() {
    let mut cache = SearchCache::default();
    cache.searches.push(CachedSearch {
        artist: "Gene Clark".to_owned(),
        album: "No Other".to_owned(),
        fetched: NOW,
        hits: no_other(),
    });
    cache.rate_limit.block(NOW + 600);
    let candidates = [
        candidate("Gene Clark", "No Other"),
        candidate("Ultra Bra", "Kroketti"),
    ];
    let mut calls = Vec::new();
    let (hits, searched) = lookup(
        &candidates,
        &[false, true],
        &mut cache,
        NOW,
        Duration::ZERO,
        fake_search(&mut calls, Vec::new()),
    )
    .await
    .unwrap();
    assert_eq!(hits, [Some(no_other()), None]);
    assert_eq!(searched, 0);
    assert!(calls.is_empty());
}

#[test]
fn cache_matches_normalized_names() {
    let mut cache = SearchCache::default();
    cache.searches.push(CachedSearch {
        artist: "Gene Clark".to_owned(),
        album: "No Other".to_owned(),
        fetched: NOW,
        hits: no_other(),
    });
    assert_eq!(
        cache.get("gene clark", "No Other (Remastered)"),
        Some(&no_other()[..])
    );
    assert_eq!(cache.get("Gene Clark", "White Light"), None);

    cache.add_seed("The Troggs", Some("Wild Thing"), NOW, Vec::new());
    cache.add_seed("Love", None, NOW, Vec::new());
    assert!(
        cache
            .get_seed("the troggs", Some("Wild Thing - Mono"))
            .is_some()
    );
    assert!(cache.get_seed("Love", None).is_some());
    assert!(cache.get_seed("Love", Some("Alone Again Or")).is_none());
    assert!(cache.get_seed("The Troggs", None).is_none());
}

#[test]
fn broken_cache_file_is_an_empty_cache() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("curate-searches.json");
    std::fs::write(&path, "{\"searches\": ").unwrap();
    let cache = SearchCache::load(&path, NOW);
    assert_eq!(cache.searches_left(), SEARCH_BUDGET);
}

#[tokio::test]
async fn rate_limit_without_retry_after_blocks_an_hour() {
    let mut cache = SearchCache::default();
    let mut calls = Vec::new();
    let limited = RateLimited { retry_after: None };
    lookup(
        &[candidate("Gene Clark", "No Other")],
        &[false],
        &mut cache,
        NOW,
        Duration::ZERO,
        fake_search(&mut calls, vec![Err(limited.into())]),
    )
    .await
    .unwrap_err();
    assert_eq!(
        cache.rate_limit.blocked_until(NOW),
        Some(NOW + DEFAULT_BLOCK)
    );
}

#[tokio::test]
async fn other_errors_name_the_candidate() {
    let mut cache = SearchCache::default();
    let mut calls = Vec::new();
    let error = lookup(
        &[candidate("Gene Clark", "No Other")],
        &[false],
        &mut cache,
        NOW,
        Duration::ZERO,
        fake_search(
            &mut calls,
            vec![Err(anyhow::anyhow!("Spotify is not responding"))],
        ),
    )
    .await
    .unwrap_err();
    assert_eq!(
        format!("{error:#}"),
        "Spotify search failed for Gene Clark – No Other: Spotify is not responding"
    );
    assert!(cache.ensure_open(NOW).is_ok());
    assert_eq!(cache.searches_left(), SEARCH_BUDGET);
}
