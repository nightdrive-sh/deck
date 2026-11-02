//! Last.fm in the config, the albums played in Deck and the taste notes.

use super::*;
use crate::{
    curate::taste::{lastfm_user, taste_notes},
    history::Played,
};

#[test]
fn lastfm_user_is_required() {
    let config = Config {
        lastfm_api_key: Some("key".to_owned()),
        ..Config::default()
    };
    let Err(error) = lastfm_client(&config) else {
        panic!("lastfm_client accepted a config without lastfm_user");
    };
    let error = format!("{error:#}");
    assert!(
        error.contains("lastfm_user = \"<your Last.fm username>\""),
        "{error}"
    );

    let blank = Config {
        lastfm_user: Some("  ".to_owned()),
        ..config
    };
    assert!(lastfm_user(&blank).is_err());

    let no_key = Config {
        lastfm_user: Some("me".to_owned()),
        ..Config::default()
    };
    assert!(lastfm_client(&no_key).is_err());
}

#[test]
fn lastfm_is_optional() {
    assert!(lastfm_client(&Config::default()).unwrap().is_none());
    let blank = Config {
        lastfm_api_key: Some(String::new()),
        lastfm_user: Some(" ".to_owned()),
        ..Config::default()
    };
    assert!(lastfm_client(&blank).unwrap().is_none());
}

#[test]
fn albums_played_in_deck_are_listened() {
    let played = [Played {
        uri: "spotify:album:x".to_owned(),
        name: "Kroketti".to_owned(),
        artist: "Ultra Bra".to_owned(),
        at: NOW,
    }];
    let listened = played_in_deck(&played);
    assert_eq!(
        known(&candidate("Ultra Bra", "Kroketti"), &[], &[], &listened),
        Some(Rejection::Listened)
    );
}

#[test]
fn taste_notes_are_optional() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("taste.md");
    assert_eq!(taste_notes(&path).unwrap(), None);
    std::fs::write(&path, " \n").unwrap();
    assert_eq!(taste_notes(&path).unwrap(), None);
    std::fs::write(&path, "Late-60s folk rock\n").unwrap();
    assert_eq!(
        taste_notes(&path).unwrap().as_deref(),
        Some("Late-60s folk rock")
    );
}
