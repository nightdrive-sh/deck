//! The check of `deck genre add`: each seed is found on Spotify (its URI's names, or a
//! search by name) and its station must give enough tracks.

use std::{collections::HashSet, time::Duration};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{
    catalog::{FoundSeed, RateLimited},
    curate::SearchCache,
    genres::{Seed, is_seed_uri},
    lists::{normalize, track_key},
    radio,
};

/// A station must give at least this many tracks.
const STATION_MIN: usize = 20;
/// Sample tracks from a station in the report.
pub(super) const STATION_SAMPLE: usize = 6;

/// Input of `deck genre add` from stdin.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Input {
    pub name: String,
    pub seeds: Vec<SeedInput>,
}

/// A suggested seed: an artist or (with `track`) a track, with or without a URI.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SeedInput {
    pub artist: String,
    #[serde(default)]
    pub track: Option<String>,
    #[serde(default)]
    pub uri: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Rejection {
    /// Not an artist or track URI, or the type does not match `track`.
    InvalidUri,
    /// The same seed is already earlier in the list.
    Duplicate,
    /// Not searched, because the daily search cap has been reached.
    NotChecked,
    /// The search found no match, or the URI's metadata could not be fetched.
    NotFound,
    /// The name of the URI's artist or track does not match the given one.
    NameMismatch,
    /// The station gave fewer than [`STATION_MIN`] tracks, or fetching it failed.
    NoStation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Accepted {
    pub artist: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track: Option<String>,
    pub uri: String,
    /// Sample tracks from the station: `Artist · Track (year)`.
    pub station: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rejected {
    pub artist: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    pub reason: Rejection,
    /// Spotify's name when it did not match (`name_mismatch`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spotify_name: Option<String>,
}

/// Result of the check: the parts of the report and the seeds to save.
#[derive(Debug, Default)]
pub struct Checked {
    pub accepted: Vec<Accepted>,
    pub rejected: Vec<Rejected>,
    pub seeds: Vec<Seed>,
    pub searched: usize,
}

/// Spotify for the check.
pub trait Spotify {
    /// Web API search: an artist (`track` is `None`) or a track.
    async fn search(&mut self, artist: &str, track: Option<&str>) -> Result<Vec<FoundSeed>>;
    /// A seed's names via librespot: the track name and artists, or the artist name.
    async fn names(&mut self, uri: &str) -> Result<FoundSeed>;
    /// The first page of the seed's apollo station.
    async fn station(&mut self, uri: &str) -> Result<Vec<radio::Track>>;
    /// The album's release year for the sample.
    async fn year(&mut self, album_uri: &str) -> Option<u16>;
}

/// Checks the seeds in order; the first matching rule rejects (see [`Rejection`]).
/// Searching uses up `cache`'s daily cap. A 429 records the block in `cache` and
/// returns an error; the cache must be saved after an error too.
pub async fn check(
    seeds: &[SeedInput],
    cache: &mut SearchCache,
    now: u64,
    pause: Duration,
    spotify: &mut impl Spotify,
) -> Result<Checked> {
    let mut out = Checked::default();
    let mut seen_uris: HashSet<String> = HashSet::new();
    let mut seen_names: HashSet<(String, Option<String>)> = HashSet::new();

    for seed in seeds {
        let reject = |reason, uri: Option<&str>, spotify_name: Option<String>| Rejected {
            artist: seed.artist.clone(),
            track: seed.track.clone(),
            uri: uri.map(str::to_owned),
            reason,
            spotify_name,
        };
        if let Some(uri) = &seed.uri
            && !uri_fits(uri, seed.track.is_some())
        {
            out.rejected
                .push(reject(Rejection::InvalidUri, Some(uri), None));
            continue;
        }
        let name_key = (
            normalize(&seed.artist),
            seed.track.as_deref().map(track_key),
        );
        let duplicate = match &seed.uri {
            Some(uri) => seen_uris.contains(uri),
            None => seen_names.contains(&name_key),
        };
        if duplicate {
            out.rejected
                .push(reject(Rejection::Duplicate, seed.uri.as_deref(), None));
            continue;
        }
        seen_names.insert(name_key);

        let found = match &seed.uri {
            Some(uri) => match spotify.names(uri).await {
                Ok(found) if matches(seed, &found) => found,
                Ok(found) => {
                    let name = display_name(seed, &found);
                    out.rejected
                        .push(reject(Rejection::NameMismatch, Some(uri), Some(name)));
                    continue;
                }
                Err(e) => {
                    log::warn!("{uri}: {e:#}");
                    out.rejected
                        .push(reject(Rejection::NotFound, Some(uri), None));
                    continue;
                }
            },
            None => {
                let track = seed.track.as_deref();
                let hits = match cache.get_seed(&seed.artist, track) {
                    Some(hits) => hits.to_vec(),
                    None if cache.searches_left() == 0 => {
                        out.rejected.push(reject(Rejection::NotChecked, None, None));
                        continue;
                    }
                    None => {
                        cache.ensure_open(now)?;
                        if out.searched > 0 {
                            tokio::time::sleep(pause).await;
                        }
                        out.searched += 1;
                        let hits = match spotify.search(&seed.artist, track).await {
                            Ok(hits) => hits,
                            Err(e) => match e.downcast_ref::<RateLimited>() {
                                Some(limited) => return Err(cache.block(now, limited)),
                                None => {
                                    return Err(e.context(format!(
                                        "Spotify search failed for {}",
                                        seed_label(seed)
                                    )));
                                }
                            },
                        };
                        cache.add_seed(&seed.artist, track, now, hits.clone());
                        hits
                    }
                };
                match hits.into_iter().find(|hit| matches(seed, hit)) {
                    Some(hit) => hit,
                    None => {
                        out.rejected.push(reject(Rejection::NotFound, None, None));
                        continue;
                    }
                }
            }
        };
        if !seen_uris.insert(found.uri.clone()) {
            out.rejected
                .push(reject(Rejection::Duplicate, Some(&found.uri), None));
            continue;
        }

        let tracks = match spotify.station(&found.uri).await {
            Ok(tracks) if tracks.len() >= STATION_MIN => tracks,
            Ok(_) => {
                out.rejected
                    .push(reject(Rejection::NoStation, Some(&found.uri), None));
                continue;
            }
            Err(e) => {
                log::warn!("{}: {e:#}", found.uri);
                out.rejected
                    .push(reject(Rejection::NoStation, Some(&found.uri), None));
                continue;
            }
        };
        let mut station = Vec::new();
        for track in sample(&tracks) {
            let year = spotify.year(&track.album.uri).await;
            station.push(match year {
                Some(year) => format!("{} · {} ({year})", track.artist, track.name),
                None => format!("{} · {}", track.artist, track.name),
            });
        }
        out.seeds.push(Seed {
            uri: found.uri.clone(),
            name: display_name(seed, &found),
        });
        out.accepted.push(Accepted {
            artist: seed.artist.clone(),
            track: seed.track.clone(),
            uri: found.uri,
            station,
        });
    }
    Ok(out)
}

/// The URI is an artist's or a track's, and a track URI is given only with `track`.
fn uri_fits(uri: &str, is_track: bool) -> bool {
    is_seed_uri(uri) && uri.starts_with("spotify:track:") == is_track
}

/// Whether Spotify's artist or track matches the suggested one: for an artist the
/// name, for a track the name and one of the artists.
fn matches(seed: &SeedInput, found: &FoundSeed) -> bool {
    let artist = normalize(&seed.artist);
    let artist_matches = found.artists.iter().any(|a| normalize(a) == artist);
    match &seed.track {
        Some(track) => artist_matches && track_key(track) == track_key(&found.name),
        None => normalize(&found.name) == artist,
    }
}

/// Spotify's name in the catalog's style: `Artist` or `Artist · Track`. For a track
/// the artist is the one matching the suggestion, otherwise the first one.
fn display_name(seed: &SeedInput, found: &FoundSeed) -> String {
    if seed.track.is_none() && !found.uri.starts_with("spotify:track:") {
        return found.name.clone();
    }
    let artist = normalize(&seed.artist);
    let shown = found
        .artists
        .iter()
        .find(|a| normalize(a) == artist)
        .or(found.artists.first())
        .map_or("", String::as_str);
    format!("{shown} · {}", found.name)
}

fn seed_label(seed: &SeedInput) -> String {
    match &seed.track {
        Some(track) => format!("{} – {track}", seed.artist),
        None => seed.artist.clone(),
    }
}

/// [`STATION_SAMPLE`] tracks at even intervals. The first is skipped, because it is
/// often the seed itself.
fn sample(tracks: &[radio::Track]) -> Vec<&radio::Track> {
    let rest = tracks.get(1..).unwrap_or_default();
    let step = (rest.len() / STATION_SAMPLE).max(1);
    rest.iter().step_by(step).take(STATION_SAMPLE).collect()
}
