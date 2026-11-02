//! What the running Deck can be told ([`Control`], with the media keys as [`Command`]),
//! and its one-line form on the control socket and the helper's stdout.

use std::path::PathBuf;

/// A media key or a Control Center button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Toggle,
    Play,
    Pause,
    Next,
    Previous,
    /// Likes the playing track or removes the like (Linux `deck ctl like`).
    ToggleLike,
}

impl Command {
    pub(super) const ALL: [Self; 6] = [
        Self::Toggle,
        Self::Play,
        Self::Pause,
        Self::Next,
        Self::Previous,
        Self::ToggleLike,
    ];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Toggle => "toggle",
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Next => "next",
            Self::Previous => "previous",
            Self::ToggleLike => "like",
        }
    }

    pub fn parse(line: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.name() == line.trim())
    }
}

/// What the running Deck can be told: a media key, an album to play (`deck ctl play-album <uri>`),
/// a search to open (`deck ctl search <query>`), e.g. from a desktop launcher, a saved playback
/// state to go back to (`deck ctl restore <file>`), or what a window built on `deck serve` needs:
/// the queue, the shelf and quitting. It travels as one line over the control socket and the
/// helper's stdout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Control {
    Media(Command),
    PlayAlbum(String),
    /// A track and its album: the album plays from the track (`deck ctl play-song`).
    PlaySong {
        track: String,
        album: String,
    },
    Search(String),
    /// A copy of `playback.json`, by its absolute path: the running Deck may have another
    /// working directory than the command that asks.
    Restore(PathBuf),
    /// Write the playback state, with the position now, to this file (absolute, as for
    /// `Restore`), for going back to it later (`deck ctl save <file>`).
    Save(PathBuf),
    /// Add the album to the end of the queue (`deck ctl queue-album <uri>`).
    QueueAlbum(String),
    /// Add one track of the album to the end of the queue (`deck ctl queue-song <track> <album>`).
    QueueSong {
        track: String,
        album: String,
    },
    /// Remove the queue's track at this position, from 0 (`deck ctl unqueue <n>`).
    Unqueue(usize),
    /// Empty the queue (`deck ctl queue-clear`).
    ClearQueue,
    /// Pick for me: an album from the shelf, Curated or both (`deck ctl pick [shelf|curated]`).
    Pick(crate::picks::PickFrom),
    /// Put the album on the shelf, or take it off (`deck ctl shelf-add|shelf-remove <uri>`).
    ShelfAdd(String),
    ShelfRemove(String),
    /// Jump to this position (ms) in the playing track (`deck ctl seek <ms>`).
    Seek(u64),
    /// Start the genre's radio (`deck ctl radio <genre id>`).
    Radio(String),
    /// Add the genre (id) to favourites, or remove it (`deck ctl favorite-add|favorite-remove <id>`).
    FavoriteAdd(String),
    FavoriteRemove(String),
    /// Reject the album (URI) from Curated (`deck ctl curated-reject <uri>`).
    RejectCurated(String),
    /// Save the playback state and exit (`deck ctl quit`), e.g. to end `deck serve`.
    Quit,
}

impl Control {
    pub fn parse(line: &str) -> Option<Self> {
        let line = line.trim();
        if let Some(uri) = line.strip_prefix("play-album ") {
            let uri = uri.trim();
            return uri
                .starts_with("spotify:album:")
                .then(|| Self::PlayAlbum(uri.to_owned()));
        }
        if let Some(rest) = line.strip_prefix("play-song ") {
            let mut uris = rest.split_whitespace();
            return match (uris.next(), uris.next(), uris.next()) {
                (Some(track), Some(album), None)
                    if track.starts_with("spotify:track:")
                        && album.starts_with("spotify:album:") =>
                {
                    Some(Self::PlaySong {
                        track: track.to_owned(),
                        album: album.to_owned(),
                    })
                }
                _ => None,
            };
        }
        if let Some(uri) = line.strip_prefix("queue-album ") {
            return album_uri(uri).map(Self::QueueAlbum);
        }
        if let Some(rest) = line.strip_prefix("queue-song ") {
            return match Self::parse(&format!("play-song {rest}")) {
                Some(Self::PlaySong { track, album }) => Some(Self::QueueSong { track, album }),
                _ => None,
            };
        }
        if let Some(index) = line.strip_prefix("unqueue ") {
            return index.trim().parse().ok().map(Self::Unqueue);
        }
        if let Some(uri) = line.strip_prefix("shelf-add ") {
            return album_uri(uri).map(Self::ShelfAdd);
        }
        if let Some(uri) = line.strip_prefix("shelf-remove ") {
            return album_uri(uri).map(Self::ShelfRemove);
        }
        if let Some(ms) = line.strip_prefix("seek ") {
            return ms.trim().parse().ok().map(Self::Seek);
        }
        if let Some(id) = line.strip_prefix("radio ") {
            return genre_id(id).map(Self::Radio);
        }
        if let Some(id) = line.strip_prefix("favorite-add ") {
            return genre_id(id).map(Self::FavoriteAdd);
        }
        if let Some(id) = line.strip_prefix("favorite-remove ") {
            return genre_id(id).map(Self::FavoriteRemove);
        }
        if let Some(uri) = line.strip_prefix("curated-reject ") {
            return album_uri(uri).map(Self::RejectCurated);
        }
        if line == "quit" {
            return Some(Self::Quit);
        }
        if line == "queue-clear" {
            return Some(Self::ClearQueue);
        }
        if line == "pick" || line.starts_with("pick ") {
            return crate::picks::PickFrom::parse(line["pick".len()..].trim()).map(Self::Pick);
        }
        if let Some(path) = line.strip_prefix("restore ") {
            let path = PathBuf::from(path.trim());
            return path.is_absolute().then_some(Self::Restore(path));
        }
        if let Some(path) = line.strip_prefix("save ") {
            let path = PathBuf::from(path.trim());
            return path.is_absolute().then_some(Self::Save(path));
        }
        if let Some(query) = line.strip_prefix("search ") {
            let query = query.trim();
            return (!query.is_empty()).then(|| Self::Search(query.to_owned()));
        }
        Command::parse(line).map(Self::Media)
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(super) fn line(&self) -> String {
        match self {
            Self::Media(command) => command.name().to_owned(),
            Self::PlayAlbum(uri) => format!("play-album {uri}"),
            Self::PlaySong { track, album } => format!("play-song {track} {album}"),
            // One line on the wire: a pasted newline must not split the query.
            Self::Search(query) => format!("search {}", query.replace(['\n', '\r'], " ")),
            Self::Restore(path) => format!("restore {}", path.display()),
            Self::Save(path) => format!("save {}", path.display()),
            Self::QueueAlbum(uri) => format!("queue-album {uri}"),
            Self::QueueSong { track, album } => format!("queue-song {track} {album}"),
            Self::Unqueue(index) => format!("unqueue {index}"),
            Self::ClearQueue => "queue-clear".to_owned(),
            Self::Pick(from) => format!("pick {}", from.word()).trim_end().to_owned(),
            Self::ShelfAdd(uri) => format!("shelf-add {uri}"),
            Self::ShelfRemove(uri) => format!("shelf-remove {uri}"),
            Self::Seek(ms) => format!("seek {ms}"),
            Self::Radio(id) => format!("radio {id}"),
            Self::FavoriteAdd(id) => format!("favorite-add {id}"),
            Self::FavoriteRemove(id) => format!("favorite-remove {id}"),
            Self::RejectCurated(uri) => format!("curated-reject {uri}"),
            Self::Quit => "quit".to_owned(),
        }
    }
}

/// An album URI argument, trimmed; `None` for anything else.
fn album_uri(uri: &str) -> Option<String> {
    let uri = uri.trim();
    uri.starts_with("spotify:album:").then(|| uri.to_owned())
}

/// A genre id argument (one word), trimmed; `None` for anything else.
fn genre_id(id: &str) -> Option<String> {
    let id = id.trim();
    (!id.is_empty() && !id.contains(char::is_whitespace)).then(|| id.to_owned())
}
