//! Favourite genres (`~/.config/deck/genres.json`), kept like the shelf: every change
//! rereads the file and saves it at once, and a broken file is never overwritten.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use super::{Genre, all, find};
use crate::{
    config,
    store::{read_json, save_json},
};

#[derive(Debug)]
pub struct Favorites {
    /// `None` when the favourites are read-only.
    path: Option<PathBuf>,
    /// The ids in the file as they are, including those no longer in the catalog.
    ids: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct FavoritesFile {
    pub(super) favorites: Vec<String>,
}

impl Favorites {
    pub fn default_path() -> Result<PathBuf> {
        Ok(config::config_dir()?.join("genres.json"))
    }

    /// Reads the favourites. A missing file means there are no favourites.
    pub fn load(path: &Path) -> Result<Self> {
        let file: Option<FavoritesFile> = read_json(path, "genre favorites")?;
        Ok(Self {
            path: Some(path.to_owned()),
            ids: file.map(|f| f.favorites).unwrap_or_default(),
        })
    }

    /// Empty favourites that cannot be saved: used when `load` failed.
    pub fn unavailable() -> Self {
        Self {
            path: None,
            ids: Vec::new(),
        }
    }

    pub fn contains(&self, id: &str) -> bool {
        self.ids.iter().any(|f| f == id)
    }

    /// The favourite genres from the catalog in alphabetical order by name. An unknown
    /// id is skipped.
    pub fn genres(&self) -> Vec<&'static Genre> {
        all().iter().filter(|g| self.contains(&g.id)).collect()
    }

    /// Adds a catalog genre and saves. Returns `false` if the genre was already a
    /// favourite.
    pub fn add(&mut self, id: &str) -> Result<bool> {
        if find(id).is_none() {
            bail!("unknown genre {id}");
        }
        self.reload()?;
        if self.contains(id) {
            return Ok(false);
        }
        self.ids.push(id.to_owned());
        if let Err(e) = self.save() {
            self.ids.pop();
            return Err(e);
        }
        Ok(true)
    }

    /// Removes the genre and saves. Returns `false` if the genre was not a favourite.
    pub fn remove(&mut self, id: &str) -> Result<bool> {
        self.reload()?;
        let Some(index) = self.ids.iter().position(|f| f == id) else {
            return Ok(false);
        };
        let removed = self.ids.remove(index);
        if let Err(e) = self.save() {
            self.ids.insert(index, removed);
            return Err(e);
        }
        Ok(true)
    }

    /// Rereads the file before a change, so that edits made by hand while Deck is open
    /// are kept. If it has become broken, the error is returned and the favourites in
    /// memory stay as they were.
    fn reload(&mut self) -> Result<()> {
        if let Some(path) = &self.path {
            self.ids = Self::load(path)?.ids;
        }
        Ok(())
    }

    fn save(&self) -> Result<()> {
        let Some(path) = &self.path else {
            bail!("genre favorites are read-only because the favorites file could not be read");
        };
        save_json(
            path,
            &FavoritesFile {
                favorites: self.ids.clone(),
            },
        )
    }
}
