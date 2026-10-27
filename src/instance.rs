//! One Deck at a time (Linux): the player, `deck serve` or Deck in a terminal, holds a lock
//! on `$XDG_RUNTIME_DIR/deck/deck.lock` from its very start until it ends.
//!
//! The control socket and the state file can't tell on their own whether a Deck runs: they
//! appear only once Deck has signed in and its Now Playing helper runs, and they go with
//! the helper. Two Decks started a few seconds apart both got past them, and so did one
//! started while another was left without its helper: both played, each on its own audio
//! stream, and only the newest could be reached. The kernel releases the lock when the
//! process ends, however it ends. Nightdrive's `deck-window` tests it too (`flock -n`).

use std::fs::File;

use anyhow::Result;

/// Takes the lock, or fails if another Deck holds it. The lock is held while the file
/// is open.
#[cfg(target_os = "linux")]
pub fn lock() -> Result<Option<File>> {
    use std::os::unix::fs::PermissionsExt;

    use anyhow::Context;

    let dir = crate::now_playing::runtime_dir();
    std::fs::create_dir_all(&dir).context("cannot create the runtime directory")?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    lock_at(&dir.join("deck.lock")).map(Some)
}

#[cfg(target_os = "linux")]
fn lock_at(path: &std::path::Path) -> Result<File> {
    use std::fs::TryLockError;

    use anyhow::Context;

    let file = File::create(path).with_context(|| format!("cannot open {}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => {
            anyhow::bail!("Deck is already running (`deck ctl quit` ends it)")
        }
        Err(TryLockError::Error(e)) => {
            Err(e).with_context(|| format!("cannot lock {}", path.display()))
        }
    }
}

/// Elsewhere Deck has no control socket to fight over: no lock.
#[cfg(not(target_os = "linux"))]
pub fn lock() -> Result<Option<File>> {
    Ok(None)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn a_second_deck_is_refused_until_the_first_ends() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deck.lock");
        let first = lock_at(&path).unwrap();
        let refused = lock_at(&path).unwrap_err();
        assert!(
            refused.to_string().contains("already running"),
            "{refused:#}"
        );
        drop(first);
        assert!(lock_at(&path).is_ok());
    }
}
