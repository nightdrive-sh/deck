//! The Linux helper: MPRIS, the control socket of `deck ctl` and the state file for panels.

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
};

use super::{Command, Control, Info};

/// `$XDG_RUNTIME_DIR/deck`: the control socket, the state file and the lock (`instance`)
/// live here.
pub fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("deck")
}

pub(super) fn socket_path() -> PathBuf {
    runtime_dir().join("control.sock")
}

fn state_path() -> PathBuf {
    runtime_dir().join("state.json")
}

/// The client side of `deck ctl`.
pub fn send_control(control: Control) -> Result<()> {
    let mut stream =
        UnixStream::connect(socket_path()).context("Deck is not running (no control socket)")?;
    writeln!(stream, "{}", control.line()).context("cannot send the command to Deck")
}

/// `deck now-playing` on Linux: registers the MPRIS player, serves the control
/// socket and writes the state file. Info arrives on stdin; commands go to stdout.
pub fn serve() -> Result<()> {
    // A socket that answers is another Deck's: taking it over would leave that one out
    // of reach.
    if super::running() {
        anyhow::bail!("another Deck answers on {}", socket_path().display());
    }
    let dir = runtime_dir();
    std::fs::create_dir_all(&dir).context("cannot create the runtime directory")?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;

    // All commands (MPRIS and the socket) go to Deck as lines on stdout.
    let stdout = Arc::new(Mutex::new(std::io::stdout()));
    let send = {
        let stdout = Arc::clone(&stdout);
        move |control: Control| {
            if let Ok(mut out) = stdout.lock() {
                let _ = writeln!(out, "{}", control.line());
                let _ = out.flush();
            }
        }
    };

    let mut controls = MediaControls::new(PlatformConfig {
        dbus_name: "deck",
        display_name: "Deck",
        hwnd: None,
    })
    .map_err(|e| anyhow::anyhow!("cannot register the MPRIS player: {e:?}"))?;
    let mpris_send = send.clone();
    controls
        .attach(move |event| {
            let command = match event {
                MediaControlEvent::Play => Command::Play,
                MediaControlEvent::Pause => Command::Pause,
                MediaControlEvent::Toggle => Command::Toggle,
                MediaControlEvent::Next => Command::Next,
                MediaControlEvent::Previous => Command::Previous,
                _ => return,
            };
            mpris_send(Control::Media(command));
        })
        .map_err(|e| anyhow::anyhow!("cannot attach the MPRIS handler: {e:?}"))?;

    // `deck ctl` commands. One that doesn't answer is left behind by a killed helper.
    let _ = std::fs::remove_file(socket_path());
    let listener = UnixListener::bind(socket_path()).context("cannot open the control socket")?;
    let socket_send = send.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                if let Some(control) = Control::parse(&line) {
                    socket_send(control);
                }
            }
        }
    });

    // Deck is running (nothing playing yet): panels see an empty state instead of no file.
    write_state(None);

    // Info from Deck: one JSON line per change (`null` clears). Ends when Deck closes stdin.
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(info) = serde_json::from_str::<Option<Info>>(&line) else {
            continue;
        };
        apply(&mut controls, info.as_ref());
        write_state(info.as_ref());
    }
    remove_files_of(std::process::id());
    Ok(())
}

/// Removes the control socket and the state file if they are the helper `pid`'s (the
/// state file names it): another helper's, which took them over, stay.
pub(super) fn remove_files_of(pid: u32) {
    let owner = std::fs::read(state_path())
        .ok()
        .and_then(|json| serde_json::from_slice::<serde_json::Value>(&json).ok())
        .and_then(|state| state["pid"].as_u64());
    if owner == Some(u64::from(pid)) {
        let _ = std::fs::remove_file(socket_path());
        let _ = std::fs::remove_file(state_path());
    }
}

fn apply(controls: &mut MediaControls, info: Option<&Info>) {
    let Some(info) = info else {
        let _ = controls.set_playback(MediaPlayback::Stopped);
        let _ = controls.set_metadata(MediaMetadata::default());
        return;
    };
    let _ = controls.set_metadata(MediaMetadata {
        title: Some(&info.title),
        artist: Some(&info.artist),
        album: Some(&info.album),
        cover_url: info.cover.as_deref(),
        duration: Some(Duration::from_millis(info.duration_ms)),
    });
    let progress = Some(MediaPosition(Duration::from_millis(info.position_ms)));
    let _ = controls.set_playback(if info.playing {
        MediaPlayback::Playing { progress }
    } else {
        MediaPlayback::Paused { progress }
    });
}

/// Writes `state.json` (`{"info": …, "updated_ms": …, "pid": …}`) atomically, for panels.
fn write_state(info: Option<&Info>) {
    let updated_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    // `pid` lets readers tell a stale file (Deck was killed and could not clean up) from a live one.
    let json =
        serde_json::json!({ "info": info, "updated_ms": updated_ms, "pid": std::process::id() });
    let tmp = runtime_dir().join("state.json.tmp");
    if std::fs::write(&tmp, json.to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, state_path());
    }
}
