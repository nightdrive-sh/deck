//! Tests: radio continuing by itself, its history and restore.

use super::*;
use crate::app::station::RADIO_HISTORY;

#[test]
fn radio_continues_by_itself_past_the_first_page() {
    let mut app = genre_app(&["lofi"]);
    let start = start_radio(&mut app, station(0..50));
    // No refill while at least five tracks remain after the playing one.
    assert!(
        play_until(&mut app, 44)
            .iter()
            .all(|a| !matches!(a, Action::LoadRadio(_)))
    );

    let actions = app.send(end_of_track());
    assert_eq!(actions[0], radio_play(45));
    let more = radio_request(&actions);
    assert_eq!(more.kind, RadioLoad::More);
    assert_eq!(more.seed, start.seed);
    assert_eq!(more.previous.len(), 50);
    assert_eq!(more.previous[49], "spotify:track:r49");
    // A fetch is already running: no second one.
    app.send(started("Song 45"));
    assert!(
        app.send(end_of_track())
            .iter()
            .all(|a| !matches!(a, Action::LoadRadio(_)))
    );

    // Refill: a known track is skipped, new ones go to the end.
    let mut page = station(46..96);
    page.insert(0, radio_track(3));
    assert!(app.send(radio_loaded(&more, page)).is_empty());
    assert_eq!(radio_of(&app).tracks.len(), 96);
    assert_eq!(radio_of(&app).seed, start.seed);

    // The radio continues by itself.
    play_until(&mut app, 60);
    assert_eq!(radio_of(&app).index, 60);
    assert_eq!(app.state.playing_genre(), Some("lofi"));
}

#[test]
fn radio_keeps_at_most_radio_history_played_tracks() {
    let mut app = genre_app(&["lofi"]);
    start_radio(&mut app, station(0..600));
    let more = radio_request(&play_until(&mut app, 595));
    // n moves the radio view's selection to the playing track.
    app.key(Char('n'));
    assert_eq!(radio_view(&app).selected, 595);

    app.send(radio_loaded(&more, station(600..650)));
    let radio = radio_of(&app);
    assert_eq!(radio.index, RADIO_HISTORY);
    assert_eq!(radio.tracks.len(), 650 - 95);
    assert_eq!(radio.tracks[radio.index].uri, "spotify:track:r595");
    assert_eq!(radio.seed_start, 0);
    assert_eq!(radio_view(&app).selected, RADIO_HISTORY);
    assert_eq!(app.state.playing_radio_track("lofi"), Some(RADIO_HISTORY));

    // Playback continues and ← goes back to the previous track as before trimming.
    assert_eq!(app.send(end_of_track())[0], radio_play(596));
    app.send(started("Song 596"));
    assert_eq!(app.key(Left), [radio_play(595)]);
}

#[test]
fn previous_without_a_track_to_go_back_to_keeps_playback() {
    let mut app = genre_app(&["lofi"]);
    start_radio(&mut app, station(0..20));
    app.state.playback.radio.as_mut().unwrap().tracks.clear();
    // An empty radio fetches more, but nothing is played.
    let actions = app.key(Left);
    assert!(
        !actions.iter().any(|a| matches!(a, Action::Play(_))),
        "{actions:?}"
    );
    assert_eq!(app.state.playback.current, Some(Current::Radio));
}

#[test]
fn exhausted_station_switches_to_an_unplayed_seed() {
    let mut app = genre_app(&["lofi"]);
    let start = start_radio(&mut app, station(0..8));
    let more = radio_request(&play_until(&mut app, 3));
    play_until(&mut app, 5);

    // Only one new: the station repeats itself, so the next fetch uses a new seed.
    let actions = app.send(radio_loaded(&more, station(0..9)));
    let radio = radio_of(&app);
    assert_eq!(radio.tracks.len(), 9);
    assert_ne!(radio.seed, start.seed);
    assert_eq!(radio.seeds, [start.seed.clone(), radio.seed.clone()]);
    assert_eq!(radio.seed_start, 9);
    // Still fewer than five after the playing one: the fetch continues right away with
    // a new seed, without the previous station's tracks.
    let next = radio_request(&actions);
    assert_eq!(next.seed, radio.seed);
    assert!(next.previous.is_empty());

    // The new seed's tracks are appended seamlessly.
    assert!(app.send(radio_loaded(&next, station(9..59))).is_empty());
    assert_eq!(radio_of(&app).tracks.len(), 59);
    assert_eq!(radio_of(&app).seed, next.seed);
    play_until(&mut app, 12);
    assert_eq!(radio_of(&app).index, 12);
}

#[test]
fn continuation_stops_when_no_seed_brings_anything_new() {
    let mut app = genre_app(&["lofi"]);
    start_radio(&mut app, station(0..6));
    let mut actions = play_until(&mut app, 1);
    let mut fetches = 0;
    while let Some(Action::LoadRadio(request)) = actions.last().cloned() {
        fetches += 1;
        actions = app.send(radio_loaded(&request, station(0..6)));
    }
    // Every seed was tried once, then fetching stopped.
    assert_eq!(fetches, lofi().seeds.len() + 1);
    assert_eq!(radio_of(&app).seeds.len(), lofi().seeds.len());
    // The remaining tracks play, and playback stops as at the end of an album.
    play_until(&mut app, 5);
    assert_eq!(app.send(end_of_track()), [Action::Stop]);
    assert_eq!(app.state.playing_genre(), None);
}

#[test]
fn failed_continuation_shows_error_and_retries_when_next_track_starts() {
    let mut app = genre_app(&["lofi"]);
    start_radio(&mut app, station(0..6));
    let more = radio_request(&play_until(&mut app, 1));
    let actions = app.send(Event::RadioLoaded {
        request: more,
        result: Err("could not load the radio: timeout".into()),
    });
    assert!(actions.is_empty());
    assert_eq!(
        app.state.error.as_deref(),
        Some("could not load the radio: timeout")
    );
    // No new attempt during the same track.
    assert!(app.wait(1000).is_empty());

    let actions = app.send(end_of_track());
    assert_eq!(actions[0], radio_play(2));
    let retry = radio_request(&actions);
    app.send(radio_loaded(&retry, station(6..56)));
    assert_eq!(app.state.error, None);
    assert_eq!(radio_of(&app).tracks.len(), 56);
}

#[test]
fn queue_plays_before_the_next_radio_track() {
    let mut app = genre_app(&["lofi"]);
    start_radio(&mut app, station(0..50));
    app.keys(&[Down, Down, Down, Ctrl('e')]);
    assert_eq!(queue_names(&app), ["Song 3"]);

    assert_eq!(app.send(end_of_track()), [radio_play(3)]);
    app.send(started("Song 3"));
    // A track playing from the queue is not the radio's ♪, but the genre still plays.
    assert_eq!(app.state.playing_radio_track("lofi"), None);
    assert_eq!(app.state.playing_genre(), Some("lofi"));
    // ← goes back to the radio track, and the queue track goes back to the queue.
    assert_eq!(app.key(Left), [radio_play(0)]);
    assert_eq!(queue_names(&app), ["Song 3"]);
    assert_eq!(app.key(Right), [radio_play(3)]);
    // After the queue the radio continues where it left off.
    assert_eq!(app.send(end_of_track()), [radio_play(1)]);
    assert_eq!(app.state.playing_radio_track("lofi"), Some(1));
}

#[test]
fn queued_radio_track_has_no_album_position() {
    let mut app = genre_app(&["lofi"]);
    start_radio(&mut app, station(0..50));
    app.keys(&[Down, Ctrl('e')]);
    app.send(end_of_track());
    app.send(started("Song 1"));
    assert_eq!(app.state.playing_track("spotify:album:record1"), None);
    assert_eq!(app.state.queue().len(), 0);
}

#[test]
fn playing_an_album_replaces_the_radio() {
    let mut app = genre_app(&["lofi"]);
    start_radio(&mut app, station(0..50));
    app.keys(&[Down, Ctrl('e')]);
    app.state.views.truncate(1);
    app.key(Ctrl('f'));
    app.search("nevermind", vec![], vec![nevermind()]);
    app.keys(&[Enter, Enter]);
    app.send(album_tracks(&nevermind(), Ok(nevermind_tracks())));
    assert_eq!(app.key(Enter), [play("nevermind", 0)]);
    assert_eq!(app.state.playing_genre(), None);
    assert_eq!(app.state.radio_tracks("lofi"), None);
    assert_eq!(queue_names(&app), ["Song 1"]);
    // Radio view without a radio: Ctrl+R starts a new one, the others do nothing.
    app.state.views.push(View::Radio(RadioView {
        genre: lofi(),
        selected: 0,
        scroll: Scroll::default(),
    }));
    assert!(app.keys(&[Down, Enter, Ctrl('e'), Ctrl('a')]).is_empty());
    assert_eq!(radio_request(&app.key(Ctrl('r'))).genre, "lofi");
}

#[test]
fn radio_is_saved_and_restored() {
    let mut app = genre_app(&["lofi"]);
    let start = start_radio(&mut app, station(0..50));
    play_until(&mut app, 2);
    app.keys(&[Down, Ctrl('e')]);
    let saved = quit(&mut app);
    let json = serde_json::to_string(&saved).unwrap();
    assert!(json.contains(r#""current":"radio""#), "{json}");
    assert_eq!(serde_json::from_str::<SavedPlayback>(&json).unwrap(), saved);

    let (mut app, actions) = restored(saved);
    assert_eq!(
        actions,
        [Action::Restore {
            item: Item::Track("spotify:track:r2".into()),
            position: Duration::ZERO,
        }]
    );
    assert_eq!(app.state.playing_genre(), Some("lofi"));
    assert_eq!(app.state.radio_tracks("lofi").unwrap().len(), 50);
    assert_eq!(queue_names(&app), ["Song 1"]);
    // Space resumes, and the radio continues after the queue.
    assert_eq!(app.key(Char(' ')), [Action::TogglePause]);
    app.send(Event::Player(player::Event::Position(0)));
    assert_eq!(
        app.send(end_of_track()),
        [Action::Play(Item::Track("spotify:track:r1".into()))]
    );
    assert_eq!(app.send(end_of_track()), [radio_play(3)]);

    // Refills work on a restored radio from the same seed.
    let more = radio_request(&play_until(&mut app, 45));
    assert_eq!(more.seed, start.seed);
    assert_eq!(more.previous.len(), 50);
}

#[test]
fn restored_radio_near_its_end_loads_more_at_once() {
    let mut app = genre_app(&["lofi"]);
    start_radio(&mut app, station(0..8));
    let more = radio_request(&play_until(&mut app, 3));
    let saved = quit(&mut app);
    let (_, actions) = restored(saved);
    assert!(matches!(actions[0], Action::Restore { .. }));
    assert_eq!(radio_request(&actions), more);
}

#[test]
fn invalid_saved_radio_starts_empty_with_error() {
    let mut app = genre_app(&["lofi"]);
    start_radio(&mut app, station(0..50));
    let mut saved = quit(&mut app);
    saved.radio.as_mut().unwrap().index = 50;
    let (app, actions) = restored(saved);
    assert!(actions.is_empty());
    assert!(app.state.error.is_some());
    assert_eq!(app.state.playing_genre(), None);
}
