//! Tests: the artist and search views, and scrolling long lists.

use super::*;

#[test]
fn artist_view_from_shelf_lists_albums_oldest_first() {
    let now = Instant::now();
    let mut state = State::new(shelf());
    for key in [Key::Down, Key::Down, Key::Down, Key::Enter] {
        state.update(Event::Key(key), now);
    }
    let buffer = render(&state, now, 70, 9);

    let rows = rows(&buffer);
    assert_eq!(rows[0], "DECK");
    assert_eq!(rows[4], "1989  Bleach");
    assert_eq!(rows[5], "1991  Nevermind");
    assert_eq!(rows[7], "─".repeat(70));
    assert_eq!(
        rows[8],
        "[e] add to queue  [d] remove from shelf  [Esc] back  [q] quit"
    );
    // On the selected row the year is a grey that stands out from the background, the
    // name light.
    assert_eq!(buffer[(0, 4)].fg, SELECTED_MUTED);
    assert_eq!(buffer[(0, 5)].fg, MUTED);
    assert!(is_selected(&buffer, 0, 4));
    assert!(is_selected(&buffer, 6, 4));
    assert_eq!(buffer[(6, 4)].fg, SELECTED_FG);
    assert!(!is_selected(&buffer, 6, 5));
}

#[test]
fn search_view_groups_results_and_marks_shelf_albums() {
    let now = Instant::now();
    let mut state = State::new(vec![colour()]);
    state.update(Event::Key(Key::Ctrl('f')), now);
    for c in "foo".chars() {
        state.update(Event::Key(Key::Char(c)), now);
    }
    let later = now + Duration::from_millis(300);
    state.update(Event::Tick, later);
    state.update(
        Event::Searched {
            query: "foo".to_owned(),
            result: Ok(SearchResults {
                artists: vec![Artist {
                    id: "id-foo-fighters".to_owned(),
                    name: "Foo Fighters".to_owned(),
                }],
                albums: vec![
                    colour(),
                    album(
                        "echoes",
                        "Echoes, Silence, Patience & Grace",
                        "Foo Fighters",
                        None,
                    ),
                ],
                songs: vec![],
            }),
        },
        later,
    );
    for key in [Key::Down, Key::Down, Key::Down] {
        state.update(Event::Key(key), later);
    }

    let mut terminal = Terminal::new(TestBackend::new(70, 15)).unwrap();
    terminal.draw(|frame| draw(frame, &state, later)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    assert_eq!(
        rows(&buffer)[..12],
        [
            "DECK",
            "—",
            format!("●{}", "─".repeat(69)).as_str(),
            "",
            "Search: foo",
            "",
            "Artists",
            "Foo Fighters",
            "",
            "Albums",
            "The Colour And The Shape  Foo Fighters · 1997 ●",
            "Echoes, Silence, Patience & Grace  Foo Fighters",
        ]
    );
    // The on-shelf mark is green, and the selection (second album) is highlighted.
    assert_eq!(buffer[(46, 10)].fg, Color::Green);
    assert!(is_selected(&buffer, 0, 11));
    assert!(!is_selected(&buffer, 0, 10));
    // The artist is dim; on the selected row a grey that stands out from the
    // background.
    assert_eq!(buffer[(26, 10)].fg, MUTED);
    assert_eq!(buffer[(35, 11)].fg, SELECTED_MUTED);
    // Group headings are highlighted in their own colour and bold.
    for y in [6, 9] {
        assert_eq!(buffer[(0, y)].fg, HEADING, "y {y}");
        assert!(buffer[(0, y)].modifier.contains(Modifier::BOLD), "y {y}");
    }
    // The search text is underlined in the accent colour up to the cursor; the prompt
    // is not.
    for x in 8..=11 {
        assert!(
            buffer[(x, 4)].modifier.contains(Modifier::UNDERLINED),
            "x {x}"
        );
        assert_eq!(buffer[(x, 4)].underline_color, ACCENT, "x {x}");
        assert_eq!(buffer[(x, 4)].fg, Color::Reset, "x {x}");
    }
    assert!(!buffer[(7, 4)].modifier.contains(Modifier::UNDERLINED));
    assert!(!buffer[(12, 4)].modifier.contains(Modifier::UNDERLINED));
    assert_eq!(buffer[(0, 4)].fg, MUTED);
}

#[test]
fn search_field_shows_cursor_and_results_show_selection() {
    let now = Instant::now();
    let mut state = long_search(2, now);
    // While typing: the cursor after the search text, no row selected, Ctrl keys.
    let mut terminal = Terminal::new(TestBackend::new(70, 12)).unwrap();
    terminal.draw(|frame| draw(frame, &state, now)).unwrap();
    terminal.backend_mut().assert_cursor_position((9, 4));
    let buffer = terminal.backend().buffer().clone();
    assert_eq!(row(&buffer, 7), "Foo Fighters");
    assert!(!is_selected(&buffer, 0, 7));
    assert_eq!(row(&buffer, 11), "[↓] results  [Esc] back  [Ctrl+C] quit");

    // In the results: the first row selected and plain letters.
    state.update(Event::Key(Key::Down), now);
    let buffer = render(&state, now, 70, 12);
    assert!(is_selected(&buffer, 0, 7));
    assert_eq!(
        row(&buffer, 11),
        "[e] add to queue  [a] add to shelf  [f] search  [Esc] back  [q] quit"
    );
}

/// Search results with `albums` albums, the last one selected.
fn long_search(albums: usize, now: Instant) -> State {
    let mut state = State::new(Vec::new());
    state.update(Event::Key(Key::Ctrl('f')), now);
    state.update(Event::Key(Key::Char('f')), now);
    state.update(Event::Tick, now + Duration::from_millis(300));
    state.update(
        Event::Searched {
            query: "f".to_owned(),
            result: Ok(SearchResults {
                artists: vec![Artist {
                    id: "id-foo-fighters".to_owned(),
                    name: "Foo Fighters".to_owned(),
                }],
                albums: (1..=albums)
                    .map(|i| album(&i.to_string(), &format!("Album {i}"), "Foo Fighters", None))
                    .collect(),
                songs: vec![],
            }),
        },
        now,
    );
    state
}

#[test]
fn long_list_scrolls_and_leaves_a_blank_row_above_help() {
    let now = Instant::now();
    let mut state = long_search(20, now);
    state.update(Event::Key(Key::Down), now);
    // The help row is at the bottom with an empty row above it, even though there are
    // more results than fit.
    let buffer = render(&state, now, 60, 17);
    assert_eq!(row(&buffer, 13), "Album 4  Foo Fighters");
    assert_eq!(row(&buffer, 14), "");
    assert_eq!(row(&buffer, 15), "─".repeat(60));
    assert!(row(&buffer, 16).starts_with("[e] add to queue"));

    // Selection to the end of the list: the list scrolls, the help row and gap stay.
    for _ in 0..30 {
        state.update(Event::Key(Key::Down), now);
    }
    let buffer = render(&state, now, 60, 17);
    assert_eq!(row(&buffer, 13), "Album 20  Foo Fighters");
    assert!(is_selected(&buffer, 0, 13));
    assert_eq!(row(&buffer, 14), "");
    assert!(row(&buffer, 16).starts_with("[e] add to queue"));
}

#[test]
fn blank_row_and_rule_above_help_in_every_view() {
    let now = Instant::now();
    let many: Vec<Album> = (1..=20)
        .map(|i| {
            album(
                &i.to_string(),
                &format!("Album {i}"),
                &format!("Artist {i:02}"),
                Some(2000),
            )
        })
        .collect();
    // Shelf.
    let mut state = State::new(many.clone());
    let buffer = render(&state, now, 60, 13);
    assert_eq!(row(&buffer, 9), "Artist 03");
    assert_eq!(row(&buffer, 10), "");
    assert_rule(&buffer, 11);
    assert!(row(&buffer, 12).starts_with("[Space] pause"));

    // Track list.
    state.update(Event::Key(Key::Down), now);
    state.update(Event::Key(Key::Down), now);
    state.update(Event::Key(Key::Enter), now);
    state.update(Event::Key(Key::Enter), now);
    state.update(
        Event::AlbumTracks {
            uri: many[0].uri.clone(),
            result: Ok((1..=20)
                .map(|i| Track {
                    uri: format!("spotify:track:{i}"),
                    name: format!("Track {i}"),
                    artist: "Nirvana".to_owned(),
                    duration: Duration::from_secs(200),
                })
                .collect()),
        },
        now,
    );
    let buffer = render(&state, now, 60, 13);
    assert!(row(&buffer, 9).starts_with("   3  Track 3"));
    assert_eq!(row(&buffer, 10), "");
    assert_rule(&buffer, 11);
    assert!(row(&buffer, 12).starts_with("[Enter] play from here"));
}

/// Row `y` is a dim full-width rule without the progress bar's ●.
fn assert_rule(buffer: &Buffer, y: u16) {
    assert_eq!(row(buffer, y), "─".repeat(usize::from(buffer.area.width)));
    for x in 0..buffer.area.width {
        assert_eq!(buffer[(x, y)].fg, RULE, "x {x}");
    }
}

#[test]
fn search_without_results_shows_hint() {
    let now = Instant::now();
    let mut state = State::new(Vec::new());
    state.update(Event::Key(Key::Ctrl('f')), now);
    state.update(Event::Key(Key::Char('x')), now);
    assert_eq!(
        row(&render(&state, now, 60, 10), 4),
        "Search: x  searching…"
    );

    let later = now + Duration::from_millis(300);
    state.update(Event::Tick, later);
    state.update(
        Event::Searched {
            query: "x".to_owned(),
            result: Ok(SearchResults::default()),
        },
        later,
    );
    let buffer = render(&state, later, 60, 10);
    assert_eq!(row(&buffer, 4), "Search: x");
    assert_eq!(row(&buffer, 6), "No results");
}
