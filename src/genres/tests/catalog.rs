//! The catalog: ids, merging own genres, the checks, the seed draw and (by hand) every
//! station.

use std::collections::HashSet;

use super::*;

fn genre(seeds: u32) -> Genre {
    Genre {
        id: "test".into(),
        name: "test".into(),
        seeds: (0..seeds).map(seed).collect(),
    }
}

fn uris(seeds: &[&Seed]) -> Vec<String> {
    seeds.iter().map(|s| s.uri.clone()).collect()
}

#[test]
fn slug_from_name() {
    assert_eq!(slug("60s rock"), "60s-rock");
    assert_eq!(slug("R&B / Soul"), "r-b-soul");
    assert_eq!(slug("  Hip Hop! "), "hip-hop");
    assert_eq!(slug("iskelmä"), "iskelmä");
}

#[test]
fn catalog_ids_are_slugs_of_names() {
    for genre in all() {
        assert_eq!(genre.id, slug(&genre.name));
    }
}

#[test]
fn own_genre_replaces_catalog_genre_and_others_are_added() {
    let catalog = vec![own("lofi", 2), own("synthwave", 2)];
    let merged = merge(catalog, &[own("lofi", 5), own("60s rock", 3)]);
    let names: Vec<&str> = merged.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["60s rock", "lofi", "synthwave"]);
    assert_eq!(merged[1].seeds.len(), 5);
}

#[test]
fn catalog_meets_the_minimum() {
    let genres = all();
    assert!(genres.len() >= 15, "only {} genres", genres.len());
    for genre in genres {
        assert!(
            genre.seeds.len() >= 6,
            "{} has only {} seeds",
            genre.id,
            genre.seeds.len()
        );
        assert_eq!(genre.name, genre.name.to_lowercase(), "{}", genre.id);
    }
    assert!(find("lofi").is_some());
    assert!(find("synthwave").is_some());
}

#[test]
fn catalog_is_sorted_by_name() {
    let names: Vec<&str> = all().iter().map(|g| g.name.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
}

#[test]
fn unknown_id_is_not_found() {
    assert!(find("polka-noir").is_none());
}

#[test]
fn parse_rejects_invalid_catalogs() {
    let ok = r#"[{"id":"a","name":"a","seeds":[{"uri":"spotify:track:0HSwIZSDeOQX8Cu9pCGkjS","name":"x"}]}]"#;
    assert_eq!(parse(ok).unwrap().len(), 1);

    let cases = [
        (r#"[{"id":"a","name":"a","seeds":[]}]"#, "no seeds"),
        (
            r#"[{"id":"a","name":"a","seeds":[{"uri":"spotify:playlist:37i9dQZF1DWWQRwui0ExPn","name":"x"}]}]"#,
            "not an artist or track",
        ),
        (
            r#"[{"id":"a","name":"a","seeds":[{"uri":"spotify:artist:5FsfZj0Mp6YwEWytuJUcWt","name":"x"},{"uri":"spotify:artist:5FsfZj0Mp6YwEWytuJUcWt","name":"x"}]}]"#,
            "duplicate seed",
        ),
        (
            r#"[{"id":"a","name":"a","seeds":[{"uri":"spotify:artist:5FsfZj0Mp6YwEWytuJUcWt","name":"x"}]},
                {"id":"a","name":"b","seeds":[{"uri":"spotify:artist:5FsfZj0Mp6YwEWytuJUcWt","name":"x"}]}]"#,
            "duplicate genre id",
        ),
        (
            r#"[{"id":"a","name":"a","seeds":[{"uri":"spotify:artist:5FsfZj0Mp6YwEWytuJUcWt","name":"x"}]},
                {"id":"b","name":"a","seeds":[{"uri":"spotify:artist:5FsfZj0Mp6YwEWytuJUcWt","name":"x"}]}]"#,
            "duplicate genre name",
        ),
        ("{", "invalid genre catalog"),
    ];
    for (text, error) in cases {
        let e = format!("{:#}", parse(text).unwrap_err());
        assert!(e.contains(error), "{e}");
    }
}

#[test]
fn pick_seed_differs_from_current() {
    let genre = genre(6);
    let current = genre.seeds[2].uri.clone();
    for random in 0..60 {
        assert_ne!(genre.pick_seed(Some(&current), &[], random).uri, current);
    }
}

#[test]
fn pick_seed_prefers_unplayed_seeds() {
    let genre = genre(6);
    let played: Vec<String> = genre.seeds[..4].iter().map(|s| s.uri.clone()).collect();
    let picked: HashSet<String> = (0..60)
        .map(|random| {
            genre
                .pick_seed(Some(&played[3]), &played, random)
                .uri
                .clone()
        })
        .collect();
    let expected: HashSet<String> = uris(&genre.seeds[4..].iter().collect::<Vec<_>>())
        .into_iter()
        .collect();
    assert_eq!(picked, expected);
}

#[test]
fn pick_seed_starts_over_when_all_are_played() {
    let genre = genre(6);
    let played: Vec<String> = genre.seeds.iter().map(|s| s.uri.clone()).collect();
    let current = &played[0];
    let picked: HashSet<String> = (0..60)
        .map(|random| genre.pick_seed(Some(current), &played, random).uri.clone())
        .collect();
    assert_eq!(picked.len(), 5);
    assert!(!picked.contains(current));
}

#[test]
fn pick_seed_without_current_uses_every_seed() {
    let genre = genre(6);
    let picked: HashSet<String> = (0..60)
        .map(|random| genre.pick_seed(None, &[], random).uri.clone())
        .collect();
    assert_eq!(picked.len(), 6);
}

#[test]
fn pick_seed_with_a_single_seed_repeats_it() {
    let genre = genre(1);
    let only = genre.seeds[0].uri.clone();
    assert_eq!(
        genre
            .pick_seed(Some(&only), std::slice::from_ref(&only), 7)
            .uri,
        only
    );
}

#[test]
fn random_varies() {
    let values: HashSet<u64> = (0..10).map(|_| random()).collect();
    assert!(values.len() > 1);
}

/// Manual run:
/// `cargo test genres::tests::catalog::live_catalog_stations -- --ignored --nocapture`.
/// Fetches the station of every seed and prints a sample of it.
#[tokio::test]
#[ignore = "requires Spotify credentials and network"]
async fn live_catalog_stations() -> Result<()> {
    let client = crate::radio::Client::new(crate::player::SharedSession::new(
        crate::player::connect().await?,
    ));
    let mut failed = Vec::new();
    for genre in all() {
        for seed in &genre.seeds {
            match client.page(&seed.uri, &[]).await {
                Ok(tracks) if tracks.len() >= 20 => {
                    let sample: Vec<String> = tracks
                        .iter()
                        .skip(1)
                        .step_by(12)
                        .map(|t| format!("{} · {}", t.artist, t.name))
                        .collect();
                    println!(
                        "{} | {} | {} | {}",
                        genre.name,
                        seed.name,
                        tracks.len(),
                        sample.join(" / ")
                    );
                }
                Ok(tracks) => failed.push(format!(
                    "{} / {}: only {} tracks",
                    genre.id,
                    seed.name,
                    tracks.len()
                )),
                Err(e) => failed.push(format!("{} / {}: {e:#}", genre.id, seed.name)),
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
    Ok(())
}
