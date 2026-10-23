//! What you have played lately, album by album (`~/.local/state/deck/history.json`), for a
//! window's History: an album goes in when one of its tracks has played halfway, newest
//! first. Playing it again soon after only moves it to the top; after [`KEEP_DAYS`] an entry
//! is dropped, so the file stays small however much you listen.
//!
//! Deck's own state, like `picked.json`: a broken file is logged and replaced.

use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{
    config,
    store::{read_json, save_json},
};

/// How long the history reaches back.
pub const KEEP_DAYS: u64 = 30;

/// Within this long, playing the same album again is the same listen: it only moves up.
const SAME_LISTEN_SECS: u64 = 6 * 60 * 60;

/// An album played, and when (seconds since the Unix epoch).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Played {
    pub uri: String,
    pub name: String,
    pub artist: String,
    #[serde(default)]
    pub at: u64,
}

/// Adds `played` at the top (its `at` is the time), and drops what is older than
/// [`KEEP_DAYS`].
pub fn record(history: &mut Vec<Played>, played: Played) {
    let now = played.at;
    if let Some(i) = history
        .iter()
        .position(|p| p.uri == played.uri && now.saturating_sub(p.at) < SAME_LISTEN_SECS)
    {
        history.remove(i);
    }
    history.insert(0, played);
    history.retain(|p| now.saturating_sub(p.at) < KEEP_DAYS * 24 * 60 * 60);
}

/// `history.json`.
#[derive(Debug, Clone)]
pub struct HistoryFile {
    path: PathBuf,
}

impl HistoryFile {
    pub fn default_path() -> Result<Self> {
        Ok(Self::at(config::state_dir()?.join("history.json")))
    }

    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    /// The history; a missing file is none, a broken one is logged and none.
    pub fn load(&self) -> Vec<Played> {
        match read_json(&self.path, "the history") {
            Ok(history) => history.unwrap_or_default(),
            Err(e) => {
                log::warn!("{e:#}");
                Vec::new()
            }
        }
    }

    /// Reads the file again, records `played` and saves: another Deck may have written it.
    pub fn record(&self, played: Played) -> Result<()> {
        let mut history = self.load();
        record(&mut history, played);
        save_json(&self.path, &history)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 24 * 60 * 60;

    fn played(uri: &str, at: u64) -> Played {
        Played {
            uri: format!("spotify:album:{uri}"),
            name: uri.to_uppercase(),
            artist: "Artist".into(),
            at,
        }
    }

    fn uris(history: &[Played]) -> Vec<&str> {
        history.iter().map(|p| p.uri.as_str()).collect()
    }

    #[test]
    fn newest_first_and_a_replay_soon_after_only_moves_up() {
        let mut history = Vec::new();
        record(&mut history, played("a", 1000));
        record(&mut history, played("b", 2000));
        record(&mut history, played("a", 3000));
        assert_eq!(uris(&history), ["spotify:album:a", "spotify:album:b"]);
        assert_eq!(history[0].at, 3000);
        // The next day it is a listen of its own
        record(&mut history, played("a", 3000 + DAY));
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn older_than_the_kept_days_is_dropped() {
        let mut history = vec![played("old", 0)];
        record(&mut history, played("new", KEEP_DAYS * DAY));
        assert_eq!(uris(&history), ["spotify:album:new"]);
    }

    #[test]
    fn history_survives_a_save_and_a_broken_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let file = HistoryFile::at(dir.path().join("history.json"));
        assert!(file.load().is_empty());
        file.record(played("a", 10)).unwrap();
        file.record(played("b", 20)).unwrap();
        assert_eq!(uris(&file.load()), ["spotify:album:b", "spotify:album:a"]);
        std::fs::write(dir.path().join("history.json"), "{ broken").unwrap();
        assert!(file.load().is_empty());
    }
}
