//! The user's own genres (`~/.config/deck/my-genres.json`), which `deck genre add` writes.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::{Genre, slug, validate};
use crate::{
    config,
    store::{read_json, save_json},
};

/// The user's own genres (`~/.config/deck/my-genres.json`, `{"genres": [...]}`).
/// A broken file is not overwritten: `load` returns an error.
#[derive(Debug)]
pub struct MyGenres {
    path: PathBuf,
    genres: Vec<Genre>,
}

#[derive(Serialize, Deserialize)]
struct MyGenresFile {
    genres: Vec<Genre>,
}

impl MyGenres {
    pub fn default_path() -> Result<PathBuf> {
        Ok(config::config_dir()?.join("my-genres.json"))
    }

    /// Reads and checks the user's own genres like the catalog; in addition, the id must
    /// be the [`slug`] of the name. A missing file means there are no own genres.
    pub fn load(path: &Path) -> Result<Self> {
        let file: Option<MyGenresFile> = read_json(path, "my genres")?;
        let genres = file.map(|f| f.genres).unwrap_or_default();
        (|| {
            validate(&genres)?;
            for genre in &genres {
                if genre.id != slug(&genre.name) {
                    bail!("genre {}: id must be {}", genre.name, slug(&genre.name));
                }
            }
            Ok(())
        })()
        .with_context(|| format!("my genres file {} is broken", path.display()))?;
        Ok(Self {
            path: path.to_owned(),
            genres,
        })
    }

    pub fn genres(&self) -> &[Genre] {
        &self.genres
    }

    pub fn find(&self, id: &str) -> Option<&Genre> {
        self.genres.iter().find(|g| g.id == id)
    }

    /// Adds the genre, or replaces the own genre with the same id, and saves.
    pub fn put(&mut self, genre: Genre) -> Result<()> {
        let mut genres = self.genres.clone();
        match genres.iter_mut().find(|g| g.id == genre.id) {
            Some(old) => *old = genre,
            None => genres.push(genre),
        }
        validate(&genres)?;
        save_json(
            &self.path,
            &MyGenresFile {
                genres: genres.clone(),
            },
        )?;
        self.genres = genres;
        Ok(())
    }

    /// Removes an own genre and saves. Returns `false` if there was no such genre.
    pub fn remove(&mut self, id: &str) -> Result<bool> {
        let Some(index) = self.genres.iter().position(|g| g.id == id) else {
            return Ok(false);
        };
        let mut genres = self.genres.clone();
        genres.remove(index);
        save_json(
            &self.path,
            &MyGenresFile {
                genres: genres.clone(),
            },
        )?;
        self.genres = genres;
        Ok(true)
    }
}
