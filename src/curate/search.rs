//! The Spotify searches of `deck curate submit` and `deck genre add`: [`lookup`] and the
//! one-day [`SearchCache`] with the day's [`SEARCH_BUDGET`] and the shared 429 block.

use std::{path::Path, time::Duration};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use super::{Candidate, DEFAULT_BLOCK, SEARCH_BUDGET};
use crate::{
    catalog::{FoundAlbum, FoundSeed, RateLimited},
    config,
    lists::{normalize, track_key},
    rate_limit::RateLimit,
    store::{read_json, save_json},
};

/// A search stays in the cache for a day, and [`SEARCH_BUDGET`] counts the same period.
pub(super) const SEARCH_MAX_AGE: u64 = 24 * 60 * 60;
/// An album not found on Spotify is not searched again for half a year: Spotify's
/// catalog changes, but seldom.
pub(super) const NOT_FOUND_MAX_AGE: u64 = 182 * 24 * 60 * 60;

/// Fetches the candidates' hits with `search`. `skip`ped (already known) candidates
/// are not searched, a repeated search comes from the cache, and once
/// [`SEARCH_BUDGET`] is used up the rest are left unsearched (`None`). Returns the
/// hits and the number of searches made.
///
/// An active block is an error only when some candidate actually needs a search.
/// A 429 records a block in the cache for the `retry-after` period and stops at
/// once without retrying. The cache must be saved after an error too.
pub(super) async fn lookup(
    candidates: &[Candidate],
    skip: &[bool],
    cache: &mut SearchCache,
    now: u64,
    pause: Duration,
    mut search: impl AsyncFnMut(&str, &str) -> Result<Vec<FoundAlbum>>,
) -> Result<(Vec<Option<Vec<FoundAlbum>>>, usize)> {
    let mut hits = Vec::with_capacity(candidates.len());
    let mut searched = 0;
    for (c, &skip) in candidates.iter().zip(skip) {
        if skip {
            hits.push(None);
            continue;
        }
        if let Some(found) = cache.get(&c.artist, &c.album) {
            hits.push(Some(found.to_vec()));
            continue;
        }
        if cache.searches_left() == 0 {
            hits.push(None);
            continue;
        }
        cache.ensure_open(now)?;
        if searched > 0 {
            tokio::time::sleep(pause).await;
        }
        searched += 1;
        match search(&c.artist, &c.album).await {
            Ok(found) => {
                cache.searches.push(CachedSearch {
                    artist: c.artist.clone(),
                    album: c.album.clone(),
                    fetched: now,
                    hits: found.clone(),
                });
                hits.push(Some(found));
            }
            Err(e) => {
                if let Some(limited) = e.downcast_ref::<RateLimited>() {
                    return Err(cache.block(now, limited));
                }
                return Err(e.context(format!(
                    "Spotify search failed for {} – {}",
                    c.artist, c.album
                )));
            }
        }
    }
    Ok((hits, searched))
}

/// Spotify searches for curation and own genres (`~/.cache/deck/curate-searches.json`):
/// the last day's searches with their hits. Dry-run rounds of the same run do not
/// search the same thing again, and both count toward [`SEARCH_BUDGET`]. The 429 block
/// is in `rate-limit.json` next to the cache, shared with Deck ([`RateLimit`]).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SearchCache {
    /// The 429 block; a cache made with `default` keeps it in memory only.
    #[serde(skip)]
    pub(super) rate_limit: RateLimit,
    #[serde(default)]
    pub(super) searches: Vec<CachedSearch>,
    /// Seed searches of `deck genre add`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    seed_searches: Vec<CachedSeedSearch>,
    /// Candidates that were not found on Spotify, kept for [`NOT_FOUND_MAX_AGE`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) not_found: Vec<NotFound>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct NotFound {
    pub(super) artist: String,
    pub(super) album: String,
    /// Search time in Unix seconds.
    checked: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedSeedSearch {
    artist: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    track: Option<String>,
    fetched: u64,
    hits: Vec<FoundSeed>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct CachedSearch {
    pub(super) artist: String,
    pub(super) album: String,
    /// Search time in Unix seconds.
    pub(super) fetched: u64,
    pub(super) hits: Vec<FoundAlbum>,
}

impl SearchCache {
    pub fn default_path() -> Result<std::path::PathBuf> {
        Ok(config::cache_dir()?.join("curate-searches.json"))
    }

    /// Reads the cache and drops expired entries. A missing file is an empty cache.
    /// A broken file (saving is atomic, so it only happens through hand editing) is
    /// empty too, but logs a warning, because the day's search count is lost with it.
    /// The 429 block is read from `rate-limit.json` in the same directory.
    pub fn load(path: &Path, now: u64) -> Self {
        let mut cache: Self = read_json(path, "search cache")
            .unwrap_or_else(|e| {
                log::warn!(
                    "{e:#}; starting with an empty cache, so today's search count is forgotten"
                );
                None
            })
            .unwrap_or_default();
        cache.rate_limit = RateLimit::at(&path.with_file_name("rate-limit.json"));
        cache.expire(now);
        cache
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        save_json(path, self)
    }

    fn expire(&mut self, now: u64) {
        self.searches
            .retain(|s| now.saturating_sub(s.fetched) < SEARCH_MAX_AGE);
        self.seed_searches
            .retain(|s| now.saturating_sub(s.fetched) < SEARCH_MAX_AGE);
        self.not_found
            .retain(|n| now.saturating_sub(n.checked) < NOT_FOUND_MAX_AGE);
    }

    /// An error if Spotify has blocked searches, in this or another Deck process.
    pub fn ensure_open(&self, now: u64) -> Result<()> {
        match self.rate_limit.blocked_until(now) {
            Some(until) => bail!(blocked_message(until, until - now)),
            None => Ok(()),
        }
    }

    /// An earlier search for the same artist and album. Names are compared normalised
    /// ([`normalize`]), because [`pick`](super::pick) compares the hits the same way:
    /// "No Other (Remastered)" uses the search for "No Other". An album remembered as
    /// not found has no hits.
    pub(super) fn get(&self, artist: &str, album: &str) -> Option<&[FoundAlbum]> {
        let (artist, album) = (normalize(artist), normalize(album));
        let same = |a: &str, b: &str| normalize(a) == artist && normalize(b) == album;
        self.searches
            .iter()
            .find(|s| same(&s.artist, &s.album))
            .map(|s| s.hits.as_slice())
            .or_else(|| {
                self.not_found
                    .iter()
                    .any(|n| same(&n.artist, &n.album))
                    .then_some(&[][..])
            })
    }

    /// Remembers that the album was not found, so that it is not searched again.
    pub(super) fn remember_not_found(&mut self, artist: &str, album: &str, now: u64) {
        let (key_artist, key_album) = (normalize(artist), normalize(album));
        self.not_found
            .retain(|n| normalize(&n.artist) != key_artist || normalize(&n.album) != key_album);
        self.not_found.push(NotFound {
            artist: artist.to_owned(),
            album: album.to_owned(),
            checked: now,
        });
    }

    pub fn searches_left(&self) -> usize {
        SEARCH_BUDGET.saturating_sub(self.searches.len() + self.seed_searches.len())
    }

    /// An earlier search for a seed (`track` `None` = artist search). Names are
    /// compared normalised, the same way `deck genre add` compares hits ([`normalize`],
    /// [`track_key`]).
    pub fn get_seed(&self, artist: &str, track: Option<&str>) -> Option<&[FoundSeed]> {
        let artist = normalize(artist);
        let track = track.map(track_key);
        self.seed_searches
            .iter()
            .find(|s| normalize(&s.artist) == artist && s.track.as_deref().map(track_key) == track)
            .map(|s| s.hits.as_slice())
    }

    pub fn add_seed(&mut self, artist: &str, track: Option<&str>, now: u64, hits: Vec<FoundSeed>) {
        self.seed_searches.push(CachedSeedSearch {
            artist: artist.to_owned(),
            track: track.map(str::to_owned),
            fetched: now,
            hits,
        });
    }

    /// Records a 429 block for the `retry-after` period and returns an error that
    /// ends the run.
    pub fn block(&self, now: u64, limited: &RateLimited) -> anyhow::Error {
        let wait = limited.retry_after.unwrap_or(DEFAULT_BLOCK);
        self.rate_limit.block(now + wait);
        anyhow::anyhow!(blocked_message(now + wait, wait))
    }
}

/// Error message for a block. The curation run (`deck curate`) logs it to `curate.log`.
fn blocked_message(until: u64, retry_after: u64) -> String {
    let until = i64::try_from(until)
        .ok()
        .and_then(|s| jiff::Timestamp::from_second(s).ok())
        .map(|t| {
            t.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "?".to_owned());
    format!(
        "Spotify is rate limiting Deck (429, retry-after {retry_after} s, until {until}). \
         Stop this run: Deck makes no Spotify searches before that."
    )
}
