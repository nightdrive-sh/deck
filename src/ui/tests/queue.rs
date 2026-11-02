//! Tests: the queue on the top row and the queue view.

use super::*;

#[test]
fn top_row_shows_notice_then_queue_length_before_stopped() {
    let now = Instant::now();
    let mut state = queued_state(now);
    let buffer = render(&state, now, 50, 6);
    assert_eq!(
        row(&buffer, 0),
        format!("DECK{}+ queued: Breed", " ".repeat(31))
    );
    // + in the accent colour, text dimmed and the name normal.
    assert_eq!(buffer[(35, 0)].fg, ACCENT);
    assert_eq!(buffer[(37, 0)].fg, MUTED);
    assert_eq!(buffer[(45, 0)].fg, Color::Reset);

    // After the confirmation, the queue length.
    let later = now + crate::app::NOTICE_TIME;
    let buffer = render(&state, later, 50, 6);
    assert_eq!(row(&buffer, 0), format!("DECK{}+2 queued", " ".repeat(37)));
    assert_eq!(buffer[(41, 0)].fg, ACCENT);
    assert_eq!(buffer[(42, 0)].fg, ACCENT);
    assert_eq!(buffer[(44, 0)].fg, MUTED);

    // When paused, ■ stopped stays at the right edge.
    state.update(Event::Player(player::Event::Paused(0)), later);
    assert_eq!(
        row(&render(&state, later, 50, 6), 0),
        format!("DECK{}+2 queued   ■ stopped", " ".repeat(25))
    );
    // A long name is truncated so that DECK and ■ stopped fit.
    state.update(
        Event::AlbumQueued {
            album: album("long", "A Very Long Album Name Indeed", "X", None),
            track: None,
            result: Ok(nevermind_tracks()),
        },
        later,
    );
    assert_eq!(
        row(&render(&state, later, 50, 6), 0),
        "DECK  + queued: A Very Long Album Nam…   ■ stopped"
    );
}

#[test]
fn empty_queue_shows_nothing_on_top_row() {
    let now = Instant::now();
    let mut state = State::new(shelf());
    playing(&mut state, now, 0);
    assert_eq!(row(&render(&state, now, 50, 6), 0), "DECK");
}

#[test]
fn queue_view_lists_tracks_with_artist_and_duration() {
    let now = Instant::now();
    let mut state = queued_state(now);
    state.update(Event::Key(Key::Ctrl('q')), now);
    state.update(Event::Key(Key::Down), now);
    let buffer = render(&state, now, 60, 12);

    assert_eq!(
        rows(&buffer)[4..],
        [
            "Queue",
            "2 tracks · 7:17",
            "",
            track_row("  1  In Bloom  Nirvana", "4:14", 60).as_str(),
            track_row("  2  Breed  Nirvana", "3:03", 60).as_str(),
            "",
            "─".repeat(60).as_str(),
            "[d] remove  [l] like playing  [Esc] back  [q] quit",
        ]
    );
    assert_eq!(buffer[(0, 4)].fg, Color::Reset);
    assert!(buffer[(0, 4)].modifier.contains(Modifier::BOLD));
    assert_eq!(buffer[(0, 5)].fg, MUTED);
    // Artist and duration dimmed, the selected row highlighted.
    assert_eq!(buffer[(15, 7)].fg, MUTED);
    assert_eq!(buffer[(59, 7)].fg, MUTED);
    assert!(is_selected(&buffer, 0, 8));
    assert!(!is_selected(&buffer, 0, 7));

    // In a wider window n, Space and ←→ fit as well.
    let buffer = render(&state, now, 120, 12);
    assert_eq!(
        row(&buffer, 11).trim_start(),
        "[d] remove  [n] now playing  [l] like playing  [Space] pause  [←→] track  [Esc] back  [q] quit"
    );
}

#[test]
fn queue_view_shortens_long_rows_keeping_duration() {
    let now = Instant::now();
    let mut state = queued_state(now);
    state.update(Event::Key(Key::Ctrl('q')), now);
    let buffer = render(&state, now, 24, 11);
    assert_eq!(row(&buffer, 7), "  1  In Bloom  Ni…  4:14");
    let buffer = render(&state, now, 16, 11);
    assert_eq!(row(&buffer, 7), "  1  In B…  4:14");
}

#[test]
fn empty_queue_view_shows_hint() {
    let now = Instant::now();
    let mut state = State::new(shelf());
    state.update(Event::Key(Key::Ctrl('q')), now);
    let buffer = render(&state, now, 70, 11);
    assert_eq!(row(&buffer, 4), "Queue");
    assert_eq!(row(&buffer, 5), "");
    assert_eq!(
        row(&buffer, 7),
        "The queue is empty – add tracks or albums with Ctrl+E"
    );
}
