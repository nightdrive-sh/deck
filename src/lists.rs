//! Album lists alongside the shelf: Curated (`~/.config/deck/curated.json`) and its
//! history (`~/.config/deck/curated-history.json`).
//!
//! `deck curate submit` writes the whole list and adds the suggestions to the history. Deck
//! removes a rejected album from the list and marks the rejection in the history
//! ([`CuratedFiles::reject`]). The files are saved atomically ([`crate::store`]) like the
//! shelf, and a broken file is never overwritten: loading returns an error and nothing
//! is written.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::Result;
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

use crate::{
    catalog::Album,
    config,
    store::{read_json, save_json},
};

/// An album list to show: name, subtitle and albums. `stale`: the list is more than a
/// week old, i.e. the weekly update has been missed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumList {
    pub name: String,
    pub subtitle: String,
    pub stale: bool,
    pub albums: Vec<ListAlbum>,
}

/// "Now" in tests: 2026-10-06 09:00 UTC, a day after the list of round 2026-W41.
#[cfg(test)]
pub const TEST_NOW: Timestamp = Timestamp::constant(1_791_277_200, 0);

/// How old a list may be before it is shown as stale.
const STALE_AFTER: SignedDuration = SignedDuration::from_hours(7 * 24);

/// An album on a list, an optional reason and its rough genre: one or two coarse words
/// from the artist's Spotify genres ([`crate::coarse_genre`]), empty when not known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListAlbum {
    pub album: Album,
    pub reason: Option<String>,
    pub genres: Vec<String>,
}

/// A curation round: `round` is an ISO week (`2026-W41`), `created` an RFC 3339 time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Curated {
    pub round: String,
    pub created: String,
    pub albums: Vec<ListAlbum>,
}

impl Curated {
    /// The list for the view: "Curated", with the subtitle "20 albums · week 41". For a
    /// list more than a week old the subtitle also has the age:
    /// "20 albums · week 40 · 9 days old". An unparsable `created` does not make the
    /// list stale.
    pub fn to_list(&self, now: Timestamp) -> AlbumList {
        let count = match self.albums.len() {
            1 => "1 album".to_owned(),
            n => format!("{n} albums"),
        };
        let mut subtitle = match self.round.split_once("-W") {
            Some((_, week)) => format!("{count} · week {}", week.trim_start_matches('0')),
            None => count,
        };
        let age = self
            .created
            .parse::<Timestamp>()
            .ok()
            .map(|created| now.duration_since(created))
            .filter(|age| *age > STALE_AFTER);
        if let Some(age) = age {
            subtitle.push_str(&format!(" · {} days old", age.as_hours() / 24));
        }
        AlbumList {
            name: "Curated".to_owned(),
            subtitle,
            stale: age.is_some(),
            albums: self.albums.clone(),
        }
    }
}

/// A history row: an album suggested once, and whether it was rejected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub uri: String,
    pub name: String,
    pub artist: String,
    pub round: String,
    pub rejected: bool,
}

/// The files of the Curated list and its history.
#[derive(Debug, Clone)]
pub struct CuratedFiles {
    curated: PathBuf,
    history: PathBuf,
}

impl CuratedFiles {
    /// The files in the directory `~/.config/deck/`.
    pub fn default_paths() -> Result<Self> {
        Ok(Self::in_dir(&config::config_dir()?))
    }

    pub fn in_dir(dir: &Path) -> Self {
        Self {
            curated: dir.join("curated.json"),
            history: dir.join("curated-history.json"),
        }
    }

    /// Reads the current list. A missing file is `None`.
    pub fn load(&self) -> Result<Option<Curated>> {
        let file: Option<CuratedFile> = read_json(&self.curated, "curated list")?;
        Ok(file.map(Curated::from))
    }

    /// Reads the history. A missing file is an empty history.
    pub fn history(&self) -> Result<Vec<HistoryEntry>> {
        Ok(read_json(&self.history, "curated history")?.unwrap_or_default())
    }

    /// Replaces the list with a new round and adds its albums to the history.
    /// The history is written first: if saving the list fails, the albums are
    /// still not suggested again.
    pub fn publish(&self, curated: &Curated) -> Result<()> {
        let mut history = self.history()?;
        history.extend(curated.albums.iter().map(|a| HistoryEntry {
            uri: a.album.uri.clone(),
            name: a.album.name.clone(),
            artist: a.album.artist.clone(),
            round: curated.round.clone(),
            rejected: false,
        }));
        save_json(&self.history, &history)?;
        save_json(&self.curated, &CuratedFile::from(curated))
    }

    /// Rejects an album: removes it from the list and marks it rejected in the history.
    /// Returns the updated list, or `None` if there is no list. The history is
    /// written first, so a failed save is safe to repeat.
    pub fn reject(&self, uri: &str) -> Result<Option<Curated>> {
        let Some(mut curated) = self.load()? else {
            return Ok(None);
        };
        let mut history = self.history()?;
        let Some(index) = curated.albums.iter().position(|a| a.album.uri == uri) else {
            return Ok(Some(curated));
        };
        let removed = curated.albums.remove(index);

        let mut marked = false;
        for entry in history.iter_mut().filter(|e| e.uri == uri) {
            entry.rejected = true;
            marked = true;
        }
        if !marked {
            history.push(HistoryEntry {
                uri: removed.album.uri.clone(),
                name: removed.album.name.clone(),
                artist: removed.album.artist.clone(),
                round: curated.round.clone(),
                rejected: true,
            });
        }
        save_json(&self.history, &history)?;
        save_json(&self.curated, &CuratedFile::from(&curated))?;
        Ok(Some(curated))
    }

    /// Gives the albums of the current list their rough genres, by artist id, and saves
    /// the list. The list is reread first, so an album rejected meanwhile stays out; an
    /// album whose artist is not in `by_artist` keeps what it had. A missing list is left
    /// missing.
    pub fn set_genres(&self, by_artist: &HashMap<String, Vec<String>>) -> Result<()> {
        let Some(mut curated) = self.load()? else {
            return Ok(());
        };
        for entry in &mut curated.albums {
            if let Some(genres) = by_artist.get(&entry.album.artist_id) {
                entry.genres.clone_from(genres);
            }
        }
        save_json(&self.curated, &CuratedFile::from(&curated))
    }
}

/// Whether two albums are the same when normalised ([`normalize`]): same artist and name.
pub fn same_album(artist_a: &str, album_a: &str, artist_b: &str, album_b: &str) -> bool {
    normalize(artist_a) == normalize(artist_b) && normalize(album_a) == normalize(album_b)
}

/// A name for comparison: lowercase, accents removed (ä → a too), & → and,
/// punctuation removed and bracketed extras skipped ("OK Computer (Remastered)"
/// = "ok computer"). A name made only of brackets keeps what is inside them.
pub fn normalize(name: &str) -> String {
    let without_extras = strip_brackets(name);
    let source = if without_extras.trim().is_empty() {
        name
    } else {
        &without_extras
    };
    let mut out = String::with_capacity(source.len());
    for c in source.nfd().filter(|&c| !is_combining_mark(c)) {
        match c {
            '&' => out.push_str(" and "),
            c if c.is_alphanumeric() => out.extend(c.to_lowercase()),
            c if c.is_whitespace() || c == '-' || c == '/' => out.push(' '),
            // Other punctuation (periods, apostrophes…) is removed without a space.
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A track name for comparison: an extra " - …" is skipped ("Gimme Some Lovin' -
/// 2015 Remaster" = "Gimme Some Lovin'"), and the rest is normalised ([`normalize`]).
pub fn track_key(name: &str) -> String {
    normalize(name.split(" - ").next().unwrap_or(name))
}

/// Removes the parts inside ( ) and [ ].
fn strip_brackets(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut depth = 0usize;
    for c in name.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

#[derive(Serialize, Deserialize)]
struct CuratedFile {
    round: String,
    created: String,
    albums: Vec<CuratedEntry>,
}

#[derive(Serialize, Deserialize)]
struct CuratedEntry {
    uri: String,
    name: String,
    artist: String,
    artist_id: String,
    year: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    /// Missing from a list written before Deck knew genres.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    genres: Vec<String>,
}

impl From<CuratedFile> for Curated {
    fn from(file: CuratedFile) -> Self {
        Self {
            round: file.round,
            created: file.created,
            albums: file
                .albums
                .into_iter()
                .map(|e| ListAlbum {
                    album: Album {
                        id: e.uri.rsplit(':').next().unwrap_or_default().to_owned(),
                        uri: e.uri,
                        name: e.name,
                        artist: e.artist,
                        artist_id: e.artist_id,
                        year: e.year,
                    },
                    reason: e.reason,
                    genres: e.genres,
                })
                .collect(),
        }
    }
}

impl From<&Curated> for CuratedFile {
    fn from(c: &Curated) -> Self {
        Self {
            round: c.round.clone(),
            created: c.created.clone(),
            albums: c
                .albums
                .iter()
                .map(|a| CuratedEntry {
                    uri: a.album.uri.clone(),
                    name: a.album.name.clone(),
                    artist: a.album.artist.clone(),
                    artist_id: a.album.artist_id.clone(),
                    year: a.album.year,
                    reason: a.reason.clone(),
                    genres: a.genres.clone(),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests;
