//! Playback state that survives restarts: what is saved and how it is restored.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::{
    Action, AlbumPosition, Current, NowPlaying, Playback, QueuedTrack, RadioFetch, RadioPosition,
    State,
};
use crate::player::Item;

/// Playback state that survives restarts (`playback` module): the play order, the
/// playing track's details for the status line, and the position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedPlayback {
    album: Option<AlbumPosition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) radio: Option<RadioPosition>,
    current: Current,
    #[serde(default)]
    played: Vec<QueuedTrack>,
    #[serde(default)]
    queue: Vec<QueuedTrack>,
    /// `None` if the player had not yet reported the track's details.
    now_playing: Option<SavedTrack>,
    #[serde(rename = "position_ms", with = "super::millis")]
    pub(super) position: Duration,
}

/// The playing track's details for the status line, even before the player has loaded it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SavedTrack {
    #[serde(default)]
    uri: String,
    artist: String,
    album: String,
    track: String,
    #[serde(rename = "duration_ms", with = "super::millis")]
    duration: Duration,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cover: Option<String>,
}

impl State {
    /// Playback state to save, with position; `None` when nothing is playing.
    pub fn playback(&self, now: Instant) -> Option<SavedPlayback> {
        let mut saved = self.unpositioned_playback()?;
        if let Some(playing) = &self.now_playing {
            saved.position = playing.elapsed(now);
        }
        Some(saved)
    }

    /// Whether the playback state has changed since the last call (or the restore), apart
    /// from the position: the track changed, the queue changed or playback ended. It is
    /// saved then, so that not even a crash loses the state. The event loop calls this
    /// after every event, so the state is only built if some event may have changed it
    /// (`playback_dirty`).
    pub fn playback_changed(&mut self) -> bool {
        if !std::mem::take(&mut self.playback_dirty) {
            return false;
        }
        let playback = self.unpositioned_playback();
        if playback == self.saved {
            return false;
        }
        self.saved = playback;
        true
    }

    fn unpositioned_playback(&self) -> Option<SavedPlayback> {
        let playback = &self.playback;
        Some(SavedPlayback {
            album: playback.album.clone(),
            radio: playback.radio.clone(),
            current: playback.current.clone()?,
            played: playback.played.clone(),
            queue: playback.queue.clone(),
            now_playing: self.now_playing.as_ref().map(|playing| SavedTrack {
                uri: playing.uri.clone(),
                artist: playing.artist.clone(),
                album: playing.album.clone(),
                track: playing.track.clone(),
                duration: playing.duration,
                cover: playing.cover.clone(),
            }),
            position: Duration::ZERO,
        })
    }

    /// Restores the previous session's playback state: the track shows on the status line
    /// right away, paused at the saved position, and the player loads it at that position.
    pub(super) fn restore(&mut self, saved: SavedPlayback, now: Instant) -> Vec<Action> {
        let item = match &saved.current {
            Current::Album => saved
                .album
                .as_ref()
                .filter(|album| album.index < album.len)
                .map(|album| Item::Album {
                    uri: album.uri.clone(),
                    track: album.index,
                }),
            Current::Radio => saved
                .radio
                .as_ref()
                .and_then(|radio| radio.tracks.get(radio.index))
                .map(|track| Item::Track(track.uri.clone())),
            Current::Queued(track) => Some(Item::Track(track.uri.clone())),
        };
        let Some(item) = item else {
            self.error = Some("could not restore playback: the saved state is invalid".into());
            return Vec::new();
        };
        let SavedPlayback {
            album,
            radio,
            current,
            played,
            queue,
            now_playing,
            position,
        } = saved;
        self.now_playing = now_playing.map(|track| NowPlaying {
            uri: track.uri,
            artist: track.artist,
            album: track.album,
            track: track.track,
            duration: track.duration,
            cover: track.cover,
            paused: true,
            position: position.min(track.duration),
            position_at: now,
        });
        self.playback = Playback {
            current: Some(current),
            album,
            radio,
            played,
            queue,
        };
        self.restoring = true;
        self.radio_fetch = RadioFetch::default();
        self.saved = self.unpositioned_playback();
        let mut actions = vec![Action::Restore { item, position }];
        // The playing and upcoming tracks. Radio tracks already played are not asked about,
        // so that a long radio does not take many queries.
        let playing = self.now_playing.iter().map(|playing| playing.uri.as_str());
        let queue = self.playback.queue.iter().map(|track| track.uri.as_str());
        let radio = self
            .playback
            .radio
            .iter()
            .flat_map(|radio| radio.tracks.iter().skip(radio.index))
            .map(|track| track.uri.as_str());
        actions.extend(self.likes.check(playing.chain(queue).chain(radio)));
        actions
    }
}
