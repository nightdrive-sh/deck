//! Albums you have listened to in Deck (`~/.local/state/deck/listened.json`): a track counts
//! once half of it has played, and an album is listened once half of its tracks have. A
//! window shows it in Curated, so you know which suggestions you have already heard.
//!
//! The file is Deck's own state, like `picked.json`: a broken one is logged and replaced.
//! An album's track count is learnt whenever Deck fetches its tracks (to play it, queue it
//! or pick it); until then it can't be listened.

use std::{collections::BTreeMap, path::PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{
    config,
    store::{read_json, save_json},
};

/// One album's plays.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumPlays {
    /// The album's track count; 0 while unknown.
    #[serde(default)]
    pub tracks: usize,
    /// The tracks (URIs) that have played at least halfway.
    #[serde(default)]
    pub played: Vec<String>,
    /// Half of the tracks have played; it stays so.
    #[serde(default)]
    pub listened: bool,
}

impl AlbumPlays {
    fn update(&mut self) {
        if self.tracks > 0 && self.played.len() * 2 >= self.tracks {
            self.listened = true;
        }
    }
}

/// Every album's plays by URI.
pub type Listened = BTreeMap<String, AlbumPlays>;

/// Notes an album's track count. Returns whether anything changed.
pub fn learn_length(listened: &mut Listened, album: &str, tracks: usize) -> bool {
    if tracks == 0
        || listened
            .get(album)
            .is_some_and(|plays| plays.tracks == tracks)
    {
        return false;
    }
    let plays = listened.entry(album.to_owned()).or_default();
    plays.tracks = tracks;
    plays.update();
    true
}

/// Notes that a track of the album played halfway. Returns whether anything changed.
pub fn played(listened: &mut Listened, album: &str, track: &str) -> bool {
    let plays = listened.entry(album.to_owned()).or_default();
    if plays.played.iter().any(|t| t == track) {
        return false;
    }
    plays.played.push(track.to_owned());
    plays.update();
    true
}

/// `listened.json`.
#[derive(Debug, Clone)]
pub struct ListenedFile {
    path: PathBuf,
}

impl ListenedFile {
    pub fn default_path() -> Result<Self> {
        Ok(Self::at(config::state_dir()?.join("listened.json")))
    }

    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    /// The plays; a missing file is none, a broken one is logged and none.
    pub fn load(&self) -> Listened {
        match read_json(&self.path, "the listened albums") {
            Ok(listened) => listened.unwrap_or_default(),
            Err(e) => {
                log::warn!("{e:#}");
                Listened::new()
            }
        }
    }

    pub fn save(&self, listened: &Listened) -> Result<()> {
        save_json(&self.path, listened)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_the_tracks_make_an_album_listened() {
        let mut listened = Listened::new();
        assert!(played(&mut listened, "a", "t1"));
        assert!(!played(&mut listened, "a", "t1"), "a track counts once");
        // The length isn't known yet
        assert!(!listened["a"].listened);
        assert!(learn_length(&mut listened, "a", 4));
        assert!(!listened["a"].listened);
        played(&mut listened, "a", "t2");
        assert!(listened["a"].listened);
        // A longer count learnt later doesn't take it back
        learn_length(&mut listened, "a", 10);
        assert!(listened["a"].listened);
        assert!(!learn_length(&mut listened, "a", 10));
        assert!(!learn_length(&mut listened, "b", 0));
    }

    #[test]
    fn plays_survive_a_save_and_a_broken_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let file = ListenedFile::at(dir.path().join("listened.json"));
        assert!(file.load().is_empty());
        let mut listened = Listened::new();
        learn_length(&mut listened, "a", 2);
        played(&mut listened, "a", "t1");
        file.save(&listened).unwrap();
        assert_eq!(file.load(), listened);
        std::fs::write(dir.path().join("listened.json"), "[ broken").unwrap();
        assert!(file.load().is_empty());
    }
}
