//! Deck's log: a file while the UI is open (with stderr sent there too), stderr for the
//! subcommands.

use std::sync::OnceLock;

use anyhow::{Context, Result};

use crate::config;

/// The log goes to `~/.cache/deck/deck.log`, because stderr would mess up the UI.
/// The level can be set with `RUST_LOG`. The log of the previous start is kept as
/// `deck.log.1`, so the cause of a crash can still be seen after a restart.
pub(super) fn init_logging() {
    let file = config::private_cache_dir().and_then(|dir| open_log(&dir.join("deck.log")));
    let mut builder =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"));
    match file {
        Ok(file) => {
            // A clone shares the file position, so the two writers don't overwrite each other.
            if let Ok(copy) = file.try_clone() {
                let _ = LOG_FILE.set(copy);
            }
            builder.target(env_logger::Target::Pipe(Box::new(file)))
        }
        Err(_) => builder.filter_level(log::LevelFilter::Off),
    };
    builder.init();
}

/// The log file for `StderrToLog`, set by `init_logging`.
static LOG_FILE: OnceLock<std::fs::File> = OnceLock::new();

/// Sends stderr to the log while the UI is open. C libraries write straight to stderr,
/// past `log` (alsa-lib's "underrun occurred", for one), and the lines would land in the
/// middle of the UI. Dropping it brings stderr back, so an error after the UI is seen.
pub(super) struct StderrToLog(#[cfg(unix)] Option<std::os::fd::OwnedFd>);

impl StderrToLog {
    #[cfg(unix)]
    pub(super) fn start() -> Self {
        use std::os::fd::{AsFd, AsRawFd};
        let Some(log) = LOG_FILE.get() else {
            return Self(None);
        };
        let Ok(saved) = std::io::stderr().as_fd().try_clone_to_owned() else {
            return Self(None);
        };
        // SAFETY: both descriptors are open; dup2 only replaces fd 2.
        if unsafe { libc::dup2(log.as_raw_fd(), libc::STDERR_FILENO) } == -1 {
            return Self(None);
        }
        Self(Some(saved))
    }

    #[cfg(not(unix))]
    pub(super) fn start() -> Self {
        Self()
    }
}

#[cfg(unix)]
impl Drop for StderrToLog {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        if let Some(saved) = self.0.take() {
            // SAFETY: `saved` is open until the end of this block.
            unsafe { libc::dup2(saved.as_raw_fd(), libc::STDERR_FILENO) };
        }
    }
}

/// Moves the previous log to `<log>.1` (over the older one) and opens a new one.
fn open_log(path: &std::path::Path) -> Result<std::fs::File> {
    let mut previous = path.as_os_str().to_owned();
    previous.push(".1");
    match std::fs::rename(path, &previous) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(anyhow::Error::from(e).context(format!("cannot rotate {}", path.display())));
        }
        _ => {}
    }
    std::fs::File::create(path).with_context(|| format!("cannot create {}", path.display()))
}

/// Subcommands log to stderr, so the log of a running Deck stays intact.
pub(super) fn init_stderr_logging() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_of_the_previous_start_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deck.log");
        std::fs::write(&path, "oldest").unwrap();
        open_log(&path).unwrap();
        std::fs::write(&path, "previous").unwrap();
        open_log(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("deck.log.1")).unwrap(),
            "previous"
        );
        // The first time there is no previous log.
        let fresh = dir.path().join("new.log");
        open_log(&fresh).unwrap();
        assert!(fresh.exists());
    }
}
