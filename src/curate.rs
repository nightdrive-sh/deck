//! The subcommands `deck taste` and `deck curate submit`, with which the curation run
//! (Claude, started by `curate_run`) reads the taste profile and offers its candidates
//! for the Curated list.
//!
//! The check rules are a pure function, [`check`]: the candidates, their Spotify hits,
//! the shelf, the history and the listened albums → a report and the list's albums.
//!
//! Last.fm is optional. With it, the taste is your listening history there and an album
//! with [`LISTENED_MIN_PLAYS`] plays is listened. Without it, the taste is what Deck itself
//! knows (the albums played in Deck lately, the shelf, earlier lists) and your own words
//! in `~/.config/deck/taste.md`, and the albums played in Deck lately are the listened ones.
//!
//! After too many Web API calls Spotify blocks the whole app for hours (429), Deck's
//! search included. That is why searches go through [`lookup`]: already known
//! candidates are not searched, a repeated search comes from a one-day cache
//! (`curate-searches.json`), there are at most [`SEARCH_BUDGET`] searches a day, and
//! the first 429 blocks searches for the `retry-after` period. The block is shared
//! with Deck itself ([`RateLimit`](crate::rate_limit::RateLimit)). The same
//! [`SearchCache`] also counts the searches of `deck genre add`.

use std::{io::Read, time::Duration};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{
    catalog::{Album, Catalog, FoundAlbum},
    config::Config,
    history::HistoryFile,
    lastfm::{self, TopAlbum},
    lists::{Curated, CuratedFiles, HistoryEntry, ListAlbum, normalize, same_album},
    store::unix_now,
};

mod search;
mod taste;

pub use search::SearchCache;
pub use taste::taste;

use search::lookup;
use taste::{lastfm_client, load_shelf, played_in_deck};

/// Length of the Curated list.
pub const MAX_ALBUMS: usize = 20;
/// This many plays on Last.fm make an album listened.
pub const LISTENED_MIN_PLAYS: u32 = 3;
/// At most this many Spotify searches a day, across all `deck curate submit` runs.
/// Spotify does not say where its limit is: 109 searches in four minutes once passed,
/// but some 40 more the next morning brought a 429 with a block of almost ten hours.
pub const SEARCH_BUDGET: usize = 50;
/// Pause between Spotify searches.
pub const SEARCH_PAUSE: Duration = Duration::from_millis(500);
/// Length of the block if the 429 response has no `retry-after`.
pub const DEFAULT_BLOCK: u64 = 60 * 60;

/// A suggestion from the curator, read from stdin.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Candidate {
    pub artist: String,
    pub album: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Rejection {
    /// No matching album was found on Spotify.
    NotFound,
    /// The same URI, or the same artist and album, is already on the shelf.
    OnShelf,
    /// Suggested in an earlier round.
    SuggestedBefore,
    /// At least [`LISTENED_MIN_PLAYS`] plays on Last.fm, or without Last.fm, played in
    /// Deck lately.
    Listened,
    /// The same album is already on this list.
    Duplicate,
    /// Not searched on Spotify, because [`SEARCH_BUDGET`] has been used up.
    NotChecked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Accepted {
    pub artist: String,
    pub album: String,
    pub year: Option<u16>,
    pub uri: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rejected {
    pub artist: String,
    pub album: String,
    pub reason: Rejection,
    /// The album found on Spotify, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
}

/// Output of `deck curate submit`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    /// The albums taken onto the list, in suggestion order, at most [`MAX_ALBUMS`].
    pub accepted: Vec<Accepted>,
    pub rejected: Vec<Rejected>,
    /// Albums that passed the check but did not fit on the list.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unused: Vec<Accepted>,
    /// `MAX_ALBUMS` − the number accepted.
    pub missing: usize,
    /// Spotify searches in this run (cache misses).
    pub searched: usize,
    /// What is left of the day's [`SEARCH_BUDGET`].
    pub searches_left: usize,
}

/// Checks the candidates. `hits[i]` holds the Spotify search hits for `candidates[i]`,
/// `None` if it was not searched. Returns the report and the albums to put on the
/// list, with their reasons. The report's `searched` and `searches_left` stay zero.
pub fn check(
    candidates: &[Candidate],
    hits: &[Option<Vec<FoundAlbum>>],
    shelf: &[Album],
    history: &[HistoryEntry],
    listened: &[TopAlbum],
) -> (Report, Vec<ListAlbum>) {
    let mut passed: Vec<ListAlbum> = Vec::new();
    let mut rejected = Vec::new();

    for (candidate, found) in candidates.iter().zip(hits) {
        let reject = |reason, uri: Option<&str>| Rejected {
            artist: candidate.artist.clone(),
            album: candidate.album.clone(),
            reason,
            uri: uri.map(str::to_owned),
        };
        if let Some(reason) = known(candidate, shelf, history, listened) {
            rejected.push(reject(reason, None));
            continue;
        }
        let Some(found) = found else {
            rejected.push(reject(Rejection::NotChecked, None));
            continue;
        };
        let Some(album) = pick(candidate, found) else {
            rejected.push(reject(Rejection::NotFound, None));
            continue;
        };
        // The same album by Spotify's names ([`known`] already checked the candidate's).
        let same = |artist: &str, name: &str| same_album(artist, name, &album.artist, &album.name);

        let reason = if shelf
            .iter()
            .any(|a| a.uri == album.uri || same(&a.artist, &a.name))
        {
            Some(Rejection::OnShelf)
        } else if history
            .iter()
            .any(|h| h.uri == album.uri || same(&h.artist, &h.name))
        {
            Some(Rejection::SuggestedBefore)
        } else if listened
            .iter()
            .any(|l| l.plays >= LISTENED_MIN_PLAYS && same(&l.artist, &l.album))
        {
            Some(Rejection::Listened)
        } else if passed
            .iter()
            .any(|p| p.album.uri == album.uri || same(&p.album.artist, &p.album.name))
        {
            Some(Rejection::Duplicate)
        } else {
            None
        };
        match reason {
            Some(reason) => rejected.push(reject(reason, Some(&album.uri))),
            None => passed.push(ListAlbum {
                album,
                reason: Some(candidate.reason.trim().to_owned()).filter(|r| !r.is_empty()),
            }),
        }
    }

    let unused = passed.split_off(passed.len().min(MAX_ALBUMS));
    let accepted_of = |albums: &[ListAlbum]| -> Vec<Accepted> {
        albums
            .iter()
            .map(|a| Accepted {
                artist: a.album.artist.clone(),
                album: a.album.name.clone(),
                year: a.album.year,
                uri: a.album.uri.clone(),
            })
            .collect()
    };
    let report = Report {
        accepted: accepted_of(&passed),
        rejected,
        unused: accepted_of(&unused),
        missing: MAX_ALBUMS - passed.len(),
        searched: 0,
        searches_left: 0,
    };
    (report, passed)
}

/// A rejection based on the candidate's own names alone: on the shelf, suggested
/// before or listened. Such a candidate need not be searched on Spotify.
fn known(
    candidate: &Candidate,
    shelf: &[Album],
    history: &[HistoryEntry],
    listened: &[TopAlbum],
) -> Option<Rejection> {
    let same =
        |artist: &str, name: &str| same_album(artist, name, &candidate.artist, &candidate.album);
    if shelf.iter().any(|a| same(&a.artist, &a.name)) {
        Some(Rejection::OnShelf)
    } else if history.iter().any(|h| same(&h.artist, &h.name)) {
        Some(Rejection::SuggestedBefore)
    } else if listened
        .iter()
        .any(|l| l.plays >= LISTENED_MIN_PLAYS && same(&l.artist, &l.album))
    {
        Some(Rejection::Listened)
    } else {
        None
    }
}

/// Picks the candidate's album from the hits: the artist and name must match when
/// normalised. Of several matches the original release is picked: an album before
/// a compilation or single, then the oldest, and on a tie the first in Spotify's
/// order.
fn pick(candidate: &Candidate, found: &[FoundAlbum]) -> Option<Album> {
    let artist = normalize(&candidate.artist);
    let name = normalize(&candidate.album);
    found
        .iter()
        .filter(|f| normalize(&f.album.name) == name)
        .filter(|f| f.artists.iter().any(|a| normalize(a) == artist))
        .min_by_key(|f| (f.album_type != "album", f.album.year.unwrap_or(u16::MAX)))
        .map(|f| f.album.clone())
}

/// `deck curate submit [--dry-run]`: reads the candidates from stdin, prints a report and
/// (without `--dry-run`) publishes the list. Zero accepted without `--dry-run` is an error.
pub async fn submit(dry_run: bool) -> Result<()> {
    let config = &Config::load()?;
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .context("cannot read stdin")?;
    let candidates: Vec<Candidate> = serde_json::from_str(&input)
        .context("stdin must be a JSON list of {\"artist\", \"album\", \"reason\"}")?;

    // The block is checked only right before the first real search ([`lookup`]), and
    // Deck signs in to Spotify only then: a run that needs no searches (all candidates
    // known or cached) succeeds even while blocked.
    let cache_path = SearchCache::default_path()?;
    let mut cache = SearchCache::load(&cache_path, unix_now());

    let shelf = load_shelf()?;
    let files = CuratedFiles::default_paths()?;
    let history = files.history()?;
    let mut catalog: Option<Catalog> = None;
    let listened = match lastfm_client(config)? {
        Some(lastfm) => {
            lastfm
                .listened_albums(&lastfm::albums_cache_path()?, LISTENED_MIN_PLAYS)
                .await?
        }
        None => played_in_deck(&HistoryFile::default_path()?.load()),
    };

    let skip: Vec<bool> = candidates
        .iter()
        .map(|c| known(c, &shelf, &history, &listened).is_some())
        .collect();
    let looked_up = lookup(
        &candidates,
        &skip,
        &mut cache,
        unix_now(),
        SEARCH_PAUSE,
        async |artist: &str, album: &str| {
            if catalog.is_none() {
                catalog = Some(Catalog::connect_stored(config).await?);
            }
            let catalog = catalog.as_ref().expect("connected above");
            catalog.search_albums(artist, album).await
        },
    )
    .await;
    // Keep the searches of an interrupted run too.
    cache.save(&cache_path)?;
    let (hits, searched) = looked_up?;

    let (mut report, albums) = check(&candidates, &hits, &shelf, &history, &listened);
    let not_found: Vec<&Rejected> = report
        .rejected
        .iter()
        .filter(|r| r.reason == Rejection::NotFound)
        .collect();
    if !not_found.is_empty() {
        for r in not_found {
            cache.remember_not_found(&r.artist, &r.album, unix_now());
        }
        cache.save(&cache_path)?;
    }
    report.searched = searched;
    report.searches_left = cache.searches_left();
    println!("{}", serde_json::to_string_pretty(&report)?);

    if dry_run {
        return Ok(());
    }
    if albums.is_empty() {
        bail!("no candidate was accepted, the current list was left as it is");
    }
    let now = jiff::Zoned::now();
    let week = now.date().iso_week_date();
    let curated = Curated {
        round: format!("{:04}-W{:02}", week.year(), week.week()),
        created: now.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string(),
        albums,
    };
    files.publish(&curated)
}
#[cfg(test)]
mod tests;
