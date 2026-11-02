//! Tests: lists on the shelf and the Curated list.

use super::*;

fn curated_selected(app: &App) -> usize {
    match app.state.view() {
        View::Curated { selected, .. } => *selected,
        other => panic!("expected the Curated list, got {other:?}"),
    }
}

fn curated_names(app: &App) -> Vec<String> {
    app.state
        .curated()
        .map(|list| list.albums.iter().map(|a| a.album.name.clone()).collect())
        .unwrap_or_default()
}

#[test]
fn list_row_comes_before_artists_and_starts_selected() {
    let mut app = curated_app();
    let lists = app.state.shelf_lists();
    assert_eq!(lists.len(), 2);
    assert!(
        matches!(&lists[0], ShelfRow::Curated(Some(list)) if list.subtitle == "3 albums · week 41")
    );
    assert_eq!(lists[1], ShelfRow::Genres);
    assert_eq!(app.shelf_selected(), 0);
    // List rows and three artists as one list.
    app.keys(&[Down, Down, Down, Down, Down, Down]);
    assert_eq!(app.shelf_selected(), 4);
    app.keys(&[Up, Up]);
    assert_eq!(app.shelf_selected(), 2);
    // Enter on an artist opens the artist's albums.
    assert!(app.key(Enter).is_empty());
    assert_eq!(app.artist_view().artist, artist("Foo Fighters"));
}

#[test]
fn enter_on_list_row_opens_list_and_reloads_it() {
    let mut app = curated_app();
    assert_eq!(app.key(Enter), [Action::LoadLists]);
    assert_eq!(curated_selected(&app), 0);
    assert_eq!(
        curated_names(&app),
        ["No Other", "#1 Record", "Transatlanticism"]
    );
    assert_eq!(app.key(Esc), [Action::LoadLists]);
    assert_eq!(app.shelf_selected(), 0);
}

#[test]
fn without_list_curated_row_stays_on_shelf() {
    for list in [None, Some(curated(&[]))] {
        let mut app = shelf_app();
        app.state = State::new(app.state.shelf.clone()).with_curated(Ok(list.clone()));
        assert_eq!(
            app.state.shelf_lists(),
            [ShelfRow::Curated(list.as_ref()), ShelfRow::Genres]
        );
        app.keys(&[Down, Down, Down, Down]);
        assert_eq!(app.shelf_selected(), 4);
        app.key(Enter);
        assert_eq!(app.artist_view().artist, artist("Radiohead"));
    }
}

#[test]
fn curated_row_without_list_opens_empty_view() {
    let mut app = App::at_top(shelf_app().state.shelf);
    assert_eq!(app.key(Enter), [Action::LoadLists]);
    assert_eq!(curated_selected(&app), 0);
    assert_eq!(app.state.curated(), None);
    // There are no albums: the keys do nothing.
    assert!(
        app.keys(&[Up, Down, Enter, Ctrl('e'), Ctrl('a'), Ctrl('d')])
            .is_empty()
    );
    assert_eq!(curated_selected(&app), 0);
    assert_eq!(app.key(Esc), [Action::LoadLists]);
    assert_eq!(app.shelf_selected(), 0);
}

#[test]
fn list_appearing_or_disappearing_keeps_selected_artist() {
    let mut app = shelf_app();
    app.keys(&[Down, Down]);
    assert_eq!(app.shelf_selected(), 4);
    app.send(Event::ListsLoaded(Ok(Some(three_curated()))));
    assert_eq!(app.shelf_selected(), 4);
    app.send(Event::ListsLoaded(Ok(None)));
    assert_eq!(app.shelf_selected(), 4);
    app.key(Enter);
    assert_eq!(app.artist_view().artist, artist("Radiohead"));

    // Selection on the Curated row: it stays there when the list empties.
    let mut app = curated_app();
    app.send(Event::ListsLoaded(Ok(Some(curated(&[])))));
    assert_eq!(app.shelf_selected(), 0);
    assert!(matches!(
        app.state.shelf_lists()[0],
        ShelfRow::Curated(Some(list)) if list.albums.is_empty()
    ));
}

#[test]
fn broken_list_file_shows_error_and_keeps_list_row_without_list() {
    let mut app = curated_app();
    app.send(Event::ListsLoaded(
        Err("curated list file is broken".into()),
    ));
    assert_eq!(
        app.state.error.as_deref(),
        Some("curated list file is broken")
    );
    assert_eq!(
        app.state.shelf_lists(),
        [ShelfRow::Curated(None), ShelfRow::Genres]
    );
    assert_eq!(app.state.curated(), None);
    assert!(app.state.curated_broken());

    // Fixed file: the error goes away and the list comes back.
    app.send(Event::ListsLoaded(Ok(Some(three_curated()))));
    assert_eq!(app.state.error, None);
    assert!(!app.state.curated_broken());
    assert!(matches!(
        app.state.shelf_lists()[0],
        ShelfRow::Curated(Some(_))
    ));

    // A successful load does not clear an unrelated error.
    app.send(Event::Error("the shelf is read-only".into()));
    app.send(Event::ListsLoaded(Ok(Some(three_curated()))));
    assert_eq!(app.state.error.as_deref(), Some("the shelf is read-only"));
}

#[test]
fn curated_keys() {
    let mut app = curated_app();
    app.key(Enter);
    assert!(app.key(Up).is_empty());
    assert_eq!(curated_selected(&app), 0);
    app.keys(&[Down, Down, Down]);
    assert_eq!(curated_selected(&app), 2);
    app.key(Up);

    assert_eq!(app.key(Ctrl('e')), [Action::QueueAlbum(number_one())]);
    assert_eq!(app.key(Ctrl('a')), [Action::AddToShelf(number_one())]);
    app.send(Event::ShelfChanged(vec![number_one(), colour()]));
    assert_eq!(
        app.key(Ctrl('a')),
        [Action::RemoveFromShelf(number_one().uri)]
    );
    assert_eq!(
        app.key(Ctrl('d')),
        [Action::RejectCurated(number_one().uri)]
    );
    // The state changes only once the rejection has been saved.
    assert_eq!(curated_names(&app).len(), 3);

    // Enter opens the track list without playing; Esc goes back without rereading.
    assert_eq!(app.key(Enter), [load_tracks("number1")]);
    assert_eq!(app.album_view().album, number_one());
    assert!(app.key(Esc).is_empty());
    assert_eq!(curated_selected(&app), 1);
}

#[test]
fn rejecting_keeps_selection_in_list_until_nothing_is_left() {
    let mut app = curated_app();
    app.keys(&[Enter, Down, Down]);
    app.send(Event::Error("could not save".into()));
    app.send(Event::CuratedChanged(Some(curated(&[
        no_other(),
        number_one(),
    ]))));
    assert_eq!(app.state.error, None);
    assert_eq!(curated_selected(&app), 1);
    assert_eq!(curated_names(&app), ["No Other", "#1 Record"]);

    app.send(Event::CuratedChanged(Some(curated(&[]))));
    assert_eq!(curated_selected(&app), 0);
    assert!(
        app.keys(&[Up, Down, Enter, Ctrl('e'), Ctrl('a'), Ctrl('d')])
            .is_empty()
    );

    // The empty list's Curated row stays on the shelf, and so does the selection.
    assert_eq!(app.key(Esc), [Action::LoadLists]);
    assert_eq!(app.state.shelf_lists().len(), 2);
    assert_eq!(app.shelf_selected(), 0);
}

#[test]
fn failed_reject_keeps_list_and_shows_error() {
    let mut app = curated_app();
    app.keys(&[Enter, Ctrl('d')]);
    app.send(Event::Error("curated history file is broken".into()));
    assert_eq!(curated_names(&app).len(), 3);
    assert_eq!(
        app.state.error.as_deref(),
        Some("curated history file is broken")
    );
}

#[test]
fn player_keys_in_curated_view() {
    let mut app = curated_app();
    app.key(Enter);
    assert_eq!(app.key(Char(' ')), [Action::TogglePause]);
    assert_eq!(app.key(Char('q')), [Action::Quit]);
    app.key(Ctrl('f'));
    app.search_view();
}
