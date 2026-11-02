//! Tests: letters alongside Ctrl keys, and n for the playing album.

use super::*;

/// Outside search the letters f, l, e, a, d and r do the same as their Ctrl
/// counterparts, and u the same as Ctrl+Q: the same actions, view, queue and
/// error. Likes are enabled and something is playing, so that l does something too.
fn assert_letters_match_ctrl(setup: impl Fn() -> App) {
    let prepared = || {
        let mut app = setup();
        app.state.likes.enabled = true;
        if app.state.now_playing.is_none() {
            app.send(started("In Bloom"));
        }
        app
    };
    let pairs = [
        ('f', 'f'),
        ('l', 'l'),
        ('e', 'e'),
        ('a', 'a'),
        ('d', 'd'),
        ('r', 'r'),
        ('u', 'q'),
    ];
    for (c, ctrl_key) in pairs {
        let mut ctrl = prepared();
        let mut letter = prepared();
        let view = format!("{:?}", ctrl.state.view());
        assert_eq!(
            same_seed(letter.key(Char(c))),
            same_seed(ctrl.key(Ctrl(ctrl_key))),
            "{c} in {view}"
        );
        assert_eq!(
            format!("{:?}", letter.state.view()),
            format!("{:?}", ctrl.state.view()),
            "{c} in {view}"
        );
        assert_eq!(queue_names(&letter), queue_names(&ctrl), "{c} in {view}");
        assert_eq!(letter.state.error, ctrl.state.error, "{c} in {view}");
    }
}

/// The new station's seed is random, so it is not compared.
fn same_seed(actions: Vec<Action>) -> Vec<Action> {
    actions
        .into_iter()
        .map(|action| match action {
            Action::LoadRadio(request) => Action::LoadRadio(RadioRequest {
                seed: String::new(),
                ..request
            }),
            action => action,
        })
        .collect()
}

#[test]
fn letters_work_like_ctrl_keys_in_every_view_but_search_field() {
    // Search results, on an album.
    assert_letters_match_ctrl(|| {
        let mut app = shelf_app();
        app.key(Ctrl('f'));
        app.search("n", vec![artist("Nirvana")], vec![in_utero()]);
        app.keys(&[Down, Down]);
        app
    });
    // Shelf: an artist and a genre on the home page.
    assert_letters_match_ctrl(shelf_app);
    assert_letters_match_ctrl(|| genre_app(&["lofi"]));
    assert_letters_match_ctrl(|| {
        let mut app = curated_app();
        app.key(Enter);
        app
    });
    // An artist from the shelf and from Spotify.
    assert_letters_match_ctrl(|| {
        let mut app = shelf_app();
        app.key(Enter);
        app
    });
    assert_letters_match_ctrl(|| {
        let mut app = spotify_artist_app();
        app.send(Event::ArtistAlbums {
            artist_id: "id-nirvana".into(),
            result: Ok(vec![unplugged(), nevermind()]),
        });
        app
    });
    assert_letters_match_ctrl(nevermind_app);
    assert_letters_match_ctrl(|| {
        let mut app = nevermind_app();
        queue_track(&mut app, 1);
        app.key(Ctrl('q'));
        app
    });
    assert_letters_match_ctrl(|| {
        let mut app = genre_app(&[]);
        app.key(Enter);
        app
    });
    assert_letters_match_ctrl(|| {
        let mut app = genre_app(&["synthwave"]);
        start_radio(&mut app, station(0..50));
        app
    });
}

#[test]
fn letters_do_their_view_specific_thing() {
    let mut app = curated_app();
    app.key(Enter);
    assert_eq!(app.key(Char('e')), [Action::QueueAlbum(no_other())]);
    assert_eq!(app.key(Char('a')), [Action::AddToShelf(no_other())]);
    assert_eq!(app.key(Char('d')), [Action::RejectCurated(no_other().uri)]);

    let mut app = in_bloom_playing();
    assert_eq!(app.key(Char('l')), [set_liked("In Bloom", true)]);
    assert!(app.key(Char('f')).is_empty());
    assert!(matches!(app.state.view(), View::Search(_)));

    let mut app = genre_app(&["synthwave"]);
    start_radio(&mut app, station(0..50));
    assert_eq!(radio_request(&app.key(Char('r'))).genre, "synthwave");
}

#[test]
fn u_opens_queue_from_every_view_but_search() {
    let views: [fn() -> App; 6] = [
        shelf_app,
        || {
            let mut app = shelf_app();
            app.key(Enter);
            app
        },
        || {
            let mut app = curated_app();
            app.key(Enter);
            app
        },
        nevermind_app,
        || {
            let mut app = genre_app(&[]);
            app.key(Enter);
            app
        },
        || {
            let mut app = genre_app(&["synthwave"]);
            start_radio(&mut app, station(0..50));
            app
        },
    ];
    for setup in views {
        let mut app = setup();
        let view = format!("{:?}", app.state.view());
        assert!(app.key(Char('u')).is_empty(), "{view}");
        assert!(matches!(app.state.view(), View::Queue { .. }), "{view}");
        // In the queue u does not stack the view; Esc returns to where you came from.
        app.key(Char('u'));
        app.key(Esc);
        assert_eq!(format!("{:?}", app.state.view()), view);
    }
}

#[test]
fn letters_type_into_search_where_ctrl_keys_still_work() {
    let mut app = in_bloom_playing();
    app.key(Ctrl('f'));
    assert!(app.type_text("fleadru").is_empty());
    assert_eq!(app.search_view().query, "fleadru");
    assert_eq!(app.key(Ctrl('l')), [set_liked("In Bloom", true)]);
    assert!(matches!(app.state.view(), View::Search(_)));
    assert!(app.key(Ctrl('q')).is_empty());
    assert!(matches!(app.state.view(), View::Queue { .. }));
}

#[test]
fn capitals_and_q_keep_their_meaning() {
    let mut app = in_bloom_playing();
    assert!(
        app.keys(&[
            Char('F'),
            Char('L'),
            Char('E'),
            Char('A'),
            Char('D'),
            Char('R'),
            Char('U')
        ])
        .is_empty()
    );
    assert!(app.is_shelf());
    // q quits; the queue opens with u and Ctrl+Q.
    assert_eq!(app.key(Char('q')), [Action::Quit]);
    app.key(Ctrl('q'));
    assert!(matches!(app.state.view(), View::Queue { .. }));
}

fn radio_album_tracks(n: usize, tracks: &[usize]) -> Event {
    Event::AlbumTracks {
        uri: format!("spotify:album:record{n}"),
        result: Ok(tracks
            .iter()
            .map(|&t| Track {
                uri: format!("spotify:track:r{t}"),
                name: format!("Song {t}"),
                artist: format!("Artist {t}"),
                duration: Duration::from_secs(200),
            })
            .collect()),
    }
}

#[test]
fn n_does_nothing_when_nothing_plays() {
    let mut app = shelf_app();
    assert!(app.keys(&[Char('n'), Ctrl('n')]).is_empty());
    assert!(app.is_shelf());
    assert!(!app.state.has_now_playing());
    // The player reported a track, but there is no playback context.
    app.send(started("In Bloom"));
    assert!(app.key(Char('n')).is_empty());
    assert!(app.is_shelf());
}

#[test]
fn n_opens_playing_album_at_playing_track_and_esc_returns() {
    let mut app = nevermind_playing(2);
    app.keys(&[Esc, Esc]);
    assert!(app.is_shelf());
    assert!(app.state.has_now_playing());
    assert_eq!(app.key(Char('n')), [load_tracks("nevermind")]);
    assert_eq!(app.album_view().album, nevermind());
    assert_eq!(app.album_view().selected, 2);
    app.send(album_tracks(&nevermind(), Ok(nevermind_tracks())));
    assert_eq!(app.album_view().selected, 2);
    app.key(Esc);
    assert!(app.is_shelf());

    // From another view too: Esc returns there.
    app.key(Char('u'));
    assert_eq!(app.key(Ctrl('n')), [load_tracks("nevermind")]);
    app.key(Esc);
    assert!(matches!(app.state.view(), View::Queue { .. }));
}

#[test]
fn n_in_open_playing_album_only_moves_selection() {
    let mut app = nevermind_playing(2);
    app.keys(&[Up, Up]);
    assert_eq!(app.album_view().selected, 0);
    assert!(app.key(Char('n')).is_empty());
    assert_eq!(app.album_view().selected, 2);
    // No new view was opened: Esc returns to the artist's albums.
    app.key(Esc);
    assert_eq!(app.artist_view().artist.name, "Nirvana");
}

#[test]
fn n_types_in_search_and_ctrl_n_opens_playing_album() {
    let mut app = nevermind_playing(1);
    app.key(Ctrl('f'));
    assert!(app.key(Char('n')).is_empty());
    assert_eq!(app.search_view().query, "n");
    assert_eq!(app.key(Ctrl('n')), [load_tracks("nevermind")]);
    assert_eq!(app.album_view().selected, 1);
    app.key(Esc);
    assert_eq!(app.search_view().query, "n");
}

#[test]
fn n_opens_album_of_track_queued_from_another_album() {
    let mut app = nevermind_playing(3);
    queue_bleach(&mut app);
    app.keys(&[Right, Right]);
    assert_eq!(queue_names(&app), Vec::<&str>::new());
    app.keys(&[Esc, Esc]);
    assert_eq!(app.key(Char('n')), [load_tracks("bleach")]);
    assert_eq!(app.album_view().album, bleach());
    assert_eq!(app.album_view().selected, 1);
}

#[test]
fn n_opens_playing_radio_at_playing_track() {
    let mut app = genre_app(&["lofi"]);
    start_radio(&mut app, station(0..50));
    play_until(&mut app, 3);
    app.key(Esc);
    assert!(app.is_shelf());
    assert!(app.key(Char('n')).is_empty());
    assert_eq!(radio_view(&app).genre, lofi());
    assert_eq!(radio_view(&app).selected, 3);
    // The radio is already visible: only the selection moves.
    app.keys(&[Up, Up]);
    app.key(Char('n'));
    assert_eq!(radio_view(&app).selected, 3);
    app.key(Esc);
    assert!(app.is_shelf());
}

#[test]
fn n_on_queued_radio_track_opens_its_album_at_the_track() {
    let mut app = genre_app(&["lofi"]);
    start_radio(&mut app, station(0..50));
    app.keys(&[Down, Down, Down, Down, Down, Ctrl('e'), Right]);
    app.send(started("Song 5"));
    app.key(Esc);
    assert_eq!(
        app.key(Char('n')),
        [Action::LoadAlbumTracks("spotify:album:record5".into())]
    );
    let album = &app.album_view().album;
    assert_eq!(album.name, "Record 5");
    assert_eq!(album.artist, "Artist 5");
    assert_eq!(album.artist_id, "a5");
    assert_eq!(album.year, None);
    // The track's position becomes known when the tracks arrive.
    app.send(radio_album_tracks(5, &[4, 5, 6]));
    assert_eq!(app.album_view().selected, 1);
    // The album has no year: adding it to the shelf fetches details as in the radio.
    assert_eq!(
        app.key(Ctrl('a')),
        [Action::AddRadioAlbumToShelf("spotify:album:record5".into())]
    );
}

#[test]
fn n_works_after_restore_and_with_old_saved_state_from_shelf() {
    let mut app = in_bloom_with_queue();
    let saved = quit(&mut app);
    let (mut app, _) = restored(saved.clone());
    assert_eq!(app.key(Char('n')), [load_tracks("nevermind")]);
    assert_eq!(app.album_view().selected, 1);

    // An old file without album details: the album is found on the shelf.
    let mut json = serde_json::to_value(&saved).unwrap();
    json["album"].as_object_mut().unwrap().remove("album");
    let old: SavedPlayback = serde_json::from_value(json).unwrap();
    let (mut app, _) = restored(old.clone());
    assert_eq!(app.key(Char('n')), [load_tracks("nevermind")]);
    // Nor on the shelf: n does nothing.
    let mut app = App::new(Vec::new());
    app.send(Event::Restore(Box::new(old)));
    assert!(app.key(Char('n')).is_empty());
    assert!(app.is_shelf());
}
