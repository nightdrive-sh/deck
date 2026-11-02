//! The genre radio catalog and favourites.
//!
//! The catalog (`genres/genres.json`) is compiled into the program. Each genre has a
//! stable id, a display name and seeds: typical artists or tracks of the genre whose
//! Spotify station the radio plays ([`crate::radio`]). Seeds are chosen so that the
//! station hits the genre; the manual run `live_catalog_stations` fetches every station.
//!
//! Favourites (`~/.config/deck/genres.json`, `{"favorites": [...]}`) are handled like
//! the shelf: every change rereads the file first and saves it atomically right away.
//! A broken file is never overwritten: `load` and every change return an error, and if
//! the file is broken at startup the caller uses [`Favorites::unavailable`], which is
//! empty and read-only. An id that has left the catalog is skipped in the view but kept in
//! the file.
//!
//! The user's own genres (`~/.config/deck/my-genres.json`, [`MyGenres`]) have the same
//! shape as the catalog, and `deck genre add` (`genre_cli`) writes them. Deck reads them
//! at startup ([`install`]), and an own genre replaces the catalog genre of the same
//! name: the id is derived from the name ([`slug`]), so favourites and playback state
//! are kept.
use std::{
    collections::HashSet,
    hash::{BuildHasher, RandomState},
    sync::OnceLock,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

mod favorites;
mod my_genres;

pub use favorites::Favorites;
pub use my_genres::MyGenres;

const CATALOG: &str = include_str!("../genres/genres.json");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Genre {
    /// Stable id by which favourites and playback state refer to the genre.
    pub id: String,
    /// Display name in lowercase.
    pub name: String,
    pub seeds: Vec<Seed>,
}

/// A radio seed: an artist or track URI. The name is for whoever reads the data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seed {
    pub uri: String,
    pub name: String,
}

static OWN: OnceLock<Vec<Genre>> = OnceLock::new();

/// Includes the user's own genres in [`all`]. Called once at startup before the first
/// `all`; a later call has no effect. Tests see the catalog alone.
pub fn install(own: Vec<Genre>) {
    let _ = OWN.set(own);
}

/// The catalog and the user's own genres combined, in alphabetical order by name.
pub fn all() -> &'static [Genre] {
    static GENRES: OnceLock<Vec<Genre>> = OnceLock::new();
    GENRES.get_or_init(|| merge(catalog(), OWN.get().map_or(&[], Vec::as_slice)))
}

/// The catalog shipped with Deck, without the user's own genres.
pub fn catalog() -> Vec<Genre> {
    parse(CATALOG).expect("genres/genres.json is invalid")
}

/// The catalog where an own genre replaces the catalog genre with the same id and the
/// other own genres are added. In alphabetical order by name.
pub fn merge(mut catalog: Vec<Genre>, own: &[Genre]) -> Vec<Genre> {
    catalog.retain(|g| !own.iter().any(|o| o.id == g.id));
    catalog.extend(own.iter().cloned());
    catalog.sort_by(|a, b| a.name.cmp(&b.name));
    catalog
}

/// An id from a name: lowercase, and anything other than letters and digits becomes a
/// hyphen (`60s rock` → `60s-rock`). Catalog ids follow the same rule.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

pub fn find(id: &str) -> Option<&'static Genre> {
    all().iter().find(|g| g.id == id)
}

/// Parses and checks the catalog: ids and names are unique, and every genre has
/// seeds that are artist or track URIs.
fn parse(text: &str) -> Result<Vec<Genre>> {
    let mut genres: Vec<Genre> = serde_json::from_str(text).context("invalid genre catalog")?;
    validate(&genres)?;
    genres.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(genres)
}

fn validate(genres: &[Genre]) -> Result<()> {
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for genre in genres {
        if !ids.insert(genre.id.as_str()) {
            bail!("duplicate genre id {}", genre.id);
        }
        if !names.insert(genre.name.as_str()) {
            bail!("duplicate genre name {}", genre.name);
        }
        if genre.seeds.is_empty() {
            bail!("genre {} has no seeds", genre.id);
        }
        let mut uris = HashSet::new();
        for seed in &genre.seeds {
            if !is_seed_uri(&seed.uri) {
                bail!(
                    "genre {}: seed {} is not an artist or track URI",
                    genre.id,
                    seed.uri
                );
            }
            if !uris.insert(seed.uri.as_str()) {
                bail!("genre {}: duplicate seed {}", genre.id, seed.uri);
            }
        }
    }
    Ok(())
}

pub fn is_seed_uri(uri: &str) -> bool {
    let id = uri
        .strip_prefix("spotify:artist:")
        .or_else(|| uri.strip_prefix("spotify:track:"));
    id.is_some_and(|id| id.len() == 22 && id.chars().all(|c| c.is_ascii_alphanumeric()))
}

impl Genre {
    /// Picks a random seed that is not `current`. Seeds not in `played` are picked
    /// first; once all have been played, the draw starts over.
    /// `random` is a random number (see [`random`]), so the draw can be tested.
    pub fn pick_seed(&self, current: Option<&str>, played: &[String], random: u64) -> &Seed {
        let others: Vec<&Seed> = self
            .seeds
            .iter()
            .filter(|s| Some(s.uri.as_str()) != current)
            .collect();
        let unplayed: Vec<&Seed> = others
            .iter()
            .copied()
            .filter(|s| !played.contains(&s.uri))
            .collect();
        let pool = match (unplayed.is_empty(), others.is_empty()) {
            (false, _) => unplayed,
            (true, false) => others,
            // The only seed is the one playing: better to play it again than nothing.
            (true, true) => self.seeds.iter().collect(),
        };
        pool[(random % pool.len() as u64) as usize]
    }
}

/// A random number for the draw without an extra dependency: `RandomState` gets new
/// keys every time.
pub fn random() -> u64 {
    RandomState::new().hash_one(())
}
#[cfg(test)]
mod tests;
