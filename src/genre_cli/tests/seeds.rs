//! The seed check: URIs, searches by name, the search cap, the 429 block and stations.

use std::time::Duration;

use anyhow::{Context, Result};

use super::*;
use crate::{
    catalog::{FoundSeed, RateLimited},
    genre_cli::check::{Checked, Rejection, STATION_SAMPLE, Spotify},
    lists::track_key,
    radio,
};

/// A fake Spotify: known URIs, searches and stations, and a record of the calls.
#[derive(Default)]
struct Fake {
    names: Vec<FoundSeed>,
    searchable: Vec<FoundSeed>,
    /// Stations with fewer than 20 tracks.
    short_stations: Vec<String>,
    rate_limited: bool,
    searches: Vec<String>,
    stations: Vec<String>,
}

fn found(uri: &str, name: &str, artists: &[&str]) -> FoundSeed {
    FoundSeed {
        uri: uri.into(),
        name: name.into(),
        artists: artists.iter().map(|a| a.to_string()).collect(),
    }
}

fn artist(uri: &str, name: &str) -> FoundSeed {
    found(uri, name, &[name])
}

fn fake() -> Fake {
    Fake {
        names: vec![
            artist(TROGGS, "The Troggs"),
            artist(TROGSS, "The Trogss"),
            artist(QUIET, "Small Faces"),
            found(
                ZOMBIES,
                "She's Not There - Mono",
                &["The Zombies", "Rod Argent"],
            ),
        ],
        searchable: vec![artist(TROGSS, "The Trogss"), artist(LOVE, "Love")],
        short_stations: vec![QUIET.into()],
        ..Fake::default()
    }
}

impl Spotify for Fake {
    async fn search(&mut self, artist: &str, track: Option<&str>) -> Result<Vec<FoundSeed>> {
        self.searches.push(artist.to_owned());
        if self.rate_limited {
            return Err(RateLimited {
                retry_after: Some(600),
            }
            .into());
        }
        Ok(match track {
            Some(_) => Vec::new(),
            None => self.searchable.clone(),
        })
    }

    async fn names(&mut self, uri: &str) -> Result<FoundSeed> {
        self.names
            .iter()
            .find(|f| f.uri == uri)
            .cloned()
            .context("404")
    }

    async fn station(&mut self, uri: &str) -> Result<Vec<radio::Track>> {
        self.stations.push(uri.to_owned());
        let count = if self.short_stations.iter().any(|s| s == uri) {
            5
        } else {
            50
        };
        Ok((0..count)
            .map(|n| radio::Track {
                uri: format!("spotify:track:{n:022}"),
                name: format!("Song {n}"),
                artist: format!("Band {n}"),
                artist_uri: format!("spotify:artist:{n:022}"),
                album: radio::Album {
                    uri: format!("spotify:album:{n:022}"),
                    name: format!("Album {n}"),
                },
            })
            .collect())
    }

    async fn year(&mut self, album_uri: &str) -> Option<u16> {
        (!album_uri.ends_with('9')).then_some(1966)
    }
}

async fn run(seeds: &[SeedInput], cache: &mut SearchCache, spotify: &mut Fake) -> Checked {
    check(seeds, cache, NOW, Duration::ZERO, spotify)
        .await
        .unwrap()
}

fn reasons(checked: &Checked) -> Vec<(String, Rejection)> {
    checked
        .rejected
        .iter()
        .map(|r| (r.artist.clone(), r.reason))
        .collect()
}

#[tokio::test]
async fn uris_are_checked_without_searching() {
    let mut spotify = fake();
    let mut cache = SearchCache::default();
    let checked = run(
        &[
            seed("The Troggs", None, Some(TROGGS)),
            seed("the zombies", Some("She's Not There"), Some(ZOMBIES)),
        ],
        &mut cache,
        &mut spotify,
    )
    .await;
    assert!(checked.rejected.is_empty(), "{:?}", checked.rejected);
    assert_eq!(checked.searched, 0);
    assert!(spotify.searches.is_empty());
    assert_eq!(
        checked.seeds,
        [
            Seed {
                uri: TROGGS.into(),
                name: "The Troggs".into()
            },
            Seed {
                uri: ZOMBIES.into(),
                name: "The Zombies · She's Not There - Mono".into()
            },
        ]
    );
    let station = &checked.accepted[0].station;
    assert_eq!(station.len(), STATION_SAMPLE);
    assert_eq!(station[0], "Band 1 · Song 1 (1966)");
    // An album whose year could not be fetched is shown without a year.
    assert!(
        station.iter().any(|s| s == "Band 9 · Song 9"),
        "{station:?}"
    );
}

#[tokio::test]
async fn rejections_have_reasons() {
    let mut spotify = fake();
    let mut cache = SearchCache::default();
    let checked = run(
        &[
            seed(
                "The Troggs",
                None,
                Some("spotify:album:57xdnSVt4ahJCIXYLieQ25"),
            ),
            seed("The Troggs", Some("Wild Thing"), Some(TROGGS)),
            seed("The Troggs", None, Some(TROGSS)),
            seed("The Troggs", None, Some(TROGGS)),
            seed("Troggs again", None, Some(TROGGS)),
            seed("Ghost", None, Some("spotify:artist:0000000000000000000000")),
            seed("Small Faces", None, Some(QUIET)),
        ],
        &mut cache,
        &mut spotify,
    )
    .await;
    assert_eq!(
        reasons(&checked),
        [
            ("The Troggs".into(), Rejection::InvalidUri),
            ("The Troggs".into(), Rejection::InvalidUri),
            ("The Troggs".into(), Rejection::NameMismatch),
            ("Troggs again".into(), Rejection::Duplicate),
            ("Ghost".into(), Rejection::NotFound),
            ("Small Faces".into(), Rejection::NoStation),
        ]
    );
    assert_eq!(
        checked.rejected[2].spotify_name.as_deref(),
        Some("The Trogss")
    );
    assert_eq!(checked.seeds.len(), 1);
}

#[tokio::test]
async fn names_are_searched_and_must_match() {
    let mut spotify = fake();
    let mut cache = SearchCache::default();
    let checked = run(
        &[
            seed("Love", None, None),
            // The search only returns an artist with an almost identical name.
            seed("The Troggs", None, None),
            seed("The Zombies", Some("Time of the Season"), None),
            seed("love", None, None),
        ],
        &mut cache,
        &mut spotify,
    )
    .await;
    assert_eq!(
        reasons(&checked),
        [
            ("The Troggs".into(), Rejection::NotFound),
            ("The Zombies".into(), Rejection::NotFound),
            ("love".into(), Rejection::Duplicate),
        ]
    );
    assert_eq!(checked.seeds[0].uri, LOVE);
    assert_eq!(checked.searched, 3);
    assert_eq!(cache.searches_left(), crate::curate::SEARCH_BUDGET - 3);

    // A new round does not search the same names again.
    let again = run(&[seed("Love", None, None)], &mut cache, &mut spotify).await;
    assert_eq!(again.searched, 0);
    assert_eq!(spotify.searches.len(), 3);
}

#[tokio::test]
async fn budget_leaves_names_unchecked_but_uris_work() {
    let mut spotify = fake();
    let mut cache = SearchCache::default();
    for n in 0..crate::curate::SEARCH_BUDGET {
        cache.add_seed(&format!("artist {n}"), None, NOW, Vec::new());
    }
    let checked = run(
        &[
            seed("Love", None, None),
            seed("The Troggs", None, Some(TROGGS)),
        ],
        &mut cache,
        &mut spotify,
    )
    .await;
    assert_eq!(reasons(&checked), [("Love".into(), Rejection::NotChecked)]);
    assert_eq!(checked.seeds.len(), 1);
    assert!(spotify.searches.is_empty());
}

#[tokio::test]
async fn rate_limit_stops_the_run_and_blocks_searches() {
    let mut spotify = Fake {
        rate_limited: true,
        ..fake()
    };
    let mut cache = SearchCache::default();
    let error = check(
        &[
            seed("Love", None, None),
            seed("The Troggs", None, Some(TROGGS)),
        ],
        &mut cache,
        NOW,
        Duration::ZERO,
        &mut spotify,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        error.starts_with("Spotify is rate limiting Deck"),
        "{error}"
    );
    assert!(spotify.stations.is_empty());
    assert!(cache.ensure_open(NOW + 599).is_err());
    assert!(cache.ensure_open(NOW + 600).is_ok());
}

#[test]
fn track_names_ignore_versions() {
    assert_eq!(
        track_key("Gimme Some Lovin' - 2015 Remaster"),
        track_key("Gimme Some Lovin'")
    );
    assert_ne!(track_key("Gimme Some Lovin'"), track_key("Gimme Gimme"));
}
