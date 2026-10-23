//! Pick for me (`deck ctl pick`): an album from the shelf or Curated, chosen at random but
//! like drawing from a hat: an album comes again only once all the others have been played.
//!
//! Every album that starts playing (picked or chosen by hand) goes into the hat's "played"
//! list, kept in `~/.local/state/deck/picked.json` (Deck's own state: a broken file is
//! logged and replaced, as `playback.json`). When every album of the source has been
//! played, a new round starts for that source.

use std::path::PathBuf;

use anyhow::Result;

use crate::{
    config,
    store::{read_json, save_json},
};

/// Where Pick for me draws from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickFrom {
    /// The shelf and Curated together.
    Any,
    Shelf,
    Curated,
}

impl PickFrom {
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "" => Some(Self::Any),
            "shelf" => Some(Self::Shelf),
            "curated" => Some(Self::Curated),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Self::Any => "",
            Self::Shelf => "shelf",
            Self::Curated => "curated",
        }
    }
}

/// Chooses an album (URI) from `candidates`, leaving out those in `played` and the one
/// playing now. When nothing is left, the candidates' entries are taken out of `played`
/// (a new round) and the draw is made again. `None` only without candidates.
pub fn choose(
    candidates: &[String],
    played: &mut Vec<String>,
    playing: Option<&str>,
    random: u64,
) -> Option<String> {
    if candidates.is_empty() {
        return None;
    }
    let left = |played: &[String]| -> Vec<&String> {
        candidates
            .iter()
            .filter(|uri| !played.contains(uri) && Some(uri.as_str()) != playing)
            .collect()
    };
    let mut remaining = left(played);
    if remaining.is_empty() {
        played.retain(|uri| !candidates.contains(uri));
        remaining = left(played);
    }
    // Only the playing album to choose from: it is the one.
    if remaining.is_empty() {
        return candidates.first().cloned();
    }
    let index = usize::try_from(random % remaining.len() as u64).unwrap_or(0);
    Some(remaining[index].clone())
}

/// `picked.json`: the albums played in the current rounds.
#[derive(Debug, Clone)]
pub struct PickedFile {
    path: PathBuf,
}

impl PickedFile {
    pub fn default_path() -> Result<Self> {
        Ok(Self::at(config::state_dir()?.join("picked.json")))
    }

    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    /// The played albums; a missing file is none, a broken one is logged and none.
    pub fn load(&self) -> Vec<String> {
        match read_json(&self.path, "the Pick for me history") {
            Ok(played) => played.unwrap_or_default(),
            Err(e) => {
                log::warn!("{e:#}");
                Vec::new()
            }
        }
    }

    pub fn save(&self, played: &[String]) -> Result<()> {
        save_json(&self.path, &played)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uris(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| format!("spotify:album:{n}")).collect()
    }

    #[test]
    fn draws_each_album_once_per_round() {
        let candidates = uris(&["a", "b", "c"]);
        let mut played = Vec::new();
        let mut seen = Vec::new();
        for random in [7, 3, 11] {
            let uri = choose(&candidates, &mut played, None, random).unwrap();
            assert!(!seen.contains(&uri), "{uri} came twice in a round");
            played.push(uri.clone());
            seen.push(uri);
        }
        // All played: a new round starts, and the other source's entries stay.
        played.push("spotify:album:elsewhere".into());
        let uri = choose(&candidates, &mut played, None, 0).unwrap();
        assert!(candidates.contains(&uri));
        assert_eq!(played, uris(&["elsewhere"]));
    }

    #[test]
    fn leaves_out_the_playing_album_even_in_a_new_round() {
        let candidates = uris(&["a", "b"]);
        let mut played = uris(&["a", "b"]);
        for random in 0..5 {
            let uri = choose(
                &candidates,
                &mut played.clone(),
                Some("spotify:album:a"),
                random,
            );
            assert_eq!(uri.as_deref(), Some("spotify:album:b"));
        }
        // The only album is the playing one.
        let only = uris(&["a"]);
        assert_eq!(
            choose(&only, &mut played, Some("spotify:album:a"), 1).as_deref(),
            Some("spotify:album:a")
        );
        assert_eq!(choose(&[], &mut played, None, 1), None);
    }

    #[test]
    fn source_words_round_trip() {
        for from in [PickFrom::Any, PickFrom::Shelf, PickFrom::Curated] {
            assert_eq!(PickFrom::parse(from.word()), Some(from));
        }
        assert_eq!(PickFrom::parse("radio"), None);
    }

    #[test]
    fn history_survives_a_save_and_a_broken_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let file = PickedFile::at(dir.path().join("picked.json"));
        assert!(file.load().is_empty());
        file.save(&uris(&["a"])).unwrap();
        assert_eq!(file.load(), uris(&["a"]));
        std::fs::write(dir.path().join("picked.json"), "{ broken").unwrap();
        assert!(file.load().is_empty());
    }
}
