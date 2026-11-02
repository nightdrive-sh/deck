//! Tests: likes (Liked Songs).

use super::*;

fn like_changed(name: &str, liked: bool, result: Result<(), String>) -> Event {
    Event::LikeChanged {
        uri: track_uri(name),
        liked,
        result,
    }
}

fn liked_checked(names: &[&str], result: Result<Vec<bool>, String>) -> Event {
    Event::LikedChecked {
        uris: names.iter().map(|name| track_uri(name)).collect(),
        result,
    }
}

#[test]
fn ctrl_l_likes_playing_track_and_again_unlikes_it() {
    let mut app = in_bloom_playing();
    app.send(liked_checked(&["In Bloom"], Ok(vec![false])));

    assert_eq!(app.key(Ctrl('l')), [set_liked("In Bloom", true)]);
    // The state changes only once Spotify has answered.
    assert!(!app.state.liked(&track_uri("In Bloom")));
    app.send(like_changed("In Bloom", true, Ok(())));
    assert!(app.state.liked(&track_uri("In Bloom")));
    assert_eq!(
        app.state.notice(app.now),
        Some((NoticeKind::Liked, "In Bloom"))
    );

    assert_eq!(app.key(Ctrl('l')), [set_liked("In Bloom", false)]);
    app.send(like_changed("In Bloom", false, Ok(())));
    assert!(!app.state.liked(&track_uri("In Bloom")));
    assert_eq!(
        app.state.notice(app.now),
        Some((NoticeKind::Unliked, "In Bloom"))
    );
    app.now += NOTICE_TIME;
    assert_eq!(app.state.notice(app.now), None);
}

#[test]
fn media_like_toggles_like_and_now_playing_reports_it() {
    let mut app = in_bloom_playing();
    app.send(liked_checked(&["In Bloom"], Ok(vec![false])));
    assert_eq!(
        app.state.now_playing_info(app.now).map(|info| info.liked),
        Some(Some(false))
    );

    // `deck ctl like` arrives as a media command and acts like Ctrl+L.
    let media = |app: &mut App, command| app.send(Event::Media(command));
    assert_eq!(
        media(&mut app, MediaCommand::ToggleLike),
        [set_liked("In Bloom", true)]
    );
    app.send(like_changed("In Bloom", true, Ok(())));
    assert_eq!(
        app.state.now_playing_info(app.now).map(|info| info.liked),
        Some(Some(true))
    );
}

#[test]
fn ctrl_l_unlikes_track_known_to_be_liked() {
    let mut app = in_bloom_playing();
    app.send(liked_checked(&["In Bloom"], Ok(vec![true])));
    assert!(app.state.liked(&track_uri("In Bloom")));
    assert_eq!(app.key(Ctrl('l')), [set_liked("In Bloom", false)]);
}

#[test]
fn ctrl_l_before_answer_likes() {
    let mut app = in_bloom_playing();
    assert_eq!(app.key(Ctrl('l')), [set_liked("In Bloom", true)]);
    // A late answer does not undo the like.
    app.send(like_changed("In Bloom", true, Ok(())));
    app.send(liked_checked(&["In Bloom"], Ok(vec![false])));
    assert!(app.state.liked(&track_uri("In Bloom")));
}

#[test]
fn ctrl_l_works_in_every_view_also_in_search() {
    let mut app = in_bloom_playing();
    app.key(Ctrl('f'));
    app.type_text("nirv");
    assert_eq!(app.key(Ctrl('l')), [set_liked("In Bloom", true)]);
    assert_eq!(app.search_view().query, "nirv");
    app.send(like_changed("In Bloom", true, Ok(())));
    app.key(Ctrl('q'));
    assert_eq!(app.key(Ctrl('l')), [set_liked("In Bloom", false)]);
}

#[test]
fn ctrl_l_waits_for_previous_change() {
    let mut app = in_bloom_playing();
    assert_eq!(app.key(Ctrl('l')), [set_liked("In Bloom", true)]);
    assert_eq!(app.key(Ctrl('l')), []);
    app.send(like_changed("In Bloom", true, Ok(())));
    assert_eq!(app.key(Ctrl('l')), [set_liked("In Bloom", false)]);
}

#[test]
fn ctrl_l_without_playing_track_does_nothing() {
    let mut app = likes_app();
    assert_eq!(app.key(Ctrl('l')), []);
    assert_eq!(app.state.error, None);
}

#[test]
fn ctrl_l_without_own_spotify_app_explains() {
    let mut app = shelf_app();
    assert_eq!(app.send(started("In Bloom")), []);
    assert_eq!(app.key(Ctrl('l')), []);
    assert_eq!(app.state.error.as_deref(), Some(LIKES_NEED_APP));
}

#[test]
fn failed_like_shows_error_and_keeps_state() {
    let mut app = in_bloom_playing();
    app.send(liked_checked(&["In Bloom"], Ok(vec![false])));
    app.key(Ctrl('l'));
    app.send(like_changed(
        "In Bloom",
        true,
        Err("Spotify is rate limiting".into()),
    ));
    assert_eq!(app.state.error.as_deref(), Some("Spotify is rate limiting"));
    assert!(!app.state.liked(&track_uri("In Bloom")));
    assert_eq!(app.state.notice(app.now), None);
    // A new attempt is the user's own: Ctrl+L tries again.
    assert_eq!(app.key(Ctrl('l')), [set_liked("In Bloom", true)]);
}

#[test]
fn album_tracks_are_checked_once_in_one_batch() {
    let mut app = likes_app();
    app.keys(&[Down, Enter, Down]);
    app.key(Enter);
    let names = [
        "Smells Like Teen Spirit",
        "In Bloom",
        "Come As You Are",
        "Breed",
    ];
    assert_eq!(
        app.send(album_tracks(&nevermind(), Ok(nevermind_tracks()))),
        [check(&names)]
    );
    app.send(liked_checked(&names, Ok(vec![false, true, false, false])));
    assert!(app.state.liked(&track_uri("In Bloom")));
    assert!(!app.state.liked(&track_uri("Breed")));

    // The track list again and playing an album track: no new query.
    app.key(Esc);
    app.key(Enter);
    assert_eq!(
        app.send(album_tracks(&nevermind(), Ok(nevermind_tracks()))),
        []
    );
    assert_eq!(app.send(started("In Bloom")), []);
}

#[test]
fn unanswered_check_is_not_repeated() {
    let mut app = in_bloom_playing();
    assert_eq!(app.send(started("In Bloom")), []);
    assert_eq!(app.send(started("Breed")), [check(&["Breed"])]);
}

#[test]
fn queued_album_is_checked() {
    let mut app = in_bloom_playing();
    let actions = app.send(Event::AlbumQueued {
        album: nevermind(),
        track: None,
        result: Ok(nevermind_tracks()),
    });
    // Nothing was playing in the play order, so the queue starts playing.
    assert_eq!(actions[0], play_track("Smells Like Teen Spirit"));
    assert_eq!(
        actions[1..],
        [check(&[
            "Smells Like Teen Spirit",
            "Come As You Are",
            "Breed"
        ])]
    );
}

#[test]
fn failed_check_stops_checks_without_retry() {
    let mut app = in_bloom_playing();
    app.send(liked_checked(
        &["In Bloom"],
        Err("Spotify is rate limiting requests, try again in 30 s".into()),
    ));
    assert_eq!(
        app.state.error.as_deref(),
        Some("Spotify is rate limiting requests, try again in 30 s")
    );
    assert_eq!(app.send(started("Breed")), []);
    app.keys(&[Down, Enter, Down]);
    app.key(Enter);
    assert_eq!(
        app.send(album_tracks(&nevermind(), Ok(nevermind_tracks()))),
        []
    );
    // Ctrl+L still works: an unknown state counts as not liked.
    assert_eq!(app.key(Ctrl('l')), [set_liked("Breed", true)]);
}

#[test]
fn radio_pages_are_checked_at_most_40_at_a_time() {
    let mut app = genre_app(&["lofi"]);
    app.state.likes.enabled = true;
    let request = radio_request(&app.key(Enter));
    let actions = app.send(radio_loaded(&request, station(0..50)));
    let uris = |range: std::ops::Range<usize>| -> Vec<String> {
        range.map(|n| format!("spotify:track:r{n}")).collect()
    };
    assert_eq!(
        actions,
        [
            radio_play(0),
            Action::CheckLiked(uris(0..40)),
            Action::CheckLiked(uris(40..50)),
        ]
    );
    app.send(Event::LikedChecked {
        uris: uris(0..40),
        result: Ok((0..40).map(|n| n == 3).collect()),
    });
    assert!(app.state.liked("spotify:track:r3"));
    assert!(!app.state.liked("spotify:track:r4"));

    // A refill asks only about the new tracks.
    let more = radio_request(&play_until(&mut app, 45));
    let actions = app.send(radio_loaded(&more, station(45..60)));
    assert_eq!(actions, [Action::CheckLiked(uris(50..60))]);
}

#[test]
fn restore_checks_playing_queue_and_upcoming_radio() {
    let mut app = in_bloom_with_queue();
    let saved = quit(&mut app);
    let mut app = likes_app();
    let actions = app.send(Event::Restore(Box::new(saved)));
    assert!(matches!(actions[0], Action::Restore { .. }));
    assert_eq!(
        actions[1..],
        [check(&["In Bloom", "Come As You Are", "Breed"])]
    );

    let mut app = genre_app(&["lofi"]);
    start_radio(&mut app, station(0..50));
    play_until(&mut app, 30);
    let saved = quit(&mut app);
    let mut app = likes_app();
    let actions = app.send(Event::Restore(Box::new(saved)));
    let upcoming: Vec<String> = (30..50).map(|n| format!("spotify:track:r{n}")).collect();
    let mut expected = vec![track_uri("Song")];
    expected.extend(upcoming);
    assert_eq!(actions[1..], [Action::CheckLiked(expected)]);
}
