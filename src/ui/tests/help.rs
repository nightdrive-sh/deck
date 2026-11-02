//! Tests: the key help row.

use super::*;

/// The help row (bottom row) in a 100-column Deck.
fn help_row(state: &State, now: Instant) -> String {
    row(&render(state, now, 100, 20), 19)
}

/// Everlong is playing and is liked.
fn liked_playing(state: &mut State, now: Instant) {
    playing(state, now, 0);
    liked(state, now, &["spotify:track:everlong"]);
}

/// Everlong is playing from the album The Colour And The Shape (restored playback
/// state), so n leads to the album.
fn playing_album(state: &mut State, now: Instant) {
    let saved = serde_json::from_value(serde_json::json!({
        "album": { "uri": colour().uri, "len": 13, "index": 3, "album": colour() },
        "current": "album",
        "now_playing": {
            "uri": "spotify:track:everlong",
            "artist": "Foo Fighters",
            "album": "The Colour And The Shape",
            "track": "Everlong",
            "duration_ms": 250_000,
        },
        "position_ms": 0,
    }))
    .unwrap();
    state.update(Event::Restore(Box::new(saved)), now);
}

fn liked_playing_album(state: &mut State, now: Instant) {
    playing_album(state, now);
    liked(state, now, &["spotify:track:everlong"]);
}

#[test]
fn help_shows_now_playing_only_when_n_leads_somewhere() {
    let now = Instant::now();
    let mut state = State::new(shelf());
    // The player reported the track, but Deck does not know the album: n leads nowhere.
    playing(&mut state, now, 0);
    assert!(!help_row(&state, now).contains("[n]"));

    playing_album(&mut state, now);
    assert_eq!(
        help_row(&state, now),
        "[Space] pause  [←→] track  [n] now playing  [l] like playing  [f] search  [u] show queue  [q] quit"
    );
    // In a narrower window quit goes first, then [n] now playing.
    assert_eq!(
        row(&render(&state, now, 92, 20), 19),
        "[Space] pause  [←→] track  [n] now playing  [l] like playing  [f] search  [u] show queue"
    );
    assert_eq!(
        row(&render(&state, now, 89, 20), 19),
        "[Space] pause  [←→] track  [l] like playing  [f] search  [u] show queue  [q] quit"
    );
    // In search, Ctrl+N.
    state.update(Event::Key(Key::Ctrl('f')), now);
    state.update(Event::Key(Key::Esc), now);
    state.update(Event::Key(Key::Ctrl('q')), now);
    assert_eq!(
        help_row(&state, now),
        "[d] remove  [n] now playing  [l] like playing  [Space] pause  [←→] track  [Esc] back  [q] quit"
    );
}

#[test]
fn help_shows_like_or_unlike_only_while_something_plays() {
    let now = Instant::now();
    let mut state = nevermind_state(now);
    assert_eq!(
        help_row(&state, now),
        "[Enter] play from here  [e] add to queue  [a] remove from shelf  [Esc] back  [q] quit"
    );
    playing(&mut state, now, 0);
    assert_eq!(
        help_row(&state, now),
        "[Enter] play from here  [e] add to queue  [a] remove from shelf  [l] like playing  [Esc] back"
    );
    liked(&mut state, now, &["spotify:track:everlong"]);
    assert_eq!(
        help_row(&state, now),
        "[Enter] play from here  [e] add to queue  [a] remove from shelf  [l] unlike playing  [Esc] back"
    );
    // In the search field the Ctrl forms, because letters type into the search.
    state.update(Event::Key(Key::Ctrl('f')), now);
    assert_eq!(
        help_row(&state, now),
        "[↓] results  [Ctrl+L] unlike playing  [Esc] back  [Ctrl+C] quit"
    );
}

#[test]
fn shelf_help_shows_d_only_on_a_genre_row() {
    let now = Instant::now();
    let mut state = State::new(shelf()).with_favorites(vec!["lofi".into()]);
    // On the Curated row, d does nothing.
    assert!(!help_row(&state, now).contains("[d]"));
    state.update(Event::Key(Key::Down), now);
    assert_eq!(
        help_row(&state, now),
        "[Space] pause  [←→] track  [d] remove from home  [f] search  [u] show queue  [q] quit"
    );
    // On the Genres row and on an artist, d does nothing.
    state.update(Event::Key(Key::Down), now);
    assert!(!help_row(&state, now).contains("[d]"));
    state.update(Event::Key(Key::Down), now);
    assert!(!help_row(&state, now).contains("[d]"));
    // While something is playing, [n] does not disappear as the selection moves: on a
    // genre row Space and ←→ are left out.
    liked_playing_album(&mut state, now);
    assert_eq!(
        help_row(&state, now),
        "[Space] pause  [←→] track  [n] now playing  [l] unlike playing  [f] search  [u] show queue  [q] quit"
    );
    state.update(Event::Key(Key::Up), now);
    state.update(Event::Key(Key::Up), now);
    assert_eq!(
        help_row(&state, now),
        "[d] remove from home  [n] now playing  [l] unlike playing  [f] search  [u] show queue  [q] quit"
    );
}

#[test]
fn shelf_help_shows_queue_as_u_before_quit() {
    let now = Instant::now();
    let mut state = State::new(shelf());
    assert_eq!(
        help_row(&state, now),
        "[Space] pause  [←→] track  [f] search  [u] show queue  [q] quit"
    );
    playing(&mut state, now, 0);
    assert_eq!(
        help_row(&state, now),
        "[Space] pause  [←→] track  [l] like playing  [f] search  [u] show queue  [q] quit"
    );
    // u opens the queue just like Ctrl+Q.
    state.update(Event::Key(Key::Char('u')), now);
    assert_eq!(row(&render(&state, now, 100, 20), 4), "Queue");
}

/// Every view's help row fits in 100 columns with the longest labels
/// (`unlike playing`, `remove from shelf`) while something plays from an album.
/// `[n] now playing` fits on the shelf, in the queue and in Genres. In search and an
/// artist's albums quit is left out, in the track list quit, Space and ←→. No view
/// has `[↑↓] select`.
#[test]
fn every_help_row_fits_100_columns_with_unlike() {
    let now = Instant::now();
    let mut rows = Vec::new();

    let mut state = State::new(shelf());
    liked_playing_album(&mut state, now);
    rows.push(help_row(&state, now));
    state.update(Event::Key(Key::Down), now);
    state.update(Event::Key(Key::Down), now);
    state.update(Event::Key(Key::Enter), now);
    rows.push(help_row(&state, now));

    // On the shelf with a home genre selected.
    let mut state = State::new(shelf()).with_favorites(vec!["lofi".into()]);
    state.update(Event::Key(Key::Down), now);
    liked_playing_album(&mut state, now);
    rows.push(help_row(&state, now));

    let mut state = curated_view(2, now);
    liked_playing_album(&mut state, now);
    rows.push(help_row(&state, now));

    let mut state = nevermind_state(now);
    liked_playing_album(&mut state, now);
    rows.push(help_row(&state, now));

    let mut state = queued_state(now);
    state.update(Event::Key(Key::Ctrl('q')), now);
    liked_playing_album(&mut state, now);
    rows.push(help_row(&state, now));

    // Search and an artist's albums from Spotify: The Colour And The Shape is on the
    // shelf.
    let mut state = State::new(vec![colour()]);
    state.update(Event::Key(Key::Ctrl('f')), now);
    state.update(Event::Key(Key::Char('f')), now);
    let later = now + Duration::from_millis(300);
    state.update(Event::Tick, later);
    state.update(
        Event::Searched {
            query: "f".to_owned(),
            result: Ok(SearchResults {
                artists: vec![Artist {
                    id: "id-foo-fighters".to_owned(),
                    name: "Foo Fighters".to_owned(),
                }],
                albums: vec![colour()],
                songs: vec![],
            }),
        },
        later,
    );
    liked_playing_album(&mut state, later);
    rows.push(help_row(&state, later));
    state.update(Event::Key(Key::Down), later);
    state.update(Event::Key(Key::Down), later);
    rows.push(help_row(&state, later));
    state.update(Event::Key(Key::Up), later);
    state.update(Event::Key(Key::Enter), later);
    state.update(
        Event::ArtistAlbums {
            artist_id: "id-foo-fighters".to_owned(),
            result: Ok(vec![colour()]),
        },
        later,
    );
    rows.push(help_row(&state, later));

    // In the Genres view with lofi (on home) selected.
    let mut state = State::new(Vec::new()).with_favorites(vec!["lofi".into()]);
    state.update(Event::Key(Key::Down), now);
    state.update(Event::Key(Key::Down), now);
    state.update(Event::Key(Key::Enter), now);
    let lofi = genres::all().iter().position(|g| g.id == "lofi").unwrap();
    for _ in 0..lofi {
        state.update(Event::Key(Key::Down), now);
    }
    liked_playing_album(&mut state, now);
    rows.push(help_row(&state, now));

    // In radio with Night Drive (on the shelf) selected.
    let mut state = synthwave_radio(now);
    state.update(Event::Key(Key::Down), now);
    state.update(Event::Key(Key::Down), now);
    liked_playing(&mut state, now);
    rows.push(help_row(&state, now));

    assert_eq!(
        rows,
        [
            "[Space] pause  [←→] track  [n] now playing  [l] unlike playing  [f] search  [u] show queue  [q] quit",
            "[e] add to queue  [d] remove from shelf  [l] unlike playing  [Space] pause  [←→] track  [Esc] back",
            "[d] remove from home  [n] now playing  [l] unlike playing  [f] search  [u] show queue  [q] quit",
            "[e] add to queue  [a] remove from shelf  [d] not for me  [l] unlike playing  [Esc] back",
            "[Enter] play from here  [e] add to queue  [a] remove from shelf  [l] unlike playing  [Esc] back",
            "[d] remove  [n] now playing  [l] unlike playing  [Space] pause  [←→] track  [Esc] back  [q] quit",
            "[↓] results  [Ctrl+N] now playing  [Ctrl+L] unlike playing  [Esc] back  [Ctrl+C] quit",
            "[e] add to queue  [a] remove from shelf  [l] unlike playing  [f] search  [Esc] back  [q] quit",
            "[e] add to queue  [a] remove from shelf  [l] unlike playing  [Space] pause  [←→] track  [Esc] back",
            "[Enter] play radio  [a] remove from home  [n] now playing  [l] unlike playing  [Esc] back",
            "[Enter] play  [e] add to queue  [a] remove from shelf  [r] new radio  [l] unlike playing  [Esc] back",
        ]
    );
    // No row is cut: even an undrawn row is at most 100 columns. Outside search the
    // queue is the letter u, not Ctrl+Q.
    for row in &rows {
        assert!(row.width() <= 100, "{row}");
        assert!(!row.contains("[Ctrl+Q]"), "{row}");
        assert!(!row.contains("[↑↓]"), "{row}");
    }
}
