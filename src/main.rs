mod app;
mod catalog;
mod cli;
mod config;
mod curate;
mod curate_run;
mod deck;
mod event_loop;
mod genre_cli;
mod genres;
mod history;
mod instance;
mod lastfm;
mod listened;
mod lists;
mod logging;
mod now_playing;
mod picks;
mod playback;
mod player;
mod radio;
mod rate_limit;
mod shelf;
#[cfg(target_os = "linux")]
mod sink;
mod status;
mod store;
mod ui;

use std::{process::ExitCode, time::Duration};

use anyhow::Result;

use crate::{
    catalog::Catalog,
    cli::{GenreCommand, Mode, USAGE},
    config::Config,
    event_loop::run,
    logging::{init_logging, init_stderr_logging},
};

/// How long Deck waits for its threads to end when it quits, before it exits without them.
const SHUTDOWN_LIMIT: Duration = Duration::from_secs(2);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = match Mode::parse(&args) {
        Ok(mode) => mode,
        Err(usage) => return usage_error(usage),
    };
    match mode {
        Mode::Help => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Mode::Version => {
            println!("deck {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        #[cfg(unix)]
        Mode::Deck => {
            if let Some(window) = desktop_window() {
                return open_window(&window);
            }
        }
        Mode::StatusJson => {
            return match status::print() {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("deck: {e:#}");
                    ExitCode::FAILURE
                }
            };
        }
        Mode::Ctl(command) => {
            return match now_playing::send_control(command) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("deck: {e:#}");
                    ExitCode::FAILURE
                }
            };
        }
        _ => {}
    }
    // Now Playing runs Cocoa's event loop on the main thread and needs no tokio.
    let outcome = if mode == Mode::NowPlaying {
        now_playing()
    } else {
        tokio::runtime::Runtime::new()
            .map_err(anyhow::Error::from)
            .and_then(|runtime| {
                let outcome = runtime.block_on(run_mode(mode));
                // librespot joins its player thread when its task is dropped, and a thread
                // stuck in the audio device would hold the exit forever (`SHUTDOWN_LIMIT`).
                runtime.shutdown_timeout(SHUTDOWN_LIMIT);
                outcome
            })
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => match e.downcast_ref::<genre_cli::Usage>() {
            Some(usage) => usage_error(usage.0),
            None => {
                eprintln!("deck: {e:#}");
                ExitCode::FAILURE
            }
        },
    }
}

/// What a desktop calls its window for Deck, which talks to `deck serve` (Nightdrive's does).
#[cfg(unix)]
const WINDOW_COMMAND: &str = "deck-window";

/// A desktop's window for Deck: `deck-window` on `PATH`, in a graphical session. Plain `deck`
/// opens it, as an app's name does; `deck tui` is always the terminal UI.
#[cfg(unix)]
fn desktop_window() -> Option<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        return None;
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(WINDOW_COMMAND))
        .find(|path| {
            path.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

/// Hands over to the window's command, which starts `deck serve` itself when no Deck plays.
#[cfg(unix)]
fn open_window(window: &std::path::Path) -> ExitCode {
    use std::os::unix::process::CommandExt;
    let error = std::process::Command::new(window).exec();
    eprintln!("deck: {}: {error}", window.display());
    ExitCode::FAILURE
}

/// Prints the usage to stderr; exit code 2 as with invalid arguments.
fn usage_error(usage: &str) -> ExitCode {
    eprintln!("{}", usage.trim_end());
    ExitCode::from(2)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn now_playing() -> Result<()> {
    now_playing::serve()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn now_playing() -> Result<()> {
    anyhow::bail!("now-playing is only available on macOS and Linux")
}

async fn run_mode(mode: Mode) -> Result<()> {
    match mode {
        // The lock first: a Deck that is already running keeps its log.
        Mode::Deck | Mode::Tui => {
            let _lock = instance::lock()?;
            init_logging();
            run(false).await
        }
        Mode::Serve => {
            let _lock = instance::lock()?;
            init_logging();
            run(true).await
        }
        Mode::Taste => {
            init_stderr_logging();
            curate::taste().await
        }
        Mode::SearchJson(query) => {
            init_stderr_logging();
            let catalog = Catalog::connect_stored(&Config::load()?).await?;
            let found = catalog.search(&query).await?;
            let artists: Vec<_> = found
                .artists
                .iter()
                .map(|a| serde_json::json!({ "id": a.id, "name": a.name }))
                .collect();
            let songs: Vec<_> = found
                .songs
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "uri": s.track.uri, "name": s.track.name, "artist": s.track.artist,
                        "duration_ms": s.track.duration.as_millis() as u64, "album": s.album,
                    })
                })
                .collect();
            let json =
                serde_json::json!({ "artists": artists, "albums": found.albums, "songs": songs });
            println!("{json}");
            Ok(())
        }
        Mode::ArtistAlbumsJson(artist_id) => {
            init_stderr_logging();
            let catalog = Catalog::connect_stored(&Config::load()?).await?;
            println!(
                "{}",
                serde_json::to_string(&catalog.artist_albums(&artist_id).await?)?
            );
            Ok(())
        }
        Mode::AlbumTracksJson(uri) => {
            init_stderr_logging();
            let catalog = Catalog::connect_stored(&Config::load()?).await?;
            let id = uri.strip_prefix("spotify:album:").unwrap_or(&uri);
            let (album, tracks) = tokio::try_join!(catalog.album(id), catalog.album_tracks(id))?;
            let tracks: Vec<_> = tracks
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "uri": t.uri, "name": t.name, "artist": t.artist,
                        "duration_ms": t.duration.as_millis() as u64,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::json!({ "album": album, "tracks": tracks })
            );
            Ok(())
        }
        Mode::Curate => {
            init_stderr_logging();
            curate_run::run()
        }
        Mode::CurateSubmit { dry_run } => {
            init_stderr_logging();
            curate::submit(dry_run).await
        }
        Mode::Genre(command) => {
            init_stderr_logging();
            match command {
                GenreCommand::Add { dry_run } => genre_cli::add(dry_run).await,
                GenreCommand::Remove(name) => genre_cli::remove(&name),
                GenreCommand::List => genre_cli::list(),
                GenreCommand::Prompt => {
                    print!("{}", genre_cli::PROMPT);
                    Ok(())
                }
            }
        }
        Mode::NowPlaying => now_playing(),
        // Already handled in `main` before tokio.
        Mode::Help | Mode::Version | Mode::Ctl(_) | Mode::StatusJson => Ok(()),
    }
}
