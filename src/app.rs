//! Application state and key logic: view stack, selections, search text, now playing,
//! play order (album and queue) and errors.
//!
//! Play order works the Spotify way: the queue plays after the current track, and once
//! the queue is empty the album continues where it left off. The player plays one track
//! at a time and reports when it ends; `app` picks the next one.
//!
//! Playback state (album, track, position and queue) survives restarts
//! ([`SavedPlayback`]): the event loop saves it on quit and whenever
//! [`State::playback_changed`] reports a change, and on startup `Event::Restore`
//! loads the track paused at the saved position.
//!
//! `State::update` is pure: it touches neither the terminal, the network nor files.
//! It changes the state and returns actions (`Action`) as data. The event loop
//! carries them out and reports the result back as an event (e.g. `Event::Searched`,
//! `Event::ShelfChanged`). Time is a parameter so that the search delay is testable.
//!
//! Above the shelf is a group of lists: Curated, favourite genres and Genres. ↑/↓
//! moves over the list rows and artists as one list. The lists are reread
//! (`Action::LoadLists`) when the shelf comes into view or a list view is opened.
//!
//! A genre plays as a radio: the radio is a playback context like an album, and its
//! track list lives only here (`RadioPosition`). The `radio` module fetches pages
//! (`Action::LoadRadio`), and the result comes back as `Event::RadioLoaded`, from which
//! only tracks not yet in the radio are taken. A refill starts when fewer than
//! `RADIO_REFILL` tracks remain after the playing one, and when the station no longer
//! brings anything new (fewer than `RADIO_FRESH_MIN`), the radio moves on to the next
//! unplayed seed.
//!
//! Mac media keys arrive as `Event::Media` (`now_playing`) and work like Space and
//! ← / → in any view. `State::now_playing_info` reports the playing track to Now
//! Playing.
//!
//! Ctrl+N (n outside the search field) opens the album or radio of the playing track
//! (`State::open_playing`): that is why album details travel in the playback state.
//!
//! Ctrl+L likes the playing track or removes the like (Liked Songs). Likes live in the
//! Web API, whose limits are strict, so their state is queried in batches
//! (`Action::CheckLiked`, at most `LIKED_BATCH` tracks) when a track list or a radio
//! page arrives, and kept in memory for the session (`Likes`). Liking and unliking
//! update the memory without a new query. Each track is queried at most once, and a
//! failed query stops all queries for the rest of the session.

use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::{
    catalog::{Album, Artist, LIKED_BATCH, SearchResults, Song, Track},
    genres::{self, Genre},
    listened::{self, Listened},
    lists::AlbumList,
    now_playing::{self, Command as MediaCommand},
    picks::{self, PickFrom},
    player::{self, Item},
    radio,
    shelf::{albums_of, artists_of, sort_by_year},
};

/// A search starts once typing has paused for this long.
pub const SEARCH_DEBOUNCE: Duration = Duration::from_millis(300);

/// Ctrl+L without your own Spotify app.
const LIKES_NEED_APP: &str =
    "liking songs needs your own Spotify app: set client_id in ~/.config/deck/config.toml";

/// How long the confirmation for queueing and liking stays visible.
pub const NOTICE_TIME: Duration = Duration::from_secs(3);

/// A radio refill starts when fewer tracks than this remain after the playing one.
const RADIO_REFILL: usize = 5;
/// If a refill brings fewer new tracks than this, the station has started repeating itself
/// and the radio switches seeds. In testing a station repeated itself after about 300
/// tracks, and pages could then bring only a couple of new ones at a time for a long while.
const RADIO_FRESH_MIN: usize = 10;
/// The radio keeps at most this many played tracks before the playing one: otherwise the
/// track list would grow for the whole session, and every save would copy it.
const RADIO_HISTORY: usize = 500;

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

/// Every view has a list whose scroll position (`Scroll`) is part of the view's state:
/// when you return to the view, the list is where you left it.
#[derive(Debug)]
pub enum View {
    /// The selection moves over the list rows and artists: lists first, then artists.
    Shelf {
        selected: usize,
        scroll: Scroll,
    },
    /// The Curated list. The albums are in the state (`State::curated`).
    Curated {
        selected: usize,
        scroll: Scroll,
    },
    Artist(ArtistView),
    Search(SearchView),
    Album(AlbumView),
    Queue {
        selected: usize,
        scroll: Scroll,
    },
    /// Genres: every genre in the catalog.
    Genres {
        selected: usize,
        scroll: Scroll,
    },
    Radio(RadioView),
}

/// A list's scroll position: the topmost visible row. The list only scrolls when the
/// selection would go past the visible area, so the position depends on the previous draw
/// and the height of the area. That is why the drawing (`ui`) updates it, not
/// `State::update`, and why it is a `Cell` in the `&State` the drawing gets.
#[derive(Debug, Default)]
pub struct Scroll(Cell<usize>);

impl Scroll {
    pub fn get(&self) -> usize {
        self.0.get()
    }

    pub fn set(&self, offset: usize) {
        self.0.set(offset);
    }
}

/// A genre's radio. The tracks are in the playback context (`State::radio_tracks`) if the
/// genre's radio is playing.
#[derive(Debug)]
pub struct RadioView {
    pub genre: &'static Genre,
    pub selected: usize,
    pub scroll: Scroll,
}

/// A list row above the shelf.
#[derive(Debug, PartialEq, Eq)]
pub enum ShelfRow<'a> {
    /// `None`: there is no list yet (Curated is not set up) or its file is broken.
    Curated(Option<&'a AlbumList>),
    Genre(&'static Genre),
    Genres,
}

#[derive(Debug)]
pub struct ArtistView {
    pub artist: Artist,
    pub source: Source,
    pub selected: usize,
    pub scroll: Scroll,
}

#[derive(Debug)]
pub enum Source {
    /// The artist's albums on the shelf by year; updated when the shelf changes.
    Shelf(Vec<Album>),
    /// All of the artist's albums from Spotify; `None` while loading.
    Spotify(Option<Vec<Album>>),
}

/// An album's tracks. Opens with Enter on an album; playback starts only with Enter
/// on a track.
#[derive(Debug)]
pub struct AlbumView {
    pub album: Album,
    /// `None` while the tracks are still being fetched.
    pub tracks: Option<Vec<Track>>,
    pub selected: usize,
    pub scroll: Scroll,
    /// Opened with n when the playing track's position is unknown (queued from a radio):
    /// the selection moves to this track (URI) when the tracks arrive.
    select_uri: Option<String>,
}

#[derive(Debug, Default)]
pub struct SearchView {
    pub query: String,
    /// Cursor position in characters.
    pub cursor: usize,
    pub results: Option<SearchResults>,
    /// Selection in the result list: artists first, then albums, then songs.
    pub selected: usize,
    pub scroll: Scroll,
    /// Keys go to the result list instead of the search field: ↓ or Enter moves there,
    /// ↑ on the first row or f goes back. Only then is a row selected.
    pub in_results: bool,
    due: Option<Instant>,
    /// The last search sent; only its results are accepted.
    requested: Option<String>,
    pending: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SearchItem<'a> {
    Artist(&'a Artist),
    Album(&'a Album),
    Song(&'a Song),
}

/// The playing track as reported by the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowPlaying {
    /// The track's URI; empty if unknown (old saved playback state).
    pub uri: String,
    pub artist: String,
    pub album: String,
    pub track: String,
    pub duration: Duration,
    /// Cover image URL for Now Playing.
    pub cover: Option<String>,
    pub paused: bool,
    position: Duration,
    position_at: Instant,
}

/// A track in the queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedTrack {
    pub uri: String,
    pub name: String,
    pub artist: String,
    #[serde(rename = "duration_ms", with = "millis")]
    pub duration: Duration,
    /// The album (URI) and the track's position in its track list: the playing track is
    /// marked in the list even when played from the queue. A radio track's position is
    /// unknown.
    album_uri: String,
    index: Option<usize>,
    /// Album details for the n track list; `None` in old saved state.
    /// A radio track's album has no year, and the artist is the track's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    album: Option<Box<Album>>,
}

/// Play order: what plays now and what plays next.
#[derive(Debug, Default)]
struct Playback {
    current: Option<Current>,
    /// The playing album; when the queue plays, the album continues after it.
    album: Option<AlbumPosition>,
    /// The playing radio instead of an album: at most one of them at a time.
    radio: Option<RadioPosition>,
    /// Tracks already played in this queue round, for the ← key. Emptied when the album
    /// continues.
    played: Vec<QueuedTrack>,
    queue: Vec<QueuedTrack>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Current {
    /// Album track `AlbumPosition::index`.
    Album,
    /// Radio track `RadioPosition::index`.
    Radio,
    Queued(QueuedTrack),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AlbumPosition {
    uri: String,
    /// Number of tracks on the album.
    len: usize,
    /// The playing track, or the last one played while the queue plays.
    index: usize,
    /// Album details for the n track list; `None` in old saved state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    album: Option<Album>,
}

/// The playing genre radio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RadioPosition {
    /// The genre's id in the catalog.
    genre: String,
    /// The current seed (URI).
    seed: String,
    /// Seeds played in this radio, including the current one.
    seeds: Vec<String>,
    /// The first track received from the current seed's station: the refill sends it and
    /// the later ones to the station.
    seed_start: usize,
    /// Tracks in the order they arrived from the station, from all seeds.
    tracks: Vec<radio::Track>,
    /// The playing track, or the last one played while the queue plays.
    index: usize,
}

/// Radio fetch state. Not saved.
#[derive(Debug, Default)]
struct RadioFetch {
    /// The pending fetch; only its result is accepted.
    pending: Option<RadioRequest>,
    /// A refill failed while this track (index) was playing: retry when the next one
    /// starts.
    failed_at: Option<usize>,
    /// Consecutive refills that brought nothing. Once every seed has been tried, refills
    /// stop and the remaining tracks play out.
    empty: usize,
}

/// Playback state that survives restarts (`playback` module): the play order, the
/// playing track's details for the status line, and the position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedPlayback {
    album: Option<AlbumPosition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    radio: Option<RadioPosition>,
    current: Current,
    #[serde(default)]
    played: Vec<QueuedTrack>,
    #[serde(default)]
    queue: Vec<QueuedTrack>,
    /// `None` if the player had not yet reported the track's details.
    now_playing: Option<SavedTrack>,
    #[serde(rename = "position_ms", with = "millis")]
    position: Duration,
}

/// The playing track's details for the status line, even before the player has loaded it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SavedTrack {
    #[serde(default)]
    uri: String,
    artist: String,
    album: String,
    track: String,
    #[serde(rename = "duration_ms", with = "millis")]
    duration: Duration,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cover: Option<String>,
}

/// Duration in the file in milliseconds.
mod millis {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(super::millis(*duration))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
        u64::deserialize(deserializer).map(Duration::from_millis)
    }
}

/// A brief confirmation: what was done, and the name of the track or album.
#[derive(Debug)]
struct Notice {
    kind: NoticeKind,
    name: String,
    until: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Queued,
    Liked,
    Unliked,
}

/// Liked tracks for the session. Not saved.
#[derive(Debug, Default)]
struct Likes {
    /// Likes need your own Spotify app (`Catalog::can_like`).
    enabled: bool,
    /// Known state by track URI.
    known: HashMap<String, bool>,
    /// URIs asked about, including pending and failed ones: they are not asked again.
    asked: HashSet<String>,
    /// A query failed (e.g. 429): no new queries are made during the session.
    stopped: bool,
    /// Pending saves and removals: URI → track name for the confirmation.
    changing: HashMap<String, String>,
}

impl Likes {
    /// Asks about the tracks whose state is neither known nor asked, `LIKED_BATCH`
    /// at a time.
    fn check<'a>(&mut self, uris: impl IntoIterator<Item = &'a str>) -> Vec<Action> {
        if !self.enabled || self.stopped {
            return Vec::new();
        }
        let mut new: Vec<String> = Vec::new();
        for uri in uris {
            // `insert` is false if the URI was already asked or is already in this batch.
            if uri.starts_with("spotify:track:")
                && !self.known.contains_key(uri)
                && self.asked.insert(uri.to_owned())
            {
                new.push(uri.to_owned());
            }
        }
        new.chunks(LIKED_BATCH)
            .map(|uris| Action::CheckLiked(uris.to_vec()))
            .collect()
    }
}

#[derive(Debug)]
pub struct State {
    /// The view stack, never empty: the shelf is always at the bottom.
    views: Vec<View>,
    shelf: Vec<Album>,
    /// The shelf's artists in order (`artists_of`) and album URIs: drawing asks for them on
    /// every frame, so they are only computed when the shelf changes (`set_shelf`).
    shelf_artists: Vec<Artist>,
    shelf_uris: HashSet<String>,
    /// The Curated list; `None` when there is no file or it is broken.
    curated: Option<AlbumList>,
    /// Ids of the favourite genres.
    favorites: Vec<String>,
    /// The error is from a broken list file: a successful load clears it.
    lists_broken: bool,
    pub now_playing: Option<NowPlaying>,
    playback: Playback,
    /// Confirmation for queueing or liking.
    notice: Option<Notice>,
    /// A restored track is loading: if that fails, the state is cleared. Loading is done
    /// when the player reports the position (`Paused` or `Position`).
    restoring: bool,
    /// The last saved playback state, without position (`playback_changed`).
    saved: Option<SavedPlayback>,
    /// An event that may change the playback state has been handled since the previous call
    /// to `playback_changed`: only then is the state built and compared.
    playback_dirty: bool,
    radio_fetch: RadioFetch,
    likes: Likes,
    /// State of the seed picker (xorshift). Tests can set it.
    rng: u64,
    /// Albums played in Pick for me's current rounds (`picks`), saved as they change
    /// (`picked_changed`).
    picked: Vec<String>,
    picked_dirty: bool,
    /// Albums listened to in Deck (`listened`), saved as they change (`listened_changed`).
    listened: Listened,
    listened_dirty: bool,
    /// The track (URI) whose halfway point has been counted, so it counts once per play.
    counted: Option<String>,
    /// Shown until the next action succeeds.
    pub error: Option<String>,
    /// An album asked for by `deck ctl play-album`, or a song's album from the search: it plays
    /// as soon as its tracks arrive, from the track selected in its view (the song's, or the first).
    play_on_load: Option<String>,
    /// The track `play_on_load`'s album starts from (`deck ctl play-song`, a song from the search):
    /// it is selected when the album opens.
    play_from: Option<String>,
}

impl State {
    pub fn new(shelf: Vec<Album>) -> Self {
        let mut state = Self {
            views: vec![View::Shelf {
                selected: 0,
                scroll: Scroll::default(),
            }],
            shelf: Vec::new(),
            shelf_artists: Vec::new(),
            shelf_uris: HashSet::new(),
            curated: None,
            favorites: Vec::new(),
            lists_broken: false,
            now_playing: None,
            playback: Playback::default(),
            notice: None,
            restoring: false,
            saved: None,
            playback_dirty: false,
            radio_fetch: RadioFetch::default(),
            likes: Likes::default(),
            rng: genres::random() | 1,
            error: None,
            play_on_load: None,
            picked: Vec::new(),
            picked_dirty: false,
            listened: Listened::new(),
            listened_dirty: false,
            counted: None,
            play_from: None,
        };
        state.set_shelf(shelf);
        state
    }

    /// The listened albums read at startup.
    pub fn with_listened(mut self, listened: Listened) -> Self {
        self.listened = listened;
        self
    }

    /// The listened albums to save, once after each change.
    pub fn listened_changed(&mut self) -> Option<&Listened> {
        std::mem::take(&mut self.listened_dirty).then_some(&self.listened)
    }

    /// Counts the playing track once half of it has played, for its album (`listened`), and
    /// puts the album in the history.
    fn count_listened(&mut self, now: Instant) -> Option<Action> {
        let playing = self.now_playing.as_ref()?;
        if playing.uri.is_empty()
            || self.counted.as_deref() == Some(playing.uri.as_str())
            || playing.elapsed(now) * 2 < playing.duration
        {
            return None;
        }
        let track = playing.uri.clone();
        // The album's URI and track count (0 when unknown), and its name and artist for the
        // history (the track's own when Deck knows no better)
        let named = |album: Option<&Album>| match album {
            Some(album) => (album.name.clone(), album.artist.clone()),
            None => (playing.album.clone(), playing.artist.clone()),
        };
        let playback = &self.playback;
        let album = match playback.current.as_ref() {
            Some(Current::Album) => playback
                .album
                .as_ref()
                .map(|a| (a.uri.clone(), a.len, named(a.album.as_ref()))),
            Some(Current::Queued(queued)) => {
                Some((queued.album_uri.clone(), 0, named(queued.album.as_deref())))
            }
            Some(Current::Radio) => playback
                .radio
                .as_ref()
                .and_then(|radio| radio.tracks.get(radio.index))
                .map(|t| {
                    (
                        t.album.uri.clone(),
                        0,
                        (t.album.name.clone(), t.artist.clone()),
                    )
                }),
            None => None,
        };
        self.counted = Some(track.clone());
        let (album, len, (name, artist)) = album.filter(|(uri, ..)| !uri.is_empty())?;
        let length = listened::learn_length(&mut self.listened, &album, len);
        if listened::played(&mut self.listened, &album, &track) || length {
            self.listened_dirty = true;
        }
        Some(Action::RecordHistory {
            uri: album,
            name,
            artist,
        })
    }

    /// An album's track count, learnt when its tracks arrive.
    fn learn_length(&mut self, album: &str, tracks: usize) {
        if listened::learn_length(&mut self.listened, album, tracks) {
            self.listened_dirty = true;
        }
    }

    /// Pick for me's history read at startup.
    pub fn with_picked(mut self, played: Vec<String>) -> Self {
        self.picked = played;
        self
    }

    /// Favourite genres read at startup.
    pub fn with_favorites(mut self, ids: Vec<String>) -> Self {
        self.set_lists(|state| state.favorites = ids);
        self
    }

    /// Enables likes: they need your own Spotify app.
    pub fn with_likes(mut self, enabled: bool) -> Self {
        self.likes.enabled = enabled;
        self
    }

    /// The Curated list read at startup. The selection starts on the top row.
    pub fn with_curated(mut self, result: Result<Option<AlbumList>, String>) -> Self {
        self.lists_loaded(result);
        if let Some(View::Shelf { selected, .. }) = self.views.first_mut() {
            *selected = 0;
        }
        self
    }

    /// The Curated list for the list view, even when empty.
    pub fn curated(&self) -> Option<&AlbumList> {
        self.curated.as_ref()
    }

    /// The album's (URI) reason, if the album is on the current Curated list.
    pub fn curated_reason(&self, album_uri: &str) -> Option<&str> {
        self.curated
            .as_ref()?
            .albums
            .iter()
            .find(|entry| entry.album.uri == album_uri)?
            .reason
            .as_deref()
    }

    /// Whether the list file could not be read (the error is shown on the top row).
    pub fn curated_broken(&self) -> bool {
        self.lists_broken
    }

    /// The list rows shown above the shelf: always Curated first (without a list it opens
    /// the setup instructions), favourite genres in alphabetical order, and always Genres
    /// last.
    pub fn shelf_lists(&self) -> Vec<ShelfRow<'_>> {
        [ShelfRow::Curated(self.curated.as_ref())]
            .into_iter()
            .chain(self.favorite_genres().into_iter().map(ShelfRow::Genre))
            .chain([ShelfRow::Genres])
            .collect()
    }

    pub fn favorite(&self, id: &str) -> bool {
        self.favorites.iter().any(|f| f == id)
    }

    /// Favourite genres from the catalog by name. Unknown ids are skipped.
    pub fn favorite_genres(&self) -> Vec<&'static Genre> {
        genres::all()
            .iter()
            .filter(|g| self.favorite(&g.id))
            .collect()
    }

    /// The playing radio's genre (id), also when paused and while the queue plays.
    pub fn playing_genre(&self) -> Option<&str> {
        self.playback.current.as_ref()?;
        self.playback
            .radio
            .as_ref()
            .map(|radio| radio.genre.as_str())
    }

    /// The genre's radio tracks, if it is the playback context.
    pub fn radio_tracks(&self, genre: &str) -> Option<&[radio::Track]> {
        self.playback
            .radio
            .as_ref()
            .filter(|radio| radio.genre == genre)
            .map(|radio| radio.tracks.as_slice())
    }

    /// The playing radio track's position in the genre's radio (not when from the queue).
    pub fn playing_radio_track(&self, genre: &str) -> Option<usize> {
        self.now_playing.as_ref()?;
        let radio = self.playback.radio.as_ref()?;
        (self.playback.current == Some(Current::Radio) && radio.genre == genre)
            .then_some(radio.index)
    }

    /// The shelf's selectable rows: lists and artists.
    fn shelf_rows(&self) -> usize {
        self.shelf_lists().len() + self.shelf_artists.len()
    }

    /// The queue's tracks in play order (the playing track is not included).
    pub fn queue(&self) -> &[QueuedTrack] {
        &self.playback.queue
    }

    /// The latest confirmation and its name, until `NOTICE_TIME` has passed.
    pub fn notice(&self, now: Instant) -> Option<(NoticeKind, &str)> {
        self.notice
            .as_ref()
            .filter(|notice| now < notice.until)
            .map(|notice| (notice.kind, notice.name.as_str()))
    }

    /// Whether the track (URI) is liked. An unknown state is `false`.
    pub fn liked(&self, uri: &str) -> bool {
        self.likes.known.get(uri).copied().unwrap_or(false)
    }

    /// The visible view.
    pub fn view(&self) -> &View {
        self.views.last().expect("the view stack is never empty")
    }

    /// The shelf's artists in alphabetical order.
    pub fn shelf_artists(&self) -> &[Artist] {
        &self.shelf_artists
    }

    /// Whether the album (URI) is on the shelf.
    pub fn on_shelf(&self, uri: &str) -> bool {
        self.shelf_uris.contains(uri)
    }

    /// Replaces the shelf's albums and computes the data derived from them.
    fn set_shelf(&mut self, albums: Vec<Album>) {
        self.shelf_artists = artists_of(&albums);
        self.shelf_uris = albums.iter().map(|album| album.uri.clone()).collect();
        self.shelf = albums;
    }

    /// The playing track's position in the album's (URI) track list, if it plays from the
    /// album or from the queue.
    pub fn playing_track(&self, album_uri: &str) -> Option<usize> {
        self.now_playing.as_ref()?;
        let playback = &self.playback;
        match playback.current.as_ref()? {
            Current::Album => playback
                .album
                .as_ref()
                .filter(|album| album.uri == album_uri)
                .map(|album| album.index),
            Current::Radio => None,
            Current::Queued(track) => track.index.filter(|_| track.album_uri == album_uri),
        }
    }

    pub fn update(&mut self, event: Event, now: Instant) -> Vec<Action> {
        // Time passing, position and pause do not change the state to save (the position is
        // saved on quit). Tick arrives ten times a second, so after it
        // `playback_changed` does not touch the state at all.
        if !matches!(
            event,
            Event::Tick | Event::Player(player::Event::Position(_) | player::Event::Paused(_))
        ) {
            self.playback_dirty = true;
        }
        let actions = match event {
            Event::Key(key) => self.key(key, now),
            Event::Media(command) => self.media(command),
            Event::PlayAlbum(uri) => self.play_album_remote(uri),
            Event::PlaySong { track, album } => {
                self.play_from = Some(track);
                self.play_album_remote(album)
            }
            Event::Search(query) => self.search_remote(&query),
            Event::Tick => self.tick(now),
            Event::Player(event) => self.player(event, now),
            Event::Searched { query, result } => {
                self.searched(query, result);
                Vec::new()
            }
            Event::AlbumTracks { uri, result } => self.album_tracks(&uri, result),
            Event::AlbumFetched { uri, result } => self.album_fetched(&uri, result),
            Event::AlbumQueued {
                album,
                track,
                result,
            } => self.album_queued(&album, track.as_deref(), result, now),
            Event::QueueRemote { album, track } => vec![Action::QueueRemote { album, track }],
            Event::Unqueue(index) => {
                if index < self.playback.queue.len() {
                    self.playback.queue.remove(index);
                }
                Vec::new()
            }
            Event::ClearQueue => {
                self.playback.queue.clear();
                Vec::new()
            }
            Event::Pick(from) => self.pick(from),
            Event::AlbumPicked { album, result } => self.album_picked(&album, result),
            // An album from elsewhere is fetched first for its year and artist, as from the radio.
            Event::Shelve { uri, on: true } if !self.on_shelf(&uri) => {
                vec![Action::AddRadioAlbumToShelf(uri)]
            }
            Event::Shelve { uri, on: false } if self.on_shelf(&uri) => {
                vec![Action::RemoveFromShelf(uri)]
            }
            Event::Shelve { .. } => Vec::new(),
            Event::Seek(position) => self.seek(position, now),
            Event::Radio(id) => match genres::find(&id) {
                Some(genre) => self.start_radio(genre),
                None => {
                    self.error = Some(format!("no such genre: {id}"));
                    Vec::new()
                }
            },
            Event::Favorite { id, on: true } if !self.favorite(&id) => {
                vec![Action::AddFavorite(id)]
            }
            Event::Favorite { id, on: false } if self.favorite(&id) => {
                vec![Action::RemoveFavorite(id)]
            }
            Event::Favorite { .. } => Vec::new(),
            Event::RejectCurated(uri) => vec![Action::RejectCurated(uri)],
            Event::Quit => vec![Action::Quit],
            Event::ArtistAlbums { artist_id, result } => {
                self.artist_albums(&artist_id, result);
                Vec::new()
            }
            Event::ShelfChanged(albums) => {
                self.shelf_changed(albums);
                Vec::new()
            }
            Event::ListsLoaded(result) => {
                self.lists_loaded(result);
                Vec::new()
            }
            Event::CuratedChanged(list) => {
                self.error = None;
                self.set_curated(list);
                Vec::new()
            }
            Event::Restore(saved) => self.restore(*saved, now),
            Event::RestoreCopy(saved) => {
                let actions = self.restore(*saved, now);
                // restore() counts the state as saved, as it is at startup; this one isn't yet.
                self.saved = None;
                self.playback_dirty = true;
                actions
            }
            Event::SavePlayback(path) => vec![Action::SavePlayback {
                path,
                playback: self.playback(now).map(Box::new),
            }],
            Event::RadioLoaded { request, result } => self.radio_loaded(request, result),
            Event::RadioAlbum(Ok(album)) if !self.on_shelf(&album.uri) => {
                vec![Action::AddToShelf(album)]
            }
            Event::RadioAlbum(Ok(_)) => Vec::new(),
            Event::RadioAlbum(Err(message)) => {
                self.error = Some(message);
                Vec::new()
            }
            Event::FavoritesChanged(ids) => {
                self.error = None;
                self.set_lists(|state| state.favorites = ids);
                Vec::new()
            }
            Event::LikedChecked { uris, result } => {
                self.liked_checked(uris, result);
                Vec::new()
            }
            Event::LikeChanged { uri, liked, result } => {
                self.like_changed(uri, liked, result, now);
                Vec::new()
            }
            Event::Error(message) => {
                self.error = Some(message);
                Vec::new()
            }
        };
        let mut actions = actions;
        actions.extend(self.radio_refill());
        // The user played something else: the restored track's load no longer matters.
        if actions.iter().any(|a| matches!(a, Action::Play(_))) {
            self.restoring = false;
        }
        actions.extend(self.count_listened(now));
        // Playback also shrinks the queue: the queue view's selection stays in the list.
        let len = self.playback.queue.len();
        for view in &mut self.views {
            if let View::Queue { selected, .. } = view {
                clamp(selected, len);
            }
        }
        actions
    }

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
    /// Pick for me's history to save, once after each change.
    pub fn picked_changed(&mut self) -> Option<&[String]> {
        std::mem::take(&mut self.picked_dirty).then_some(self.picked.as_slice())
    }

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
    fn restore(&mut self, saved: SavedPlayback, now: Instant) -> Vec<Action> {
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

    /// The playing track for Now Playing; `None` when nothing is playing.
    pub fn now_playing_info(&self, now: Instant) -> Option<now_playing::Info> {
        let playing = self.now_playing.as_ref()?;
        Some(now_playing::Info {
            title: playing.track.clone(),
            artist: playing.artist.clone(),
            album: playing.album.clone(),
            duration_ms: millis(playing.duration),
            position_ms: millis(playing.elapsed(now)),
            playing: !playing.paused,
            cover: playing.cover.clone(),
            liked: (self.likes.enabled && !playing.uri.is_empty())
                .then(|| self.liked(&playing.uri)),
            track_uri: (!playing.uri.is_empty()).then(|| playing.uri.clone()),
            album_uri: match self.playback.current.as_ref() {
                Some(Current::Album) => self.playback.album.as_ref().map(|a| a.uri.clone()),
                Some(Current::Queued(track)) => Some(track.album_uri.clone()),
                Some(Current::Radio) | None => None,
            },
            queued: matches!(self.playback.current, Some(Current::Queued(_))),
            queue: self
                .playback
                .queue
                .iter()
                .map(|track| now_playing::QueuedInfo {
                    uri: track.uri.clone(),
                    title: track.name.clone(),
                    artist: track.artist.clone(),
                    duration_ms: millis(track.duration),
                    album_uri: track.album_uri.clone(),
                })
                .collect(),
            radio: self.radio_info(),
            error: self.error.clone(),
        })
    }

    /// The playing radio for Now Playing: its tracks from the playing one on.
    fn radio_info(&self) -> Option<now_playing::RadioInfo> {
        let radio = self.playback.radio.as_ref()?;
        let genre = genres::find(&radio.genre)?;
        Some(now_playing::RadioInfo {
            genre: genre.id.clone(),
            name: genre.name.clone(),
            tracks: radio
                .tracks
                .iter()
                .skip(radio.index)
                .map(|track| now_playing::RadioTrackInfo {
                    uri: track.uri.clone(),
                    title: track.name.clone(),
                    artist: track.artist.clone(),
                    album: track.album.name.clone(),
                    album_uri: track.album.uri.clone(),
                })
                .collect(),
        })
    }

    /// A jump in the playing track, shown at once; the player confirms it with its position.
    fn seek(&mut self, position: Duration, now: Instant) -> Vec<Action> {
        let Some(playing) = &mut self.now_playing else {
            return Vec::new();
        };
        let position = position.min(playing.duration);
        playing.position = position;
        playing.position_at = now;
        vec![Action::Seek(position)]
    }

    fn media(&mut self, command: MediaCommand) -> Vec<Action> {
        let paused = self.now_playing.as_ref().map(|playing| playing.paused);
        match command {
            MediaCommand::Toggle => vec![Action::TogglePause],
            MediaCommand::Play if paused == Some(true) => vec![Action::TogglePause],
            MediaCommand::Pause if paused == Some(false) => vec![Action::TogglePause],
            MediaCommand::Play | MediaCommand::Pause => Vec::new(),
            MediaCommand::Next => self.next().into_iter().collect(),
            MediaCommand::Previous => self.previous().into_iter().collect(),
            MediaCommand::ToggleLike => self.toggle_like(),
        }
    }

    fn key(&mut self, key: Key, now: Instant) -> Vec<Action> {
        // Outside the search field, Ctrl keys also work as plain letters. The queue's
        // letter is u, because q quits.
        let typing = matches!(self.view(), View::Search(search) if !search.in_results);
        let key = match key {
            Key::Char(c @ ('f' | 'l' | 'e' | 'a' | 'd' | 'r' | 'n')) if !typing => Key::Ctrl(c),
            Key::Char('u') if !typing => Key::Ctrl('q'),
            key => key,
        };
        match key {
            Key::Ctrl('c') => return vec![Action::Quit],
            Key::Ctrl('f') => {
                self.open_search();
                return Vec::new();
            }
            Key::Ctrl('q') => {
                self.open_queue();
                return Vec::new();
            }
            Key::Ctrl('l') => return self.toggle_like(),
            Key::Ctrl('n') => return self.open_playing(),
            Key::Esc => {
                if self.views.len() > 1 {
                    self.views.pop();
                    // The shelf came into view: a new list shows up without a restart.
                    if let View::Shelf { .. } = self.view() {
                        return vec![Action::LoadLists];
                    }
                }
                return Vec::new();
            }
            _ => {}
        }
        if typing {
            return self.search_key(key, now);
        }
        match key {
            Key::Char(' ') => vec![Action::TogglePause],
            Key::Left => self.previous().into_iter().collect(),
            Key::Right => self.next().into_iter().collect(),
            Key::Char('q') => vec![Action::Quit],
            _ => match self.view() {
                View::Shelf { .. } => self.shelf_key(key),
                View::Curated { .. } => self.curated_key(key),
                View::Artist(_) => self.artist_key(key),
                View::Album(_) => self.album_key(key, now),
                View::Queue { .. } => self.queue_key(key),
                View::Genres { .. } => self.genres_key(key),
                View::Radio(_) => self.radio_key(key, now),
                View::Search(_) => self.search_results_key(key, now),
            },
        }
    }

    /// `deck ctl play-album <uri>`: opens the album and plays it from the first track once the
    /// tracks have arrived. The details come from the shelf or Curated, otherwise from Spotify.
    fn play_album_remote(&mut self, uri: String) -> Vec<Action> {
        let curated = self.curated.as_ref().map_or(&[][..], |list| &list.albums);
        let album = self
            .shelf
            .iter()
            .find(|album| album.uri == uri)
            .cloned()
            .or_else(|| {
                let entry = curated.iter().find(|entry| entry.album.uri == uri)?;
                Some(entry.album.clone())
            });
        self.play_on_load = Some(uri.clone());
        match album {
            Some(album) => self.open_album_to_play(album),
            // Not on the shelf or in Curated: fetch its details first (`album_fetched`).
            None => vec![Action::FetchAlbum(uri)],
        }
    }

    /// `Event::AlbumFetched`: an album asked for by `deck ctl play-album` opens, and it plays
    /// when its tracks arrive (`album_tracks`).
    fn album_fetched(&mut self, uri: &str, result: Result<Album, String>) -> Vec<Action> {
        if self.play_on_load.as_deref() != Some(uri) {
            return Vec::new();
        }
        match result {
            Ok(album) => self.open_album_to_play(album),
            Err(message) => {
                self.play_on_load = None;
                self.play_from = None;
                self.error = Some(message);
                Vec::new()
            }
        }
    }

    /// `deck ctl search <query>`: opens the search view with the query and searches right away,
    /// without the typing debounce.
    fn search_remote(&mut self, query: &str) -> Vec<Action> {
        let query = query.trim().to_owned();
        if query.is_empty() {
            return Vec::new();
        }
        self.open_search();
        let Some(search) = search_view(&mut self.views) else {
            return Vec::new();
        };
        search.cursor = query.chars().count();
        search.query = query.clone();
        search.in_results = false;
        search.due = None;
        search.requested = Some(query.clone());
        search.pending = true;
        vec![Action::Search(query)]
    }

    /// Plays an album (`len` tracks) from track `track`. The queue is kept and plays after
    /// this track.
    fn play_album(&mut self, album: Album, len: usize, track: usize) -> Vec<Action> {
        let uri = album.uri.clone();
        // Every album that starts, picked or not, counts as played for Pick for me.
        if !self.picked.contains(&uri) {
            self.picked.push(uri.clone());
            self.picked_dirty = true;
        }
        // An album replaces the radio, including a pending fetch.
        self.radio_fetch = RadioFetch::default();
        let playback = &mut self.playback;
        playback.current = Some(Current::Album);
        playback.played.clear();
        playback.radio = None;
        playback.album = Some(AlbumPosition {
            uri: uri.clone(),
            len,
            index: track,
            album: Some(album),
        });
        vec![Action::Play(Item::Album { uri, track })]
    }

    /// Pick for me: an album from the shelf or Curated (or both) that hasn't played in this
    /// round and isn't playing now, which then plays as `deck ctl play-album` would.
    fn pick(&mut self, from: PickFrom) -> Vec<Action> {
        let shelf = self.shelf.iter().map(|album| album.uri.clone());
        let curated = self
            .curated
            .iter()
            .flat_map(|list| list.albums.iter().map(|entry| entry.album.uri.clone()));
        let mut candidates: Vec<String> = match from {
            PickFrom::Shelf => shelf.collect(),
            PickFrom::Curated => curated.collect(),
            PickFrom::Any => shelf.chain(curated).collect(),
        };
        candidates.dedup();
        let playing = match self.playback.current.as_ref() {
            Some(Current::Album) => self.playback.album.as_ref().map(|a| a.uri.clone()),
            Some(Current::Queued(track)) => Some(track.album_uri.clone()),
            Some(Current::Radio) | None => None,
        };
        let random = self.random();
        let Some(uri) = picks::choose(&candidates, &mut self.picked, playing.as_deref(), random)
        else {
            self.error = Some(match from {
                PickFrom::Curated => "nothing to pick from: there is no Curated list".into(),
                _ => "nothing to pick from: your shelf is empty".into(),
            });
            return Vec::new();
        };
        // A new round may have taken the source's albums out of the history.
        self.picked_dirty = true;
        let album = self
            .shelf
            .iter()
            .chain(
                self.curated
                    .iter()
                    .flat_map(|list| list.albums.iter().map(|e| &e.album)),
            )
            .find(|album| album.uri == uri)
            .cloned();
        album.map(Action::PickAlbum).into_iter().collect()
    }

    /// `Event::AlbumPicked`: the picked album's tracks go to the front of the queue, all of
    /// them, and the first plays now; what was queued plays after the album.
    fn album_picked(&mut self, album: &Album, result: Result<Vec<Track>, String>) -> Vec<Action> {
        let tracks = match result {
            Ok(tracks) if !tracks.is_empty() => tracks,
            Ok(_) => {
                self.error = Some(format!("{} has no tracks", album.name));
                return Vec::new();
            }
            Err(message) => {
                self.error = Some(message);
                return Vec::new();
            }
        };
        self.error = None;
        self.learn_length(&album.uri, tracks.len());
        if !self.picked.contains(&album.uri) {
            self.picked.push(album.uri.clone());
            self.picked_dirty = true;
        }
        let mut actions = self.likes.check(tracks.iter().map(|t| t.uri.as_str()));
        let queued: Vec<QueuedTrack> = (0..tracks.len())
            .map(|i| queued(album, &tracks, i))
            .collect();
        self.playback.queue.splice(0..0, queued);
        // Nothing playing: the queue's first track starts as `next` would start it.
        let play = if self.playback.current.is_some() {
            self.next()
        } else {
            let track = self.playback.queue.remove(0);
            let item = Item::Track(track.uri.clone());
            self.playback.current = Some(Current::Queued(track));
            Some(Action::Play(item))
        };
        actions.splice(0..0, play);
        actions
    }

    /// Plays the radio from track `index`. The queue is kept and plays after this track.
    fn play_radio(&mut self, index: usize) -> Vec<Action> {
        let playback = &mut self.playback;
        let Some(radio) = &mut playback.radio else {
            return Vec::new();
        };
        let Some(track) = radio.tracks.get(index) else {
            return Vec::new();
        };
        radio.index = index;
        let item = Item::Track(track.uri.clone());
        playback.current = Some(Current::Radio);
        playback.played.clear();
        vec![Action::Play(item)]
    }

    /// Where the next track comes from: the queue first, then the album or radio where it
    /// left off. `None` if nothing is playing or there is nothing left to play.
    fn upcoming_source(&self) -> Option<Upcoming> {
        let playback = &self.playback;
        playback.current.as_ref()?;
        if !playback.queue.is_empty() {
            return Some(Upcoming::Queue);
        }
        if let Some(album) = &playback.album {
            return (album.index + 1 < album.len).then_some(Upcoming::Album(album.index + 1));
        }
        let radio = playback.radio.as_ref()?;
        (radio.index + 1 < radio.tracks.len()).then_some(Upcoming::Radio(radio.index + 1))
    }

    fn upcoming(&self) -> Option<Item> {
        let playback = &self.playback;
        Some(match self.upcoming_source()? {
            Upcoming::Queue => Item::Track(playback.queue[0].uri.clone()),
            Upcoming::Album(track) => Item::Album {
                uri: playback.album.as_ref()?.uri.clone(),
                track,
            },
            Upcoming::Radio(index) => {
                Item::Track(playback.radio.as_ref()?.tracks[index].uri.clone())
            }
        })
    }

    /// → and the end of a track: moves to the next one (`upcoming`).
    fn next(&mut self) -> Option<Action> {
        let source = self.upcoming_source()?;
        let item = self.upcoming()?;
        let playback = &mut self.playback;
        match source {
            Upcoming::Queue => {
                let track = playback.queue.remove(0);
                if let Some(Current::Queued(previous)) = playback.current.take() {
                    playback.played.push(previous);
                }
                playback.current = Some(Current::Queued(track));
            }
            Upcoming::Album(track) => {
                if let Some(album) = &mut playback.album {
                    album.index = track;
                }
                playback.played.clear();
                playback.current = Some(Current::Album);
            }
            Upcoming::Radio(index) => {
                if let Some(radio) = &mut playback.radio {
                    radio.index = index;
                }
                playback.played.clear();
                playback.current = Some(Current::Radio);
            }
        }
        Some(Action::Play(item))
    }

    /// ←: on an album or radio, the previous track (the first one starts over). From a
    /// queue track, goes back to the previous queue track, or to the album or radio track
    /// after which the queue started, and the track goes back to the front of the queue.
    fn previous(&mut self) -> Option<Action> {
        let playback = &mut self.playback;
        // The playback state changes only once the previous track has been found.
        let item = match playback.current.as_ref()? {
            Current::Album => {
                let album = playback.album.as_mut()?;
                album.index = album.index.saturating_sub(1);
                Item::Album {
                    uri: album.uri.clone(),
                    track: album.index,
                }
            }
            Current::Radio => {
                let radio = playback.radio.as_mut()?;
                let index = radio.index.saturating_sub(1);
                let item = Item::Track(radio.tracks.get(index)?.uri.clone());
                radio.index = index;
                item
            }
            Current::Queued(_) => {
                // Every branch below sets the playing track again.
                let Some(Current::Queued(track)) = playback.current.take() else {
                    return None;
                };
                if let Some(previous) = playback.played.pop() {
                    playback.queue.insert(0, track);
                    let item = Item::Track(previous.uri.clone());
                    playback.current = Some(Current::Queued(previous));
                    item
                } else if let Some(album) = &playback.album {
                    playback.queue.insert(0, track);
                    playback.current = Some(Current::Album);
                    Item::Album {
                        uri: album.uri.clone(),
                        track: album.index,
                    }
                } else if let Some(radio_track) = playback
                    .radio
                    .as_ref()
                    .and_then(|radio| radio.tracks.get(radio.index))
                {
                    let item = Item::Track(radio_track.uri.clone());
                    playback.queue.insert(0, track);
                    playback.current = Some(Current::Radio);
                    item
                } else {
                    // The queue started from nothing: the first track starts over.
                    let item = Item::Track(track.uri.clone());
                    playback.current = Some(Current::Queued(track));
                    item
                }
            }
        };
        Some(Action::Play(item))
    }

    /// Adds the tracks to the end of the queue and shows a confirmation. If nothing is
    /// playing, the queue starts playing.
    fn enqueue(&mut self, tracks: Vec<QueuedTrack>, name: &str, now: Instant) -> Vec<Action> {
        if tracks.is_empty() {
            return Vec::new();
        }
        self.playback.queue.extend(tracks);
        self.notice = Some(Notice {
            kind: NoticeKind::Queued,
            name: name.to_owned(),
            until: now + NOTICE_TIME,
        });
        if self.playback.current.is_some() {
            return Vec::new();
        }
        let track = self.playback.queue.remove(0);
        let action = Action::Play(Item::Track(track.uri.clone()));
        self.playback.current = Some(Current::Queued(track));
        vec![action]
    }

    /// Playback ended (album and queue played, or the player stopped).
    fn stopped(&mut self) {
        let playback = &mut self.playback;
        playback.current = None;
        playback.album = None;
        playback.radio = None;
        playback.played.clear();
        self.now_playing = None;
    }

    /// A song from the search: its album opens with the song selected, and plays from it when
    /// the tracks arrive (`album_tracks`), so the album goes on after the song.
    fn play_song(&mut self, song: Song) -> Vec<Action> {
        self.play_on_load = Some(song.album.uri.clone());
        self.play_from = Some(song.track.uri);
        self.open_album_to_play(song.album)
    }

    /// Opens the album that `play_on_load` waits for, with `play_from`'s track selected: it plays
    /// from the selected track when the tracks arrive.
    fn open_album_to_play(&mut self, album: Album) -> Vec<Action> {
        let actions = self.open_album(album);
        if let Some(View::Album(view)) = self.views.last_mut() {
            view.select_uri = self.play_from.take();
        }
        actions
    }

    /// Opens the album's track list without playing it. The tracks arrive as
    /// `Event::AlbumTracks`.
    fn open_album(&mut self, album: Album) -> Vec<Action> {
        let action = Action::LoadAlbumTracks(album.uri.clone());
        self.views.push(View::Album(AlbumView {
            album,
            tracks: None,
            selected: 0,
            scroll: Scroll::default(),
            select_uri: None,
        }));
        vec![action]
    }

    /// Where n leads: the playing track's album or radio. `None` when nothing is playing or
    /// the album details are missing from a restored old state.
    fn playing_target(&self) -> Option<PlayingTarget> {
        let playback = &self.playback;
        // An album from old saved state can still be found on the shelf.
        let album = |album: Option<&Album>, uri: &str| {
            album
                .or_else(|| self.shelf.iter().find(|a| a.uri == uri))
                .cloned()
        };
        match playback.current.as_ref()? {
            Current::Album => {
                let position = playback.album.as_ref()?;
                Some(PlayingTarget::Album {
                    album: album(position.album.as_ref(), &position.uri)?,
                    index: Some(position.index),
                    uri: String::new(),
                })
            }
            Current::Radio => {
                let radio = playback.radio.as_ref()?;
                Some(PlayingTarget::Radio {
                    genre: genres::find(&radio.genre)?,
                    index: radio.index,
                })
            }
            Current::Queued(track) => Some(PlayingTarget::Album {
                album: album(track.album.as_deref(), &track.album_uri)?,
                index: track.index,
                uri: track.uri.clone(),
            }),
        }
    }

    /// Whether n leads anywhere (`[n] now playing` in the help row).
    pub fn has_now_playing(&self) -> bool {
        self.playing_target().is_some()
    }

    /// n: opens the playing album's track list or the playing radio, selecting the playing
    /// track. If it is already visible, only moves the selection; Esc returns to where n
    /// was pressed.
    fn open_playing(&mut self) -> Vec<Action> {
        match self.playing_target() {
            None => Vec::new(),
            Some(PlayingTarget::Radio { genre, index }) => {
                match self.views.last_mut() {
                    Some(View::Radio(view)) if view.genre.id == genre.id => view.selected = index,
                    _ => self.views.push(View::Radio(RadioView {
                        genre,
                        selected: index,
                        scroll: Scroll::default(),
                    })),
                }
                Vec::new()
            }
            Some(PlayingTarget::Album { album, index, uri }) => {
                if let Some(View::Album(view)) = self.views.last_mut()
                    && view.album.uri == album.uri
                {
                    view.select_uri = None;
                    match (index, &view.tracks) {
                        (Some(index), Some(tracks)) => {
                            view.selected = index;
                            clamp(&mut view.selected, tracks.len());
                        }
                        (Some(index), None) => view.selected = index,
                        (None, Some(tracks)) => {
                            if let Some(index) = tracks.iter().position(|t| t.uri == uri) {
                                view.selected = index;
                            }
                        }
                        (None, None) => view.select_uri = Some(uri),
                    }
                    return Vec::new();
                }
                let action = Action::LoadAlbumTracks(album.uri.clone());
                self.views.push(View::Album(AlbumView {
                    album,
                    tracks: None,
                    selected: index.unwrap_or(0),
                    scroll: Scroll::default(),
                    select_uri: index.is_none().then_some(uri),
                }));
                vec![action]
            }
        }
    }

    /// Opens search in the search field. If search is already on the stack, returns to
    /// it (the text and results are kept).
    fn open_search(&mut self) {
        match self.views.iter().position(|v| matches!(v, View::Search(_))) {
            Some(index) => {
                self.views.truncate(index + 1);
                if let Some(search) = search_view(&mut self.views) {
                    search.in_results = false;
                }
            }
            None => self.views.push(View::Search(SearchView::default())),
        }
    }

    /// Opens the queue view. If it is already on the stack, returns to it.
    fn open_queue(&mut self) {
        match self
            .views
            .iter()
            .position(|v| matches!(v, View::Queue { .. }))
        {
            Some(index) => self.views.truncate(index + 1),
            None => self.views.push(View::Queue {
                selected: 0,
                scroll: Scroll::default(),
            }),
        }
    }

    /// Queue view: Ctrl+D removes the selected track from the queue.
    fn queue_key(&mut self, key: Key) -> Vec<Action> {
        let queue = &mut self.playback.queue;
        let Some(View::Queue { selected, .. }) = self.views.last_mut() else {
            return Vec::new();
        };
        if key == Key::Ctrl('d') && *selected < queue.len() {
            queue.remove(*selected);
            clamp(selected, queue.len());
        } else {
            step(selected, key, queue.len());
        }
        Vec::new()
    }

    /// Shelf: Enter on a list row opens the list, on a genre starts its radio, on the
    /// Genres row opens the genre list, and on an artist opens the artist's albums. Ctrl+D
    /// on a genre removes it from the home page.
    fn shelf_key(&mut self, key: Key) -> Vec<Action> {
        let lists = self.shelf_lists();
        let target = match lists.get(self.shelf_selected()) {
            Some(ShelfRow::Curated(_)) => ShelfTarget::Curated,
            Some(ShelfRow::Genre(genre)) => ShelfTarget::Genre(genre),
            Some(ShelfRow::Genres) => ShelfTarget::Genres,
            None => ShelfTarget::Artist,
        };
        let lists = lists.len();
        let artists = self.shelf_artists.len();
        let Some(View::Shelf { selected, .. }) = self.views.last_mut() else {
            return Vec::new();
        };
        if step(selected, key, lists + artists) {
            return Vec::new();
        }
        let selected = *selected;
        match (key, target) {
            (Key::Enter, ShelfTarget::Curated) => {
                self.views.push(View::Curated {
                    selected: 0,
                    scroll: Scroll::default(),
                });
                vec![Action::LoadLists]
            }
            (Key::Enter, ShelfTarget::Genre(genre)) => self.start_radio(genre),
            (Key::Ctrl('d'), ShelfTarget::Genre(genre)) => {
                vec![Action::RemoveFavorite(genre.id.clone())]
            }
            (Key::Enter, ShelfTarget::Genres) => {
                self.views.push(View::Genres {
                    selected: 0,
                    scroll: Scroll::default(),
                });
                Vec::new()
            }
            (Key::Enter, ShelfTarget::Artist) => {
                if let Some(artist) = self.shelf_artists.get(selected - lists) {
                    let albums = albums_of(&self.shelf, &artist.id);
                    self.views.push(View::Artist(ArtistView {
                        artist: artist.clone(),
                        source: Source::Shelf(albums),
                        selected: 0,
                        scroll: Scroll::default(),
                    }));
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn shelf_selected(&self) -> usize {
        match self.view() {
            View::Shelf { selected, .. } => *selected,
            _ => 0,
        }
    }

    /// Genres: Enter starts the genre's radio and Ctrl+A adds the genre to the home page or
    /// removes it from there.
    fn genres_key(&mut self, key: Key) -> Vec<Action> {
        let all = genres::all();
        let Some(View::Genres { selected, .. }) = self.views.last_mut() else {
            return Vec::new();
        };
        if step(selected, key, all.len()) {
            return Vec::new();
        }
        let Some(genre) = all.get(*selected) else {
            return Vec::new();
        };
        match key {
            Key::Enter => self.start_radio(genre),
            Key::Ctrl('a') if self.favorite(&genre.id) => {
                vec![Action::RemoveFavorite(genre.id.clone())]
            }
            Key::Ctrl('a') => vec![Action::AddFavorite(genre.id.clone())],
            _ => Vec::new(),
        }
    }

    /// Radio view: Enter plays from the selected track, Ctrl+E adds the track to the queue,
    /// Ctrl+A adds the track's album to the shelf or removes it from there, and Ctrl+R
    /// starts the genre's radio from a new seed.
    fn radio_key(&mut self, key: Key, now: Instant) -> Vec<Action> {
        let View::Radio(view) = self.view() else {
            return Vec::new();
        };
        let genre = view.genre;
        let selected = view.selected;
        if key == Key::Ctrl('r') {
            let current = self
                .playback
                .radio
                .as_ref()
                .filter(|radio| radio.genre == genre.id)
                .map(|radio| radio.seed.clone());
            return self.request_station(genre, current.as_deref());
        }
        let tracks = self.radio_tracks(&genre.id).unwrap_or_default();
        let len = tracks.len();
        match (key, tracks.get(selected)) {
            (Key::Enter, Some(_)) => self.play_radio(selected),
            (Key::Ctrl('e'), Some(track)) => {
                let track = radio_queued(track);
                let name = track.name.clone();
                self.enqueue(vec![track], &name, now)
            }
            (Key::Ctrl('a'), Some(track)) if self.on_shelf(&track.album.uri) => {
                vec![Action::RemoveFromShelf(track.album.uri.clone())]
            }
            (Key::Ctrl('a'), Some(track)) => {
                vec![Action::AddRadioAlbumToShelf(track.album.uri.clone())]
            }
            _ => {
                if let Some(View::Radio(view)) = self.views.last_mut() {
                    step(&mut view.selected, key, len);
                }
                Vec::new()
            }
        }
    }

    /// Enter on a genre: starts the genre's radio from a random seed. The radio view opens
    /// once the station has been fetched. If the genre's radio is already playing, only
    /// opens the view.
    fn start_radio(&mut self, genre: &'static Genre) -> Vec<Action> {
        if self.playing_genre() == Some(genre.id.as_str()) {
            let selected = self.playback.radio.as_ref().map_or(0, |radio| radio.index);
            self.views.push(View::Radio(RadioView {
                genre,
                selected,
                scroll: Scroll::default(),
            }));
            return Vec::new();
        }
        self.request_station(genre, None)
    }

    /// Fetches the genre's station from a seed that is not `current`.
    fn request_station(&mut self, genre: &Genre, current: Option<&str>) -> Vec<Action> {
        let random = self.random();
        let seed = genre.pick_seed(current, &[], random).uri.clone();
        let request = RadioRequest {
            genre: genre.id.clone(),
            seed,
            previous: Vec::new(),
            kind: RadioLoad::Station,
        };
        self.radio_fetch.pending = Some(request.clone());
        vec![Action::LoadRadio(request)]
    }

    /// A station page arrived. A new station replaces the playback context and opens the
    /// radio view (with Ctrl+R the view is already open); a failed fetch changes nothing.
    fn radio_loaded(
        &mut self,
        request: RadioRequest,
        result: Result<Vec<radio::Track>, String>,
    ) -> Vec<Action> {
        if self.radio_fetch.pending.as_ref() != Some(&request) {
            return Vec::new();
        }
        self.radio_fetch.pending = None;
        let page = match result {
            Ok(page) => page,
            Err(message) => {
                self.error = Some(message);
                if request.kind == RadioLoad::More {
                    self.radio_fetch.failed_at = self.playback.radio.as_ref().map(|r| r.index);
                }
                return Vec::new();
            }
        };
        match request.kind {
            RadioLoad::Station => self.new_station(request, page),
            RadioLoad::More => {
                let actions = self.more_radio(page);
                self.trim_radio_history();
                actions
            }
        }
    }

    fn new_station(&mut self, request: RadioRequest, page: Vec<radio::Track>) -> Vec<Action> {
        let Some(genre) = genres::find(&request.genre) else {
            return Vec::new();
        };
        let tracks = radio::new_tracks(page, &[]);
        let Some(first) = tracks.first() else {
            self.error = Some(format!("the {} radio station is empty", genre.name));
            return Vec::new();
        };
        let item = Item::Track(first.uri.clone());
        let mut actions = self
            .likes
            .check(tracks.iter().map(|track| track.uri.as_str()));
        self.error = None;
        self.radio_fetch = RadioFetch::default();
        let playback = &mut self.playback;
        playback.album = None;
        playback.played.clear();
        playback.current = Some(Current::Radio);
        playback.radio = Some(RadioPosition {
            genre: request.genre,
            seed: request.seed.clone(),
            seeds: vec![request.seed],
            seed_start: 0,
            tracks,
            index: 0,
        });
        match self.views.last_mut() {
            Some(View::Radio(view)) => {
                view.genre = genre;
                view.selected = 0;
            }
            _ => self.views.push(View::Radio(RadioView {
                genre,
                selected: 0,
                scroll: Scroll::default(),
            })),
        }
        actions.insert(0, Action::Play(item));
        actions
    }

    /// A refill page: new tracks go to the end of the radio. If little new came in, the
    /// station is repeating itself, and the next fetch uses the genre's next unplayed seed.
    fn more_radio(&mut self, page: Vec<radio::Track>) -> Vec<Action> {
        let random = self.random();
        let Some(radio) = &mut self.playback.radio else {
            return Vec::new();
        };
        let added = radio::new_tracks(page, &radio.tracks);
        let fresh = added.len();
        let actions = self
            .likes
            .check(added.iter().map(|track| track.uri.as_str()));
        radio.tracks.extend(added);
        self.error = None;
        self.radio_fetch.failed_at = None;
        self.radio_fetch.empty = if fresh == 0 {
            self.radio_fetch.empty + 1
        } else {
            0
        };
        if fresh >= RADIO_FRESH_MIN {
            return actions;
        }
        let Some(genre) = genres::find(&radio.genre) else {
            return actions;
        };
        let seed = genre
            .pick_seed(Some(&radio.seed), &radio.seeds, random)
            .uri
            .clone();
        if !radio.seeds.contains(&seed) {
            radio.seeds.push(seed.clone());
        }
        radio.seed = seed;
        radio.seed_start = radio.tracks.len();
        actions
    }

    /// Drops played tracks from the start of the radio when more than `RADIO_HISTORY`
    /// precede the playing one. Indices into the radio (the playing track, the seed's
    /// start, and the radio view's selection and scroll position) shift along with them.
    fn trim_radio_history(&mut self) {
        let Some(radio) = &mut self.playback.radio else {
            return;
        };
        let drop = radio.index.saturating_sub(RADIO_HISTORY);
        if drop == 0 {
            return;
        }
        radio.tracks.drain(..drop);
        radio.index -= drop;
        radio.seed_start = radio.seed_start.saturating_sub(drop);
        self.radio_fetch.failed_at = self
            .radio_fetch
            .failed_at
            .map(|index| index.saturating_sub(drop));
        for view in &mut self.views {
            if let View::Radio(view) = view
                && view.genre.id == radio.genre
            {
                view.selected = view.selected.saturating_sub(drop);
                view.scroll.set(view.scroll.get().saturating_sub(drop));
            }
        }
    }

    /// A refill when fewer than `RADIO_REFILL` tracks remain after the playing radio track
    /// and no fetch is already running or has failed during this track.
    fn radio_refill(&mut self) -> Option<Action> {
        self.playback.current.as_ref()?;
        let radio = self.playback.radio.as_ref()?;
        let fetch = &self.radio_fetch;
        if fetch.pending.is_some()
            || fetch.failed_at == Some(radio.index)
            || radio.tracks.len() >= radio.index + 1 + RADIO_REFILL
        {
            return None;
        }
        let seeds = genres::find(&radio.genre).map_or(0, |genre| genre.seeds.len());
        if fetch.empty > seeds {
            return None;
        }
        let request = RadioRequest {
            genre: radio.genre.clone(),
            seed: radio.seed.clone(),
            previous: radio.tracks[radio.seed_start.min(radio.tracks.len())..]
                .iter()
                .map(|track| track.uri.clone())
                .collect(),
            kind: RadioLoad::More,
        };
        self.radio_fetch.pending = Some(request.clone());
        Some(Action::LoadRadio(request))
    }

    /// The next random number (xorshift).
    fn random(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }

    /// Curated list: Enter opens the album's tracks, Ctrl+E adds the album to the queue,
    /// Ctrl+A adds it to the shelf or removes it from there, and Ctrl+D rejects it.
    fn curated_key(&mut self, key: Key) -> Vec<Action> {
        let View::Curated { selected, .. } = *self.view() else {
            return Vec::new();
        };
        let albums = self.curated.as_ref().map_or(&[][..], |list| &list.albums);
        let len = albums.len();
        let Some(album) = albums.get(selected).map(|entry| entry.album.clone()) else {
            return Vec::new();
        };
        match key {
            Key::Enter => self.open_album(album),
            Key::Ctrl('e') => vec![Action::QueueAlbum(album)],
            Key::Ctrl('a') => toggle_shelf(&self.shelf_uris, &album),
            Key::Ctrl('d') => vec![Action::RejectCurated(album.uri)],
            _ => {
                if let Some(View::Curated { selected, .. }) = self.views.last_mut() {
                    step(selected, key, len);
                }
                Vec::new()
            }
        }
    }

    fn artist_key(&mut self, key: Key) -> Vec<Action> {
        let View::Artist(view) = self.view() else {
            return Vec::new();
        };
        let albums = view.albums();
        let len = albums.len();
        let from_shelf = matches!(view.source, Source::Shelf(_));
        let Some(album) = albums.get(view.selected) else {
            return Vec::new();
        };
        match key {
            Key::Enter => self.open_album(album.clone()),
            Key::Ctrl('e') => vec![Action::QueueAlbum(album.clone())],
            Key::Ctrl('d') if from_shelf => vec![Action::RemoveFromShelf(album.uri.clone())],
            Key::Ctrl('a') => toggle_shelf(&self.shelf_uris, album),
            _ => {
                if let Some(View::Artist(view)) = self.views.last_mut() {
                    step(&mut view.selected, key, len);
                }
                Vec::new()
            }
        }
    }

    /// Track list: Enter plays the album from the selected track, Ctrl+E adds the track to
    /// the queue, and Ctrl+A adds the album to the shelf or removes it from there.
    fn album_key(&mut self, key: Key, now: Instant) -> Vec<Action> {
        let View::Album(view) = self.view() else {
            return Vec::new();
        };
        let tracks = view.tracks.as_deref().unwrap_or_default();
        let len = tracks.len();
        match key {
            Key::Enter if view.selected < len => {
                let album = view.album.clone();
                let track = view.selected;
                self.play_album(album, len, track)
            }
            Key::Ctrl('e') if view.selected < len => {
                let track = queued(&view.album, tracks, view.selected);
                let name = track.name.clone();
                self.enqueue(vec![track], &name, now)
            }
            // A radio track's album (n from the queue) has no year: when adding it to the
            // shelf, the year and the album's artist are fetched as in the radio view.
            Key::Ctrl('a') if view.album.year.is_none() && !self.on_shelf(&view.album.uri) => {
                vec![Action::AddRadioAlbumToShelf(view.album.uri.clone())]
            }
            Key::Ctrl('a') => toggle_shelf(&self.shelf_uris, &view.album),
            _ => {
                if let Some(View::Album(view)) = self.views.last_mut() {
                    step(&mut view.selected, key, len);
                }
                Vec::new()
            }
        }
    }

    /// A key in the search field.
    fn search_key(&mut self, key: Key, now: Instant) -> Vec<Action> {
        let Some(View::Search(search)) = self.views.last_mut() else {
            return Vec::new();
        };
        match key {
            Key::Char(c) => {
                let at = byte_index(&search.query, search.cursor);
                search.query.insert(at, c);
                search.cursor += 1;
                search.edited(now);
            }
            Key::Backspace if search.cursor > 0 => {
                search.cursor -= 1;
                let at = byte_index(&search.query, search.cursor);
                search.query.remove(at);
                search.edited(now);
            }
            Key::Left => search.cursor = search.cursor.saturating_sub(1),
            Key::Right => search.cursor = (search.cursor + 1).min(search.query.chars().count()),
            Key::Down | Key::Enter if !search.items().is_empty() => {
                search.in_results = true;
                search.selected = 0;
            }
            _ => {}
        }
        Vec::new()
    }

    /// A key in the search results; the keys every view shares are handled already.
    fn search_results_key(&mut self, key: Key, now: Instant) -> Vec<Action> {
        let Some(View::Search(search)) = self.views.last_mut() else {
            return Vec::new();
        };
        match key {
            Key::Up if search.selected == 0 => search.in_results = false,
            Key::Up | Key::Down => {
                let len = search.items().len();
                step(&mut search.selected, key, len);
            }
            Key::Enter => match search.items().get(search.selected) {
                Some(SearchItem::Artist(artist)) => {
                    let artist = (*artist).clone();
                    let action = Action::LoadArtistAlbums(artist.id.clone());
                    self.views.push(View::Artist(ArtistView {
                        artist,
                        source: Source::Spotify(None),
                        selected: 0,
                        scroll: Scroll::default(),
                    }));
                    return vec![action];
                }
                Some(SearchItem::Album(album)) => {
                    let album = (*album).clone();
                    return self.open_album(album);
                }
                Some(SearchItem::Song(song)) => {
                    let song = (*song).clone();
                    return self.play_song(song);
                }
                None => {}
            },
            Key::Ctrl('a') => match search.items().get(search.selected) {
                Some(SearchItem::Album(album)) => return toggle_shelf(&self.shelf_uris, album),
                Some(SearchItem::Song(song)) => {
                    return toggle_shelf(&self.shelf_uris, &song.album);
                }
                _ => {}
            },
            Key::Ctrl('e') => match search.items().get(search.selected) {
                Some(SearchItem::Album(album)) => {
                    return vec![Action::QueueAlbum((*album).clone())];
                }
                Some(SearchItem::Song(song)) => {
                    let track = song_queued(song);
                    let name = track.name.clone();
                    return self.enqueue(vec![track], &name, now);
                }
                _ => {}
            },
            _ => {}
        }
        Vec::new()
    }

    /// Sends the search once typing has paused.
    fn tick(&mut self, now: Instant) -> Vec<Action> {
        let Some(search) = search_view(&mut self.views) else {
            return Vec::new();
        };
        match search.due {
            Some(due) if now >= due => {
                search.due = None;
                let query = search.query.trim().to_owned();
                if search.requested.as_ref() == Some(&query) {
                    return Vec::new();
                }
                search.requested = Some(query.clone());
                search.pending = true;
                vec![Action::Search(query)]
            }
            _ => Vec::new(),
        }
    }

    fn searched(&mut self, query: String, result: Result<SearchResults, String>) {
        let Some(search) = search_view(&mut self.views) else {
            return;
        };
        if !search.pending || search.requested.as_ref() != Some(&query) {
            return;
        }
        search.pending = false;
        match result {
            Ok(results) => {
                search.results = Some(results);
                search.selected = 0;
                search.in_results &= !search.items().is_empty();
                self.error = None;
            }
            Err(message) => {
                search.requested = None;
                self.error = Some(message);
            }
        }
    }

    fn album_tracks(&mut self, uri: &str, result: Result<Vec<Track>, String>) -> Vec<Action> {
        if let Ok(tracks) = &result {
            self.learn_length(uri, tracks.len());
        }
        let mut actions = match &result {
            Ok(tracks) => self
                .likes
                .check(tracks.iter().map(|track| track.uri.as_str())),
            Err(_) => Vec::new(),
        };
        for view in &mut self.views {
            if let View::Album(view) = view
                && view.album.uri == uri
            {
                let tracks = result.clone().unwrap_or_default();
                if let Some(uri) = view.select_uri.take()
                    && let Some(index) = tracks.iter().position(|track| track.uri == uri)
                {
                    view.selected = index;
                }
                clamp(&mut view.selected, tracks.len());
                view.tracks = Some(tracks);
            }
        }
        if self.play_on_load.as_deref() == Some(uri) {
            self.play_on_load = None;
            let album = self.views.iter().rev().find_map(|view| match view {
                View::Album(view) if view.album.uri == uri => {
                    Some((view.album.clone(), view.selected))
                }
                _ => None,
            });
            if let (Ok(tracks), Some((album, track))) = (&result, album)
                && !tracks.is_empty()
            {
                actions.extend(self.play_album(album, tracks.len(), track));
            }
        }
        match result {
            Ok(_) => self.error = None,
            Err(message) => self.error = Some(message),
        }
        actions
    }

    /// The album's tracks arrived, to be added to the queue.
    fn album_queued(
        &mut self,
        album: &Album,
        only: Option<&str>,
        result: Result<Vec<Track>, String>,
        now: Instant,
    ) -> Vec<Action> {
        match result {
            Ok(tracks) => {
                self.error = None;
                self.learn_length(&album.uri, tracks.len());
                let indices: Vec<usize> = (0..tracks.len())
                    .filter(|&index| only.is_none_or(|uri| tracks[index].uri == uri))
                    .collect();
                if indices.is_empty() {
                    self.error = Some("could not queue the song: it is not on its album".into());
                    return Vec::new();
                }
                let mut actions = self
                    .likes
                    .check(indices.iter().map(|&index| tracks[index].uri.as_str()));
                // A song is confirmed by its own name, an album by the album's.
                let name = match only {
                    Some(_) => tracks[indices[0]].name.clone(),
                    None => album.name.clone(),
                };
                let tracks = indices
                    .into_iter()
                    .map(|index| queued(album, &tracks, index))
                    .collect();
                actions.splice(0..0, self.enqueue(tracks, &name, now));
                actions
            }
            Err(message) => {
                self.error = Some(message);
                Vec::new()
            }
        }
    }

    fn artist_albums(&mut self, artist_id: &str, result: Result<Vec<Album>, String>) {
        for view in &mut self.views {
            if let View::Artist(ArtistView {
                artist,
                source: source @ Source::Spotify(None),
                ..
            }) = view
                && artist.id == artist_id
            {
                let albums = match &result {
                    Ok(albums) => {
                        let mut albums = albums.clone();
                        sort_by_year(&mut albums);
                        albums
                    }
                    Err(_) => Vec::new(),
                };
                *source = Source::Spotify(Some(albums));
            }
        }
        match result {
            Ok(_) => self.error = None,
            Err(message) => self.error = Some(message),
        }
    }

    /// The shelf changed: an artist view whose last album was removed is closed, and
    /// selections are kept within their lists.
    fn shelf_changed(&mut self, albums: Vec<Album>) {
        self.set_shelf(albums);
        self.error = None;
        let shelf = &self.shelf;
        for view in &mut self.views {
            if let View::Artist(ArtistView {
                artist,
                source: Source::Shelf(albums),
                ..
            }) = view
            {
                *albums = albums_of(shelf, &artist.id);
            }
        }
        self.views.retain(|view| match view {
            View::Artist(
                view @ ArtistView {
                    source: Source::Shelf(_),
                    ..
                },
            ) => !view.albums().is_empty(),
            _ => true,
        });
        let rows = self.shelf_rows();
        for view in &mut self.views {
            match view {
                View::Shelf { selected, .. } => clamp(selected, rows),
                View::Artist(view) => {
                    let len = view.albums().len();
                    clamp(&mut view.selected, len);
                }
                View::Curated { .. }
                | View::Search(_)
                | View::Album(_)
                | View::Queue { .. }
                | View::Genres { .. }
                | View::Radio(_) => {}
            }
        }
    }

    /// The lists were read: a broken file is shown as an error and its list row is hidden.
    fn lists_loaded(&mut self, result: Result<Option<AlbumList>, String>) {
        match result {
            Ok(list) => {
                if self.lists_broken {
                    self.error = None;
                    self.lists_broken = false;
                }
                self.set_curated(list);
            }
            Err(message) => {
                self.error = Some(message);
                self.lists_broken = true;
                self.set_curated(None);
            }
        }
    }

    /// Replaces the Curated list. The list view's selection stays within the list.
    fn set_curated(&mut self, list: Option<AlbumList>) {
        self.set_lists(|state| state.curated = list);
        let len = self.curated.as_ref().map_or(0, |list| list.albums.len());
        for view in &mut self.views {
            if let View::Curated { selected, .. } = view {
                clamp(selected, len);
            }
        }
    }

    /// Changes the list rows (Curated or favourite genres). The shelf's selection stays on
    /// the same artist even as list rows appear or disappear. On a list row the selection
    /// stays at the same position, so the next row takes the removed row's place.
    fn set_lists(&mut self, change: impl FnOnce(&mut Self)) {
        let before = self.shelf_lists().len();
        change(self);
        let after = self.shelf_lists().len();
        let rows = self.shelf_rows();
        for view in &mut self.views {
            if let View::Shelf { selected, .. } = view {
                if *selected >= before {
                    *selected = *selected - before + after;
                }
                clamp(selected, rows);
            }
        }
    }

    /// Ctrl+L: likes the playing track or removes the like. An unknown state counts as
    /// not liked, and while a change is pending the key does nothing.
    fn toggle_like(&mut self) -> Vec<Action> {
        let Some(playing) = self.now_playing.as_ref().filter(|p| !p.uri.is_empty()) else {
            return Vec::new();
        };
        if !self.likes.enabled {
            self.error = Some(LIKES_NEED_APP.to_owned());
            return Vec::new();
        }
        if self.likes.changing.contains_key(&playing.uri) {
            return Vec::new();
        }
        let uri = playing.uri.clone();
        let liked = !self.liked(&uri);
        self.likes
            .changing
            .insert(uri.clone(), playing.track.clone());
        vec![Action::SetLiked { uri, liked }]
    }

    fn liked_checked(&mut self, uris: Vec<String>, result: Result<Vec<bool>, String>) {
        match result {
            Ok(flags) => {
                // A like made during the query is newer information than the answer.
                for (uri, liked) in uris.into_iter().zip(flags) {
                    self.likes.known.entry(uri).or_insert(liked);
                }
            }
            Err(message) => {
                self.likes.stopped = true;
                self.error = Some(message);
            }
        }
    }

    fn like_changed(&mut self, uri: String, liked: bool, result: Result<(), String>, now: Instant) {
        let name = self.likes.changing.remove(&uri).unwrap_or_default();
        match result {
            Ok(()) => {
                self.error = None;
                self.likes.known.insert(uri, liked);
                self.notice = Some(Notice {
                    kind: if liked {
                        NoticeKind::Liked
                    } else {
                        NoticeKind::Unliked
                    },
                    name,
                    until: now + NOTICE_TIME,
                });
            }
            Err(message) => self.error = Some(message),
        }
    }

    fn player(&mut self, event: player::Event, now: Instant) -> Vec<Action> {
        match event {
            player::Event::TrackChanged {
                uri,
                artist,
                album,
                track,
                duration,
                cover,
            } => {
                // Usually the state has already been asked along with the track list or
                // radio; e.g. the tracks of a restored album have not.
                let actions = self.likes.check([uri.as_str()]);
                self.now_playing = Some(NowPlaying {
                    uri,
                    artist,
                    album,
                    track,
                    duration,
                    cover,
                    paused: false,
                    position: Duration::ZERO,
                    position_at: now,
                });
                // Loading the restored track is not a user action: a startup error
                // (e.g. a broken shelf) stays visible.
                if !self.restoring {
                    self.error = None;
                }
                return actions;
            }
            player::Event::EndOfTrack => {
                if let Some(action) = self.next() {
                    return vec![action];
                }
                self.stopped();
                return vec![Action::Stop];
            }
            player::Event::PreloadNext => {
                return self.upcoming().map(Action::Preload).into_iter().collect();
            }
            player::Event::Position(ms) => {
                self.restoring = false;
                if let Some(playing) = &mut self.now_playing {
                    playing.position = Duration::from_millis(ms.into());
                    playing.position_at = now;
                    playing.paused = false;
                }
            }
            player::Event::Paused(ms) => {
                if let Some(playing) = &mut self.now_playing {
                    playing.position = Duration::from_millis(ms.into()).min(playing.duration);
                    playing.position_at = now;
                    playing.paused = true;
                }
                if self.restoring {
                    self.restoring = false;
                } else {
                    self.error = None;
                }
            }
            player::Event::Stopped => self.stopped(),
            // The restored track could not be loaded (network, removed album): Deck
            // continues empty.
            player::Event::Error(message) if self.restoring => {
                self.restoring = false;
                self.playback = Playback::default();
                self.now_playing = None;
                self.error = Some(format!("could not restore playback: {message}"));
            }
            player::Event::Error(message) => self.error = Some(message),
        }
        Vec::new()
    }
}

/// Album track `index`, to be added to the queue.
fn queued(album: &Album, tracks: &[Track], index: usize) -> QueuedTrack {
    let track = &tracks[index];
    QueuedTrack {
        uri: track.uri.clone(),
        name: track.name.clone(),
        artist: track.artist.clone(),
        duration: track.duration,
        album_uri: album.uri.clone(),
        index: Some(index),
        album: Some(Box::new(album.clone())),
    }
}

/// A song from the search to be added to the queue. Its place on the album is not known.
fn song_queued(song: &Song) -> QueuedTrack {
    QueuedTrack {
        uri: song.track.uri.clone(),
        name: song.track.name.clone(),
        artist: song.track.artist.clone(),
        duration: song.track.duration,
        album_uri: song.album.uri.clone(),
        index: None,
        album: Some(Box::new(song.album.clone())),
    }
}

/// A radio track to be added to the queue. The station does not report the duration, so
/// it is zero (the queue view leaves it out).
fn radio_queued(track: &radio::Track) -> QueuedTrack {
    QueuedTrack {
        uri: track.uri.clone(),
        name: track.name.clone(),
        artist: track.artist.clone(),
        duration: Duration::ZERO,
        album_uri: track.album.uri.clone(),
        index: None,
        album: Some(Box::new(Album {
            id: spotify_id(&track.album.uri),
            uri: track.album.uri.clone(),
            name: track.album.name.clone(),
            artist: track.artist.clone(),
            artist_id: spotify_id(&track.artist_uri),
            year: None,
        })),
    }
}

/// The search on the stack, if there is one.
fn search_view(views: &mut [View]) -> Option<&mut SearchView> {
    views.iter_mut().find_map(|view| match view {
        View::Search(search) => Some(search),
        _ => None,
    })
}

/// The id part of a URI: `spotify:album:abc` → `abc`.
fn spotify_id(uri: &str) -> String {
    uri.rsplit(':').next().unwrap_or_default().to_owned()
}

/// Where n leads (`State::playing_target`).
enum PlayingTarget {
    /// The album's track list. The track's position, or its URI if the position is missing.
    Album {
        album: Album,
        index: Option<usize>,
        uri: String,
    },
    Radio {
        genre: &'static Genre,
        index: usize,
    },
}

/// Where the next track comes from (`State::upcoming_source`).
enum Upcoming {
    Queue,
    /// An album track.
    Album(usize),
    /// A radio track.
    Radio(usize),
}

/// The selected shelf row, for handling a key.
enum ShelfTarget {
    Curated,
    Genre(&'static Genre),
    Genres,
    Artist,
}

impl ArtistView {
    /// The artist view's albums by year, oldest first.
    pub fn albums(&self) -> &[Album] {
        match &self.source {
            Source::Shelf(albums) => albums,
            Source::Spotify(albums) => albums.as_deref().unwrap_or_default(),
        }
    }
}

impl SearchView {
    /// The results as one list: artists first, then albums, then songs.
    pub fn items(&self) -> Vec<SearchItem<'_>> {
        let Some(results) = &self.results else {
            return Vec::new();
        };
        results
            .artists
            .iter()
            .map(SearchItem::Artist)
            .chain(results.albums.iter().map(SearchItem::Album))
            .chain(results.songs.iter().map(SearchItem::Song))
            .collect()
    }

    /// The search is waiting for the delay or for results.
    pub fn searching(&self) -> bool {
        self.due.is_some() || self.pending
    }

    /// The search text changed: search after the delay; empty text clears the results.
    fn edited(&mut self, now: Instant) {
        if self.query.trim().is_empty() {
            *self = Self {
                query: std::mem::take(&mut self.query),
                cursor: self.cursor,
                ..Self::default()
            };
        } else {
            self.due = Some(now + SEARCH_DEBOUNCE);
        }
    }
}

impl NowPlaying {
    /// Elapsed time: the player's latest position plus the time passed since.
    pub fn elapsed(&self, now: Instant) -> Duration {
        if self.paused {
            return self.position;
        }
        (self.position + now.saturating_duration_since(self.position_at)).min(self.duration)
    }
}

/// Duration in milliseconds; a duration too long to fit is `u64::MAX`.
fn millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

/// Ctrl+A: the album onto the shelf, or off it if already there (`shelf_uris`).
fn toggle_shelf(shelf_uris: &HashSet<String>, album: &Album) -> Vec<Action> {
    if shelf_uris.contains(&album.uri) {
        vec![Action::RemoveFromShelf(album.uri.clone())]
    } else {
        vec![Action::AddToShelf(album.clone())]
    }
}

/// Moves the selection within the list with ↑/↓. Returns `true` if the key was an arrow.
fn step(selected: &mut usize, key: Key, len: usize) -> bool {
    match key {
        Key::Up => *selected = selected.saturating_sub(1),
        Key::Down => *selected = (*selected + 1).min(len.saturating_sub(1)),
        _ => return false,
    }
    true
}

fn clamp(selected: &mut usize, len: usize) {
    *selected = (*selected).min(len.saturating_sub(1));
}

/// The byte offset of a character index in a string.
fn byte_index(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map_or(text.len(), |(i, _)| i)
}

#[cfg(test)]
mod tests;
