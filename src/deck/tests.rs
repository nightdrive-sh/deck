//! Curated and the saved playback, from their files to the screen.

use std::time::{Duration, Instant};

use jiff::Timestamp;
use ratatui::{Terminal, backend::TestBackend};

use super::*;
use crate::{
    app::{Key, State, View},
    catalog, ui,
};

/// Example `curated.json`: two albums with their reasons.
const CURATED_JSON: &str = r##"{
  "round": "2026-W41",
  "created": "2026-10-05T07:00:00+03:00",
  "albums": [
{ "uri": "spotify:album:noother", "name": "No Other", "artist": "Gene Clark",
  "artist_id": "gc", "year": 1974,
  "reason": "Cosmic country-rock that Teenage Fanclub grew up on." },
{ "uri": "spotify:album:number1", "name": "#1 Record", "artist": "Big Star",
  "artist_id": "bs", "year": 1972, "reason": "Power pop at its source." }
  ]
}"##;

const HISTORY_JSON: &str = r##"[
  { "uri": "spotify:album:noother", "name": "No Other", "artist": "Gene Clark",
"round": "2026-W41", "rejected": false },
  { "uri": "spotify:album:number1", "name": "#1 Record", "artist": "Big Star",
"round": "2026-W41", "rejected": false }
]"##;

fn screen(state: &State) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(70, 17)).unwrap();
    terminal
        .draw(|frame| ui::draw(frame, state, Instant::now()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            let row: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            row.trim_end().to_owned()
        })
        .collect()
}

#[test]
fn curated_file_shows_on_shelf_and_ctrl_d_rejects_into_files() {
    let dir = tempfile::tempdir().unwrap();
    // A fresh list: the shelf row shows no age.
    let curated = CURATED_JSON.replace("2026-10-05T07:00:00+03:00", &Timestamp::now().to_string());
    std::fs::write(dir.path().join("curated.json"), curated).unwrap();
    std::fs::write(dir.path().join("curated-history.json"), HISTORY_JSON).unwrap();
    let files = CuratedFiles::in_dir(dir.path());
    let now = Instant::now();
    let shelf = vec![catalog::Album {
        id: "bleach".to_owned(),
        uri: "spotify:album:bleach".to_owned(),
        name: "Bleach".to_owned(),
        artist: "Nirvana".to_owned(),
        artist_id: "nirvana".to_owned(),
        year: Some(1989),
    }];
    let mut state = State::new(shelf).with_curated(curated_list(&files));

    // Shelf: list rows, an empty row and the artists.
    let rows = screen(&state);
    assert_eq!(
        rows[4],
        format!("✦ Curated{}2 albums · week 41", " ".repeat(43))
    );
    assert!(rows[5].starts_with("≋ Genres"));
    assert_eq!(rows[6], "");
    assert_eq!(rows[7], "Nirvana");

    // Enter opens the list and reads the file again.
    let actions = state.update(app::Event::Key(Key::Enter), now);
    assert_eq!(actions, [Action::LoadLists]);
    state.update(app::Event::ListsLoaded(curated_list(&files)), now);
    let rows = screen(&state);
    assert_eq!(rows[4], "Curated");
    assert_eq!(rows[7], "Gene Clark · No Other  1974");
    assert_eq!(rows[8], "Big Star · #1 Record  1972");
    assert_eq!(
        rows[12],
        "Cosmic country-rock that Teenage Fanclub grew up on."
    );

    // The reason follows the selection.
    state.update(app::Event::Key(Key::Down), now);
    assert_eq!(screen(&state)[12], "Power pop at its source.");

    // Ctrl+D removes the album from the list and the file and marks it in the history.
    let actions = state.update(app::Event::Key(Key::Ctrl('d')), now);
    assert_eq!(
        actions,
        [Action::RejectCurated("spotify:album:number1".to_owned())]
    );
    let event = reject_curated(&files, "spotify:album:number1");
    state.update(event, now);
    let rows = screen(&state);
    assert_eq!(rows[5], "1 album · week 41");
    assert_eq!(rows[7], "Gene Clark · No Other  1974");
    assert_eq!(rows[8], "");
    assert!(matches!(state.view(), View::Curated { selected: 0, .. }));
    let saved = files.load().unwrap().unwrap();
    assert_eq!(saved.albums.len(), 1);
    assert_eq!(saved.albums[0].album.name, "No Other");
    let history = files.history().unwrap();
    assert!(!history[0].rejected);
    assert!(history[1].rejected);
}

#[test]
fn broken_or_missing_curated_file() {
    let dir = tempfile::tempdir().unwrap();
    let files = CuratedFiles::in_dir(dir.path());
    assert_eq!(curated_list(&files), Ok(None));

    std::fs::write(dir.path().join("curated.json"), "{").unwrap();
    let error = curated_list(&files).unwrap_err();
    assert!(error.contains("is broken"), "{error}");
    // Rejecting does not overwrite a broken file.
    assert!(matches!(
        reject_curated(&files, "spotify:album:x"),
        app::Event::Error(_)
    ));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("curated.json")).unwrap(),
        "{"
    );
}

#[test]
fn saved_playback_shows_stopped_track_with_queue_on_start() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("playback.json");
    std::fs::write(
        &path,
        r#"{
  "album": { "uri": "spotify:album:nevermind", "len": 12, "index": 1 },
  "current": "album",
  "queue": [
{ "uri": "spotify:track:comeasyouare", "name": "Come As You Are", "artist": "Nirvana",
  "duration_ms": 218000, "album_uri": "spotify:album:nevermind", "index": 2 },
{ "uri": "spotify:track:breed", "name": "Breed", "artist": "Nirvana",
  "duration_ms": 183000, "album_uri": "spotify:album:nevermind", "index": 3 }
  ],
  "now_playing": { "artist": "Nirvana", "album": "Nevermind", "track": "In Bloom",
"duration_ms": 254000 },
  "position_ms": 127000
}"#,
    )
    .unwrap();
    let file = PlaybackFile::at(path);
    let now = Instant::now();
    let mut state = State::new(Vec::new());
    let saved = file.load().unwrap().unwrap();
    let actions = state.update(app::Event::Restore(Box::new(saved)), now);
    assert!(matches!(actions.as_slice(), [Action::Restore { .. }]));

    let rows = screen(&state);
    assert_eq!(
        rows[0],
        format!("DECK{}+2 queued   ■ stopped", " ".repeat(45))
    );
    assert_eq!(
        rows[1],
        format!("Nirvana / Nevermind / In Bloom{}02:07", " ".repeat(35))
    );
    // The bar's ● is halfway (127 s / 254 s).
    assert_eq!(rows[2].chars().position(|c| c == '●'), Some(34));

    // On quit the state is saved unchanged, even though time passes.
    let later = now + Duration::from_secs(10);
    file.save(state.playback(later).as_ref()).unwrap();
    assert_eq!(file.load().unwrap(), state.playback(now));
    assert_eq!(state.playback(later).unwrap(), state.playback(now).unwrap());
}
