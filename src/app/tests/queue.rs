//! Tests: queue.

use super::*;

fn queue_view_selected(app: &App) -> usize {
    match app.state.view() {
        View::Queue { selected, .. } => *selected,
        other => panic!("expected the queue, got {other:?}"),
    }
}

#[test]
fn ctrl_e_queues_selected_track_with_short_notice() {
    let mut app = nevermind_playing(0);
    assert!(queue_track(&mut app, 1).is_empty());
    assert_eq!(queue_names(&app), ["In Bloom"]);
    let queued = &app.state.queue()[0];
    assert_eq!(queued.artist, "Nirvana");
    assert_eq!(queued.duration, Duration::from_secs(254));
    assert_eq!(
        app.state.notice(app.now),
        Some((NoticeKind::Queued, "In Bloom"))
    );
    app.wait(2_900);
    assert_eq!(
        app.state.notice(app.now),
        Some((NoticeKind::Queued, "In Bloom"))
    );
    app.wait(100);
    assert_eq!(app.state.notice(app.now), None);
    // The list stays open and the selection in place.
    assert_eq!(app.album_view().selected, 1);
}

#[test]
fn ctrl_e_before_tracks_arrive_does_nothing() {
    let mut app = shelf_app();
    app.keys(&[Down, Enter, Enter]);
    assert!(app.key(Ctrl('e')).is_empty());
    assert!(app.state.queue().is_empty());
}

#[test]
fn ctrl_e_on_album_lists_queues_whole_album() {
    let mut app = shelf_app();
    // On the shelf (artists) Ctrl+E does nothing.
    assert!(app.key(Ctrl('e')).is_empty());
    app.keys(&[Down, Enter]);
    assert_eq!(app.key(Ctrl('e')), [Action::QueueAlbum(bleach())]);

    let mut app = spotify_artist_app();
    app.send(Event::ArtistAlbums {
        artist_id: "id-nirvana".into(),
        result: Ok(vec![unplugged(), nevermind()]),
    });
    app.key(Down);
    assert_eq!(app.key(Ctrl('e')), [Action::QueueAlbum(unplugged())]);

    let mut app = App::new(vec![]);
    app.key(Ctrl('f'));
    app.search("n", vec![artist("Nirvana")], vec![nevermind()]);
    // An artist cannot be queued, an album can (with e too, in the results).
    app.key(Down);
    assert!(app.key(Ctrl('e')).is_empty());
    app.key(Down);
    assert_eq!(app.key(Char('e')), [Action::QueueAlbum(nevermind())]);
    assert_eq!(app.search_view().query, "n");
}

#[test]
fn queued_album_tracks_are_added_in_order() {
    let mut app = nevermind_playing(0);
    assert!(queue_bleach(&mut app).is_empty());
    assert_eq!(queue_names(&app), ["Blew", "Floyd The Barber"]);
    assert_eq!(
        app.state.notice(app.now),
        Some((NoticeKind::Queued, "Bleach"))
    );
    assert_eq!(app.state.error, None);
}

#[test]
fn pick_for_me_draws_every_shelf_album_once_before_repeating() {
    let mut app = shelf_app();
    let mut seen = Vec::new();
    for _ in 0..5 {
        let actions = app.send(Event::Pick(PickFrom::Shelf));
        let [Action::PickAlbum(album)] = actions.as_slice() else {
            panic!("expected the picked album, got {actions:?}");
        };
        assert!(
            !seen.contains(&album.uri),
            "{} came twice in a round",
            album.uri
        );
        seen.push(album.uri.clone());
        let album = album.clone();
        app.send(Event::AlbumPicked {
            album,
            result: Ok(nevermind_tracks()),
        });
        assert!(app.state.picked_changed().is_some());
    }
    // A new round, without the playing album first
    let playing = seen.last().unwrap().clone();
    let actions = app.send(Event::Pick(PickFrom::Shelf));
    assert!(!matches!(actions.as_slice(), [Action::PickAlbum(a)] if a.uri == playing));
    // Nothing in Curated to pick from
    assert!(app.send(Event::Pick(PickFrom::Curated)).is_empty());
    assert!(app.state.error.is_some());
}

#[test]
fn picked_album_goes_whole_to_the_front_of_the_queue_and_plays() {
    let mut app = nevermind_playing(0);
    queue_bleach(&mut app);
    let actions = app.send(Event::AlbumPicked {
        album: nevermind(),
        result: Ok(nevermind_tracks()),
    });
    assert_eq!(actions, [play_track("Smells Like Teen Spirit")]);
    assert_eq!(
        queue_names(&app),
        [
            "In Bloom",
            "Come As You Are",
            "Breed",
            "Blew",
            "Floyd The Barber"
        ]
    );
    assert!(app.state.now_playing_info(app.now).unwrap().queued);

    // With nothing playing, it starts all the same
    let mut app = shelf_app();
    let actions = app.send(Event::AlbumPicked {
        album: nevermind(),
        result: Ok(nevermind_tracks()),
    });
    assert_eq!(actions, [play_track("Smells Like Teen Spirit")]);
    assert_eq!(queue_names(&app), ["In Bloom", "Come As You Are", "Breed"]);
}

#[test]
fn half_of_half_the_tracks_makes_an_album_listened() {
    // Nevermind has four tracks (learnt when they arrived)
    let mut app = nevermind_playing(0);
    assert_eq!(app.state.listened["spotify:album:nevermind"].tracks, 4);
    app.state.listened_changed();
    app.send(started("Smells Like Teen Spirit"));
    app.send(Event::Player(player::Event::Position(0)));
    assert!(app.wait(99_000).is_empty());
    assert!(app.state.listened_changed().is_none(), "not halfway yet");
    // Halfway: counted, and the album goes in the history
    assert_eq!(
        app.wait(2_000),
        [Action::RecordHistory {
            uri: "spotify:album:nevermind".into(),
            name: "Nevermind".into(),
            artist: "Nirvana".into(),
        }]
    );
    assert!(app.state.listened_changed().is_some());
    app.wait(50_000);
    assert!(
        app.state.listened_changed().is_none(),
        "a track counts once"
    );
    assert!(!app.state.listened["spotify:album:nevermind"].listened);
    // A second track halfway: two of four
    app.send(started("In Bloom"));
    app.send(Event::Player(player::Event::Position(0)));
    app.wait(101_000);
    assert!(app.state.listened["spotify:album:nevermind"].listened);
    // Paused, the time doesn't run
    app.send(started("Come As You Are"));
    app.send(Event::Player(player::Event::Paused(0)));
    app.wait(150_000);
    assert_eq!(
        app.state.listened["spotify:album:nevermind"].played.len(),
        2
    );
}

#[test]
fn next_after_a_remote_song_plays_the_queue_first() {
    let mut app = nevermind_playing(0);
    queue_bleach(&mut app);
    // Another song of the album, from a window (`deck ctl play-song`)
    app.send(Event::PlaySong {
        track: track_uri("Come As You Are"),
        album: nevermind().uri,
    });
    app.send(album_tracks(&nevermind(), Ok(nevermind_tracks())));
    app.send(started("Come As You Are"));
    assert_eq!(queue_names(&app), ["Blew", "Floyd The Barber"]);
    assert_eq!(
        app.send(Event::Media(MediaCommand::Next)),
        [play_track("Blew")]
    );
    assert_eq!(queue_names(&app), ["Floyd The Barber"]);
    // The window tells a queued track from the album's
    assert!(app.state.now_playing_info(app.now).unwrap().queued);
    app.send(Event::ClearQueue);
    assert!(queue_names(&app).is_empty());
}

#[test]
fn remote_queue_takes_one_song_or_the_album_and_unqueue_removes() {
    let mut app = nevermind_playing(0);
    assert_eq!(
        app.send(Event::QueueRemote {
            album: bleach().uri,
            track: None,
        }),
        [Action::QueueRemote {
            album: bleach().uri,
            track: None,
        }]
    );
    let floyd = bleach_tracks()[1].uri.clone();
    app.send(Event::AlbumQueued {
        album: bleach(),
        track: Some(floyd),
        result: Ok(bleach_tracks()),
    });
    assert_eq!(queue_names(&app), ["Floyd The Barber"]);
    assert_eq!(
        app.state.notice(app.now),
        Some((NoticeKind::Queued, "Floyd The Barber"))
    );
    queue_bleach(&mut app);
    let info = app.state.now_playing_info(app.now).unwrap();
    let titles: Vec<_> = info.queue.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(titles, ["Floyd The Barber", "Blew", "Floyd The Barber"]);
    assert_eq!(info.album_uri.as_deref(), Some("spotify:album:nevermind"));

    app.send(Event::Unqueue(0));
    app.send(Event::Unqueue(9));
    assert_eq!(queue_names(&app), ["Blew", "Floyd The Barber"]);

    // A song that is not on the album queues nothing.
    app.send(Event::AlbumQueued {
        album: bleach(),
        track: Some("spotify:track:nope".into()),
        result: Ok(bleach_tracks()),
    });
    assert_eq!(queue_names(&app), ["Blew", "Floyd The Barber"]);
    assert!(app.state.error.is_some());
}

#[test]
fn remote_shelf_changes_only_what_is_needed() {
    let mut app = shelf_app();
    let on = bleach().uri;
    let off = unplugged().uri;
    assert!(
        app.send(Event::Shelve {
            uri: on.clone(),
            on: true
        })
        .is_empty()
    );
    assert_eq!(
        app.send(Event::Shelve {
            uri: on.clone(),
            on: false
        }),
        [Action::RemoveFromShelf(on)]
    );
    assert_eq!(
        app.send(Event::Shelve {
            uri: off.clone(),
            on: true
        }),
        [Action::AddRadioAlbumToShelf(off.clone())]
    );
    assert!(
        app.send(Event::Shelve {
            uri: off,
            on: false
        })
        .is_empty()
    );
    assert_eq!(app.send(Event::Quit), [Action::Quit]);
}

#[test]
fn failed_album_queueing_shows_error() {
    let mut app = nevermind_playing(0);
    let actions = app.send(Event::AlbumQueued {
        album: bleach(),
        track: None,
        result: Err("could not queue the album: x".into()),
    });
    assert!(actions.is_empty());
    assert!(app.state.queue().is_empty());
    assert_eq!(app.state.notice(app.now), None);
    assert_eq!(
        app.state.error.as_deref(),
        Some("could not queue the album: x")
    );
}

#[test]
fn queue_plays_after_current_track_then_album_continues() {
    let mut app = nevermind_playing(0);
    queue_track(&mut app, 3);
    queue_bleach(&mut app);
    assert_eq!(app.send(end_of_track()), [play_track("Breed")]);
    assert_eq!(queue_names(&app), ["Blew", "Floyd The Barber"]);
    assert_eq!(app.send(end_of_track()), [play_track("Blew")]);
    assert_eq!(app.send(end_of_track()), [play_track("Floyd The Barber")]);
    assert!(app.state.queue().is_empty());
    // The album continues where it left off.
    assert_eq!(app.send(end_of_track()), [play("nevermind", 1)]);
    assert_eq!(app.send(end_of_track()), [play("nevermind", 2)]);
    assert_eq!(app.send(end_of_track()), [play("nevermind", 3)]);
    // When the album ends, playback stops.
    assert_eq!(app.send(end_of_track()), [Action::Stop]);
    assert_eq!(app.state.now_playing, None);
    assert!(app.keys(&[Left, Right]).is_empty());
}

#[test]
fn tracks_queued_while_queue_plays_go_to_its_end() {
    let mut app = nevermind_playing(0);
    queue_track(&mut app, 3);
    app.send(end_of_track());
    queue_track(&mut app, 2);
    assert_eq!(app.send(end_of_track()), [play_track("Come As You Are")]);
    assert_eq!(app.send(end_of_track()), [play("nevermind", 1)]);
}

#[test]
fn right_plays_queue_first() {
    let mut app = nevermind_playing(1);
    queue_track(&mut app, 3);
    assert_eq!(app.key(Right), [play_track("Breed")]);
    assert_eq!(app.key(Right), [play("nevermind", 2)]);
    assert_eq!(app.key(Right), [play("nevermind", 3)]);
    // On the last track → does nothing.
    assert!(app.key(Right).is_empty());
}

#[test]
fn left_walks_back_through_queue_to_album() {
    let mut app = nevermind_playing(1);
    queue_track(&mut app, 3);
    queue_bleach(&mut app);
    app.keys(&[Right, Right]);
    assert_eq!(queue_names(&app), ["Floyd The Barber"]);
    // The previous queue track, and the played one goes back to the front of the queue.
    assert_eq!(app.key(Left), [play_track("Breed")]);
    assert_eq!(queue_names(&app), ["Blew", "Floyd The Barber"]);
    // The album track after which the queue started.
    assert_eq!(app.key(Left), [play("nevermind", 1)]);
    assert_eq!(queue_names(&app), ["Breed", "Blew", "Floyd The Barber"]);
    assert_eq!(app.key(Left), [play("nevermind", 0)]);
    assert_eq!(app.key(Left), [play("nevermind", 0)]);
    // Going forward, the queue plays first again.
    assert_eq!(app.key(Right), [play_track("Breed")]);
}

#[test]
fn ctrl_e_when_nothing_plays_starts_the_queue() {
    let mut app = nevermind_app();
    assert_eq!(queue_track(&mut app, 1), [play_track("In Bloom")]);
    assert!(app.state.queue().is_empty());
    assert_eq!(
        app.state.notice(app.now),
        Some((NoticeKind::Queued, "In Bloom"))
    );
    app.send(started("In Bloom"));
    // The playing track is marked in its album's list.
    assert_eq!(app.state.playing_track(&nevermind().uri), Some(1));
    assert!(queue_track(&mut app, 2).is_empty());
    // On the first track ← starts it over.
    assert_eq!(app.key(Left), [play_track("In Bloom")]);
    assert_eq!(queue_names(&app), ["Come As You Are"]);
    assert_eq!(app.send(end_of_track()), [play_track("Come As You Are")]);
    // After the queue there is no album to continue.
    assert_eq!(app.send(end_of_track()), [Action::Stop]);
    assert_eq!(app.state.now_playing, None);
}

#[test]
fn queued_album_starts_when_nothing_plays() {
    let mut app = shelf_app();
    assert_eq!(queue_bleach(&mut app), [play_track("Blew")]);
    assert_eq!(queue_names(&app), ["Floyd The Barber"]);
}

#[test]
fn paused_counts_as_playing() {
    let mut app = nevermind_playing(0);
    app.send(Event::Player(player::Event::Paused(0)));
    assert!(queue_track(&mut app, 2).is_empty());
    assert_eq!(queue_names(&app), ["Come As You Are"]);
}

#[test]
fn playing_another_album_keeps_the_queue() {
    let mut app = nevermind_playing(0);
    queue_bleach(&mut app);
    app.keys(&[Down, Down]);
    assert_eq!(app.key(Enter), [play("nevermind", 2)]);
    assert_eq!(app.send(end_of_track()), [play_track("Blew")]);
    assert_eq!(app.send(end_of_track()), [play_track("Floyd The Barber")]);
    assert_eq!(app.send(end_of_track()), [play("nevermind", 3)]);
}

#[test]
fn player_stopping_keeps_the_queue() {
    let mut app = nevermind_playing(0);
    queue_track(&mut app, 2);
    app.send(Event::Player(player::Event::Stopped));
    assert_eq!(app.state.now_playing, None);
    assert_eq!(app.state.playing_track(&nevermind().uri), None);
    assert!(app.keys(&[Left, Right]).is_empty());
    assert_eq!(queue_names(&app), ["Come As You Are"]);
}

#[test]
fn preload_asks_for_the_upcoming_track() {
    let preload = || Event::Player(player::Event::PreloadNext);
    let mut app = nevermind_app();
    assert!(app.send(preload()).is_empty());

    let mut app = nevermind_playing(2);
    assert_eq!(
        app.send(preload()),
        [Action::Preload(Item::Album {
            uri: nevermind().uri,
            track: 3
        })]
    );
    queue_track(&mut app, 0);
    assert_eq!(
        app.send(preload()),
        [Action::Preload(Item::Track(track_uri(
            "Smells Like Teen Spirit"
        )))]
    );
    // Preloading does not change the order.
    assert_eq!(queue_names(&app), ["Smells Like Teen Spirit"]);
    app.send(end_of_track());
    app.send(end_of_track());
    assert!(app.send(preload()).is_empty());
}

#[test]
fn ctrl_q_opens_queue_view_and_esc_returns() {
    let mut app = nevermind_playing(0);
    app.key(Ctrl('q'));
    assert_eq!(queue_view_selected(&app), 0);
    // A second Ctrl+Q does not stack the view.
    app.key(Ctrl('q'));
    app.key(Esc);
    assert_eq!(app.album_view().album, nevermind());

    app.key(Ctrl('f'));
    app.key(Ctrl('q'));
    queue_view_selected(&app);
    app.key(Esc);
    app.search_view();
}

#[test]
fn queue_view_selects_and_removes() {
    let mut app = nevermind_playing(0);
    for index in [1, 2, 3] {
        queue_track(&mut app, index);
    }
    app.key(Ctrl('q'));
    app.key(Up);
    assert_eq!(queue_view_selected(&app), 0);
    app.keys(&[Down; 5]);
    assert_eq!(queue_view_selected(&app), 2);
    app.key(Up);
    assert!(app.key(Ctrl('d')).is_empty());
    assert_eq!(queue_names(&app), ["In Bloom", "Breed"]);
    assert_eq!(queue_view_selected(&app), 1);
    // Removing the last one moves the selection to the previous one.
    app.key(Ctrl('d'));
    assert_eq!(queue_names(&app), ["In Bloom"]);
    assert_eq!(queue_view_selected(&app), 0);
    app.key(Ctrl('d'));
    assert!(app.state.queue().is_empty());
    assert!(app.keys(&[Ctrl('d'), Down, Enter, Ctrl('e')]).is_empty());
    // After the queue empties, the album continues.
    assert_eq!(app.send(end_of_track()), [play("nevermind", 1)]);
}

#[test]
fn queue_view_selection_follows_shrinking_queue() {
    let mut app = nevermind_playing(0);
    queue_track(&mut app, 1);
    queue_track(&mut app, 2);
    app.key(Ctrl('q'));
    app.key(Down);
    assert_eq!(queue_view_selected(&app), 1);
    app.send(end_of_track());
    assert_eq!(queue_names(&app), ["Come As You Are"]);
    assert_eq!(queue_view_selected(&app), 0);
}

#[test]
fn player_keys_in_queue_view() {
    let mut app = nevermind_playing(0);
    queue_track(&mut app, 2);
    app.key(Ctrl('q'));
    assert_eq!(app.key(Char(' ')), [Action::TogglePause]);
    assert_eq!(app.key(Right), [play_track("Come As You Are")]);
    assert_eq!(app.key(Char('q')), [Action::Quit]);
}
