//! `deck status --json`: what the running Deck plays, for scripts and assistants that have no
//! window of their own. It reads the state file the Now Playing helper writes (`now_playing`),
//! so it needs no Spotify session and makes no Web API request.
//!
//! The state file is written only when something other than the position changes, so its
//! `position_ms` is the position at `updated_ms`: while playing, the position now is worked out
//! from the time since then.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::now_playing::Info;

/// The state file as the helper writes it.
#[derive(Deserialize)]
struct StateFile {
    info: Option<Info>,
    #[serde(default)]
    updated_ms: u64,
}

/// Prints the status as one line of JSON.
pub fn print() -> Result<()> {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    println!(
        "{}",
        report(crate::now_playing::running(), read_state(), now_ms)
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn read_state() -> Option<String> {
    std::fs::read_to_string(crate::now_playing::runtime_dir().join("state.json")).ok()
}

#[cfg(not(target_os = "linux"))]
fn read_state() -> Option<String> {
    None
}

/// The status: `{"running": false}` when no Deck answers, `{"running": true, "playing": false}`
/// when it has nothing loaded, else the track, where it is, the like, the queue and the radio.
/// A state file left by a Deck that was killed is not read: `running` comes from the socket.
fn report(running: bool, state: Option<String>, now_ms: u64) -> Value {
    if !running {
        return json!({ "running": false });
    }
    let state = state.and_then(|s| serde_json::from_str::<StateFile>(&s).ok());
    let Some(StateFile {
        info: Some(info),
        updated_ms,
    }) = state
    else {
        return json!({ "running": true, "playing": false });
    };
    let mut position_ms = info.position_ms;
    if info.playing {
        position_ms = (position_ms + now_ms.saturating_sub(updated_ms)).min(info.duration_ms);
    }
    // The radio's tracks are left out: only its name tells what plays; its next tracks are
    // many and say little.
    let radio = info
        .radio
        .as_ref()
        .map(|r| json!({ "genre": r.genre, "name": r.name }));
    json!({
        "running": true,
        "playing": info.playing,
        "title": info.title,
        "artist": info.artist,
        "album": info.album,
        "track_uri": info.track_uri,
        "album_uri": info.album_uri,
        "position_ms": position_ms,
        "duration_ms": info.duration_ms,
        "liked": info.liked,
        "queued": info.queued,
        "queue": info.queue,
        "radio": radio,
        "error": info.error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(playing: bool, position_ms: u64, updated_ms: u64) -> String {
        json!({
            "info": {
                "title": "Blue Eyes", "artist": "Destroyer", "album": "Kaputt",
                "duration_ms": 247_000, "position_ms": position_ms, "playing": playing,
                "cover": null, "liked": true,
                "track_uri": "spotify:track:t", "album_uri": "spotify:album:a",
                "queue": [{ "uri": "spotify:track:q", "title": "Q", "artist": "A",
                            "duration_ms": 1000, "album_uri": "spotify:album:q" }],
                "radio": { "genre": "synthwave", "name": "Synthwave", "tracks": [] },
            },
            "updated_ms": updated_ms,
            "pid": 1,
        })
        .to_string()
    }

    #[test]
    fn not_running_says_only_that() {
        let status = report(false, Some(state(true, 0, 0)), 5000);
        assert_eq!(status, json!({ "running": false }));
    }

    #[test]
    fn nothing_loaded_is_running_but_not_playing() {
        let empty = json!({ "info": null, "updated_ms": 1, "pid": 1 }).to_string();
        let expected = json!({ "running": true, "playing": false });
        assert_eq!(report(true, Some(empty), 5000), expected);
        assert_eq!(report(true, None, 5000), expected);
        assert_eq!(report(true, Some("{".into()), 5000), expected);
    }

    #[test]
    fn playing_position_moves_on_from_the_state_file() {
        let status = report(true, Some(state(true, 10_000, 1_000)), 4_500);
        assert_eq!(status["position_ms"], 13_500);
        assert_eq!(status["title"], "Blue Eyes");
        assert_eq!(status["liked"], true);
        assert_eq!(status["queue"][0]["uri"], "spotify:track:q");
        assert_eq!(
            status["radio"],
            json!({ "genre": "synthwave", "name": "Synthwave" })
        );
    }

    #[test]
    fn paused_position_stays_and_playing_stops_at_the_end() {
        let paused = report(true, Some(state(false, 10_000, 1_000)), 900_000);
        assert_eq!(paused["position_ms"], 10_000);
        let ended = report(true, Some(state(true, 10_000, 1_000)), 900_000);
        assert_eq!(ended["position_ms"], 247_000);
    }
}
