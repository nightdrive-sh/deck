//! Tests: the shelf, its list rows and the Curated list.

use super::*;

#[test]
fn shelf_view_lists_artists_with_header_and_help() {
    let now = Instant::now();
    let mut state = State::new(shelf());
    state.update(Event::Key(Key::Down), now);
    state.update(Event::Key(Key::Down), now);
    let buffer = render(&state, now, 60, 12);

    assert_eq!(
        rows(&buffer),
        [
            "DECK",
            "—",
            format!("●{}", "─".repeat(59)).as_str(),
            "",
            format!("✦ Curated{}not set up", " ".repeat(41)).as_str(),
            format!("≋ Genres{}26 genres", " ".repeat(43)).as_str(),
            "",
            "Foo Fighters",
            "Nirvana",
            "",
            "─".repeat(60).as_str(),
            "[Space] pause  [←→] track  [f] search  [u] show queue",
        ]
    );
    // DECK is in the accent colour and bold.
    assert_eq!(buffer[(0, 0)].fg, ACCENT);
    assert!(buffer[(3, 0)].modifier.contains(Modifier::BOLD));
    // The selected row is highlighted across the full width with light text, the
    // others are not.
    assert!(is_selected(&buffer, 0, 7));
    assert!(is_selected(&buffer, 59, 7));
    assert_eq!(buffer[(0, 7)].fg, SELECTED_FG);
    // Curated is not set up yet: "not set up" is dim. Genres is dim.
    assert_eq!(buffer[(2, 4)].fg, Color::Reset);
    assert_eq!(buffer[(55, 4)].fg, MUTED);
    assert_eq!(buffer[(2, 5)].fg, MUTED);
    assert_eq!(buffer[(59, 4)].fg, MUTED);
    assert!(!is_selected(&buffer, 0, 4));
    assert_eq!(buffer[(0, 4)].bg, Color::Reset);
    // When nothing is playing, row 2 and the bar are dim and ● is at the start.
    assert_eq!(buffer[(0, 1)].fg, MUTED);
    assert_eq!(buffer[(0, 2)].fg, ACCENT);
    assert_eq!(buffer[(1, 2)].fg, RULE);
    assert_eq!(buffer[(59, 2)].fg, RULE);
    // A dim full-width rule above the help row.
    assert_eq!(buffer[(0, 10)].fg, RULE);
    assert_eq!(buffer[(59, 10)].fg, RULE);
    // Help row: [key] in the accent colour and bold without a background, label dim.
    for x in [0, 1, 6] {
        assert_eq!(buffer[(x, 11)].fg, ACCENT, "x {x}");
        assert!(buffer[(x, 11)].modifier.contains(Modifier::BOLD), "x {x}");
        assert_eq!(buffer[(x, 11)].bg, Color::Reset, "x {x}");
    }
    assert_eq!(buffer[(8, 11)].fg, MUTED);
    assert!(!buffer[(8, 11)].modifier.contains(Modifier::BOLD));
}

#[test]
fn shelf_shows_list_row_above_artists() {
    let now = Instant::now();
    let state = State::new(shelf()).with_curated(Ok(Some(curated_list())));
    let buffer = render(&state, now, 60, 12);
    assert_eq!(
        rows(&buffer)[3..],
        [
            "",
            format!("✦ Curated{}3 albums · week 41", " ".repeat(33)).as_str(),
            format!("≋ Genres{}26 genres", " ".repeat(43)).as_str(),
            "",
            "Foo Fighters",
            "Nirvana",
            "",
            "─".repeat(60).as_str(),
            "[Space] pause  [←→] track  [f] search  [u] show queue",
        ]
    );
    // The list row is selected across the full width, count and week dimmed (lighter
    // on the selected row). The empty row and the artists are not selected.
    assert!(is_selected(&buffer, 0, 4));
    assert!(is_selected(&buffer, 59, 4));
    assert_eq!(buffer[(0, 4)].fg, SELECTED_FG);
    assert_eq!(buffer[(42, 4)].fg, SELECTED_MUTED);
    assert!(!is_selected(&buffer, 0, 5));
    assert!(!is_selected(&buffer, 0, 6));
    assert!(!is_selected(&buffer, 0, 7));

    // Selection on an artist: the list row's count is dim.
    let mut state = state;
    state.update(Event::Key(Key::Down), now);
    state.update(Event::Key(Key::Down), now);
    let buffer = render(&state, now, 60, 12);
    assert!(is_selected(&buffer, 0, 7));
    assert!(!is_selected(&buffer, 0, 4));
    assert_eq!(buffer[(42, 4)].fg, MUTED);
}

#[test]
fn shelf_without_list_shows_curated_row() {
    let now = Instant::now();
    let none = render(&State::new(shelf()), now, 60, 10);
    assert_eq!(
        row(&none, 4),
        format!("✦ Curated{}not set up", " ".repeat(41))
    );
    // An emptied list keeps its subtitle.
    let empty = Curated {
        round: "2026-W41".to_owned(),
        created: String::new(),
        albums: Vec::new(),
    }
    .to_list(TEST_NOW);
    let state = State::new(shelf()).with_curated(Ok(Some(empty)));
    let buffer = render(&state, now, 60, 10);
    assert_eq!(
        row(&buffer, 4),
        format!("✦ Curated{}0 albums · week 41", " ".repeat(33))
    );
    assert_eq!(rows(&buffer)[5..], rows(&none)[5..]);

    // Broken file: an error on the top row and the row says so in the warning colour.
    let state = State::new(shelf()).with_curated(Err(
        "curated list file ~/.config/deck/curated.json is broken".into(),
    ));
    let buffer = render(&state, now, 60, 10);
    assert_eq!(
        row(&buffer, 1),
        "curated list file ~/.config/deck/curated.json is broken"
    );
    assert_eq!(buffer[(0, 1)].fg, Color::Red);
    assert_eq!(
        row(&buffer, 4),
        format!("✦ Curated{}file broken", " ".repeat(40))
    );
    assert_eq!(buffer[(55, 4)].fg, WARNING);
    assert_eq!(rows(&buffer)[5..], rows(&none)[5..]);
}

#[test]
fn curated_view_without_list_shows_setup_instructions() {
    let now = Instant::now();
    let mut state = State::new(shelf());
    state.update(Event::Key(Key::Enter), now);
    let guide = rows(&render(&state, now, 70, 24));
    assert_eq!(guide[4], "Curated");
    assert!(
        guide[7].starts_with("Curated is a weekly list"),
        "{guide:?}"
    );
    assert!(
        guide.iter().any(|row| row.contains("deck curate")),
        "{guide:?}"
    );

    // Broken file: no instructions, the error explains.
    let mut state = State::new(shelf()).with_curated(Err("broken".into()));
    state.update(Event::Key(Key::Enter), now);
    let buffer = render(&state, now, 70, 24);
    assert_eq!(
        row(&buffer, 7),
        "The list file cannot be read – see the error above."
    );
}

#[test]
fn empty_shelf_shows_hint_below_list_row() {
    let now = Instant::now();
    let state = State::new(Vec::new()).with_curated(Ok(Some(curated_list())));
    let buffer = render(&state, now, 60, 11);
    assert!(row(&buffer, 4).starts_with("✦ Curated"));
    assert!(row(&buffer, 5).starts_with("≋ Genres"));
    assert_eq!(row(&buffer, 6), "");
    assert_eq!(
        row(&buffer, 7),
        "The shelf is empty – search for albums with Ctrl+F"
    );
    assert!(is_selected(&buffer, 0, 4));
}

#[test]
fn stale_list_shows_its_age_in_warning_color() {
    let now = Instant::now();
    let mut list = curated_list();
    list.subtitle = "3 albums · week 41 · 9 days old".to_owned();
    list.stale = true;
    let mut state = State::new(shelf()).with_curated(Ok(Some(list)));
    state.update(Event::Key(Key::Down), now);

    // The shelf's list row: the age on the right edge in the warning colour.
    let buffer = render(&state, now, 60, 11);
    assert_eq!(
        row(&buffer, 4),
        format!("✦ Curated{}3 albums · week 41 · 9 days old", " ".repeat(20))
    );
    assert_eq!(buffer[(28, 4)].fg, Color::Reset);
    assert_eq!(buffer[(29, 4)].fg, WARNING);
    assert_eq!(buffer[(59, 4)].fg, WARNING);

    // List view: the subtitle in the warning colour, followed by a hint about the log.
    state.update(Event::Key(Key::Up), now);
    state.update(Event::Key(Key::Enter), now);
    let buffer = render(&state, now, 80, 16);
    assert_eq!(
        row(&buffer, 5),
        "3 albums · week 41 · 9 days old – see ~/.cache/deck/curate.log"
    );
    assert_eq!(buffer[(0, 5)].fg, WARNING);
    assert_eq!(buffer[(40, 5)].fg, MUTED);
}

#[test]
fn curated_view_lists_albums_with_reason_below() {
    let now = Instant::now();
    let state = curated_view(0, now);
    let buffer = render(&state, now, 60, 17);
    assert_eq!(
        rows(&buffer)[3..],
        [
            "",
            "Curated",
            "3 albums · week 41",
            "",
            "Gene Clark · No Other  1974",
            "Big Star · #1 Record  1972",
            "Foo Fighters · The Colour And The Shape  1997 ●",
            "",
            "",
            "Cosmic country-rock that Teenage Fanclub and The Posies grew",
            "up on, overlooked when it came out, and a record you return…",
            "",
            "─".repeat(60).as_str(),
            "[e] add to queue  [a] add to shelf  [d] not for me  [Esc] ba",
        ]
    );
    // Heading bold, subtitle dimmed.
    assert!(buffer[(0, 4)].modifier.contains(Modifier::BOLD));
    assert_eq!(buffer[(0, 5)].fg, MUTED);
    // Selected album highlighted; year dim, the ● of an album on the shelf green.
    assert!(is_selected(&buffer, 0, 7));
    assert!(!is_selected(&buffer, 0, 8));
    assert_eq!(buffer[(24, 7)].fg, SELECTED_MUTED);
    assert_eq!(buffer[(22, 8)].fg, MUTED);
    assert_eq!(buffer[(46, 9)].fg, Color::Green);
    // Reason dimmed.
    assert_eq!(buffer[(0, 12)].fg, MUTED);
    assert_eq!(buffer[(0, 13)].fg, MUTED);

    // The reason follows the selection. Ctrl+A on an album on the shelf removes it.
    let state = curated_view(2, now);
    let buffer = render(&state, now, 100, 17);
    assert_eq!(row(&buffer, 12), "Already on your shelf.");
    assert_eq!(row(&buffer, 13), "");
    assert_eq!(
        row(&buffer, 16),
        "[e] add to queue  [a] remove from shelf  [d] not for me  [Esc] back"
    );
    let state = curated_view(1, now);
    assert_eq!(
        row(&render(&state, now, 100, 17), 16),
        "[e] add to queue  [a] add to shelf  [d] not for me  [Esc] back"
    );
}

#[test]
fn curated_view_scrolls_keeping_reason_in_place() {
    let now = Instant::now();
    let state = curated_view(2, now);
    // Two rows for the list: the list scrolls, the reason and the help row stay.
    let buffer = render(&state, now, 60, 15);
    assert_eq!(row(&buffer, 7), "Big Star · #1 Record  1972");
    assert!(row(&buffer, 8).starts_with("Foo Fighters · The Colour"));
    assert!(is_selected(&buffer, 0, 8));
    assert_eq!(row(&buffer, 9), "");
    assert_eq!(row(&buffer, 10), "Already on your shelf.");
    assert!(row(&buffer, 14).starts_with("[e] add to queue"));
}

#[test]
fn long_curated_row_is_shortened_keeping_year() {
    let now = Instant::now();
    let state = curated_view(0, now);
    let buffer = render(&state, now, 30, 16);
    assert_eq!(row(&buffer, 7), "Gene Clark · No Other  1974");
    assert_eq!(row(&buffer, 9), "Foo Fighters · The Co…  1997 ●");
    let buffer = render(&state, now, 20, 16);
    assert_eq!(row(&buffer, 9), "Foo F… · Th…  1997 ●");
}

#[test]
fn curated_view_without_albums_shows_hint() {
    let now = Instant::now();
    let mut state = curated_view(0, now);
    state.update(
        Event::CuratedChanged(Some(
            Curated {
                round: "2026-W41".to_owned(),
                created: String::new(),
                albums: Vec::new(),
            }
            .to_list(TEST_NOW),
        )),
        now,
    );
    let buffer = render(&state, now, 60, 16);
    assert_eq!(row(&buffer, 4), "Curated");
    assert_eq!(row(&buffer, 5), "");
    assert_eq!(
        row(&buffer, 7),
        "Nothing left – a new list arrives with the next weekly run"
    );
}

#[test]
fn wrap_breaks_words_and_marks_the_rest() {
    assert_eq!(wrap("one two three", 7, 2), ["one two", "three"]);
    assert_eq!(wrap("one two three four", 7, 2), ["one two", "three…"]);
    assert_eq!(wrap("one two three fours", 9, 1), ["one two…"]);
    assert_eq!(wrap("abcdefghij", 5, 2), ["abcd…"]);
    assert!(wrap("", 10, 2).is_empty());
}

#[test]
fn empty_shelf_shows_hint() {
    let now = Instant::now();
    let buffer = render(&State::new(Vec::new()), now, 60, 11);
    assert!(row(&buffer, 4).starts_with("✦ Curated"));
    assert!(row(&buffer, 5).starts_with("≋ Genres"));
    assert_eq!(
        row(&buffer, 7),
        "The shelf is empty – search for albums with Ctrl+F"
    );
}
