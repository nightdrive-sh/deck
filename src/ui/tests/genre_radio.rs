//! Tests: genres and radio.

use super::*;
use crate::ui::list::first_visible;

#[test]
fn shelf_shows_favorite_genres_and_genres_row() {
    let now = Instant::now();
    let mut state = synthwave_radio(now);
    state.update(Event::Key(Key::Esc), now);
    let buffer = render(&state, now, 70, 13);
    assert_eq!(
        rows(&buffer)[3..],
        [
            "",
            format!("✦ Curated{}not set up", " ".repeat(51)).as_str(),
            format!("≋ lofi{}radio", " ".repeat(59)).as_str(),
            format!("≋ synthwave{}♪ playing", " ".repeat(50)).as_str(),
            format!("≋ Genres{}26 genres", " ".repeat(53)).as_str(),
            "",
            "Timecop1983",
            "",
            "─".repeat(70).as_str(),
            "[d] remove from home  [l] like playing  [f] search  [u] show queue",
        ]
    );
    // Genre name normal, radio dimmed. On the playing row ♪ in the accent colour.
    assert_eq!(buffer[(2, 5)].fg, Color::Reset);
    assert_eq!(buffer[(65, 5)].fg, MUTED);
    assert!(is_selected(&buffer, 0, 6));
    assert_eq!(buffer[(61, 6)].fg, ACCENT);
    assert_eq!(buffer[(63, 6)].fg, SELECTED_MUTED);
    // Genres dimmed.
    assert_eq!(buffer[(2, 7)].fg, MUTED);
    assert_eq!(buffer[(61, 7)].fg, MUTED);
}

#[test]
fn genres_view_lists_catalog_with_stars() {
    let now = Instant::now();
    let mut state = State::new(Vec::new()).with_favorites(vec!["lofi".into(), "synthwave".into()]);
    for key in [Key::Down, Key::Down, Key::Down, Key::Enter] {
        state.update(Event::Key(key), now);
    }
    let buffer = render(&state, now, 70, 30);
    let rows = rows(&buffer);
    assert_eq!(
        rows[4..10],
        [
            "Genres",
            "26 genres · 2 on home",
            "",
            "60s rock",
            "70s rock",
            "80s rock"
        ]
    );
    assert_eq!(buffer[(0, 5)].fg, MUTED);
    let lofi = 7 + genres::all().iter().position(|g| g.id == "lofi").unwrap();
    assert_eq!(rows[lofi], "lofi ★");
    assert_eq!(buffer[(5, lofi as u16)].fg, STAR);
    assert_eq!(rows[lofi + 1], "metal");
    assert_eq!(rows[29], "[Enter] play radio  [a] add to home  [Esc] back");

    // On a favourite, Ctrl+A removes it from home.
    for _ in 7..lofi {
        state.update(Event::Key(Key::Down), now);
    }
    let buffer = render(&state, now, 70, 30);
    assert!(is_selected(&buffer, 0, lofi as u16));
    assert_eq!(
        row(&buffer, 29),
        "[Enter] play radio  [a] remove from home  [Esc] back"
    );
}

/// The list scrolls only when the selection goes past the visible area: going back
/// up from the end, the list stays put until the selection is on the top row.
#[test]
fn list_scrolls_only_past_the_visible_edge() {
    let now = Instant::now();
    let mut state = State::new(Vec::new());
    state.update(Event::Key(Key::Down), now);
    state.update(Event::Key(Key::Enter), now);
    let names: Vec<&str> = genres::all().iter().map(|g| g.name.as_str()).collect();
    // Rows 7–14 are left for the list.
    let (top, bottom) = (7, 14);
    let press = |state: &mut State, key: Key, times: usize| {
        for _ in 0..times {
            state.update(Event::Key(key), now);
            render(state, now, 70, 18);
        }
        render(state, now, 70, 18)
    };

    // To the end: the last genre on the bottom row.
    let last = names.len() - 1;
    let buffer = press(&mut state, Key::Down, last);
    let first = last - (bottom - top);
    assert_eq!(row(&buffer, top as u16), names[first]);
    assert_eq!(row(&buffer, bottom as u16), names[last]);
    assert!(is_selected(&buffer, 0, bottom as u16));

    // ↑ moves the selection up, the list stays put until the top row.
    for up in 1..=bottom - top {
        let buffer = press(&mut state, Key::Up, 1);
        assert_eq!(row(&buffer, top as u16), names[first]);
        assert!(is_selected(&buffer, 0, (bottom - up) as u16));
    }
    // Only the next ↑ scrolls the list.
    let buffer = press(&mut state, Key::Up, 1);
    assert_eq!(row(&buffer, top as u16), names[first - 1]);
    assert!(is_selected(&buffer, 0, top as u16));

    // ↓ does not scroll before the bottom row.
    let buffer = press(&mut state, Key::Down, 1);
    assert_eq!(row(&buffer, top as u16), names[first - 1]);
    assert!(is_selected(&buffer, 0, top as u16 + 1));
}

/// `first_visible` scrolls like ratatui's `List` when given all the rows.
#[test]
fn first_visible_matches_ratatui_list() {
    use ratatui::widgets::{ListState, StatefulWidget};
    for len in 1..8 {
        for height in 1..6u16 {
            for offset in 0..10 {
                for selected in [None, Some(0), Some(2), Some(4), Some(7), Some(9)] {
                    let area = Rect::new(0, 0, 10, height);
                    let mut buffer = Buffer::empty(area);
                    let mut list_state = ListState::default()
                        .with_offset(offset)
                        .with_selected(selected);
                    let items = (0..len).map(|i| ListItem::new(i.to_string()));
                    StatefulWidget::render(List::new(items), area, &mut buffer, &mut list_state);
                    let selected = selected.map(|s| s.min(len - 1));
                    assert_eq!(
                        first_visible(len, usize::from(height), selected, offset),
                        list_state.offset(),
                        "len {len}, height {height}, offset {offset}, selected {selected:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn radio_view_lists_tracks_with_playing_and_shelf_marks() {
    let now = Instant::now();
    let mut state = synthwave_radio(now);
    let buffer = render(&state, now, 70, 15);
    assert_eq!(
        rows(&buffer)[4..13],
        [
            "synthwave",
            "radio",
            "",
            "♪ The Midnight · Sunset  Endless Summer",
            "  FM-84 · Running in the Night  Atlas",
            "  Timecop1983 · On the Run  Night Drive ●",
            "  Gunship · Tech Noir  Gunship",
            "",
            "",
        ]
    );
    assert!(buffer[(0, 4)].modifier.contains(Modifier::BOLD));
    assert_eq!(buffer[(0, 5)].fg, MUTED);
    assert_eq!(buffer[(0, 7)].fg, ACCENT);
    assert!(is_selected(&buffer, 0, 7));
    // Album dimmed, shelf ● green.
    assert_eq!(buffer[(32, 8)].fg, MUTED);
    assert_eq!(buffer[(40, 9)].fg, Color::Green);

    // The help row fits in 100 columns even with the label "remove from shelf".
    let buffer = render(&state, now, 100, 15);
    assert_eq!(
        row(&buffer, 14),
        "[Enter] play  [e] add to queue  [a] add to shelf  [r] new radio  [l] like playing  [Esc] back"
    );
    state.update(Event::Key(Key::Down), now);
    state.update(Event::Key(Key::Down), now);
    let buffer = render(&state, now, 100, 15);
    assert_eq!(
        row(&buffer, 14),
        "[Enter] play  [e] add to queue  [a] remove from shelf  [r] new radio  [l] like playing  [Esc] back"
    );
}

#[test]
fn long_radio_row_is_shortened_album_first() {
    let now = Instant::now();
    let state = synthwave_radio(now);
    let buffer = render(&state, now, 36, 14);
    assert_eq!(row(&buffer, 8), "  FM-84 · Running in the Night  Atl…");
    assert_eq!(row(&buffer, 9), "  Timecop1983 · On the Run  Night… ●");
    // The album is not shown as just "A…".
    let buffer = render(&state, now, 34, 14);
    assert_eq!(row(&buffer, 8), "  FM-84 · Running in the Night");
    let buffer = render(&state, now, 24, 14);
    assert_eq!(row(&buffer, 8), "  FM-84 · Running in th…");
    assert_eq!(row(&buffer, 9), "  Timecop1983 · On th… ●");
}

#[test]
fn stopped_radio_view_shows_hint() {
    let now = Instant::now();
    let mut state = synthwave_radio(now);
    state.update(Event::Player(player::Event::Stopped), now);
    let buffer = render(&state, now, 70, 14);
    assert_eq!(row(&buffer, 4), "synthwave");
    assert_eq!(
        row(&buffer, 7),
        "The radio has stopped – Ctrl+R starts it again"
    );
}

#[test]
fn queued_radio_track_shows_no_duration() {
    let now = Instant::now();
    let mut state = synthwave_radio(now);
    for key in [Key::Down, Key::Ctrl('e'), Key::Ctrl('q')] {
        state.update(Event::Key(key), now);
    }
    let buffer = render(&state, now, 50, 11);
    assert_eq!(row(&buffer, 5), "1 track · 0:00");
    assert_eq!(row(&buffer, 7), "  1  Running in the Night  FM-84");
}
