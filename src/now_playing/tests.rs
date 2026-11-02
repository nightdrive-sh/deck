//! Tests of what the helper is sent, the control lines and the cover downloads.

use std::path::PathBuf;

use super::*;

fn info(position_ms: u64, playing: bool) -> Info {
    Info {
        title: "Syndicate".into(),
        artist: "The Fall".into(),
        album: "Hex".into(),
        duration_ms: 200_000,
        position_ms,
        playing,
        cover: None,
        liked: None,
        track_uri: None,
        album_uri: None,
        queued: false,
        queue: Vec::new(),
        radio: None,
        error: None,
    }
}

#[test]
fn first_info_is_sent_but_empty_is_not() {
    let now = Instant::now();
    assert!(changed(None, Some(&info(0, true)), now));
    assert!(!changed(None, None, now));
}

#[test]
fn playing_progress_is_not_resent() {
    let at = Instant::now();
    let sent = (Some(info(10_000, true)), at);
    let now = at + Duration::from_secs(30);
    assert!(!changed(Some(&sent), Some(&info(40_000, true)), now));
    // At the end of the track, the position stops at the duration.
    let late = at + Duration::from_secs(300);
    assert!(!changed(Some(&sent), Some(&info(200_000, true)), late));
}

#[test]
fn pause_track_change_and_jump_are_sent() {
    let at = Instant::now();
    let sent = (Some(info(10_000, true)), at);
    let now = at + Duration::from_secs(5);
    assert!(changed(Some(&sent), Some(&info(15_000, false)), now));
    let other = Info {
        title: "Hey! Luciferase".into(),
        ..info(15_000, true)
    };
    assert!(changed(Some(&sent), Some(&other), now));
    // ← to the start of the same track.
    assert!(changed(Some(&sent), Some(&info(0, true)), now));
    assert!(changed(Some(&sent), None, now));
}

#[test]
fn position_does_not_advance_when_paused() {
    let at = Instant::now();
    let sent = (Some(info(10_000, false)), at);
    let now = at + Duration::from_secs(60);
    assert!(!changed(Some(&sent), Some(&info(10_000, false)), now));
}

#[test]
fn button_name_parses_back_to_command() {
    for command in Command::ALL {
        assert_eq!(
            Command::parse(&format!("{}\n", command.name())),
            Some(command)
        );
    }
    assert_eq!(Command::parse("seek"), None);
}

#[test]
fn control_lines_round_trip() {
    let album = Control::PlayAlbum("spotify:album:abc".to_owned());
    assert_eq!(album.line(), "play-album spotify:album:abc");
    assert_eq!(Control::parse(&format!("{}\n", album.line())), Some(album));
    assert_eq!(Control::parse("next"), Some(Control::Media(Command::Next)));
    assert_eq!(Control::parse("play-album spotify:track:abc"), None);
    assert_eq!(Control::parse("play-album"), None);
    let search = Control::Search("fm-84 atlas".to_owned());
    assert_eq!(Control::parse(&search.line()), Some(search));
    assert_eq!(Control::Search("a\nb".to_owned()).line(), "search a b");
    assert_eq!(Control::parse("search   "), None);
    let song = Control::PlaySong {
        track: "spotify:track:t".to_owned(),
        album: "spotify:album:a".to_owned(),
    };
    assert_eq!(song.line(), "play-song spotify:track:t spotify:album:a");
    assert_eq!(Control::parse(&song.line()), Some(song));
    assert_eq!(
        Control::parse("play-song spotify:album:a spotify:track:t"),
        None
    );
    assert_eq!(Control::parse("play-song spotify:track:t"), None);
    let restore = Control::Restore(PathBuf::from("/tmp/playback.json"));
    assert_eq!(restore.line(), "restore /tmp/playback.json");
    assert_eq!(
        Control::parse(&format!("{}\n", restore.line())),
        Some(restore)
    );
    assert_eq!(Control::parse("restore playback.json"), None);
    let save = Control::Save(PathBuf::from("/tmp/undo.json"));
    assert_eq!(save.line(), "save /tmp/undo.json");
    assert_eq!(Control::parse(&save.line()), Some(save));
    assert_eq!(Control::parse("save undo.json"), None);
    assert_eq!(Control::parse("restore "), None);
    let window = [
        Control::QueueAlbum("spotify:album:a".to_owned()),
        Control::QueueSong {
            track: "spotify:track:t".to_owned(),
            album: "spotify:album:a".to_owned(),
        },
        Control::Unqueue(3),
        Control::ClearQueue,
        Control::Pick(crate::picks::PickFrom::Any),
        Control::Pick(crate::picks::PickFrom::Curated),
        Control::Seek(83_000),
        Control::Radio("indie-rock".to_owned()),
        Control::FavoriteAdd("lofi".to_owned()),
        Control::FavoriteRemove("lofi".to_owned()),
        Control::RejectCurated("spotify:album:a".to_owned()),
        Control::ShelfAdd("spotify:album:a".to_owned()),
        Control::ShelfRemove("spotify:album:a".to_owned()),
        Control::Quit,
    ];
    for control in window {
        assert_eq!(Control::parse(&control.line()), Some(control));
    }
    assert_eq!(Control::parse("queue-album spotify:track:t"), None);
    assert_eq!(Control::parse("unqueue first"), None);
    assert_eq!(Control::parse("shelf-add "), None);
    assert_eq!(Control::parse("seek soon"), None);
    assert_eq!(Control::parse("pick radio"), None);
    assert_eq!(Control::parse("picked"), None);
    assert_eq!(Control::parse("radio "), None);
    assert_eq!(Control::parse("radio indie rock"), None);
    assert_eq!(Control::parse("curated-reject spotify:track:t"), None);
}

#[test]
fn cover_is_downloaded_once_and_retried_after_a_failure() {
    let now = Instant::now();
    let url = "https://i.scdn.co/image/a";
    assert!(should_download(url, None, None, now));
    assert!(should_download(
        url,
        Some("https://i.scdn.co/image/b"),
        None,
        now
    ));
    assert!(!should_download(url, Some(url), None, now));
    // A failed download is retried only after a while.
    let failed = (url.to_owned(), now);
    assert!(!should_download(url, None, Some(&failed), now));
    assert!(should_download(url, None, Some(&failed), now + COVER_RETRY));
    // Another track's cover is downloaded right away.
    assert!(should_download(
        "https://i.scdn.co/image/b",
        None,
        Some(&failed),
        now
    ));
}
