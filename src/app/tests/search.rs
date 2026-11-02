//! Tests: search.

use super::*;

#[test]
fn ctrl_f_opens_search_from_shelf_and_esc_returns() {
    let mut app = shelf_app();
    app.key(Down);
    assert!(app.key(Ctrl('f')).is_empty());
    let search = app.search_view();
    assert_eq!(search.query, "");
    assert_eq!(search.results, None);
    app.key(Esc);
    assert_eq!(app.shelf_selected(), 3);
}

#[test]
fn ctrl_f_opens_search_from_artist_and_esc_returns() {
    let mut app = shelf_app();
    app.keys(&[Down, Enter, Down, Ctrl('f')]);
    app.search_view();
    app.key(Esc);
    assert_eq!(app.artist_view().selected, 1);
    assert_eq!(app.artist_view().artist, artist("Nirvana"));
}

#[test]
fn ctrl_f_in_search_keeps_search() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    app.type_text("foo");
    app.key(Ctrl('f'));
    assert_eq!(app.search_view().query, "foo");
    app.key(Esc);
    assert!(app.is_shelf());
}

#[test]
fn typing_edits_query_at_cursor() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    // Space, q and the arrows edit the search and do not control the player.
    assert!(app.type_text("the qeen").is_empty());
    assert!(app.keys(&[Left, Left, Left]).is_empty());
    app.key(Char('u'));
    assert_eq!(app.search_view().query, "the queen");
    assert_eq!(app.search_view().cursor, 6);
    app.keys(&[Right, Right, Right, Right, Right]);
    assert_eq!(app.search_view().cursor, 9);
    app.key(Backspace);
    assert_eq!(app.search_view().query, "the quee");
    app.keys(&[Left; 20]);
    assert_eq!(app.search_view().cursor, 0);
    app.key(Backspace);
    assert_eq!(app.search_view().query, "the quee");
}

#[test]
fn cursor_counts_characters_not_bytes() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    app.type_text("björk");
    app.keys(&[Left, Left, Left, Backspace]);
    assert_eq!(app.search_view().query, "börk");
    app.key(Char('j'));
    assert_eq!(app.search_view().query, "björk");
}

#[test]
fn search_starts_after_typing_pause() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    app.key(Char('f'));
    assert!(app.wait(100).is_empty());
    app.key(Char('o'));
    assert!(app.wait(100).is_empty());
    app.key(Char('o'));
    assert!(app.wait(299).is_empty());
    assert!(app.search_view().searching());
    assert_eq!(app.wait(1), [Action::Search("foo".into())]);
    assert!(app.search_view().searching());
    assert!(app.wait(300).is_empty());
}

#[test]
fn each_keystroke_restarts_debounce() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    app.type_text("fo");
    assert!(app.wait(200).is_empty());
    app.key(Char('o'));
    assert!(app.wait(200).is_empty());
    assert_eq!(app.wait(100), [Action::Search("foo".into())]);
}

#[test]
fn search_query_is_trimmed() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    app.type_text(" foo ");
    assert_eq!(app.wait(300), [Action::Search("foo".into())]);
}

#[test]
fn results_are_shown_and_stale_results_ignored() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    app.type_text("nir");
    assert_eq!(app.wait(300), [Action::Search("nir".into())]);
    app.type_text("vana");
    assert_eq!(app.wait(300), [Action::Search("nirvana".into())]);

    app.send(Event::Searched {
        query: "nir".into(),
        result: Ok(SearchResults {
            artists: vec![artist("Nirvana")],
            albums: vec![],
            songs: vec![],
        }),
    });
    assert_eq!(app.search_view().results, None);
    assert!(app.search_view().searching());

    app.send(Event::Searched {
        query: "nirvana".into(),
        result: Ok(SearchResults {
            artists: vec![artist("Nirvana")],
            albums: vec![nevermind()],
            songs: vec![],
        }),
    });
    let search = app.search_view();
    assert!(!search.searching());
    assert_eq!(
        search.items(),
        [
            SearchItem::Artist(&artist("Nirvana")),
            SearchItem::Album(&nevermind())
        ]
    );
}

#[test]
fn new_results_reset_selection() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    app.search("nirvana", vec![artist("Nirvana")], vec![nevermind()]);
    app.keys(&[Down, Down]);
    assert_eq!(app.search_view().selected, 1);
    // f returns to the search field with the cursor at the end.
    app.keys(&[Char('f'), Char('s')]);
    assert_eq!(app.wait(300), [Action::Search("nirvanas".into())]);
    assert_eq!(app.search_view().selected, 1);
    app.send(Event::Searched {
        query: "nirvanas".into(),
        result: Ok(SearchResults {
            artists: vec![artist("Nirvanas")],
            albums: vec![nevermind()],
            songs: vec![],
        }),
    });
    assert_eq!(app.search_view().selected, 0);
}

#[test]
fn same_query_is_not_searched_again() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    app.search("foo", vec![], vec![colour()]);
    app.key(Backspace);
    app.key(Char('o'));
    assert!(app.wait(300).is_empty());
    assert_eq!(app.search_view().results.as_ref().unwrap().albums.len(), 1);
}

#[test]
fn clearing_query_clears_results_without_search() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    app.search("ab", vec![artist("ABBA")], vec![]);
    app.keys(&[Backspace, Backspace]);
    assert_eq!(app.search_view().results, None);
    assert!(!app.search_view().searching());
    assert!(app.wait(300).is_empty());
    // The same search starts again after clearing.
    app.type_text("ab");
    assert_eq!(app.wait(300), [Action::Search("ab".into())]);
}

#[test]
fn whitespace_only_query_is_not_searched() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    app.type_text("  ");
    assert!(app.wait(300).is_empty());
    assert!(!app.search_view().searching());
}

#[test]
fn failed_search_shows_error_and_can_retry() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    app.type_text("foo");
    app.wait(300);
    app.send(Event::Searched {
        query: "foo".into(),
        result: Err("Spotify is not responding".into()),
    });
    assert_eq!(
        app.state.error.as_deref(),
        Some("Spotify is not responding")
    );
    assert!(!app.search_view().searching());
    app.key(Backspace);
    app.key(Char('o'));
    assert_eq!(app.wait(300), [Action::Search("foo".into())]);
}

#[test]
fn search_up_down_moves_over_artists_then_albums() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    app.search(
        "n",
        vec![artist("Nirvana"), artist("Nick Cave")],
        vec![nevermind()],
    );
    app.keys(&[Down, Down, Down, Down, Down]);
    assert_eq!(app.search_view().selected, 2);
    app.key(Up);
    assert_eq!(app.search_view().selected, 1);
}

#[test]
fn search_results_are_entered_with_down_or_enter_and_left_with_up_or_f() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    app.search(
        "n",
        vec![artist("Nirvana"), artist("Nick Cave")],
        vec![nevermind()],
    );
    // While typing no row is selected, and Up stays in the field.
    assert!(!app.search_view().in_results);
    assert!(app.key(Up).is_empty());
    assert!(!app.search_view().in_results);
    app.key(Down);
    assert!(app.search_view().in_results);
    assert_eq!(app.search_view().selected, 0);
    // Up on the first row returns to the field.
    app.key(Up);
    assert!(!app.search_view().in_results);
    // Enter in the field moves to the first row, like Down.
    assert!(app.key(Enter).is_empty());
    assert!(app.search_view().in_results);
    app.key(Down);
    assert_eq!(app.search_view().selected, 1);
    // f returns to the field, and the next visit starts from the first row.
    app.key(Char('f'));
    assert!(!app.search_view().in_results);
    assert_eq!(app.search_view().query, "n");
    app.key(Down);
    assert_eq!(app.search_view().selected, 0);
}

#[test]
fn search_results_take_player_keys_and_ignore_other_letters() {
    let mut app = in_bloom_playing();
    app.key(Ctrl('f'));
    app.search("n", vec![artist("Nirvana")], vec![]);
    app.key(Down);
    assert!(app.keys(&[Char('m'), Char('x'), Backspace]).is_empty());
    assert_eq!(app.search_view().query, "n");
    assert_eq!(app.key(Char(' ')), [Action::TogglePause]);
    assert_eq!(app.key(Char('q')), [Action::Quit]);
    // Esc closes search, like everywhere.
    app.key(Esc);
    assert!(!matches!(app.state.view(), View::Search(_)));
}

#[test]
fn ctrl_f_returns_to_search_field() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    app.search("nirvana", vec![artist("Nirvana")], vec![nevermind()]);
    app.keys(&[Down, Down, Enter]);
    app.key(Ctrl('f'));
    assert!(!app.search_view().in_results);
    assert_eq!(app.search_view().query, "nirvana");
}

#[test]
fn empty_results_leave_search_results() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    app.search("nirvana", vec![artist("Nirvana")], vec![]);
    app.key(Down);
    // A search still waiting for its delay brings no results.
    app.key(Up);
    app.key(Char('x'));
    app.key(Down);
    assert!(app.search_view().in_results);
    assert_eq!(app.wait(300), [Action::Search("nirvanax".into())]);
    app.send(Event::Searched {
        query: "nirvanax".into(),
        result: Ok(SearchResults::default()),
    });
    assert!(!app.search_view().in_results);
}

#[test]
fn search_keys_without_results_do_nothing() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    assert!(app.keys(&[Up, Down, Enter, Ctrl('a')]).is_empty());
    assert_eq!(app.search_view().selected, 0);
}

#[test]
fn enter_on_search_album_opens_its_tracks_without_playing() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    app.search("n", vec![artist("Nirvana")], vec![nevermind(), in_utero()]);
    app.keys(&[Down, Down, Down]);
    assert_eq!(app.key(Enter), [load_tracks("inutero")]);
    assert_eq!(app.album_view().album, in_utero());
    // Esc returns to the search results, with the text and selection intact.
    app.key(Esc);
    assert_eq!(app.search_view().query, "n");
    assert!(app.search_view().in_results);
    assert_eq!(app.search_view().selected, 2);
}

fn in_bloom() -> Song {
    Song {
        track: track("In Bloom", 254),
        album: nevermind(),
    }
}

#[test]
fn search_lists_songs_after_albums() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    app.search_all(
        "bloom",
        vec![artist("Nirvana")],
        vec![nevermind()],
        vec![in_bloom()],
    );
    let song = in_bloom();
    assert_eq!(
        app.search_view().items(),
        [
            SearchItem::Artist(&artist("Nirvana")),
            SearchItem::Album(&nevermind()),
            SearchItem::Song(&song),
        ]
    );
}

#[test]
fn enter_on_search_song_plays_its_album_from_the_song() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    app.search_all("bloom", vec![], vec![], vec![in_bloom()]);
    app.key(Down);
    assert_eq!(app.key(Enter), [load_tracks("nevermind")]);
    assert_eq!(app.album_view().album, nevermind());
    // The album plays from the song once its tracks are in, and the song is selected.
    let actions = app.send(album_tracks(&nevermind(), Ok(nevermind_tracks())));
    assert!(actions.contains(&play("nevermind", 1)), "{actions:?}");
    assert_eq!(app.album_view().selected, 1);
}

#[test]
fn ctrl_e_on_search_song_plays_or_queues_it_and_ctrl_a_shelves_its_album() {
    let mut app = App::new(vec![]);
    app.key(Ctrl('f'));
    app.search_all("bloom", vec![], vec![], vec![in_bloom()]);
    app.key(Down);
    // Nothing is playing: the queued song starts at once.
    assert_eq!(
        app.key(Ctrl('e')),
        [Action::Play(Item::Track(track_uri("In Bloom")))]
    );
    assert_eq!(app.key(Ctrl('a')), [Action::AddToShelf(nevermind())]);
}

#[test]
fn ctrl_a_on_search_album_adds_to_shelf() {
    let mut app = App::new(vec![]);
    app.key(Ctrl('f'));
    app.search("n", vec![artist("Nirvana")], vec![nevermind()]);
    // In the field and on an artist Ctrl+A does nothing.
    assert!(app.key(Ctrl('a')).is_empty());
    app.key(Down);
    assert!(app.key(Ctrl('a')).is_empty());
    app.key(Down);
    assert_eq!(app.key(Ctrl('a')), [Action::AddToShelf(nevermind())]);
    assert!(!app.state.on_shelf(&nevermind().uri));

    app.send(Event::ShelfChanged(vec![nevermind()]));
    assert!(app.state.on_shelf(&nevermind().uri));
    // Ctrl+A on an album already on the shelf removes it.
    assert_eq!(
        app.key(Ctrl('a')),
        [Action::RemoveFromShelf(nevermind().uri)]
    );
    app.search_view();
}

#[test]
fn search_marks_albums_on_shelf() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    app.search(
        "n",
        vec![],
        vec![nevermind(), album("x", "Nevermind (Live)", "Nirvana", None)],
    );
    let marked: Vec<bool> = app
        .search_view()
        .items()
        .iter()
        .map(|item| match item {
            SearchItem::Album(a) => app.state.on_shelf(&a.uri),
            SearchItem::Artist(_) => false,
            SearchItem::Song(s) => app.state.on_shelf(&s.album.uri),
        })
        .collect();
    assert_eq!(marked, [true, false]);
}

#[test]
fn search_ctrl_d_does_nothing() {
    let mut app = shelf_app();
    app.key(Ctrl('f'));
    app.search("n", vec![], vec![nevermind()]);
    app.key(Down);
    assert!(app.key(Ctrl('d')).is_empty());
}
