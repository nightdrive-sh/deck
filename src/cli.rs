//! The command line: what `deck …` asks for ([`Mode`]) and the usage printed for a
//! wrong one.

use crate::{genre_cli, now_playing};

pub(super) const USAGE: &str = "usage: deck [tui | taste | curate | curate submit [--dry-run] \
                     | genre add [--dry-run] | genre remove <name> | genre list | genre prompt \
                     | serve \
                     | ctl <toggle|play|pause|next|previous|like|quit> \
                     | ctl play-album <uri> | ctl play-song <track uri> <album uri> \
                     | ctl queue-album <uri> | ctl queue-song <track uri> <album uri> \
                     | ctl unqueue <n> | ctl queue-clear | ctl pick [shelf|curated] | ctl shelf-add <uri> | ctl shelf-remove <uri> \
                     | ctl seek <ms> | ctl radio <genre id> \
                     | ctl favorite-add <genre id> | ctl favorite-remove <genre id> \
                     | ctl curated-reject <uri> \
                     | ctl search <query> | ctl save <file> | ctl restore <file> \
                     | status --json | search --json <query> | artist-albums --json <artist id> \
                     | album-tracks --json <album uri> \
                     | --help | --version]";

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Mode {
    /// Plain `deck`: the player and the UI, or a desktop's window for Deck when there is one
    /// ([`desktop_window`](crate::desktop_window)).
    Deck,
    /// `deck tui`: the player and the UI in the terminal, window or not.
    Tui,
    /// `deck serve`: the player without the UI, for a window that talks to it with `deck ctl`
    /// and reads the state file (Linux).
    Serve,
    Taste,
    /// The Curated run (`curate_run`).
    Curate,
    /// What the run's Claude calls: checks candidates and writes the list (`curate`).
    CurateSubmit {
        dry_run: bool,
    },
    Genre(GenreCommand),
    /// The Now Playing helper process Deck starts itself (`now_playing`), not in the help.
    NowPlaying,
    /// `deck ctl <command>`: tells the running Deck what to do (Linux).
    Ctl(now_playing::Control),
    /// `deck status --json`: what the running Deck plays, as JSON (`status`).
    StatusJson,
    /// `deck search --json <query>`: artists and albums as JSON, for desktop launchers.
    SearchJson(String),
    /// `deck artist-albums --json <artist id>`: the artist's albums as JSON.
    ArtistAlbumsJson(String),
    /// `deck album-tracks --json <album uri>`: the album and its tracks as JSON.
    AlbumTracksJson(String),
    /// `--help` / `-h`: usage to stdout.
    Help,
    /// `--version` / `-V`.
    Version,
}

/// `deck genre …` (`genre_cli`).
#[derive(Debug, PartialEq, Eq)]
pub(super) enum GenreCommand {
    Add {
        dry_run: bool,
    },
    /// The name may span several arguments: `deck genre remove 60s rock`.
    Remove(String),
    List,
    Prompt,
}

impl Mode {
    /// The error is the usage to print: `deck genre …` has its own, the rest [`USAGE`].
    pub(super) fn parse(args: &[String]) -> Result<Self, &'static str> {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        match args.as_slice() {
            [] => Ok(Self::Deck),
            ["tui"] => Ok(Self::Tui),
            ["taste"] => Ok(Self::Taste),
            ["curate"] => Ok(Self::Curate),
            ["curate", "submit"] => Ok(Self::CurateSubmit { dry_run: false }),
            ["curate", "submit", "--dry-run"] => Ok(Self::CurateSubmit { dry_run: true }),
            ["genre", "add"] => Ok(Self::Genre(GenreCommand::Add { dry_run: false })),
            ["genre", "add", "--dry-run"] => Ok(Self::Genre(GenreCommand::Add { dry_run: true })),
            ["genre", "remove", name @ ..] if !name.is_empty() => {
                Ok(Self::Genre(GenreCommand::Remove(name.join(" "))))
            }
            ["genre", "list"] => Ok(Self::Genre(GenreCommand::List)),
            ["genre", "prompt"] => Ok(Self::Genre(GenreCommand::Prompt)),
            ["now-playing"] => Ok(Self::NowPlaying),
            ["serve"] => Ok(Self::Serve),
            ["status", "--json"] => Ok(Self::StatusJson),
            ["search", "--json", query @ ..] if !query.is_empty() => {
                Ok(Self::SearchJson(query.join(" ")))
            }
            ["artist-albums", "--json", id] => Ok(Self::ArtistAlbumsJson((*id).to_owned())),
            ["album-tracks", "--json", uri] if uri.starts_with("spotify:album:") => {
                Ok(Self::AlbumTracksJson((*uri).to_owned()))
            }
            // The command as one line, as it travels over the control socket.
            ["ctl", command @ ..] if !command.is_empty() => {
                now_playing::Control::parse(&command.join(" "))
                    .map(Self::Ctl)
                    .ok_or(USAGE)
            }
            ["--help" | "-h"] => Ok(Self::Help),
            ["--version" | "-V"] => Ok(Self::Version),
            ["genre", ..] => Err(genre_cli::USAGE),
            _ => Err(USAGE),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn mode(args: &[&str]) -> Option<Mode> {
        Mode::parse(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>()).ok()
    }

    fn usage(args: &[&str]) -> Option<&'static str> {
        Mode::parse(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>()).err()
    }
    #[test]
    fn modes_from_arguments() {
        assert_eq!(mode(&[]), Some(Mode::Deck));
        assert_eq!(mode(&["tui"]), Some(Mode::Tui));
        assert_eq!(mode(&["taste"]), Some(Mode::Taste));
        assert_eq!(mode(&["curate"]), Some(Mode::Curate));
        assert_eq!(mode(&["now-playing"]), Some(Mode::NowPlaying));
        assert_eq!(mode(&["status", "--json"]), Some(Mode::StatusJson));
        assert_eq!(mode(&["status"]), None);
        assert_eq!(
            mode(&["ctl", "like"]),
            Some(Mode::Ctl(now_playing::Control::Media(
                now_playing::Command::ToggleLike
            )))
        );
        assert_eq!(
            mode(&["ctl", "next"]),
            Some(Mode::Ctl(now_playing::Control::Media(
                now_playing::Command::Next
            )))
        );
        assert_eq!(
            mode(&["ctl", "play-album", "spotify:album:abc"]),
            Some(Mode::Ctl(now_playing::Control::PlayAlbum(
                "spotify:album:abc".to_owned()
            )))
        );
        assert_eq!(mode(&["ctl", "play-album", "spotify:track:abc"]), None);
        assert_eq!(
            mode(&["ctl", "search", "fm-84", "atlas"]),
            Some(Mode::Ctl(now_playing::Control::Search(
                "fm-84 atlas".to_owned()
            )))
        );
        assert_eq!(
            mode(&["ctl", "restore", "/tmp/playback.json"]),
            Some(Mode::Ctl(now_playing::Control::Restore(
                "/tmp/playback.json".into()
            )))
        );
        assert_eq!(mode(&["ctl", "restore", "playback.json"]), None);
        assert_eq!(
            mode(&["ctl", "play-song", "spotify:track:t", "spotify:album:a"]),
            Some(Mode::Ctl(now_playing::Control::PlaySong {
                track: "spotify:track:t".to_owned(),
                album: "spotify:album:a".to_owned(),
            }))
        );
        assert_eq!(mode(&["ctl", "play-song", "spotify:track:t"]), None);
        assert_eq!(mode(&["ctl"]), None);
        assert_eq!(mode(&["ctl", "dance"]), None);
        assert_eq!(
            mode(&["curate", "submit"]),
            Some(Mode::CurateSubmit { dry_run: false })
        );
        assert_eq!(
            mode(&["curate", "submit", "--dry-run"]),
            Some(Mode::CurateSubmit { dry_run: true })
        );
        assert_eq!(mode(&["curate", "--dry-run"]), None);
        assert_eq!(mode(&["curate", "submit", "--wet"]), None);
        assert_eq!(mode(&["taste", "--dry-run"]), None);
        assert_eq!(mode(&["play"]), None);
        assert_eq!(
            mode(&["genre", "add", "--dry-run"]),
            Some(Mode::Genre(GenreCommand::Add { dry_run: true }))
        );
        assert_eq!(
            mode(&["genre", "add"]),
            Some(Mode::Genre(GenreCommand::Add { dry_run: false }))
        );
        assert_eq!(
            mode(&["genre", "remove", "60s", "rock"]),
            Some(Mode::Genre(GenreCommand::Remove("60s rock".into())))
        );
        assert_eq!(mode(&["genre", "remove"]), None);
        assert_eq!(
            mode(&["genre", "list"]),
            Some(Mode::Genre(GenreCommand::List))
        );
        assert_eq!(
            mode(&["genre", "prompt"]),
            Some(Mode::Genre(GenreCommand::Prompt))
        );
        assert_eq!(mode(&["genre"]), None);
        assert_eq!(mode(&["--help"]), Some(Mode::Help));
        assert_eq!(mode(&["-h"]), Some(Mode::Help));
        assert_eq!(mode(&["--version"]), Some(Mode::Version));
        assert_eq!(mode(&["-V"]), Some(Mode::Version));
        assert_eq!(mode(&["--help", "taste"]), None);
    }

    #[test]
    fn genre_commands_get_genre_usage() {
        assert_eq!(usage(&["genre"]), Some(genre_cli::USAGE));
        assert_eq!(usage(&["genre", "play"]), Some(genre_cli::USAGE));
        assert_eq!(usage(&["genre", "remove"]), Some(genre_cli::USAGE));
        assert_eq!(usage(&["genre", "add", "--wet"]), Some(genre_cli::USAGE));
        assert_eq!(usage(&["play"]), Some(USAGE));
        assert_eq!(usage(&["genre", "list"]), None);
    }
}
