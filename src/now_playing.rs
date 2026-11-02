//! Now Playing and the media keys (⏯ ⏮ ⏭): macOS Now Playing, and MPRIS on Linux.
//!
//! On Linux the helper process registers Deck as the MPRIS player "Deck" (media keys,
//! `playerctl`, desktop widgets), writes the playing track to `$XDG_RUNTIME_DIR/deck/state.json`
//! for panels, and listens on `$XDG_RUNTIME_DIR/deck/control.sock` for commands from
//! `deck ctl <command>` (including `like`, which MPRIS has no concept of).
//!
//! The rest of this text describes the macOS side.
//!
//! macOS does not send media keys to the terminal but to the Now Playing player, so
//! Deck registers itself as a player (`MPRemoteCommandCenter` and
//! `MPNowPlayingInfoCenter`). Commands arrive through the main thread's event loop
//! (CFRunLoop), and Deck's main thread runs tokio, so Now Playing is a helper process of
//! its own: Deck starts itself with `deck now-playing`, writes the playing track's info
//! to its stdin as JSON lines (`Option<Info>`, `null` clears) and reads the keys from
//! its stdout as lines (`Command`). The helper exits when its stdin closes.
//!
//! macOS gives the keys to the player that most recently started playing. If Spotify or
//! Music plays after Deck, the keys move to it, and they come back to Deck when Deck
//! starts playing again. While paused, Deck keeps the keys, so ⏯ resumes playback.
//!
//! The info is sent only when something other than the position changes, or the
//! position differs from the predicted one (`DRIFT`): macOS works out the position
//! itself from the playback rate.

use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

mod control;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod mac;

pub use control::{Command, Control};
#[cfg(target_os = "linux")]
pub use linux::{runtime_dir, serve};
#[cfg(target_os = "macos")]
pub use mac::serve;

/// How far the position may drift from the predicted one before it is sent again.
const DRIFT: Duration = Duration::from_secs(2);

/// A helper that ended is started again no sooner than this after its last start, so one
/// that dies at once doesn't spin.
const HELPER_RETRY: Duration = Duration::from_secs(5);

/// A failed cover image download is retried no sooner than this (with the next
/// update), so that while the network is down, not every update waits for the
/// download timeout.
const COVER_RETRY: Duration = Duration::from_secs(30);

/// The playing track's info for Now Playing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Info {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub position_ms: u64,
    pub playing: bool,
    /// Cover image URL (`https://i.scdn.co/image/…`).
    pub cover: Option<String>,
    /// Whether the track is liked; `None` when likes are not available.
    #[serde(default)]
    pub liked: Option<bool>,
    /// The playing track's URI, and its album's when it plays from an album (also from the
    /// queue): a window marks them in its lists.
    #[serde(default)]
    pub track_uri: Option<String>,
    #[serde(default)]
    pub album_uri: Option<String>,
    /// The playing track came from the queue (the album or radio goes on after the queue).
    #[serde(default)]
    pub queued: bool,
    /// What plays next, in order (the playing track is not included).
    #[serde(default)]
    pub queue: Vec<QueuedInfo>,
    /// The playing genre radio, when it is the playback context.
    #[serde(default)]
    pub radio: Option<RadioInfo>,
    /// Deck's latest error (a failed search, an empty station…), for a window to show.
    #[serde(default)]
    pub error: Option<String>,
}

/// A queued track in [`Info`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedInfo {
    pub uri: String,
    pub title: String,
    pub artist: String,
    pub duration_ms: u64,
    /// The track's album, for its cover.
    #[serde(default)]
    pub album_uri: String,
}

/// The playing genre radio in [`Info`]: its tracks, from the playing one on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadioInfo {
    /// The genre's id and name.
    pub genre: String,
    pub name: String,
    pub tracks: Vec<RadioTrackInfo>,
}

/// A radio track in [`RadioInfo`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadioTrackInfo {
    pub uri: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_uri: String,
}
/// What the helper passes on: the keys and `deck ctl` commands.
type OnControl = Arc<dyn Fn(Control) + Send + Sync>;

/// Deck's side: the helper process that is told the playing track and sends the keys.
pub struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    /// The info sent last, and when it was sent.
    sent: Option<(Option<Info>, Instant)>,
    on_control: OnControl,
    /// When the helper was started last (`HELPER_RETRY`).
    started: Instant,
}

impl Session {
    /// Starts the helper process (`deck now-playing`). Keys are read in a thread of
    /// their own and passed to `on_command`.
    pub fn start(on_control: impl Fn(Control) + Send + Sync + 'static) -> Result<Self> {
        let on_control: OnControl = Arc::new(on_control);
        let (child, stdin) = spawn_helper(on_control.clone())?;
        Ok(Self {
            stdin,
            child,
            sent: None,
            on_control,
            started: Instant::now(),
        })
    }

    /// Reports the playing track (`None`: nothing is playing) if it has changed.
    pub fn update(&mut self, info: Option<Info>, now: Instant) {
        self.restart_if_gone(now);
        if !changed(self.sent.as_ref(), info.as_ref(), now) {
            return;
        }
        let Some(stdin) = &mut self.stdin else { return };
        let written = serde_json::to_string(&info)
            .map_err(anyhow::Error::from)
            .and_then(|line| Ok(writeln!(stdin, "{line}")?));
        if let Err(e) = written {
            // The helper crashed: Deck works without Now Playing.
            log::warn!("now-playing stopped: {e:#}");
            self.stdin = None;
        }
        self.sent = Some((info, now));
    }

    /// Starts the helper again if it has ended (killed, or crashed) while Deck plays on.
    /// Without it nothing reaches Deck: on Linux its control socket and state file go
    /// with it, so the media keys and a window can't, and a window takes Deck for gone
    /// and starts another.
    fn restart_if_gone(&mut self, now: Instant) {
        let ended = matches!(self.child.try_wait(), Ok(Some(_)));
        if !(ended || self.stdin.is_none())
            || now.saturating_duration_since(self.started) < HELPER_RETRY
        {
            return;
        }
        self.started = now;
        if !ended {
            // Alive but no longer listening (a write failed): replaced.
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        match spawn_helper(self.on_control.clone()) {
            Ok((child, stdin)) => {
                log::warn!("now-playing had ended: started again");
                self.child = child;
                self.stdin = stdin;
                // The new helper knows nothing yet.
                self.sent = None;
            }
            Err(e) => log::warn!("{e:#}"),
        }
    }
}

/// Starts `deck now-playing`, whose stdout lines (keys and commands) go to `on_control`
/// from a thread of their own.
fn spawn_helper(on_control: OnControl) -> Result<(Child, Option<ChildStdin>)> {
    let exe = std::env::current_exe().context("cannot find the deck executable")?;
    let mut child = std::process::Command::new(exe)
        .arg("now-playing")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("cannot start now-playing")?;
    let stdout = child.stdout.take().context("now-playing has no stdout")?;
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(control) = Control::parse(&line) {
                on_control(control);
            }
        }
    });
    let stdin = child.stdin.take();
    Ok((child, stdin))
}

impl Drop for Session {
    /// Stops the helper process, which clears Now Playing. Killed directly rather than
    /// by closing stdin, because a cover image download may delay reading.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // The killed helper could not clean up: a socket left behind would look like a running Deck.
        #[cfg(target_os = "linux")]
        linux::remove_files_of(self.child.id());
    }
}

/// Whether the info must be sent: something other than the position changed, or the
/// position differs from the one predicted from the sent info by more than `DRIFT`
/// (e.g. ← to the start of the same track).
fn changed(sent: Option<&(Option<Info>, Instant)>, info: Option<&Info>, now: Instant) -> bool {
    let Some((sent, at)) = sent else {
        return info.is_some();
    };
    let (sent, info) = match (sent, info) {
        (None, None) => return false,
        (Some(sent), Some(info)) => (sent, info),
        _ => return true,
    };
    let unpositioned = |info: &Info| Info {
        position_ms: 0,
        ..info.clone()
    };
    if unpositioned(sent) != unpositioned(info) {
        return true;
    }
    let mut expected = Duration::from_millis(sent.position_ms);
    if sent.playing {
        expected += now.saturating_duration_since(*at);
    }
    let expected = expected.min(Duration::from_millis(sent.duration_ms));
    expected.abs_diff(Duration::from_millis(info.position_ms)) > DRIFT
}

/// Whether to download the cover image `url`: it is not already loaded (`loaded`),
/// and its download did not fail just now (`failed`, see [`COVER_RETRY`]).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn should_download(
    url: &str,
    loaded: Option<&str>,
    failed: Option<&(String, Instant)>,
    now: Instant,
) -> bool {
    if loaded == Some(url) {
        return false;
    }
    match failed {
        Some((failed_url, at)) if failed_url == url => {
            now.saturating_duration_since(*at) >= COVER_RETRY
        }
        _ => true,
    }
}

/// `deck ctl <command>`: tells the running Deck what to do (Linux: through the control
/// socket of the Now Playing helper).
#[cfg(target_os = "linux")]
pub fn send_control(control: Control) -> Result<()> {
    linux::send_control(control)
}

#[cfg(not(target_os = "linux"))]
pub fn send_control(_control: Control) -> Result<()> {
    anyhow::bail!("deck ctl is only available on Linux")
}

/// Whether another Deck answers on the control socket (Linux; elsewhere always `false`).
#[cfg(target_os = "linux")]
pub fn running() -> bool {
    std::os::unix::net::UnixStream::connect(linux::socket_path()).is_ok()
}

#[cfg(not(target_os = "linux"))]
pub fn running() -> bool {
    false
}

#[cfg(test)]
mod tests;
