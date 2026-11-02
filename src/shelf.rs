//! The album shelf (`~/.config/deck/shelf.json`): Deck's own hand-picked album list.
//!
//! The file is a JSON list of `{uri, name, artist, artist_id, year}`. Every change
//! rereads the file first, so that edits made by hand while Deck is open are kept,
//! and is saved atomically right away ([`crate::store`]). A broken file is never
//! overwritten: `load` and every change return an error, and if the file is broken at
//! startup the caller uses `Shelf::unavailable()` for the session, which is empty and
//! read-only.
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{
    catalog::Album,
    config,
    store::{read_json, write_atomic},
};

mod sort;

pub use sort::{albums_of, artists_of, sort_by_year};

#[derive(Debug)]
pub struct Shelf {
    /// `None` when the shelf is read-only.
    path: Option<PathBuf>,
    albums: Vec<Album>,
}

impl Shelf {
    pub fn default_path() -> Result<PathBuf> {
        Ok(config::config_dir()?.join("shelf.json"))
    }

    /// Reads the shelf. A missing file means an empty shelf.
    pub fn load(path: &Path) -> Result<Self> {
        let albums = read_json::<Vec<Entry>>(path, "shelf")?
            .unwrap_or_default()
            .into_iter()
            .map(Album::from)
            .collect();
        Ok(Self {
            path: Some(path.to_owned()),
            albums,
        })
    }

    /// An empty shelf that cannot be saved: used when `load` failed.
    pub fn unavailable() -> Self {
        Self {
            path: None,
            albums: Vec::new(),
        }
    }

    pub fn contains(&self, uri: &str) -> bool {
        self.albums.iter().any(|a| a.uri == uri)
    }

    /// Adds the album and saves. Returns `false` if the album was already on the shelf.
    pub fn add(&mut self, album: Album) -> Result<bool> {
        self.reload()?;
        if self.contains(&album.uri) {
            return Ok(false);
        }
        self.albums.push(album);
        if let Err(e) = self.save() {
            self.albums.pop();
            return Err(e);
        }
        Ok(true)
    }

    /// Removes the album and saves. Returns `false` if the album was not on the shelf.
    pub fn remove(&mut self, uri: &str) -> Result<bool> {
        self.reload()?;
        let Some(index) = self.albums.iter().position(|a| a.uri == uri) else {
            return Ok(false);
        };
        let album = self.albums.remove(index);
        if let Err(e) = self.save() {
            self.albums.insert(index, album);
            return Err(e);
        }
        Ok(true)
    }

    /// The shelf's albums in the order they were saved.
    pub fn albums(&self) -> &[Album] {
        &self.albums
    }

    /// Rereads the file before a change. If it has become broken, the error is
    /// returned and the albums in memory stay as they were.
    fn reload(&mut self) -> Result<()> {
        if let Some(path) = &self.path {
            self.albums = Self::load(path)?.albums;
        }
        Ok(())
    }

    fn save(&self) -> Result<()> {
        let Some(path) = &self.path else {
            bail!("the shelf is read-only because the shelf file could not be read");
        };
        let entries: Vec<Entry> = self.albums.iter().map(Entry::from).collect();
        let mut json = serde_json::to_string_pretty(&entries)?;
        json.push('\n');
        write_atomic(path, json.as_bytes())
            .with_context(|| format!("cannot save the shelf to {}", path.display()))
    }
}
/// An album in the shelf file. The album id is the last part of the Spotify URI.
#[derive(Serialize, Deserialize)]
struct Entry {
    uri: String,
    name: String,
    artist: String,
    artist_id: String,
    year: Option<u16>,
}

impl From<&Album> for Entry {
    fn from(a: &Album) -> Self {
        Self {
            uri: a.uri.clone(),
            name: a.name.clone(),
            artist: a.artist.clone(),
            artist_id: a.artist_id.clone(),
            year: a.year,
        }
    }
}

impl From<Entry> for Album {
    fn from(e: Entry) -> Self {
        Self {
            id: e.uri.rsplit(':').next().unwrap_or_default().to_owned(),
            uri: e.uri,
            name: e.name,
            artist: e.artist,
            artist_id: e.artist_id,
            year: e.year,
        }
    }
}
#[cfg(test)]
mod tests;
