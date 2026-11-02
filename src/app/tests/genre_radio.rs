//! Tests: genres and radio.

use super::*;

fn synthwave() -> &'static Genre {
    genres::find("synthwave").unwrap()
}

#[test]
fn remote_radio_starts_the_genre_and_shows_in_now_playing_info() {
    let mut app = genre_app(&[]);
    let request = radio_request(&app.send(Event::Radio("lofi".into())));
    assert_eq!(request.genre, "lofi");
    app.send(radio_loaded(&request, station(0..3)));
    app.send(started("Song"));
    let radio = app.state.now_playing_info(app.now).unwrap().radio.unwrap();
    assert_eq!(
        (radio.genre.as_str(), radio.name.as_str()),
        ("lofi", lofi().name.as_str())
    );
    let titles: Vec<_> = radio.tracks.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(titles, ["Song 0", "Song 1", "Song 2"]);
    assert_eq!(radio.tracks[1].album_uri, "spotify:album:record1");

    assert!(app.send(Event::Radio("no-such-genre".into())).is_empty());
    assert!(app.state.error.is_some());
}

#[test]
fn remote_favorites_and_curated_reject() {
    let mut app = genre_app(&["lofi"]);
    let on = |id: &str, on| Event::Favorite { id: id.into(), on };
    assert!(app.send(on("lofi", true)).is_empty());
    assert_eq!(
        app.send(on("synthwave", true)),
        [Action::AddFavorite("synthwave".into())]
    );
    assert_eq!(
        app.send(on("lofi", false)),
        [Action::RemoveFavorite("lofi".into())]
    );
    assert!(app.send(on("synthwave", false)).is_empty());
    assert_eq!(
        app.send(Event::RejectCurated("spotify:album:a".into())),
        [Action::RejectCurated("spotify:album:a".into())]
    );
}

#[test]
fn favorite_genres_come_after_curated_and_before_genres_row() {
    let app = genre_app(&["synthwave", "polka-noir", "lofi"]);
    assert_eq!(
        app.state.shelf_lists(),
        [
            ShelfRow::Curated(None),
            ShelfRow::Genre(lofi()),
            ShelfRow::Genre(synthwave()),
            ShelfRow::Genres
        ]
    );

    let state = State::new(Vec::new())
        .with_favorites(vec!["lofi".into()])
        .with_curated(Ok(Some(three_curated())));
    assert!(matches!(state.shelf_lists()[0], ShelfRow::Curated(_)));
    assert_eq!(state.shelf_lists()[1], ShelfRow::Genre(lofi()));
    assert!(matches!(state.view(), View::Shelf { selected: 0, .. }));
}

#[test]
fn enter_on_genre_starts_radio_and_opens_it_when_the_station_arrives() {
    let mut app = genre_app(&["lofi", "synthwave"]);
    let request = radio_request(&app.key(Enter));
    assert_eq!(request.genre, "lofi");
    assert_eq!(request.kind, RadioLoad::Station);
    assert!(request.previous.is_empty());
    assert!(lofi().seeds.iter().any(|s| s.uri == request.seed));
    // The view changes only once the station has been fetched.
    assert!(app.is_shelf());

    let actions = app.send(radio_loaded(&request, station(0..50)));
    assert_eq!(actions, [radio_play(0)]);
    assert_eq!(radio_view(&app).genre, lofi());
    assert_eq!(radio_view(&app).selected, 0);
    assert_eq!(app.state.playing_genre(), Some("lofi"));
    assert_eq!(app.state.radio_tracks("lofi").unwrap().len(), 50);
    assert_eq!(app.state.radio_tracks("synthwave"), None);
    assert_eq!(radio_of(&app).seeds, [request.seed]);

    // Esc returns to the shelf.
    assert_eq!(app.key(Esc), [Action::LoadLists]);
    assert!(app.is_shelf());
}

#[test]
fn enter_on_playing_genre_only_opens_the_radio() {
    let mut app = genre_app(&["lofi"]);
    start_radio(&mut app, station(0..50));
    play_until(&mut app, 2);
    app.key(Esc);
    // When opened, the selection is on the playing track.
    assert!(app.key(Enter).is_empty());
    assert_eq!(radio_view(&app).selected, 2);
    assert_eq!(app.state.views.len(), 2);
}

#[test]
fn failed_station_changes_nothing() {
    let mut app = nevermind_playing(0);
    app.state.views.truncate(1);
    app.state.favorites = vec!["lofi".into()];
    if let Some(View::Shelf { selected, .. }) = app.state.views.last_mut() {
        *selected = 1;
    }
    let request = radio_request(&app.key(Enter));
    let actions = app.send(Event::RadioLoaded {
        request,
        result: Err("could not load the radio: offline".into()),
    });
    assert!(actions.is_empty());
    assert_eq!(
        app.state.error.as_deref(),
        Some("could not load the radio: offline")
    );
    assert!(app.is_shelf());
    assert_eq!(app.state.playing_genre(), None);
    assert_eq!(app.state.playing_track("spotify:album:nevermind"), Some(0));

    // A stale result is ignored: only the latest request counts. Fixed random state so
    // that the requests get different seeds.
    app.state.rng = 1;
    let first = radio_request(&app.key(Enter));
    let second = radio_request(&app.key(Enter));
    assert_ne!(first, second);
    assert!(app.send(radio_loaded(&first, station(0..50))).is_empty());
    assert_eq!(
        app.send(radio_loaded(&second, station(0..50))),
        [radio_play(0)]
    );
    // A new radio replaces the album.
    assert_eq!(app.state.playing_track("spotify:album:nevermind"), None);
    assert_eq!(app.state.error, None);
}

#[test]
fn ctrl_d_on_genre_row_removes_it_from_home() {
    let mut app = genre_app(&["lofi", "synthwave"]);
    assert_eq!(app.key(Ctrl('d')), [Action::RemoveFavorite("lofi".into())]);
    app.send(Event::FavoritesChanged(vec!["synthwave".into()]));
    // The selection stays at the same position: the next row.
    assert_eq!(app.shelf_selected(), 1);
    assert_eq!(app.state.shelf_lists()[1], ShelfRow::Genre(synthwave()));
    // Ctrl+D on the Genres row, the Curated row or an artist does not remove a genre.
    app.key(Down);
    assert!(app.key(Ctrl('d')).is_empty());
    app.keys(&[Up, Up]);
    assert!(app.key(Ctrl('d')).is_empty());

    // A selection on an artist stays on the artist when more genre rows appear.
    app.keys(&[Down, Down, Down, Down]);
    assert_eq!(app.shelf_selected(), 4);
    app.send(Event::FavoritesChanged(vec![
        "lofi".into(),
        "synthwave".into(),
    ]));
    assert_eq!(app.shelf_selected(), 5);
    app.key(Enter);
    assert_eq!(app.artist_view().artist, artist("Nirvana"));
}

#[test]
fn more_genres_lists_catalog_and_ctrl_a_adds_synthwave_to_home() {
    let mut app = genre_app(&[]);
    assert_eq!(
        app.state.shelf_lists(),
        [ShelfRow::Curated(None), ShelfRow::Genres]
    );
    assert!(app.key(Enter).is_empty());
    assert!(matches!(app.state.view(), View::Genres { selected: 0, .. }));

    let index = genres::all()
        .iter()
        .position(|g| g.id == "synthwave")
        .unwrap();
    app.keys(&vec![Down; index]);
    assert!(matches!(app.state.view(), View::Genres { selected, .. } if *selected == index));
    assert_eq!(
        app.key(Ctrl('a')),
        [Action::AddFavorite("synthwave".into())]
    );
    app.send(Event::FavoritesChanged(vec!["synthwave".into()]));
    assert!(app.state.favorite("synthwave"));
    // A toggle: Ctrl+A again removes it.
    assert_eq!(
        app.key(Ctrl('a')),
        [Action::RemoveFavorite("synthwave".into())]
    );

    // Esc returns to the home page, where synthwave now is.
    assert_eq!(app.key(Esc), [Action::LoadLists]);
    assert_eq!(
        app.state.shelf_lists(),
        [
            ShelfRow::Curated(None),
            ShelfRow::Genre(synthwave()),
            ShelfRow::Genres
        ]
    );
}

#[test]
fn enter_in_more_genres_starts_radio_and_esc_returns_there() {
    let mut app = genre_app(&[]);
    app.key(Enter);
    let request = radio_request(&app.key(Enter));
    assert_eq!(request.genre, genres::all()[0].id);
    app.send(radio_loaded(&request, station(0..50)));
    assert_eq!(radio_view(&app).genre, &genres::all()[0]);
    assert!(app.key(Esc).is_empty());
    assert!(matches!(app.state.view(), View::Genres { selected: 0, .. }));
}

#[test]
fn radio_view_keys() {
    let mut app = genre_app(&["synthwave"]);
    let first = start_radio(&mut app, station(0..50));

    // Enter plays onward from the selected track.
    app.keys(&[Down, Down, Up]);
    assert_eq!(app.key(Enter), [radio_play(1)]);
    app.send(started("Song 1"));
    assert_eq!(app.state.playing_radio_track("synthwave"), Some(1));
    assert_eq!(app.send(end_of_track()), [radio_play(2)]);

    // Ctrl+E adds the track to the queue.
    assert!(app.key(Ctrl('e')).is_empty());
    assert_eq!(queue_names(&app), ["Song 1"]);
    assert_eq!(
        app.state.notice(app.now),
        Some((NoticeKind::Queued, "Song 1"))
    );

    // Ctrl+A fetches the album's year and adds it to the shelf, or removes it if there.
    let record = album("record1", "Record 1", "Various Artists", Some(2019));
    assert_eq!(
        app.key(Ctrl('a')),
        [Action::AddRadioAlbumToShelf(record.uri.clone())]
    );
    assert_eq!(
        app.send(Event::RadioAlbum(Ok(record.clone()))),
        [Action::AddToShelf(record.clone())]
    );
    app.send(Event::ShelfChanged(vec![record.clone()]));
    assert!(app.state.on_shelf(&record.uri));
    assert_eq!(
        app.key(Ctrl('a')),
        [Action::RemoveFromShelf(record.uri.clone())]
    );
    // The album reached the shelf before the result: it is not added twice.
    assert!(app.send(Event::RadioAlbum(Ok(record))).is_empty());
    // Fetching the album failed.
    app.send(Event::RadioAlbum(Err("could not add the album".into())));
    assert_eq!(app.state.error.as_deref(), Some("could not add the album"));

    // Ctrl+R: a new seed, and the radio starts over in the same view.
    let request = radio_request(&app.key(Ctrl('r')));
    assert_eq!(request.kind, RadioLoad::Station);
    assert_eq!(request.genre, "synthwave");
    assert_ne!(request.seed, first.seed);
    assert_eq!(
        app.send(radio_loaded(&request, station(100..150))),
        [radio_play(100)]
    );
    assert_eq!(app.state.views.len(), 2);
    assert_eq!(radio_view(&app).selected, 0);
    assert_eq!(radio_of(&app).seeds, [request.seed]);
    assert_eq!(app.state.error, None);
    // The queue is kept.
    assert_eq!(queue_names(&app), ["Song 1"]);
}

#[test]
fn player_keys_in_radio_and_genre_views() {
    let mut app = genre_app(&["lofi"]);
    start_radio(&mut app, station(0..50));
    assert_eq!(app.key(Char(' ')), [Action::TogglePause]);
    assert_eq!(app.key(Right), [radio_play(1)]);
    assert_eq!(app.key(Left), [radio_play(0)]);
    // The first track starts over.
    assert_eq!(app.key(Left), [radio_play(0)]);
    app.key(Esc);
    app.keys(&[Down, Enter]);
    assert!(matches!(app.state.view(), View::Genres { .. }));
    assert_eq!(app.key(Char(' ')), [Action::TogglePause]);
    assert_eq!(app.key(Char('q')), [Action::Quit]);
}
