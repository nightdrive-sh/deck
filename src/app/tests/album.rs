//! Tests: track list.

use super::*;

#[test]
fn ctl_play_album_opens_the_shelf_album_and_plays_it_when_tracks_arrive() {
    let mut app = shelf_app();
    let uri = nevermind().uri;
    assert_eq!(app.send(Event::PlayAlbum(uri)), [load_tracks("nevermind")]);
    let actions = app.send(album_tracks(&nevermind(), Ok(nevermind_tracks())));
    assert!(actions.contains(&play("nevermind", 0)), "{actions:?}");
    assert_eq!(app.album_view().album, nevermind());
    // Only once: tracks arriving again (e.g. reopening the album) don't restart playback.
    let again = app.send(album_tracks(&nevermind(), Ok(nevermind_tracks())));
    assert!(!again.contains(&play("nevermind", 0)));
}

#[test]
fn ctl_play_song_plays_its_album_from_the_song() {
    let mut app = shelf_app();
    let event = Event::PlaySong {
        track: track_uri("Come As You Are"),
        album: nevermind().uri,
    };
    assert_eq!(app.send(event), [load_tracks("nevermind")]);
    let actions = app.send(album_tracks(&nevermind(), Ok(nevermind_tracks())));
    assert!(actions.contains(&play("nevermind", 2)), "{actions:?}");
    assert_eq!(app.album_view().selected, 2);
}

#[test]
fn ctl_search_opens_search_and_searches_at_once() {
    let mut app = shelf_app();
    let actions = app.send(Event::Search("  fm-84 atlas ".to_owned()));
    assert_eq!(actions, [Action::Search("fm-84 atlas".into())]);
    assert_eq!(app.search_view().query, "fm-84 atlas");
    assert!(app.search_view().searching());
    // The debounce doesn't send the same search again.
    assert!(app.wait(300).is_empty());
}

#[test]
fn ctl_play_album_fetches_an_unknown_album_first() {
    let mut app = shelf_app();
    let uri = "spotify:album:unknown".to_owned();
    assert_eq!(
        app.send(Event::PlayAlbum(uri.clone())),
        [Action::FetchAlbum(uri.clone())]
    );
    let album = Album {
        uri: uri.clone(),
        id: "unknown".into(),
        ..nevermind()
    };
    let actions = app.send(Event::AlbumFetched {
        uri: uri.clone(),
        result: Ok(album.clone()),
    });
    assert_eq!(actions, [Action::LoadAlbumTracks(uri)]);
    let actions = app.send(album_tracks(&album, Ok(nevermind_tracks())));
    assert!(actions.contains(&play("unknown", 0)), "{actions:?}");
}

#[test]
fn ctl_play_album_reports_a_failed_fetch() {
    let mut app = shelf_app();
    let uri = "spotify:album:gone".to_owned();
    app.send(Event::PlayAlbum(uri.clone()));
    let actions = app.send(Event::AlbumFetched {
        uri,
        result: Err("not found".into()),
    });
    assert!(actions.is_empty());
    assert_eq!(app.state.error.as_deref(), Some("not found"));
}

#[test]
fn album_tracks_arrive_without_playing() {
    let app = nevermind_app();
    let view = app.album_view();
    assert_eq!(view.tracks, Some(nevermind_tracks()));
    assert_eq!(view.selected, 0);
    assert_eq!(app.state.error, None);
    assert_eq!(app.state.now_playing, None);
    assert_eq!(app.state.playing_track(&nevermind().uri), None);
}

#[test]
fn opening_the_playing_album_marks_its_playing_track() {
    let mut app = nevermind_app();
    app.keys(&[Down, Down, Enter]);
    app.send(started("Come As You Are"));
    app.keys(&[Esc, Esc]);
    // Opening does not restart playback.
    assert_eq!(app.keys(&[Enter, Down, Enter]), [load_tracks("nevermind")]);
    app.send(album_tracks(&nevermind(), Ok(nevermind_tracks())));
    assert_eq!(app.state.playing_track(&nevermind().uri), Some(2));
}

#[test]
fn failed_playback_keeps_track_list() {
    let mut app = nevermind_app();
    app.key(Down);
    assert_eq!(app.key(Enter), [play("nevermind", 1)]);
    app.send(Event::Player(player::Event::Error(
        "Spotify is not responding".into(),
    )));
    assert_eq!(app.album_view().tracks, Some(nevermind_tracks()));
    assert_eq!(app.album_view().selected, 1);
    assert_eq!(
        app.state.error.as_deref(),
        Some("Spotify is not responding")
    );
}

#[test]
fn tracks_of_other_album_are_ignored() {
    let mut app = shelf_app();
    app.keys(&[Down, Enter, Down, Enter]);
    app.send(album_tracks(&bleach(), Ok(vec![track("Blew", 175)])));
    assert_eq!(app.album_view().tracks, None);
}

#[test]
fn failed_album_shows_error_and_empty_list() {
    let mut app = shelf_app();
    app.keys(&[Down, Enter, Enter]);
    app.send(album_tracks(
        &bleach(),
        Err("Spotify is not responding".into()),
    ));
    assert_eq!(app.album_view().tracks, Some(vec![]));
    assert_eq!(
        app.state.error.as_deref(),
        Some("Spotify is not responding")
    );
    // In an empty list Enter plays nothing.
    assert!(app.keys(&[Down, Enter]).is_empty());
}

#[test]
fn enter_before_tracks_arrive_does_nothing() {
    let mut app = shelf_app();
    app.keys(&[Down, Enter, Enter]);
    assert!(app.keys(&[Down, Enter]).is_empty());
    assert_eq!(app.album_view().selected, 0);
}

#[test]
fn album_up_down_stay_in_bounds() {
    let mut app = nevermind_app();
    app.key(Up);
    assert_eq!(app.album_view().selected, 0);
    app.keys(&[Down; 6]);
    assert_eq!(app.album_view().selected, 3);
    app.key(Up);
    assert_eq!(app.album_view().selected, 2);
}

#[test]
fn enter_on_track_plays_album_from_it() {
    let mut app = nevermind_app();
    app.keys(&[Down, Down]);
    assert_eq!(app.key(Enter), [play("nevermind", 2)]);
    // The list stays open and the selection in place.
    assert_eq!(app.album_view().selected, 2);
    app.keys(&[Up, Up]);
    assert_eq!(app.key(Enter), [play("nevermind", 0)]);
}

#[test]
fn playing_mark_follows_track_changes() {
    let mut app = nevermind_app();
    let uri = nevermind().uri;
    assert_eq!(app.state.playing_track(&uri), None);

    app.key(Enter);
    // The mark shows only once the player has started the track.
    assert_eq!(app.state.playing_track(&uri), None);
    app.send(started("Smells Like Teen Spirit"));
    assert_eq!(app.state.playing_track(&uri), Some(0));
    assert_eq!(app.send(end_of_track()), [play("nevermind", 1)]);
    assert_eq!(app.state.playing_track(&uri), Some(1));
    assert_eq!(app.key(Right), [play("nevermind", 2)]);
    assert_eq!(app.state.playing_track(&uri), Some(2));
    // Another album is playing: this list has no playing track.
    app.keys(&[Esc, Up, Enter]);
    app.send(album_tracks(&bleach(), Ok(vec![track("Blew", 175)])));
    assert_eq!(app.key(Enter), [play("bleach", 0)]);
    assert_eq!(app.state.playing_track(&uri), None);
    assert_eq!(app.state.playing_track(&bleach().uri), Some(0));

    app.send(Event::Player(player::Event::Stopped));
    assert_eq!(app.state.playing_track(&bleach().uri), None);
}

#[test]
fn esc_from_album_returns_to_artist_albums() {
    let mut app = nevermind_app();
    app.key(Down);
    app.key(Esc);
    let view = app.artist_view();
    assert_eq!(view.artist, artist("Nirvana"));
    assert_eq!(view.selected, 1);
    app.key(Esc);
    assert_eq!(app.shelf_selected(), 3);
}

#[test]
fn esc_from_spotify_album_returns_to_spotify_artist() {
    let mut app = spotify_artist_app();
    app.send(Event::ArtistAlbums {
        artist_id: "id-nirvana".into(),
        result: Ok(vec![unplugged(), nevermind()]),
    });
    app.key(Down);
    assert_eq!(app.key(Enter), [load_tracks("unplugged")]);
    app.key(Esc);
    assert!(matches!(app.artist_view().source, Source::Spotify(Some(_))));
    assert_eq!(app.artist_view().selected, 1);
    app.key(Esc);
    assert_eq!(app.search_view().query, "nirvana");
}

#[test]
fn player_keys_in_album_view() {
    let mut app = nevermind_app();
    assert_eq!(app.key(Char(' ')), [Action::TogglePause]);
    app.key(Enter);
    assert_eq!(app.key(Right), [play("nevermind", 1)]);
    assert_eq!(app.key(Left), [play("nevermind", 0)]);
    assert_eq!(app.key(Char('q')), [Action::Quit]);
    assert_eq!(app.key(Ctrl('c')), [Action::Quit]);
    assert!(app.keys(&[Char('x'), Backspace]).is_empty());
    app.album_view();
}

#[test]
fn media_keys_control_playback_in_any_view() {
    let mut app = nevermind_app();
    app.key(Enter);
    app.send(started("Smells Like Teen Spirit"));
    app.send(Event::Player(player::Event::Position(0)));
    // In search Space types, but media keys control playback.
    app.key(Ctrl('f'));
    let media = |app: &mut App, command| app.send(Event::Media(command));
    assert_eq!(media(&mut app, MediaCommand::Toggle), [Action::TogglePause]);
    assert!(media(&mut app, MediaCommand::Play).is_empty());
    assert_eq!(media(&mut app, MediaCommand::Pause), [Action::TogglePause]);
    app.send(Event::Player(player::Event::Paused(1_000)));
    assert!(media(&mut app, MediaCommand::Pause).is_empty());
    assert_eq!(media(&mut app, MediaCommand::Play), [Action::TogglePause]);
    assert_eq!(media(&mut app, MediaCommand::Next), [play("nevermind", 1)]);
    assert_eq!(
        media(&mut app, MediaCommand::Previous),
        [play("nevermind", 0)]
    );
    assert_eq!(app.search_view().query, "");
}

#[test]
fn ctrl_a_in_album_view_toggles_album_on_shelf() {
    let mut app = App::new(vec![]);
    app.key(Ctrl('f'));
    app.search("n", vec![], vec![nevermind()]);
    app.keys(&[Enter, Enter]);
    app.send(album_tracks(&nevermind(), Ok(nevermind_tracks())));
    app.key(Down);
    assert_eq!(app.key(Ctrl('a')), [Action::AddToShelf(nevermind())]);
    app.send(Event::ShelfChanged(vec![nevermind()]));
    // Ctrl+A on an album on the shelf removes it; Ctrl+D does nothing in the list.
    assert_eq!(
        app.key(Ctrl('a')),
        [Action::RemoveFromShelf(nevermind().uri)]
    );
    assert!(app.key(Ctrl('d')).is_empty());
    assert_eq!(app.album_view().selected, 1);
}

#[test]
fn ctrl_f_from_album_view_opens_search() {
    let mut app = nevermind_app();
    app.key(Ctrl('f'));
    app.search_view();
    app.key(Esc);
    assert_eq!(app.album_view().album, nevermind());
}
