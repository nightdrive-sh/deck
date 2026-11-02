//! Tests: now playing, errors, and saving and restoring playback.

use std::path::PathBuf;

use super::*;

fn track_changed() -> Event {
    Event::Player(player::Event::TrackChanged {
        uri: "spotify:track:everlong".into(),
        artist: "Foo Fighters".into(),
        album: "The Colour And The Shape".into(),
        track: "Everlong".into(),
        duration: Duration::from_secs(250),
        cover: Some("https://i.scdn.co/image/everlong".into()),
    })
}

#[test]
fn player_events_update_now_playing() {
    let mut app = shelf_app();
    app.send(track_changed());
    let now = app.state.now_playing.clone().unwrap();
    assert_eq!(now.track, "Everlong");
    assert_eq!(now.duration, Duration::from_secs(250));
    assert!(!now.paused);
    assert_eq!(now.elapsed(app.now), Duration::ZERO);

    app.send(Event::Player(player::Event::Position(83_000)));
    assert_eq!(
        app.state.now_playing.as_ref().unwrap().elapsed(app.now),
        Duration::from_secs(83)
    );

    app.send(Event::Player(player::Event::Paused(83_000)));
    assert!(app.state.now_playing.as_ref().unwrap().paused);

    app.send(Event::Player(player::Event::Position(83_000)));
    assert!(!app.state.now_playing.as_ref().unwrap().paused);

    app.send(Event::Player(player::Event::Stopped));
    assert_eq!(app.state.now_playing, None);
}

#[test]
fn elapsed_runs_while_playing_and_stops_when_paused() {
    let mut app = shelf_app();
    app.send(track_changed());
    app.send(Event::Player(player::Event::Position(10_000)));
    app.wait(400);
    let now = app.state.now_playing.clone().unwrap();
    assert_eq!(now.elapsed(app.now), Duration::from_millis(10_400));

    // When paused, the time is the position the player reported.
    app.send(Event::Player(player::Event::Paused(10_400)));
    app.wait(5_000);
    let now = app.state.now_playing.clone().unwrap();
    assert_eq!(now.elapsed(app.now), Duration::from_millis(10_400));

    // Never beyond the track's duration.
    app.send(Event::Player(player::Event::Position(249_900)));
    app.wait(1_000);
    let now = app.state.now_playing.clone().unwrap();
    assert_eq!(now.elapsed(app.now), Duration::from_secs(250));
}

#[test]
fn now_playing_info_follows_track_and_pause() {
    let mut app = shelf_app();
    assert_eq!(app.state.now_playing_info(app.now), None);
    app.send(track_changed());
    app.send(Event::Player(player::Event::Position(83_000)));
    app.wait(2_000);
    let expected = now_playing::Info {
        title: "Everlong".into(),
        artist: "Foo Fighters".into(),
        album: "The Colour And The Shape".into(),
        duration_ms: 250_000,
        position_ms: 85_000,
        playing: true,
        cover: Some("https://i.scdn.co/image/everlong".into()),
        liked: None,
        track_uri: Some("spotify:track:everlong".into()),
        album_uri: None,
        queued: false,
        queue: Vec::new(),
        radio: None,
        error: None,
    };
    assert_eq!(app.state.now_playing_info(app.now), Some(expected.clone()));

    app.send(Event::Player(player::Event::Paused(85_000)));
    app.wait(5_000);
    assert_eq!(
        app.state.now_playing_info(app.now),
        Some(now_playing::Info {
            playing: false,
            ..expected
        })
    );

    app.send(Event::Player(player::Event::Stopped));
    assert_eq!(app.state.now_playing_info(app.now), None);
}

#[test]
fn seek_jumps_at_once_and_stays_paused() {
    let mut app = shelf_app();
    assert!(app.send(Event::Seek(Duration::from_secs(10))).is_empty());
    app.send(track_changed());
    app.send(Event::Player(player::Event::Paused(83_000)));
    assert_eq!(
        app.send(Event::Seek(Duration::from_secs(120))),
        [Action::Seek(Duration::from_secs(120))]
    );
    let info = app.state.now_playing_info(app.now).unwrap();
    assert_eq!((info.position_ms, info.playing), (120_000, false));
    // Never beyond the track's end.
    assert_eq!(
        app.send(Event::Seek(Duration::from_secs(999))),
        [Action::Seek(Duration::from_secs(250))]
    );
}

#[test]
fn error_reaches_now_playing_info() {
    let mut app = shelf_app();
    app.send(track_changed());
    app.send(Event::Error("could not save the shelf".into()));
    let info = app.state.now_playing_info(app.now).unwrap();
    assert_eq!(info.error.as_deref(), Some("could not save the shelf"));
}

#[test]
fn error_stays_until_next_success() {
    let mut app = shelf_app();
    app.send(Event::Player(player::Event::Error(
        "playback failed".into(),
    )));
    assert_eq!(app.state.error.as_deref(), Some("playback failed"));
    // Position updates are not actions: the error stays visible.
    app.send(Event::Player(player::Event::Position(1_000)));
    app.wait(300);
    assert!(app.state.error.is_some());

    app.send(track_changed());
    assert_eq!(app.state.error, None);

    app.send(Event::Error("cannot save the shelf".into()));
    app.send(Event::ShelfChanged(vec![colour()]));
    assert_eq!(app.state.error, None);

    app.send(Event::Error("x".into()));
    app.send(Event::Player(player::Event::Paused(0)));
    assert_eq!(app.state.error, None);
}

#[test]
fn successful_search_clears_error() {
    let mut app = shelf_app();
    app.send(Event::Error("x".into()));
    app.key(Ctrl('f'));
    app.search("foo", vec![], vec![]);
    assert_eq!(app.state.error, None);
}

// --- Saving and restoring playback ---

#[test]
fn save_playback_writes_the_state_with_the_position_now() {
    let mut app = in_bloom_with_queue();
    let path = PathBuf::from("/tmp/undo.json");
    let actions = app.send(Event::SavePlayback(path.clone()));
    let playback = app.state.playback(app.now).map(Box::new);
    assert_eq!(playback.as_ref().unwrap().position, Duration::from_secs(83));
    assert_eq!(actions, [Action::SavePlayback { path, playback }]);
}

#[test]
fn a_restored_copy_is_saved_but_the_start_s_restore_is_not() {
    let mut app = in_bloom_with_queue();
    let saved = quit(&mut app);
    let (mut started, _) = restored(saved.clone());
    assert!(!started.state.playback_changed());
    let mut copied = shelf_app();
    copied.send(Event::RestoreCopy(Box::new(saved)));
    assert!(copied.state.playback_changed());
    assert!(!copied.state.playback_changed());
}

#[test]
fn saved_playback_has_album_track_position_and_queue() {
    let mut app = in_bloom_with_queue();
    let saved = quit(&mut app);
    let json = serde_json::to_value(&saved).unwrap();
    let nevermind = serde_json::to_value(nevermind()).unwrap();
    let queued = |name: &str, secs: u64, index: usize| {
        serde_json::json!({
            "uri": track_uri(name),
            "name": name,
            "artist": "Nirvana",
            "duration_ms": secs * 1000,
            "album_uri": "spotify:album:nevermind",
            "index": index,
            "album": nevermind,
        })
    };
    assert_eq!(
        json,
        serde_json::json!({
            "album": {
                "uri": "spotify:album:nevermind",
                "len": 4,
                "index": 1,
                "album": nevermind,
            },
            "current": "album",
            "played": [],
            "queue": [queued("Come As You Are", 218, 2), queued("Breed", 183, 3)],
            "now_playing": {
                "uri": track_uri("In Bloom"),
                "artist": "Nirvana",
                "album": "Nevermind",
                "track": "In Bloom",
                "duration_ms": 200_000,
            },
            "position_ms": 83_000,
        })
    );
    // The same state when read from the file.
    assert_eq!(
        serde_json::from_value::<SavedPlayback>(json).unwrap(),
        saved
    );
}

#[test]
fn restore_shows_track_stopped_at_saved_position_with_queue() {
    let mut app = in_bloom_with_queue();
    let (app, actions) = restored(quit(&mut app));
    assert_eq!(
        actions,
        [Action::Restore {
            item: Item::Album {
                uri: nevermind().uri,
                track: 1,
            },
            position: Duration::from_secs(83),
        }]
    );
    // The status line shows right away, before the player has loaded the track.
    let playing = app.state.now_playing.clone().unwrap();
    assert_eq!(playing.track, "In Bloom");
    assert!(playing.paused);
    assert_eq!(playing.elapsed(app.now), Duration::from_secs(83));
    assert_eq!(queue_names(&app), ["Come As You Are", "Breed"]);
    assert_eq!(app.state.playing_track(&nevermind().uri), Some(1));
    assert!(app.is_shelf());
    assert_eq!(app.state.error, None);
}

#[test]
fn restored_track_continues_with_queue_then_album() {
    let mut app = in_bloom_with_queue();
    let (mut app, _) = restored(quit(&mut app));
    // The player loaded the track paused at the same position.
    app.send(started("In Bloom"));
    app.send(Event::Player(player::Event::Paused(83_000)));
    let playing = app.state.now_playing.clone().unwrap();
    assert!(playing.paused);
    assert_eq!(playing.elapsed(app.now), Duration::from_secs(83));

    assert_eq!(app.key(Char(' ')), [Action::TogglePause]);
    app.send(Event::Player(player::Event::Position(83_000)));
    assert_eq!(app.send(end_of_track()), [play_track("Come As You Are")]);
    assert_eq!(app.send(end_of_track()), [play_track("Breed")]);
    assert_eq!(app.send(end_of_track()), [play("nevermind", 2)]);
}

#[test]
fn arrows_work_on_restored_track_as_when_paused() {
    let mut app = in_bloom_with_queue();
    let saved = quit(&mut app);
    let (mut app, _) = restored(saved.clone());
    assert_eq!(app.key(Right), [play_track("Come As You Are")]);
    let (mut app, _) = restored(saved);
    assert_eq!(app.key(Left), [play("nevermind", 0)]);
}

#[test]
fn restored_queue_track_plays_album_after_queue() {
    let mut app = in_bloom_with_queue();
    app.send(end_of_track());
    app.send(started("Come As You Are"));
    let saved = quit(&mut app);
    let (mut app, actions) = restored(saved);
    assert_eq!(
        actions,
        [Action::Restore {
            item: Item::Track(track_uri("Come As You Are")),
            position: Duration::ZERO,
        }]
    );
    assert_eq!(queue_names(&app), ["Breed"]);
    assert_eq!(app.state.playing_track(&nevermind().uri), Some(2));
    // ← goes back to the album track after which the queue started.
    assert_eq!(app.key(Left), [play("nevermind", 1)]);
}

#[test]
fn nothing_playing_saves_nothing() {
    let mut app = shelf_app();
    assert_eq!(app.key(Char('q')), [Action::Quit]);
    assert_eq!(app.state.playback(app.now), None);
    assert!(!app.state.playback_changed());
}

#[test]
fn playback_is_saved_when_track_or_queue_changes() {
    let mut app = nevermind_app();
    assert!(!app.state.playback_changed());

    app.key(Enter);
    assert!(app.state.playback_changed());
    app.send(started("Smells Like Teen Spirit"));
    assert!(app.state.playback_changed());
    assert!(!app.state.playback_changed());

    // Position and pause are not changes: the position is saved on quit.
    app.send(Event::Player(player::Event::Position(30_000)));
    app.send(Event::Player(player::Event::Paused(30_000)));
    assert!(!app.state.playback_changed());

    queue_track(&mut app, 3);
    assert!(app.state.playback_changed());
    app.key(Ctrl('q'));
    app.key(Ctrl('d'));
    assert!(app.state.playback_changed());

    // The album ended: the state is cleared.
    app.key(Esc);
    for _ in 0..4 {
        app.send(end_of_track());
    }
    assert_eq!(app.state.now_playing, None);
    assert!(app.state.playback_changed());
    assert_eq!(app.state.playback(app.now), None);
}

#[test]
fn tick_and_position_do_not_build_playback_but_next_track_is_a_change() {
    let mut app = nevermind_app();
    app.key(Enter);
    app.send(started("Smells Like Teen Spirit"));
    assert!(app.state.playback_changed());

    // Tick, position and pause do not even mark the state for building.
    app.wait(100);
    app.send(Event::Player(player::Event::Position(1_000)));
    app.send(Event::Player(player::Event::Paused(1_000)));
    assert!(!app.state.playback_dirty);
    assert!(!app.state.playback_changed());

    app.key(Right);
    assert!(app.state.playback_changed());
    app.wait(100);
    assert!(!app.state.playback_changed());
}

#[test]
fn restore_is_not_a_change_to_save() {
    let mut app = in_bloom_with_queue();
    let (mut app, _) = restored(quit(&mut app));
    assert!(!app.state.playback_changed());
}

#[test]
fn quitting_before_restored_track_loads_keeps_position() {
    let mut app = in_bloom_with_queue();
    let saved = quit(&mut app);
    let (mut app, _) = restored(saved.clone());
    app.wait(5_000);
    assert_eq!(quit(&mut app), saved);
}

#[test]
fn failed_restore_starts_empty_with_error() {
    let mut app = in_bloom_with_queue();
    let (mut app, _) = restored(quit(&mut app));
    app.send(Event::Player(player::Event::Error(
        "could not load the album's tracks: 404".into(),
    )));
    assert_eq!(
        app.state.error.as_deref(),
        Some("could not restore playback: could not load the album's tracks: 404")
    );
    assert_eq!(app.state.now_playing, None);
    assert!(app.state.queue().is_empty());
    assert!(app.state.playback_changed());
    assert_eq!(app.state.playback(app.now), None);
    assert!(app.key(Right).is_empty());
    assert!(app.key(Left).is_empty());
}

#[test]
fn startup_error_stays_while_restored_track_loads() {
    let mut app = in_bloom_with_queue();
    let saved = quit(&mut app);
    let mut app = shelf_app();
    app.send(Event::Error("shelf file is broken".into()));
    app.send(Event::Restore(Box::new(saved)));
    app.send(started("In Bloom"));
    app.send(Event::Player(player::Event::Paused(83_000)));
    assert_eq!(app.state.error.as_deref(), Some("shelf file is broken"));
    assert!(app.state.now_playing.as_ref().unwrap().paused);
}

#[test]
fn error_after_restored_track_loads_keeps_playback() {
    let mut app = in_bloom_with_queue();
    let (mut app, _) = restored(quit(&mut app));
    app.send(started("In Bloom"));
    app.send(Event::Player(player::Event::Paused(83_000)));
    app.send(Event::Player(player::Event::Error("x".into())));
    assert_eq!(app.state.error.as_deref(), Some("x"));
    assert_eq!(queue_names(&app), ["Come As You Are", "Breed"]);
    assert!(app.state.now_playing.is_some());

    // Playing another track while loading also ends the restore.
    let mut app = in_bloom_with_queue();
    let (mut app, _) = restored(quit(&mut app));
    app.key(Right);
    app.send(Event::Player(player::Event::Error("x".into())));
    assert_eq!(queue_names(&app), ["Breed"]);
}

#[test]
fn invalid_saved_playback_starts_empty_with_error() {
    let saved: SavedPlayback = serde_json::from_value(serde_json::json!({
        "album": null,
        "current": "album",
        "now_playing": null,
        "position_ms": 0,
    }))
    .unwrap();
    let (app, actions) = restored(saved);
    assert!(actions.is_empty());
    assert_eq!(
        app.state.error.as_deref(),
        Some("could not restore playback: the saved state is invalid")
    );
    assert_eq!(app.state.now_playing, None);
    assert_eq!(app.state.playback(app.now), None);
}
