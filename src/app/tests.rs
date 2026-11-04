//! Tests of `State::update`, one file per topic in `tests/`. The fixtures shared by
//! several topics are here; a fixture used by one topic stays in its file.

use super::*;
use crate::lists::{Curated, ListAlbum, TEST_NOW};
use Key::*;

fn album(id: &str, name: &str, artist: &str, year: Option<u16>) -> Album {
    Album {
        id: id.to_owned(),
        uri: format!("spotify:album:{id}"),
        name: name.to_owned(),
        artist: artist.to_owned(),
        artist_id: artist_id(artist),
        year,
    }
}

fn artist(name: &str) -> Artist {
    Artist {
        id: artist_id(name),
        name: name.to_owned(),
    }
}

fn artist_id(name: &str) -> String {
    format!("id-{}", name.to_lowercase().replace(' ', "-"))
}

fn bleach() -> Album {
    album("bleach", "Bleach", "Nirvana", Some(1989))
}
fn nevermind() -> Album {
    album("nevermind", "Nevermind", "Nirvana", Some(1991))
}
fn in_utero() -> Album {
    album("inutero", "In Utero", "Nirvana", Some(1993))
}
fn ok_computer() -> Album {
    album("okc", "OK Computer", "Radiohead", Some(1997))
}
fn colour() -> Album {
    album(
        "colour",
        "The Colour And The Shape",
        "Foo Fighters",
        Some(1997),
    )
}

/// The state and the test's clock.
struct App {
    state: State,
    now: Instant,
}

impl App {
    /// Shelf with the selection on the first artist: the list group always has at least
    /// the Curated and Genres rows, and the selection starts on the top one.
    fn new(shelf: Vec<Album>) -> Self {
        let mut app = Self::at_top(shelf);
        for _ in 0..app.state.shelf_lists().len() {
            app.key(Down);
        }
        app
    }

    /// Shelf with the selection on the top list row, as at startup.
    fn at_top(shelf: Vec<Album>) -> Self {
        Self {
            state: State::new(shelf),
            now: Instant::now(),
        }
    }

    fn send(&mut self, event: Event) -> Vec<Action> {
        self.state.update(event, self.now)
    }

    fn key(&mut self, key: Key) -> Vec<Action> {
        self.send(Event::Key(key))
    }

    fn keys(&mut self, keys: &[Key]) -> Vec<Action> {
        keys.iter().flat_map(|&k| self.key(k)).collect()
    }

    fn type_text(&mut self, text: &str) -> Vec<Action> {
        text.chars().flat_map(|c| self.key(Char(c))).collect()
    }

    /// Lets time pass and sends a tick.
    fn wait(&mut self, ms: u64) -> Vec<Action> {
        self.now += Duration::from_millis(ms);
        self.send(Event::Tick)
    }

    /// Types a search, waits for the delay and returns the results.
    fn search(&mut self, text: &str, artists: Vec<Artist>, albums: Vec<Album>) {
        self.search_all(text, artists, albums, vec![]);
    }

    /// As `search`, with songs too.
    fn search_all(
        &mut self,
        text: &str,
        artists: Vec<Artist>,
        albums: Vec<Album>,
        songs: Vec<Song>,
    ) {
        self.type_text(text);
        let actions = self.wait(300);
        assert_eq!(actions, [Action::Search(text.to_owned())]);
        self.send(Event::Searched {
            query: text.to_owned(),
            result: Ok(SearchResults {
                artists,
                albums,
                songs,
            }),
        });
    }

    fn shelf_selected(&self) -> usize {
        match self.state.view() {
            View::Shelf { selected, .. } => *selected,
            other => panic!("expected the shelf, got {other:?}"),
        }
    }

    fn artist_view(&self) -> &ArtistView {
        match self.state.view() {
            View::Artist(view) => view,
            other => panic!("expected an artist view, got {other:?}"),
        }
    }

    fn artist_albums(&self) -> Vec<String> {
        self.artist_view()
            .albums()
            .iter()
            .map(|a| a.name.clone())
            .collect()
    }

    fn album_view(&self) -> &AlbumView {
        match self.state.view() {
            View::Album(view) => view,
            other => panic!("expected a track list, got {other:?}"),
        }
    }

    fn search_view(&self) -> &SearchView {
        match self.state.view() {
            View::Search(view) => view,
            other => panic!("expected search, got {other:?}"),
        }
    }

    fn is_shelf(&self) -> bool {
        matches!(self.state.view(), View::Shelf { .. })
    }
}

fn shelf_app() -> App {
    App::new(vec![
        in_utero(),
        ok_computer(),
        nevermind(),
        colour(),
        bleach(),
    ])
}

fn play(album_id: &str, track: usize) -> Action {
    Action::Play(Item::Album {
        uri: format!("spotify:album:{album_id}"),
        track,
    })
}

fn load_tracks(album_id: &str) -> Action {
    Action::LoadAlbumTracks(format!("spotify:album:{album_id}"))
}

fn names(artists: &[Artist]) -> Vec<&str> {
    artists.iter().map(|a| a.name.as_str()).collect()
}

fn track(name: &str, secs: u64) -> Track {
    Track {
        uri: track_uri(name),
        name: name.to_owned(),
        artist: "Nirvana".to_owned(),
        duration: Duration::from_secs(secs),
    }
}

fn track_uri(name: &str) -> String {
    format!("spotify:track:{}", name.to_lowercase().replace(' ', "-"))
}

fn nevermind_tracks() -> Vec<Track> {
    vec![
        track("Smells Like Teen Spirit", 301),
        track("In Bloom", 254),
        track("Come As You Are", 218),
        track("Breed", 183),
    ]
}

fn album_tracks(album: &Album, result: Result<Vec<Track>, String>) -> Event {
    Event::AlbumTracks {
        uri: album.uri.clone(),
        result,
    }
}

/// The player started track `name`.
fn started(name: &str) -> Event {
    Event::Player(player::Event::TrackChanged {
        uri: track_uri(name),
        artist: "Nirvana".into(),
        album: "Nevermind".into(),
        track: name.into(),
        duration: Duration::from_secs(200),
        cover: None,
    })
}

fn end_of_track() -> Event {
    Event::Player(player::Event::EndOfTrack)
}

/// Shelf › Nirvana › Nevermind open, tracks arrived. Nothing is playing.
fn nevermind_app() -> App {
    let mut app = shelf_app();
    app.keys(&[Down, Enter, Down]);
    assert_eq!(app.key(Enter), [load_tracks("nevermind")]);
    app.send(album_tracks(&nevermind(), Ok(nevermind_tracks())));
    app
}

fn spotify_artist_app() -> App {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    app.search("nirvana", vec![artist("Nirvana")], vec![]);
    app.key(Down);
    assert_eq!(
        app.key(Enter),
        [Action::LoadArtistAlbums("id-nirvana".into())]
    );
    app
}

fn unplugged() -> Album {
    album(
        "unplugged",
        "MTV Unplugged In New York",
        "Nirvana",
        Some(1994),
    )
}

fn no_other() -> Album {
    album("noother", "No Other", "Gene Clark", Some(1974))
}

fn three_curated() -> AlbumList {
    curated(&[no_other(), number_one(), transatlanticism()])
}

/// Shelf and a Curated list (three albums).
fn curated_app() -> App {
    let mut app = shelf_app();
    app.state = State::new(app.state.shelf.clone()).with_curated(Ok(Some(three_curated())));
    app
}

fn lofi() -> &'static Genre {
    genres::find("lofi").unwrap()
}

/// Shelf and favourite genres; the selection on the first row below Curated.
fn genre_app(favorites: &[&str]) -> App {
    let mut app = App::at_top(shelf_app().state.shelf);
    app.state = State::new(app.state.shelf.clone())
        .with_favorites(favorites.iter().map(|id| id.to_string()).collect());
    app.key(Down);
    app
}

fn radio_track(n: usize) -> radio::Track {
    radio::Track {
        uri: format!("spotify:track:r{n}"),
        name: format!("Song {n}"),
        artist: format!("Artist {n}"),
        artist_uri: format!("spotify:artist:a{n}"),
        album: radio::Album {
            uri: format!("spotify:album:record{n}"),
            name: format!("Record {n}"),
        },
    }
}

fn station(range: std::ops::Range<usize>) -> Vec<radio::Track> {
    range.map(radio_track).collect()
}

fn radio_play(n: usize) -> Action {
    Action::Play(Item::Track(format!("spotify:track:r{n}")))
}

/// The only radio fetch among the actions.
fn radio_request(actions: &[Action]) -> RadioRequest {
    let requests: Vec<&RadioRequest> = actions
        .iter()
        .filter_map(|a| match a {
            Action::LoadRadio(request) => Some(request),
            _ => None,
        })
        .collect();
    assert_eq!(requests.len(), 1, "{actions:?}");
    requests[0].clone()
}

fn radio_loaded(request: &RadioRequest, tracks: Vec<radio::Track>) -> Event {
    Event::RadioLoaded {
        request: request.clone(),
        result: Ok(tracks),
    }
}

/// Enter on the selected genre and station `tracks`: the radio plays the first track.
fn start_radio(app: &mut App, tracks: Vec<radio::Track>) -> RadioRequest {
    let request = radio_request(&app.key(Enter));
    let first = tracks[0].uri.clone();
    let actions = app.send(radio_loaded(&request, tracks));
    assert_eq!(actions, [Action::Play(Item::Track(first))]);
    app.send(started("Song"));
    request
}

fn radio_of(app: &App) -> &RadioPosition {
    app.state.playback.radio.as_ref().expect("radio playing")
}

fn radio_view(app: &App) -> &RadioView {
    match app.state.view() {
        View::Radio(view) => view,
        other => panic!("expected the radio, got {other:?}"),
    }
}

/// Plays the radio forward until track `n` plays. Returns the actions.
fn play_until(app: &mut App, n: usize) -> Vec<Action> {
    let mut actions = Vec::new();
    while radio_of(app).index < n {
        actions = app.send(end_of_track());
        app.send(started("Song"));
    }
    actions
}

fn set_liked(name: &str, liked: bool) -> Action {
    Action::SetLiked {
        uri: track_uri(name),
        liked,
    }
}

/// In Bloom is playing, and its like has not been asked yet.
fn in_bloom_playing() -> App {
    let mut app = likes_app();
    assert_eq!(app.send(started("In Bloom")), [check(&["In Bloom"])]);
    app
}

/// Nevermind playing from In Bloom at 1:23, with Come As You Are and
/// Breed in the queue.
fn in_bloom_with_queue() -> App {
    let mut app = nevermind_playing(1);
    app.send(started("In Bloom"));
    queue_track(&mut app, 2);
    queue_track(&mut app, 3);
    app.send(Event::Player(player::Event::Position(83_000)));
    app
}

/// Quitting with `q`: the event loop saves `playback` before quitting.
fn quit(app: &mut App) -> SavedPlayback {
    assert_eq!(app.key(Char('q')), [Action::Quit]);
    app.state.playback(app.now).expect("something is playing")
}

/// A new Deck that restores the saved playback state.
fn restored(saved: SavedPlayback) -> (App, Vec<Action>) {
    let mut app = shelf_app();
    let actions = app.send(Event::Restore(Box::new(saved)));
    (app, actions)
}

fn play_track(name: &str) -> Action {
    Action::Play(Item::Track(track_uri(name)))
}

fn queue_names(app: &App) -> Vec<&str> {
    app.state.queue().iter().map(|t| t.name.as_str()).collect()
}

/// Nevermind playing from track `track`.
fn nevermind_playing(track: usize) -> App {
    let mut app = nevermind_app();
    app.keys(&vec![Down; track]);
    assert_eq!(app.key(Enter), [play("nevermind", track)]);
    app.send(started("track"));
    app
}

/// Adds the selected track to the queue in the track list.
fn queue_track(app: &mut App, index: usize) -> Vec<Action> {
    while app.album_view().selected > index {
        app.key(Up);
    }
    while app.album_view().selected < index {
        app.key(Down);
    }
    app.key(Ctrl('e'))
}

fn queue_bleach(app: &mut App) -> Vec<Action> {
    app.send(Event::AlbumQueued {
        album: bleach(),
        track: None,
        result: Ok(bleach_tracks()),
    })
}

fn number_one() -> Album {
    album("number1", "#1 Record", "Big Star", Some(1972))
}

fn transatlanticism() -> Album {
    album(
        "transat",
        "Transatlanticism",
        "Death Cab for Cutie",
        Some(2003),
    )
}

/// A Curated list for the view, round 2026-W41.
fn curated(albums: &[Album]) -> AlbumList {
    Curated {
        round: "2026-W41".to_owned(),
        created: "2026-10-05T07:00:00+03:00".to_owned(),
        albums: albums
            .iter()
            .map(|album| ListAlbum {
                album: album.clone(),
                reason: Some(format!("Because of {}.", album.artist)),
                genres: Vec::new(),
            })
            .collect(),
    }
    .to_list(TEST_NOW)
}

/// Shelf with likes enabled.
fn likes_app() -> App {
    let mut app = shelf_app();
    app.state.likes.enabled = true;
    app
}

fn check(names: &[&str]) -> Action {
    Action::CheckLiked(names.iter().map(|name| track_uri(name)).collect())
}

fn bleach_tracks() -> Vec<Track> {
    vec![track("Blew", 175), track("Floyd The Barber", 138)]
}

mod album;
mod browse;
mod curated;
mod genre_radio;
mod letters;
mod likes;
mod playback;
mod queue;
mod radio_refill;
mod search;
