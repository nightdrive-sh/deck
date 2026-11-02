//! Tests of the drawing, one file per topic in `tests/`. The fixtures shared by
//! several topics are here; a fixture used by one topic stays in its file.

use std::time::Duration;

use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, style::Modifier};

use super::*;
use crate::{
    app::{Action, Event, Key},
    catalog::{Artist, SearchResults, Track},
    lists::{Curated, TEST_NOW},
    player,
};

const NO_OTHER_REASON: &str = "Cosmic country-rock that Teenage Fanclub and The Posies \
    grew up on, overlooked when it came out, and a record you return to for decades.";

fn album(id: &str, name: &str, artist: &str, year: Option<u16>) -> Album {
    Album {
        id: id.to_owned(),
        uri: format!("spotify:album:{id}"),
        name: name.to_owned(),
        artist: artist.to_owned(),
        artist_id: format!("id-{}", artist.to_lowercase().replace(' ', "-")),
        year,
    }
}

fn colour() -> Album {
    album(
        "colour",
        "The Colour And The Shape",
        "Foo Fighters",
        Some(1997),
    )
}

fn shelf() -> Vec<Album> {
    vec![
        album("nevermind", "Nevermind", "Nirvana", Some(1991)),
        colour(),
        album("bleach", "Bleach", "Nirvana", Some(1989)),
    ]
}

fn render(state: &State, now: Instant, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| draw(frame, state, now)).unwrap();
    terminal.backend().buffer().clone()
}

fn row(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect::<String>()
        .trim_end()
        .to_owned()
}

fn rows(buffer: &Buffer) -> Vec<String> {
    (0..buffer.area.height).map(|y| row(buffer, y)).collect()
}

/// Selected row: dim background and bold text.
fn is_selected(buffer: &Buffer, x: u16, y: u16) -> bool {
    let cell = &buffer[(x, y)];
    cell.bg == SELECTED_BG && cell.modifier.contains(Modifier::BOLD)
}

/// Plays Everlong (4:10), `position_ms` into the track.
fn playing(state: &mut State, now: Instant, position_ms: u32) {
    state.update(
        Event::Player(player::Event::TrackChanged {
            uri: "spotify:track:everlong".to_owned(),
            artist: "Foo Fighters".to_owned(),
            album: "The Colour And The Shape".to_owned(),
            track: "Everlong".to_owned(),
            duration: Duration::from_secs(250),
            cover: None,
        }),
        now,
    );
    state.update(Event::Player(player::Event::Position(position_ms)), now);
}

fn nevermind() -> Album {
    album("nevermind", "Nevermind", "Nirvana", Some(1991))
}

fn nevermind_tracks() -> Vec<Track> {
    [
        ("Smells Like Teen Spirit", 301),
        ("In Bloom", 254),
        ("Come As You Are", 218),
        ("Breed", 183),
    ]
    .into_iter()
    .map(|(name, secs)| Track {
        uri: format!("spotify:track:{name}"),
        name: name.to_owned(),
        artist: "Nirvana".to_owned(),
        duration: Duration::from_secs(secs),
    })
    .collect()
}

/// Shelf › Nirvana › Nevermind: Enter opens the album's tracks without playing it.
fn nevermind_state(now: Instant) -> State {
    let mut state = State::new(shelf());
    for key in [
        Key::Down,
        Key::Down,
        Key::Down,
        Key::Enter,
        Key::Down,
        Key::Enter,
    ] {
        state.update(Event::Key(key), now);
    }
    state
}

/// Plays the open Nevermind track list from track `index`. The selection returns
/// to where it was.
fn nevermind_playing(state: &mut State, now: Instant, index: usize) {
    let View::Album(view) = state.view() else {
        panic!("odotettiin kappalelistaa");
    };
    let selected = view.selected;
    let keys = [
        vec![Key::Up; selected],
        vec![Key::Down; index],
        vec![Key::Enter],
        vec![Key::Up; index],
        vec![Key::Down; selected],
    ];
    for key in keys.concat() {
        state.update(Event::Key(key), now);
    }
    let track = &nevermind_tracks()[index];
    state.update(
        Event::Player(player::Event::TrackChanged {
            uri: track.uri.clone(),
            artist: "Nirvana".to_owned(),
            album: "Nevermind".to_owned(),
            track: track.name.clone(),
            duration: track.duration,
            cover: None,
        }),
        now,
    );
}

/// A track row `width` columns wide: duration on the right edge.
fn track_row(start: &str, duration: &str, width: usize) -> String {
    let room = width - start.chars().count() - duration.len();
    format!("{start}{}{duration}", " ".repeat(room))
}

/// lofi and synthwave on home, synthwave's radio is playing and its view is open.
/// Night Drive is on the shelf.
fn synthwave_radio(now: Instant) -> State {
    let mut state =
        State::new(vec![night_drive()]).with_favorites(vec!["lofi".into(), "synthwave".into()]);
    state.update(Event::Key(Key::Down), now);
    state.update(Event::Key(Key::Down), now);
    let actions = state.update(Event::Key(Key::Enter), now);
    let [Action::LoadRadio(request)] = actions.as_slice() else {
        panic!("odotettiin radiohakua, oli {actions:?}");
    };
    let tracks = vec![
        radio_track("The Midnight", "Sunset", "endless", "Endless Summer"),
        radio_track("FM-84", "Running in the Night", "atlas", "Atlas"),
        radio_track("Timecop1983", "On the Run", "nightdrive", "Night Drive"),
        radio_track("Gunship", "Tech Noir", "gunship", "Gunship"),
    ];
    state.update(
        Event::RadioLoaded {
            request: request.clone(),
            result: Ok(tracks),
        },
        now,
    );
    state.update(
        Event::Player(player::Event::TrackChanged {
            uri: "spotify:track:sunset".to_owned(),
            artist: "The Midnight".to_owned(),
            album: "Endless Summer".to_owned(),
            track: "Sunset".to_owned(),
            duration: Duration::from_secs(300),
            cover: None,
        }),
        now,
    );
    state
}

/// Curated list: three albums with their reasons, round 2026-W41.
fn curated_list() -> AlbumList {
    let entry = |album: Album, reason: &str| ListAlbum {
        album,
        reason: Some(reason.to_owned()),
    };
    Curated {
        round: "2026-W41".to_owned(),
        created: "2026-10-05T07:00:00+03:00".to_owned(),
        albums: vec![
            entry(
                album("noother", "No Other", "Gene Clark", Some(1974)),
                NO_OTHER_REASON,
            ),
            entry(
                album("number1", "#1 Record", "Big Star", Some(1972)),
                "Power pop at its source.",
            ),
            entry(colour(), "Already on your shelf."),
        ],
    }
    .to_list(TEST_NOW)
}

/// Shelf list row › Curated, with album `index` selected.
fn curated_view(index: usize, now: Instant) -> State {
    let mut state = State::new(shelf()).with_curated(Ok(Some(curated_list())));
    state.update(Event::Key(Key::Enter), now);
    for _ in 0..index {
        state.update(Event::Key(Key::Down), now);
    }
    state
}

/// The tracks (URIs) as liked, as Spotify's response would report them.
fn liked(state: &mut State, now: Instant, uris: &[&str]) {
    state.update(
        Event::LikedChecked {
            uris: uris.iter().map(|&uri| uri.to_owned()).collect(),
            result: Ok(vec![true; uris.len()]),
        },
        now,
    );
}

/// Nevermind plays from the first track, and In Bloom and Breed are in the queue.
fn queued_state(now: Instant) -> State {
    let mut state = nevermind_state(now);
    state.update(
        Event::AlbumTracks {
            uri: nevermind().uri,
            result: Ok(nevermind_tracks()),
        },
        now,
    );
    nevermind_playing(&mut state, now, 0);
    let keys = [
        Key::Down,
        Key::Ctrl('e'),
        Key::Down,
        Key::Down,
        Key::Ctrl('e'),
    ];
    for key in keys {
        state.update(Event::Key(key), now);
    }
    state
}

fn radio_track(artist: &str, name: &str, album_id: &str, album: &str) -> radio::Track {
    radio::Track {
        uri: format!("spotify:track:{}", name.to_lowercase().replace(' ', "")),
        name: name.to_owned(),
        artist: artist.to_owned(),
        artist_uri: format!("spotify:artist:{}", artist.to_lowercase()),
        album: radio::Album {
            uri: format!("spotify:album:{album_id}"),
            name: album.to_owned(),
        },
    }
}

fn night_drive() -> Album {
    album("nightdrive", "Night Drive", "Timecop1983", Some(2018))
}

mod album;
mod genre_radio;
mod help;
mod home;
mod likes;
mod queue;
mod search;
mod top;
