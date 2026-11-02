//! Tests of the responses, errors, the memory and the saved sign-in.

use std::fs;

use anyhow::bail;
use librespot_oauth::OAuthError;

use super::*;
use crate::catalog::{
    api::{SearchPage, search_results},
    auth::{has_library_scopes, rejected, write_private},
};

#[test]
fn search_response_gives_songs_with_their_albums() {
    let page: SearchPage = serde_json::from_str(
        r#"{
          "artists": { "items": [{ "id": "nirvana", "name": "Nirvana" }], "next": null },
          "albums": { "items": [], "next": null },
          "tracks": { "items": [{
            "uri": "spotify:track:inbloom", "name": "In Bloom", "duration_ms": 254000,
            "artists": [{ "id": "nirvana", "name": "Nirvana" }],
            "album": { "id": "nevermind", "uri": "spotify:album:nevermind", "name": "Nevermind",
              "artists": [{ "id": "nirvana", "name": "Nirvana" }], "release_date": "1991-09-24",
              "album_type": "album" }
          }], "next": null }
        }"#,
    )
    .unwrap();
    let found = search_results(page);
    assert_eq!(found.artists.len(), 1);
    let [song] = found.songs.as_slice() else {
        panic!("one song: {:?}", found.songs);
    };
    assert_eq!(song.track.name, "In Bloom");
    assert_eq!(song.track.artist, "Nirvana");
    assert_eq!(song.track.duration, Duration::from_secs(254));
    assert_eq!(song.album.uri, "spotify:album:nevermind");
    assert_eq!(song.album.year, Some(1991));
}

#[test]
fn search_response_without_tracks_has_no_songs() {
    let page: SearchPage = serde_json::from_str(
        r#"{ "artists": { "items": [], "next": null }, "albums": { "items": [], "next": null } }"#,
    )
    .unwrap();
    assert!(search_results(page).songs.is_empty());
}

fn scopes(names: &[&str]) -> Vec<String> {
    names.iter().map(|&name| name.to_owned()).collect()
}

#[test]
fn library_scopes_are_needed_both() {
    assert!(has_library_scopes(&scopes(&[
        "user-library-modify",
        "user-library-read",
    ])));
    assert!(has_library_scopes(&scopes(&[
        "playlist-read-private",
        "user-library-read",
        "user-library-modify",
    ])));
    assert!(!has_library_scopes(&scopes(&["user-library-read"])));
    assert!(!has_library_scopes(&[]));
}

#[test]
fn missing_scope_asks_to_sign_in_again() {
    let body = r#"{"error":{"status":403,"message":"Insufficient client scope"}}"#;
    assert_eq!(
        api_error(StatusCode::FORBIDDEN, body),
        "Deck has no access to your Liked Songs – restart Deck to sign in again"
    );
}

#[test]
fn other_errors_keep_status_and_body() {
    let limited = |retry_after| anyhow::Error::from(RateLimited { retry_after }).to_string();
    assert_eq!(
        limited(Some(34560)),
        "Spotify is rate limiting requests, try again in 34560 s"
    );
    assert_eq!(
        limited(None),
        "Spotify is rate limiting requests, try again in ? s"
    );
    assert_eq!(
        api_error(StatusCode::FORBIDDEN, "Forbidden"),
        "Spotify Web API 403 Forbidden: Forbidden"
    );
}

#[test]
fn contains_answer_has_a_flag_per_uri() {
    let flags = serde_json::from_str("[false,true]").unwrap();
    assert_eq!(liked_flags(2, flags).unwrap(), [false, true]);
    let short = serde_json::from_str("[true]").unwrap();
    assert_eq!(
        liked_flags(2, short).unwrap_err().to_string(),
        "Spotify answered 1 of 2 liked songs"
    );
}

#[tokio::test]
async fn memo_fetches_a_key_once_even_concurrently() {
    let memo = Memo::<u32>::new(10);
    let calls = std::sync::atomic::AtomicU32::new(0);
    let fetch = || async {
        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        tokio::task::yield_now().await;
        Ok(7)
    };
    let (a, b) = tokio::join!(memo.get_or_fetch("x", fetch), memo.get_or_fetch("x", fetch));
    assert_eq!((a.unwrap(), b.unwrap()), (7, 7));
    assert_eq!(memo.get_or_fetch("x", fetch).await.unwrap(), 7);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn memo_does_not_keep_errors() {
    let memo = Memo::<u32>::new(10);
    let failed = memo
        .get_or_fetch("x", || async { bail!("Spotify is not responding") })
        .await;
    assert!(failed.is_err());
    assert_eq!(memo.get_or_fetch("x", || async { Ok(1) }).await.unwrap(), 1);
}

#[tokio::test]
async fn memo_forgets_the_oldest_when_full() {
    let memo = Memo::<u32>::new(2);
    for (key, value) in [("a", 1), ("b", 2), ("c", 3)] {
        memo.get_or_fetch(key, || async move { Ok(value) })
            .await
            .unwrap();
    }
    // "a" was forgotten, "b" and "c" are in memory.
    assert_eq!(
        memo.get_or_fetch("a", || async { Ok(10) }).await.unwrap(),
        10
    );
    assert_eq!(
        memo.get_or_fetch("c", || async { Ok(30) }).await.unwrap(),
        3
    );
}

#[test]
fn only_a_rejection_by_spotify_needs_a_new_sign_in() {
    let failure = |message: &str| {
        anyhow::Error::from(OAuthError::ExchangeCode {
            e: message.to_owned(),
        })
        .context("could not refresh the Web API token")
    };
    assert!(rejected(&failure(
        "Server returned error response: invalid_grant: Refresh token revoked"
    )));
    assert!(rejected(&failure(
        "Server returned error response: invalid_client: Invalid client"
    )));
    assert!(!rejected(&failure("Request failed")));
    assert!(!rejected(&failure("Failed to parse server response")));
    assert!(!rejected(&failure(
        "Server returned error response: server_error"
    )));
    assert!(!rejected(&anyhow::anyhow!("something else")));
}

#[test]
fn token_file_is_written_atomically_for_the_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("webapi.json");
    let tmp = dir.path().join("webapi.json.tmp");
    // A leftover from a crash, readable by everyone.
    fs::write(&tmp, "old").unwrap();
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o644)).unwrap();

    write_private(&path, b"{\"refresh_token\":\"r\"}").unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "{\"refresh_token\":\"r\"}"
    );
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!tmp.exists());

    write_private(&path, b"new").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "new");
}
