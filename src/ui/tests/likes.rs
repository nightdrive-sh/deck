//! Tests: likes.

use super::*;

#[test]
fn liked_tracks_have_heart_on_top_row_and_track_list() {
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
    liked(
        &mut state,
        now,
        &["spotify:track:In Bloom", "spotify:track:Breed"],
    );
    let buffer = render(&state, now, 50, 14);

    assert_eq!(
        row(&buffer, 1),
        format!("{:<45}00:00", "Nirvana / Nevermind / In Bloom ♥")
    );
    assert_eq!(buffer[(31, 1)].fg, ACCENT);
    assert_eq!(
        rows(&buffer)[7..11],
        [
            track_row("  1  Smells Like Teen Spirit", "5:01", 50).as_str(),
            track_row("♪ 2  In Bloom ♥", "4:14", 50).as_str(),
            track_row("  3  Come As You Are", "3:38", 50).as_str(),
            track_row("  4  Breed ♥", "3:03", 50).as_str(),
        ]
    );
    assert_eq!(buffer[(14, 8)].fg, ACCENT);
    // A long name is truncated so that ♥ and the duration fit.
    let buffer = render(&state, now, 21, 14);
    assert_eq!(row(&buffer, 8), "♪ 2  In Bloom ♥  4:14");
    let buffer = render(&state, now, 20, 14);
    assert_eq!(row(&buffer, 8), "♪ 2  In Blo… ♥  4:14");
}

#[test]
fn liked_radio_tracks_have_heart_after_name() {
    let now = Instant::now();
    let mut state = synthwave_radio(now);
    liked(
        &mut state,
        now,
        &["spotify:track:sunset", "spotify:track:ontherun"],
    );
    let buffer = render(&state, now, 70, 15);
    assert_eq!(
        rows(&buffer)[7..11],
        [
            "♪ The Midnight · Sunset ♥  Endless Summer",
            "  FM-84 · Running in the Night  Atlas",
            "  Timecop1983 · On the Run ♥  Night Drive ●",
            "  Gunship · Tech Noir  Gunship",
        ]
    );
    assert_eq!(buffer[(24, 7)].fg, ACCENT);
    assert!(row(&buffer, 1).starts_with("The Midnight / Endless Summer / Sunset ♥"));
}

#[test]
fn top_row_confirms_like_and_unlike() {
    let now = Instant::now();
    let mut state = State::new(shelf()).with_likes(true);
    playing(&mut state, now, 0);
    let actions = state.update(Event::Key(Key::Ctrl('l')), now);
    assert_eq!(
        actions,
        [Action::SetLiked {
            uri: "spotify:track:everlong".to_owned(),
            liked: true,
        }]
    );
    state.update(
        Event::LikeChanged {
            uri: "spotify:track:everlong".to_owned(),
            liked: true,
            result: Ok(()),
        },
        now,
    );
    let buffer = render(&state, now, 50, 6);
    assert_eq!(
        row(&buffer, 0),
        format!("DECK{}♥ liked: Everlong", " ".repeat(29))
    );
    // ♥ in the accent colour, text dimmed and the name normal.
    assert_eq!(buffer[(33, 0)].fg, ACCENT);
    assert_eq!(buffer[(35, 0)].fg, MUTED);
    assert_eq!(buffer[(42, 0)].fg, Color::Reset);

    state.update(Event::Key(Key::Ctrl('l')), now);
    state.update(
        Event::LikeChanged {
            uri: "spotify:track:everlong".to_owned(),
            liked: false,
            result: Ok(()),
        },
        now,
    );
    assert_eq!(
        row(&render(&state, now, 50, 6), 0),
        format!("DECK{}unliked: Everlong", " ".repeat(29))
    );
    assert_eq!(
        row(&render(&state, now + crate::app::NOTICE_TIME, 50, 6), 0),
        "DECK"
    );
}
