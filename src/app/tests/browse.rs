//! Tests: album shelf and artist views.

use super::*;

#[test]
fn starts_on_shelf_with_artists_alphabetical() {
    let app = App::at_top(shelf_app().state.shelf);
    assert!(app.is_shelf());
    assert_eq!(app.shelf_selected(), 0);
    assert_eq!(
        app.state.shelf_lists(),
        [ShelfRow::Curated(None), ShelfRow::Genres]
    );
    assert_eq!(
        names(app.state.shelf_artists()),
        ["Foo Fighters", "Nirvana", "Radiohead"]
    );
    assert_eq!(app.state.now_playing, None);
    assert_eq!(app.state.error, None);
}

#[test]
fn shelf_up_down_stay_in_bounds() {
    let mut app = App::at_top(shelf_app().state.shelf);
    assert!(app.key(Up).is_empty());
    assert_eq!(app.shelf_selected(), 0);
    // Curated, Genres and three artists.
    app.keys(&[Down, Down, Down, Down, Down]);
    assert_eq!(app.shelf_selected(), 4);
    app.key(Up);
    assert_eq!(app.shelf_selected(), 3);
}

#[test]
fn empty_shelf_keys_do_nothing_and_search_opens() {
    let mut app = App::new(vec![]);
    assert!(app.state.shelf_artists().is_empty());
    assert!(app.keys(&[Up, Down, Ctrl('d'), Ctrl('a'), Esc]).is_empty());
    assert!(app.is_shelf());
    assert_eq!(app.shelf_selected(), 1);

    app.key(Ctrl('f'));
    app.search_view();

    // The rows are Curated and Genres; the selection is on Genres.
    app.key(Esc);
    assert!(app.key(Enter).is_empty());
    assert!(matches!(app.state.view(), View::Genres { selected: 0, .. }));
}

#[test]
fn enter_opens_artist_shelf_albums_oldest_first() {
    let mut app = shelf_app();
    app.key(Down);
    assert!(app.key(Enter).is_empty());
    let view = app.artist_view();
    assert_eq!(view.artist, artist("Nirvana"));
    assert!(matches!(view.source, Source::Shelf(_)));
    assert_eq!(view.selected, 0);
    assert_eq!(app.artist_albums(), ["Bleach", "Nevermind", "In Utero"]);
}

#[test]
fn esc_returns_to_shelf_keeping_selection() {
    let mut app = shelf_app();
    app.keys(&[Down, Down, Enter]);
    // The shelf comes into view: the lists are reread.
    assert_eq!(app.key(Esc), [Action::LoadLists]);
    assert_eq!(app.shelf_selected(), 4);
    // The shelf is at the bottom: Esc goes nowhere.
    app.key(Esc);
    assert_eq!(app.shelf_selected(), 4);
}

// --- Artist's albums from the shelf ---

#[test]
fn artist_up_down_stay_in_bounds() {
    let mut app = shelf_app();
    app.keys(&[Down, Enter, Up]);
    assert_eq!(app.artist_view().selected, 0);
    app.keys(&[Down, Down, Down, Down]);
    assert_eq!(app.artist_view().selected, 2);
}

#[test]
fn enter_opens_selected_albums_tracks_without_playing() {
    let mut app = shelf_app();
    app.keys(&[Down, Enter, Down]);
    assert_eq!(app.key(Enter), [load_tracks("nevermind")]);
    let view = app.album_view();
    assert_eq!(view.album, nevermind());
    assert_eq!(view.tracks, None);
    assert_eq!(view.selected, 0);
}

#[test]
fn ctrl_d_removes_selected_album_from_shelf() {
    let mut app = shelf_app();
    app.keys(&[Down, Enter, Down]);
    assert_eq!(
        app.key(Ctrl('d')),
        [Action::RemoveFromShelf("spotify:album:nevermind".into())]
    );
    // The state changes only once the removal has been saved.
    assert_eq!(app.artist_albums(), ["Bleach", "Nevermind", "In Utero"]);

    app.send(Event::ShelfChanged(vec![
        in_utero(),
        ok_computer(),
        colour(),
        bleach(),
    ]));
    assert_eq!(app.artist_albums(), ["Bleach", "In Utero"]);
    assert_eq!(app.artist_view().selected, 1);
}

#[test]
fn removing_last_album_clamps_selection() {
    let mut app = shelf_app();
    app.keys(&[Down, Enter, Down, Down]);
    app.key(Ctrl('d'));
    app.send(Event::ShelfChanged(vec![
        ok_computer(),
        nevermind(),
        colour(),
        bleach(),
    ]));
    assert_eq!(app.artist_albums(), ["Bleach", "Nevermind"]);
    assert_eq!(app.artist_view().selected, 1);
}

#[test]
fn removing_artists_last_album_returns_to_shelf() {
    let mut app = shelf_app();
    app.keys(&[Down, Down, Enter]);
    assert_eq!(
        app.key(Ctrl('d')),
        [Action::RemoveFromShelf("spotify:album:okc".into())]
    );
    app.send(Event::ShelfChanged(vec![
        in_utero(),
        nevermind(),
        colour(),
        bleach(),
    ]));
    assert!(app.is_shelf());
    assert_eq!(
        names(app.state.shelf_artists()),
        ["Foo Fighters", "Nirvana"]
    );
    assert_eq!(app.shelf_selected(), 3);
}

#[test]
fn removing_only_album_leaves_empty_shelf() {
    let mut app = App::new(vec![colour()]);
    app.key(Enter);
    app.key(Ctrl('d'));
    app.send(Event::ShelfChanged(vec![]));
    assert!(app.is_shelf());
    // The selection is on the last row, Genres.
    assert_eq!(app.shelf_selected(), 1);
    assert!(app.state.shelf_artists().is_empty());
}

#[test]
fn failed_removal_keeps_view_and_shows_error() {
    let mut app = shelf_app();
    app.keys(&[Down, Down, Enter, Ctrl('d')]);
    app.send(Event::Error("the shelf is read-only".into()));
    assert_eq!(app.artist_albums(), ["OK Computer"]);
    assert_eq!(app.state.error.as_deref(), Some("the shelf is read-only"));
}

#[test]
fn ctrl_a_removes_from_shelf_artist_view() {
    let mut app = shelf_app();
    app.keys(&[Down, Down, Enter]);
    assert_eq!(
        app.key(Ctrl('a')),
        [Action::RemoveFromShelf("spotify:album:okc".into())]
    );
}

// --- Artist's albums from Spotify ---

#[test]
fn enter_on_search_artist_loads_spotify_albums() {
    let mut app = spotify_artist_app();
    let view = app.artist_view();
    assert_eq!(view.artist, artist("Nirvana"));
    assert!(matches!(view.source, Source::Spotify(None)));
    assert!(app.artist_albums().is_empty());

    app.send(Event::ArtistAlbums {
        artist_id: "id-nirvana".into(),
        result: Ok(vec![unplugged(), in_utero(), bleach(), nevermind()]),
    });
    assert_eq!(
        app.artist_albums(),
        [
            "Bleach",
            "Nevermind",
            "In Utero",
            "MTV Unplugged In New York"
        ]
    );
}

#[test]
fn albums_of_other_artist_are_ignored() {
    let mut app = spotify_artist_app();
    app.send(Event::ArtistAlbums {
        artist_id: "id-radiohead".into(),
        result: Ok(vec![ok_computer()]),
    });
    assert!(matches!(app.artist_view().source, Source::Spotify(None)));
}

#[test]
fn failed_artist_albums_shows_error() {
    let mut app = spotify_artist_app();
    app.send(Event::ArtistAlbums {
        artist_id: "id-nirvana".into(),
        result: Err("Spotify is not responding".into()),
    });
    assert!(matches!(
        app.artist_view().source,
        Source::Spotify(Some(ref albums)) if albums.is_empty()
    ));
    assert_eq!(
        app.state.error.as_deref(),
        Some("Spotify is not responding")
    );
}

#[test]
fn spotify_artist_keys() {
    let mut app = spotify_artist_app();
    app.send(Event::ArtistAlbums {
        artist_id: "id-nirvana".into(),
        result: Ok(vec![unplugged(), nevermind()]),
    });
    // Nevermind is on the shelf: Ctrl+A removes it, Ctrl+D does nothing.
    assert!(app.state.on_shelf(&nevermind().uri));
    assert!(!app.state.on_shelf(&unplugged().uri));
    assert_eq!(
        app.key(Ctrl('a')),
        [Action::RemoveFromShelf(nevermind().uri)]
    );
    assert!(app.key(Ctrl('d')).is_empty());

    app.key(Down);
    assert_eq!(app.key(Ctrl('a')), [Action::AddToShelf(unplugged())]);
    assert_eq!(app.key(Enter), [load_tracks("unplugged")]);
    assert_eq!(app.album_view().album, unplugged());
    app.key(Esc);

    app.send(Event::ShelfChanged(vec![
        in_utero(),
        ok_computer(),
        nevermind(),
        colour(),
        bleach(),
        unplugged(),
    ]));
    assert!(app.state.on_shelf(&unplugged().uri));
    assert_eq!(app.artist_view().selected, 1);
}

#[test]
fn esc_from_spotify_artist_returns_to_search() {
    let mut app = spotify_artist_app();
    app.key(Esc);
    assert_eq!(app.search_view().query, "nirvana");
    app.key(Esc);
    assert!(app.is_shelf());
}

#[test]
fn ctrl_f_from_spotify_artist_returns_to_search() {
    let mut app = spotify_artist_app();
    app.key(Ctrl('f'));
    assert_eq!(app.search_view().query, "nirvana");
    app.key(Esc);
    assert!(app.is_shelf());
}

// --- Keys everywhere ---

/// When nothing is playing, ←/→ do nothing (play order: see Queue).
#[test]
fn player_keys_outside_search() {
    let mut app = shelf_app();
    for setup in [&[][..], &[Enter][..]] {
        app.keys(setup);
        assert_eq!(app.key(Char(' ')), [Action::TogglePause]);
        assert!(app.keys(&[Left, Right]).is_empty());
        assert_eq!(app.key(Char('q')), [Action::Quit]);
    }
    app.artist_view();
}

#[test]
fn player_keys_in_spotify_artist_view() {
    let mut app = spotify_artist_app();
    assert_eq!(app.key(Char(' ')), [Action::TogglePause]);
    assert!(app.keys(&[Left, Right]).is_empty());
    assert_eq!(app.key(Char('q')), [Action::Quit]);
}

#[test]
fn ctrl_c_quits_everywhere() {
    let mut app = shelf_app();
    assert_eq!(app.key(Ctrl('c')), [Action::Quit]);
    app.key(Enter);
    assert_eq!(app.key(Ctrl('c')), [Action::Quit]);
    app.key(Ctrl('f'));
    assert_eq!(app.key(Ctrl('c')), [Action::Quit]);
}

#[test]
fn other_keys_do_nothing() {
    let mut app = shelf_app();
    assert!(app.keys(&[Char('x'), Ctrl('x'), Backspace]).is_empty());
    app.key(Enter);
    assert!(app.keys(&[Char('x'), Ctrl('x'), Backspace]).is_empty());
    app.artist_view();
}
