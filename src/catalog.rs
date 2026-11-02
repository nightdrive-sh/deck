//! Spotify Web API: search, artist albums, album tracks and Liked Songs.
//!
//! The token comes from an OAuth sign-in to the user's own Spotify app when
//! `config.toml` gives a `client_id`. Otherwise the librespot session token (login5)
//! is used, which Spotify rate limits because of the shared client ID (429).
//! Subcommands (`deck curate submit`) sign in without a session, using only the saved token
//! ([`Catalog::connect_stored`]).
//!
//! Likes go through `/me/library` (the old `/me/tracks` has been shut down), and they
//! need an own-app token with the library scopes (`LIBRARY_SCOPES`). A token saved
//! before those scopes is not accepted: startup asks the user to sign in again.
//!
//! After too many requests Spotify blocks all of the app's requests for hours (429),
//! so requests are saved: album track lists, artist albums and the latest searches are
//! kept in memory (shared by clones), and after a 429 no new requests are sent until
//! `retry-after` has passed. The block is shared with the other Deck processes through
//! [`RateLimit`].

use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crate::{config::Config, player::SharedSession, rate_limit::RateLimit, store::unix_now};

mod api;
mod auth;
mod memo;
mod search;

use api::{ApiAlbum, ApiTrack, Page};
use auth::AppAuth;
use memo::Memo;

const API: &str = "https://api.spotify.com/v1";

/// How many URIs `/me/library/contains` takes at once.
pub const LIKED_BATCH: usize = 40;

/// The response when the token was saved before the library scopes.
const MISSING_SCOPE: &str = "Insufficient client scope";

/// Timeout for a Web API request, from opening the connection to the end of the response.
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// How long (seconds) no requests are sent after a 429 if Spotify gives no
/// `retry-after`.
const DEFAULT_BLOCK: u64 = 60;

/// Maximum number of responses kept in memory.
const SEARCHES_KEPT: usize = 50;
const ARTISTS_KEPT: usize = 100;
const ALBUMS_KEPT: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artist {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Album {
    pub id: String,
    pub uri: String,
    pub name: String,
    pub artist: String,
    pub artist_id: String,
    pub year: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub uri: String,
    pub name: String,
    /// The track's artists, comma separated (on a compilation, not the album's).
    pub artist: String,
    pub duration: Duration,
}

/// An album search hit: the album, all its artists and Spotify's album type
/// (`album`, `single` or `compilation`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoundAlbum {
    pub album: Album,
    pub artists: Vec<String>,
    pub album_type: String,
}

/// A hit from searching for a genre seed (`deck genre add`): an artist or a track. For
/// an artist, `name` is the artist's name and `artists` contains only that.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoundSeed {
    pub uri: String,
    pub name: String,
    pub artists: Vec<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SearchResults {
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub songs: Vec<Song>,
}

/// A song search hit: the track and the album it is on, which plays on from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Song {
    pub track: Track,
    pub album: Album,
}

#[derive(Clone)]
pub struct Catalog {
    auth: Auth,
    http: reqwest::Client,
    shared: Arc<Shared>,
}

/// Memory shared by clones: responses and the 429 block.
struct Shared {
    searches: Memo<SearchResults>,
    artist_albums: Memo<Vec<Album>>,
    album_tracks: Memo<Vec<Track>>,
    rate_limit: RateLimit,
}

#[derive(Clone)]
enum Auth {
    /// The librespot session token (login5), from the session shared with the player.
    Session(SharedSession),
    App(Arc<AppAuth>),
}

impl Catalog {
    /// Prepares Web API authentication. With the user's own app, the first start
    /// opens the browser.
    pub async fn connect(session: SharedSession, config: &Config) -> Result<Self> {
        let auth = match &config.client_id {
            Some(client_id) => Auth::App(Arc::new(AppAuth::connect(client_id).await?)),
            None => Auth::Session(session),
        };
        Self::new(auth)
    }

    /// Signs in without a librespot session or browser, using the saved Web API
    /// token (`~/.cache/deck/webapi.json`). Without it, returns an error telling the
    /// user to start `deck` once.
    pub async fn connect_stored(config: &Config) -> Result<Self> {
        let Some(client_id) = &config.client_id else {
            bail!("client_id is missing from ~/.config/deck/config.toml");
        };
        Self::new(Auth::App(Arc::new(AppAuth::stored(client_id).await?)))
    }

    fn new(auth: Auth) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .context("cannot create the HTTP client")?;
        Ok(Self {
            auth,
            http,
            shared: Arc::new(Shared {
                searches: Memo::new(SEARCHES_KEPT),
                artist_albums: Memo::new(ARTISTS_KEPT),
                album_tracks: Memo::new(ALBUMS_KEPT),
                rate_limit: RateLimit::at(&RateLimit::default_path()?),
            }),
        })
    }
    /// The artist's albums. Kept in memory for the whole run.
    pub async fn artist_albums(&self, artist_id: &str) -> Result<Vec<Album>> {
        self.shared
            .artist_albums
            .get_or_fetch(artist_id, || self.fetch_artist_albums(artist_id))
            .await
    }

    async fn fetch_artist_albums(&self, artist_id: &str) -> Result<Vec<Album>> {
        let items: Vec<ApiAlbum> = self
            .get_all(
                &format!("/artists/{artist_id}/albums"),
                &[("include_groups", "album"), ("limit", "10")],
            )
            .await?;
        Ok(items.into_iter().map(Album::from).collect())
    }

    /// One album's details (name, artist, year), e.g. for playing an album that is not on
    /// the shelf (`deck ctl play-album`).
    pub async fn album(&self, album_id: &str) -> Result<Album> {
        let album: ApiAlbum = self.get(&format!("/albums/{album_id}"), &[]).await?;
        Ok(Album::from(album))
    }

    /// The album's tracks. The list does not change, so it is kept in memory: opening
    /// an album fetches it for both the view and the player with one request.
    pub async fn album_tracks(&self, album_id: &str) -> Result<Vec<Track>> {
        self.shared
            .album_tracks
            .get_or_fetch(album_id, || self.fetch_album_tracks(album_id))
            .await
    }

    async fn fetch_album_tracks(&self, album_id: &str) -> Result<Vec<Track>> {
        let items: Vec<ApiTrack> = self
            .get_all(&format!("/albums/{album_id}/tracks"), &[("limit", "50")])
            .await?;
        Ok(items.into_iter().map(Track::from).collect())
    }

    /// Whether likes work: they need the user's own app (`client_id`).
    pub fn can_like(&self) -> bool {
        matches!(self.auth, Auth::App(_))
    }

    /// Whether each track (URI) is liked, in the same order as `uris`. At most
    /// [`LIKED_BATCH`] at once.
    pub async fn liked(&self, uris: &[String]) -> Result<Vec<bool>> {
        let joined = uris.join(",");
        let flags = self
            .get("/me/library/contains", &[("uris", joined.as_str())])
            .await?;
        liked_flags(uris.len(), flags)
    }

    /// Saves the track (URI) to Liked Songs or removes it from there.
    pub async fn set_liked(&self, uri: &str, liked: bool) -> Result<()> {
        let url = format!("{API}/me/library");
        let request = if liked {
            self.http.put(url)
        } else {
            self.http.delete(url)
        };
        self.send(
            request
                .query(&[("uris", uri)])
                .header(reqwest::header::CONTENT_LENGTH, 0),
        )
        .await?;
        Ok(())
    }

    async fn token(&self) -> Result<String> {
        match &self.auth {
            Auth::Session(session) => Ok(session
                .get()
                .await?
                .login5()
                .auth_token()
                .await
                .context("could not get an access token from the session")?
                .access_token),
            Auth::App(app) => app.token().await,
        }
    }

    async fn get<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T> {
        self.fetch(self.http.get(format!("{API}{path}")).query(query))
            .await
    }

    /// Fetches all pages (following the `next` link) and returns the items as one list.
    async fn get_all<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<Vec<T>> {
        let mut page: Page<T> = self.get(path, query).await?;
        let mut items = std::mem::take(&mut page.items);
        while let Some(next) = page.next.take() {
            page = self.fetch(self.http.get(next)).await?;
            items.append(&mut page.items);
        }
        Ok(items)
    }

    async fn fetch<T: for<'de> Deserialize<'de>>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T> {
        self.send(request)
            .await?
            .json()
            .await
            .context("could not read the Spotify response")
    }

    /// Sends the request with the token. An error response is returned as an error, and
    /// nothing is retried. After a 429, from any Deck process, the request is not sent
    /// at all until the time given by Spotify has passed.
    async fn send(&self, request: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        let now = unix_now();
        if let Some(until) = self.shared.rate_limit.blocked_until(now) {
            return Err(RateLimited {
                retry_after: Some(until - now),
            }
            .into());
        }
        let response = request
            .bearer_auth(self.token().await?)
            .send()
            .await
            .context("Spotify is not responding")?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse().ok());
            let wait = retry_after.unwrap_or(DEFAULT_BLOCK);
            self.shared.rate_limit.block(unix_now() + wait);
            return Err(RateLimited { retry_after }.into());
        }
        let body = response.text().await.unwrap_or_default();
        bail!(api_error(status, &body))
    }
}
/// The Web API answered 429: Spotify blocks all of the app's requests for `retry-after`
/// seconds. A separate type so that `deck curate submit` can recognise it (`downcast_ref`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimited {
    /// The `retry-after` header in seconds, if Spotify gave one.
    pub retry_after: Option<u64>,
}

impl std::fmt::Display for RateLimited {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let wait = self.retry_after.map_or("?".to_owned(), |s| s.to_string());
        write!(
            f,
            "Spotify is rate limiting requests, try again in {wait} s"
        )
    }
}

impl std::error::Error for RateLimited {}

/// Turns a Web API error response into a message (other than 429, see [`RateLimited`]).
fn api_error(status: StatusCode, body: &str) -> String {
    if status == StatusCode::FORBIDDEN && body.contains(MISSING_SCOPE) {
        return "Deck has no access to your Liked Songs – restart Deck to sign in again".into();
    }
    format!("Spotify Web API {status}: {body}")
}

/// The `/me/library/contains` response: one boolean for each URI asked about.
fn liked_flags(asked: usize, flags: Vec<bool>) -> Result<Vec<bool>> {
    if flags.len() != asked {
        bail!("Spotify answered {} of {asked} liked songs", flags.len());
    }
    Ok(flags)
}
#[cfg(test)]
mod tests;
