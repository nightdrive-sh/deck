//! Tests: the track list.

use super::*;

#[test]
fn album_view_lists_numbered_tracks_and_marks_the_playing_one() {
    let now = Instant::now();
    let mut state = nevermind_state(now);
    state.update(
        Event::AlbumTracks {
            uri: nevermind().uri,
            result: Ok(nevermind_tracks()),
        },
        now,
    );
    nevermind_playing(&mut state, now, 1);
    for key in [Key::Down, Key::Down] {
        state.update(Event::Key(key), now);
    }
    let buffer = render(&state, now, 50, 14);

    assert_eq!(
        rows(&buffer)[3..],
        [
            "",
            "Nevermind",
            "Nirvana · 1991 ●",
            "",
            track_row("  1  Smells Like Teen Spirit", "5:01", 50).as_str(),
            track_row("♪ 2  In Bloom", "4:14", 50).as_str(),
            track_row("  3  Come As You Are", "3:38", 50).as_str(),
            track_row("  4  Breed", "3:03", 50).as_str(),
            "",
            "─".repeat(50).as_str(),
            "[Enter] play from here  [e] add to queue  [a] remo",
        ]
    );
    // Ctrl+A on an album on the shelf removes it from the shelf.
    let help = row(&render(&state, now, 100, 14), 13);
    assert!(help.contains("[a] remove from shelf"), "{help}");
    assert!(row(&buffer, 1).starts_with("Nirvana / Nevermind / In Bloom  "));
    assert!(row(&buffer, 1).ends_with("00:00"));
    // Heading: album bold, artist and year dimmed, shelf ● green.
    assert!(buffer[(0, 4)].modifier.contains(Modifier::BOLD));
    assert_eq!(buffer[(0, 5)].fg, MUTED);
    assert_eq!(buffer[(15, 5)].fg, Color::Green);
    // The playing track's ♪ in the accent colour, number and duration dimmed, name
    // normal.
    assert_eq!(buffer[(0, 8)].fg, ACCENT);
    assert_eq!(buffer[(2, 8)].fg, MUTED);
    assert_eq!(buffer[(5, 8)].fg, Color::Reset);
    assert_eq!(buffer[(49, 8)].fg, MUTED);
    // The selected track (3rd) highlighted across the full width, dim parts lighter.
    assert!(is_selected(&buffer, 0, 9));
    assert!(is_selected(&buffer, 49, 9));
    assert_eq!(buffer[(2, 9)].fg, SELECTED_MUTED);
    assert_eq!(buffer[(49, 9)].fg, SELECTED_MUTED);
    assert!(!is_selected(&buffer, 0, 8));

    // The track changes: the mark moves.
    nevermind_playing(&mut state, now, 2);
    let buffer = render(&state, now, 50, 13);
    assert!(row(&buffer, 8).starts_with("  2  In Bloom"));
    assert!(row(&buffer, 9).starts_with("♪ 3  Come As You Are"));
    assert_eq!(buffer[(0, 9)].fg, ACCENT);
}

#[test]
fn album_view_shows_loading_until_tracks_arrive() {
    let now = Instant::now();
    let state = nevermind_state(now);
    let buffer = render(&state, now, 50, 13);
    assert_eq!(row(&buffer, 4), "Nevermind");
    assert_eq!(row(&buffer, 7), "Loading tracks…");
}

#[test]
fn long_track_name_is_shortened_so_duration_stays_visible() {
    let now = Instant::now();
    let mut state = nevermind_state(now);
    let long = Track {
        uri: "spotify:track:long".to_owned(),
        artist: "Nirvana".to_owned(),
        name: "Endless, Nameless (Hidden Track Included After Silence)".to_owned(),
        duration: Duration::from_secs(403),
    };
    state.update(
        Event::AlbumTracks {
            uri: nevermind().uri,
            result: Ok(vec![long]),
        },
        now,
    );
    let buffer = render(&state, now, 40, 11);
    assert_eq!(row(&buffer, 7), "  1  Endless, Nameless (Hidden Tr…  6:43");
}

#[test]
fn album_view_offers_ctrl_a_for_album_not_on_shelf() {
    let now = Instant::now();
    let mut state = State::new(Vec::new());
    state.update(Event::Key(Key::Ctrl('f')), now);
    state.update(Event::Key(Key::Char('n')), now);
    let later = now + Duration::from_millis(300);
    state.update(Event::Tick, later);
    state.update(
        Event::Searched {
            query: "n".to_owned(),
            result: Ok(SearchResults {
                artists: vec![],
                albums: vec![nevermind()],
                songs: vec![],
            }),
        },
        later,
    );
    for key in [Key::Down, Key::Enter] {
        state.update(Event::Key(key), later);
    }
    let buffer = render(&state, later, 80, 10);
    // Not on the shelf: no ● in the heading.
    assert_eq!(row(&buffer, 5), "Nirvana · 1991");
    assert!(
        row(&buffer, 9).starts_with("[Enter] play from here  [e] add to queue  [a] add to shelf")
    );
}

/// Curated › No Other open, with Nevermind's four tracks as its tracks.
fn curated_album_view(now: Instant) -> State {
    let mut state = curated_view(0, now);
    state.update(Event::Key(Key::Enter), now);
    state.update(
        Event::AlbumTracks {
            uri: "spotify:album:noother".to_owned(),
            result: Ok(nevermind_tracks()),
        },
        now,
    );
    state
}

#[test]
fn album_view_shows_curated_reason_below_title() {
    let now = Instant::now();
    let state = curated_album_view(now);
    let buffer = render(&state, now, 60, 18);
    assert_eq!(
        rows(&buffer)[3..],
        [
            "",
            "No Other",
            "Gene Clark · 1974",
            "",
            "Cosmic country-rock that Teenage Fanclub and The Posies grew",
            "up on, overlooked when it came out, and a record you return",
            "to for decades.",
            "",
            track_row("  1  Smells Like Teen Spirit", "5:01", 60).as_str(),
            track_row("  2  In Bloom", "4:14", 60).as_str(),
            track_row("  3  Come As You Are", "3:38", 60).as_str(),
            track_row("  4  Breed", "3:03", 60).as_str(),
            "",
            "─".repeat(60).as_str(),
            "[Enter] play from here  [e] add to queue  [a] add to shelf",
        ]
    );
    // Reason dimmed, the selected track highlighted below it.
    assert_eq!(buffer[(0, 7)].fg, MUTED);
    assert_eq!(buffer[(0, 9)].fg, MUTED);
    assert!(is_selected(&buffer, 0, 11));

    // When narrower, a longer reason is truncated on the third line.
    let buffer = render(&state, now, 40, 18);
    assert_eq!(
        rows(&buffer)[7..11],
        [
            "Cosmic country-rock that Teenage Fanclub",
            "and The Posies grew up on, overlooked",
            "when it came out, and a record you…",
            "",
        ]
    );
}

#[test]
fn album_view_scrolls_tracks_keeping_curated_reason_in_place() {
    let now = Instant::now();
    let mut state = curated_album_view(now);
    for _ in 0..3 {
        state.update(Event::Key(Key::Down), now);
    }
    // Two rows for the tracks: the list scrolls, the reason stays under the heading.
    let buffer = render(&state, now, 60, 16);
    assert!(row(&buffer, 7).starts_with("Cosmic country-rock"));
    assert_eq!(row(&buffer, 10), "");
    assert!(row(&buffer, 11).starts_with("  3  Come As You Are"));
    assert!(row(&buffer, 12).starts_with("  4  Breed"));
    assert!(is_selected(&buffer, 0, 12));
    assert_eq!(row(&buffer, 13), "");
}

#[test]
fn album_view_shows_curated_reason_wherever_album_was_opened() {
    let now = Instant::now();
    let search = |name: &str, found: Album| {
        let mut state = State::new(Vec::new()).with_curated(Ok(Some(curated_list())));
        state.update(Event::Key(Key::Ctrl('f')), now);
        state.update(Event::Key(Key::Char('n')), now);
        let later = now + Duration::from_millis(300);
        state.update(Event::Tick, later);
        state.update(
            Event::Searched {
                query: "n".to_owned(),
                result: Ok(SearchResults {
                    artists: vec![],
                    albums: vec![found],
                    songs: vec![],
                }),
            },
            later,
        );
        for key in [Key::Down, Key::Enter] {
            state.update(Event::Key(key), later);
        }
        let buffer = render(&state, later, 60, 14);
        assert_eq!(row(&buffer, 4), name);
        buffer
    };
    // A Curated album opened via search: the reason under the heading.
    let buffer = search(
        "#1 Record",
        album("number1", "#1 Record", "Big Star", Some(1972)),
    );
    assert_eq!(row(&buffer, 6), "");
    assert_eq!(row(&buffer, 7), "Power pop at its source.");
    assert_eq!(row(&buffer, 8), "");
    assert_eq!(row(&buffer, 9), "Loading tracks…");
    // An album that is not on the list looks the same as before.
    let buffer = search("Nevermind", nevermind());
    assert_eq!(row(&buffer, 6), "");
    assert_eq!(row(&buffer, 7), "Loading tracks…");
}
