//! Tests: the top rows (now playing, progress bar, errors), the layout and the colours.

use super::*;

#[test]
fn now_playing_shows_elapsed_time_and_track() {
    let now = Instant::now();
    let mut state = State::new(shelf());
    playing(&mut state, now, 83_000);
    let buffer = render(&state, now, 70, 6);
    // Band / album / track on the left, time on the right edge.
    assert_eq!(
        row(&buffer, 1),
        format!(
            "{:<65}01:23",
            "Foo Fighters / The Colour And The Shape / Everlong"
        )
    );
    assert_eq!(buffer[(0, 1)].fg, Color::Reset);
    assert_eq!(buffer[(65, 1)].fg, Color::Reset);
    // The separators are dim and the track name bold.
    assert_eq!(buffer[(13, 1)].fg, MUTED);
    assert!(!buffer[(2, 1)].modifier.contains(Modifier::BOLD));
    assert!(buffer[(45, 1)].modifier.contains(Modifier::BOLD));

    // Time moves between ticks; when paused it stops.
    let later = now + Duration::from_secs(2);
    assert!(row(&render(&state, later, 70, 6), 1).ends_with("  01:25"));
    state.update(Event::Player(player::Event::Paused(85_000)), later);
    let buffer = render(&state, later + Duration::from_secs(5), 70, 6);
    assert!(row(&buffer, 1).starts_with("Foo Fighters / "));
    assert!(row(&buffer, 1).ends_with("  01:25"));

    // In a narrow window a long row is cut before the time.
    let buffer = render(&state, later, 30, 6);
    assert!(row(&buffer, 1).starts_with("Foo Fighters / The Colo"));
    assert!(row(&buffer, 1).ends_with("  01:25"));
}

#[test]
fn paused_shows_stopped_at_the_right_edge_of_the_top_row() {
    let now = Instant::now();
    let mut state = State::new(shelf());
    playing(&mut state, now, 83_000);
    assert_eq!(row(&render(&state, now, 70, 6), 0), "DECK");

    state.update(Event::Player(player::Event::Paused(83_000)), now);
    let buffer = render(&state, now, 70, 6);
    assert_eq!(row(&buffer, 0), format!("DECK{}■ stopped", " ".repeat(57)));
    // ■ in the accent colour, text dimmed.
    assert_eq!(buffer[(61, 0)].fg, ACCENT);
    assert_eq!(buffer[(63, 0)].fg, MUTED);
    assert_eq!(buffer[(69, 0)].fg, MUTED);
    // In a wide window the mark is at the right edge of Deck's area.
    let buffer = render(&state, now, 120, 6);
    assert!(row(&buffer, 0).ends_with("■ stopped"));
    assert_eq!(row(&buffer, 0).chars().count(), 110);

    // Playback resumes: the mark goes away.
    state.update(Event::Player(player::Event::Position(84_000)), now);
    assert_eq!(row(&render(&state, now, 70, 6), 0), "DECK");
}

#[test]
fn progress_bar_fills_the_deck_width() {
    let now = Instant::now();
    let mut state = State::new(shelf());
    // Halfway: 125 s / 250 s.
    playing(&mut state, now, 125_000);

    // A narrow window is used across its full width; in a wide one Deck is 100 columns
    // in the middle of the window.
    for (width, margin) in [(40u16, 0u16), (120, 10)] {
        let buffer = render(&state, now, width, 6);
        let bar = row(&buffer, 2);
        let deck_width = width - 2 * margin;
        assert_eq!(
            bar,
            format!("{}{}", " ".repeat(margin.into()), bar.trim_start()),
            "leveys {width}"
        );
        let bar = bar.trim_start();
        assert_eq!(
            bar.chars().count(),
            usize::from(deck_width),
            "leveys {width}"
        );
        let head = usize::from(deck_width - 1) / 2;
        assert_eq!(
            bar.chars().position(|c| c == '●'),
            Some(head),
            "leveys {width}"
        );
        assert!(bar.chars().all(|c| c == '─' || c == '●'));
        // The played part and ● are in the accent colour, the remaining part dim.
        let head = margin + head as u16;
        assert_eq!(buffer[(margin, 2)].fg, ACCENT);
        assert_eq!(buffer[(head, 2)].fg, ACCENT);
        assert_eq!(buffer[(head + 1, 2)].fg, RULE);
        assert_eq!(buffer[(margin + deck_width - 1, 2)].fg, RULE);
    }
}

#[test]
fn large_window_centres_a_deck_of_at_most_30_rows() {
    let now = Instant::now();
    let buffer = render(&State::new(shelf()), now, 120, 50);
    let rows = rows(&buffer);

    // 30 rows in the middle of a 50-row window: rows 10–39, empty above and below.
    let top = 10;
    let bottom = top + MAX_HEIGHT as usize - 1;
    for (y, line) in rows.iter().enumerate() {
        if y < top || y > bottom {
            assert_eq!(line, "", "rivi {y}");
        }
    }
    // 100 columns in the middle of a 120-column window: 10 empty on each side.
    let left = 10;
    let margin = " ".repeat(left.into());
    assert_eq!(rows[top], format!("{margin}DECK"));
    assert_eq!(
        rows[top + 2],
        format!("{margin}●{}", "─".repeat(usize::from(MAX_WIDTH) - 1))
    );
    assert!(rows[top + 4].starts_with(&format!("{margin}✦ Curated")));
    assert!(rows[top + 5].starts_with(&format!("{margin}≋ Genres")));
    assert_eq!(rows[top + 7], format!("{margin}Foo Fighters"));
    assert!(rows[bottom].starts_with(&format!("{margin}[Space] pause")));
    // The selected row's highlight reaches the edges of Deck's area but not beyond.
    let right = left + MAX_WIDTH - 1;
    let y = top as u16 + 4;
    assert!(is_selected(&buffer, left, y));
    assert!(is_selected(&buffer, right, y));
    assert!(!is_selected(&buffer, left - 1, y));
    assert!(!is_selected(&buffer, right + 1, y));
}

#[test]
fn small_window_is_filled_by_the_deck() {
    let now = Instant::now();
    let buffer = render(&State::new(shelf()), now, 60, 20);
    let rows = rows(&buffer);
    assert_eq!(rows[0], "DECK");
    assert_eq!(rows[2], format!("●{}", "─".repeat(59)));
    assert!(rows[19].starts_with("[Space] pause"));

    // Low but wide window: the height is filled, the width is limited to the middle.
    let buffer = render(&State::new(shelf()), now, 108, 12);
    assert_eq!(row(&buffer, 0), "    DECK");
    assert!(row(&buffer, 11).starts_with("    [Space] pause"));
}

/// WCAG contrast ratio between two RGB colours.
fn contrast(a: Color, b: Color) -> f64 {
    fn luminance(color: Color) -> f64 {
        let Color::Rgb(r, g, b) = color else {
            panic!("{color:?} is not an RGB colour");
        };
        let channel = |c: u8| {
            let c = f64::from(c) / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
    }
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[test]
fn selected_row_text_is_readable_on_its_background() {
    // The selected row sets its own background, so it is legible with any theme.
    assert!(contrast(SELECTED_FG, SELECTED_BG) >= 7.0);
    assert!(contrast(SELECTED_MUTED, SELECTED_BG) >= 4.5);
}

#[test]
fn muted_text_is_readable_on_a_dark_theme() {
    // A fixed grey, not the theme's DarkGray: legible on a dark background, but clearly
    // dimmer than normal text and distinct from the selected row's dim background.
    let background = Color::Rgb(0x15, 0x15, 0x1C);
    let muted = contrast(MUTED, background);
    assert!(muted >= 4.5, "{muted}");
    assert!(contrast(SELECTED_FG, background) > 2.5 * muted);
    assert!(contrast(SELECTED_MUTED, background) > 1.4 * muted);
    // The rules stay darker than text.
    assert!(contrast(RULE, background) < muted);
}

#[test]
fn progress_bar_ends() {
    let now = Instant::now();
    let mut state = State::new(shelf());
    playing(&mut state, now, 0);
    assert!(row(&render(&state, now, 30, 6), 2).starts_with('●'));
    let end = now + Duration::from_secs(300);
    assert!(row(&render(&state, end, 30, 6), 2).ends_with('●'));
}

#[test]
fn error_is_shown_in_red_instead_of_now_playing() {
    let now = Instant::now();
    let mut state = State::new(shelf());
    playing(&mut state, now, 1_000);
    state.update(
        Event::Player(player::Event::Error(
            "connection to Spotify lost".to_owned(),
        )),
        now,
    );
    let buffer = render(&state, now, 60, 6);
    assert_eq!(row(&buffer, 1), "connection to Spotify lost");
    assert_eq!(buffer[(0, 1)].fg, Color::Red);
    // The bar still shows the playing track.
    assert!(row(&buffer, 2).contains('●'));
}
