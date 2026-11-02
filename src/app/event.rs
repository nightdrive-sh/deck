//! What goes into `State::update` and what comes out: keys, events and actions.

use std::{path::PathBuf, time::Duration};

use crate::{
    catalog::{Album, SearchResults, Track},
    lists::AlbumList,
    now_playing::Command as MediaCommand,
    picks::PickFrom,
    player::{self, Item},
    radio,
};

use super::SavedPlayback;

/// A key, independent of the terminal. `Ctrl` letters are lowercase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Up,
    Down,
    Left,
    Right,
    Enter,
    Esc,
    Backspace,
}

#[derive(Debug)]
pub enum Event {
    Key(Key),
    /// A Mac media key or a Control Center button: works like Space and ← / →
    /// regardless of the view (in search too).
    Media(MediaCommand),
    /// `deck ctl play-album <uri>`: open the album (shelf or Curated) and play it from the start.
    PlayAlbum(String),
    /// `deck ctl play-song <track> <album>`: as `PlayAlbum`, from that track.
    PlaySong {
        track: String,
        album: String,
    },
    /// `deck ctl search <query>`: open the search view with the query and search right away.
    Search(String),
    /// Time passing: fires the debounced search. The loop must send this more often
    /// than `SEARCH_DEBOUNCE`.
    Tick,
    Player(player::Event),
    /// Result of `Action::Search`.
    Searched {
        query: String,
        result: Result<SearchResults, String>,
    },
    /// Result of `Action::LoadAlbumTracks`.
    AlbumTracks {
        uri: String,
        result: Result<Vec<Track>, String>,
    },
    /// Result of `Action::QueueAlbum` and `Action::QueueRemote`: the album's tracks for the
    /// queue, or only `track` (URI) of them.
    AlbumQueued {
        album: Album,
        track: Option<String>,
        result: Result<Vec<Track>, String>,
    },
    /// `deck ctl queue-album <uri>` and `queue-song <track> <album>`: the album, or that
    /// track of it, to the end of the queue.
    QueueRemote {
        album: String,
        track: Option<String>,
    },
    /// `deck ctl unqueue <n>`: remove the queue's track at that position.
    Unqueue(usize),
    /// `deck ctl queue-clear`: empty the queue.
    ClearQueue,
    /// `deck ctl pick [shelf|curated]`: Pick for me, an album not played in this round.
    Pick(PickFrom),
    /// Result of `Action::PickAlbum`: the picked album's tracks.
    AlbumPicked {
        album: Album,
        result: Result<Vec<Track>, String>,
    },
    /// `deck ctl shelf-add|shelf-remove <uri>`: put the album (URI) on the shelf or take it off.
    Shelve {
        uri: String,
        on: bool,
    },
    /// `deck ctl seek <ms>`: jump to that position in the playing track.
    Seek(Duration),
    /// `deck ctl radio <genre id>`: start the genre's radio.
    Radio(String),
    /// `deck ctl favorite-add|favorite-remove <id>`: the genre on the home page, or off it.
    Favorite {
        id: String,
        on: bool,
    },
    /// `deck ctl curated-reject <uri>`: the album out of Curated, as Ctrl+D there.
    RejectCurated(String),
    /// `deck ctl quit`.
    Quit,
    /// Result of `Action::FetchAlbum`.
    AlbumFetched {
        uri: String,
        result: Result<Album, String>,
    },
    /// Result of `Action::LoadArtistAlbums`.
    ArtistAlbums {
        artist_id: String,
        result: Result<Vec<Album>, String>,
    },
    /// The shelf's albums after a successful add or removal.
    ShelfChanged(Vec<Album>),
    /// Result of `Action::LoadLists`: the Curated list, `None` when there is no file.
    ListsLoaded(Result<Option<AlbumList>, String>),
    /// The Curated list after a successful rejection (`Action::RejectCurated`).
    CuratedChanged(Option<AlbumList>),
    /// The previous session's playback state, read at startup.
    Restore(Box<SavedPlayback>),
    /// A copy of the playback state to go back to (`deck ctl restore <file>`): as `Restore`,
    /// and then saved, since Deck's own file still holds what played before.
    RestoreCopy(Box<SavedPlayback>),
    /// Write the playback state, with the position now, to this file (`deck ctl save <file>`),
    /// e.g. for an undo that goes back to it with `deck ctl restore`.
    SavePlayback(PathBuf),
    /// Result of `Action::LoadRadio`: the station's page as is.
    RadioLoaded {
        request: RadioRequest,
        result: Result<Vec<radio::Track>, String>,
    },
    /// Result of `Action::AddRadioAlbumToShelf`: the album with its year, for the shelf.
    RadioAlbum(Result<Album, String>),
    /// Favourite genres (ids) after a successful add or removal.
    FavoritesChanged(Vec<String>),
    /// Result of `Action::CheckLiked`: liked or not, in the same order.
    LikedChecked {
        uris: Vec<String>,
        result: Result<Vec<bool>, String>,
    },
    /// Result of `Action::SetLiked`.
    LikeChanged {
        uri: String,
        liked: bool,
        result: Result<(), String>,
    },
    /// An action failed (e.g. saving the shelf).
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Play(Item),
    Preload(Item),
    /// Load the track paused at `position` (restored playback state).
    Restore {
        item: Item,
        position: Duration,
    },
    TogglePause,
    /// Jump to this position in the playing track.
    Seek(Duration),
    Stop,
    Search(String),
    /// Fetch all albums of the artist (id) from Spotify.
    LoadArtistAlbums(String),
    /// Fetch the album's (URI) tracks for the track list.
    LoadAlbumTracks(String),
    /// Fetch an album's details (URI) to play it (`deck ctl play-album` for an album that is
    /// neither on the shelf nor in Curated).
    FetchAlbum(String),
    /// Fetch the album's tracks for the queue.
    QueueAlbum(Album),
    /// Pick for me: fetch the album's tracks for the front of the queue (`Event::AlbumPicked`).
    PickAlbum(Album),
    /// Put the album in the history (`history.json`), with the time.
    RecordHistory {
        uri: String,
        name: String,
        artist: String,
    },
    /// Fetch the album's (URI) details and tracks for the queue, or only `track` (URI) of them.
    QueueRemote {
        album: String,
        track: Option<String>,
    },
    AddToShelf(Album),
    /// Remove the album (URI) from the shelf.
    RemoveFromShelf(String),
    /// Reread the lists (`curated.json`).
    LoadLists,
    /// Reject the album (URI) from Curated: it leaves the list and is recorded in history.
    RejectCurated(String),
    /// Fetch a page from the radio station.
    LoadRadio(RadioRequest),
    /// Fetch the year and artist of a radio track's album (URI) for adding it to the shelf
    /// (also in the track list, if the album has no year).
    AddRadioAlbumToShelf(String),
    /// Add the genre (id) to favourites, i.e. to the home page.
    AddFavorite(String),
    RemoveFavorite(String),
    /// Ask whether the tracks (URIs, at most `LIKED_BATCH`) are liked.
    CheckLiked(Vec<String>),
    /// Save the track (URI) to Liked Songs or remove it from there.
    SetLiked {
        uri: String,
        liked: bool,
    },
    /// Write the playback state to this file (`Event::SavePlayback`); `None` removes it.
    SavePlayback {
        path: PathBuf,
        playback: Option<Box<SavedPlayback>>,
    },
    Quit,
}

/// A radio fetch. The result is valid only if it matches the pending request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RadioRequest {
    /// The genre id.
    pub genre: String,
    /// The seed URI.
    pub seed: String,
    /// Tracks (URIs) already received from the seed's station, for the refill.
    pub previous: Vec<String>,
    pub kind: RadioLoad,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadioLoad {
    /// A new station (Enter or Ctrl+R): replaces the context and plays the first track.
    Station,
    /// A refill for the playing radio.
    More,
}
