//! Sign-in to the user's own Spotify app: the saved refresh token
//! (`~/.cache/deck/webapi.json`), its renewal, and the browser when that is not enough.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use librespot_oauth::{OAuthClient, OAuthError, OAuthToken};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::config;

/// Scopes for the user's own app: Liked Songs. Search and albums need no scopes.
const LIBRARY_SCOPES: &[&str] = &["user-library-read", "user-library-modify"];
/// Whether the token covers the library scopes.
pub(super) fn has_library_scopes(scopes: &[String]) -> bool {
    LIBRARY_SCOPES
        .iter()
        .all(|needed| scopes.iter().any(|scope| scope == needed))
}

/// The token of the user's own app. The refresh token is kept in the cache, so the
/// browser is needed only the first time.
pub(super) struct AppAuth {
    client: OAuthClient,
    token_path: PathBuf,
    token: Mutex<OAuthToken>,
}

#[derive(Serialize, Deserialize)]
struct StoredToken {
    refresh_token: String,
}

impl AppAuth {
    /// Refreshes the saved token. The browser is opened only when there is no token,
    /// Spotify rejected it or it lacks the library scopes. A network error is an error:
    /// otherwise every lost connection would needlessly open the browser.
    pub(super) async fn connect(client_id: &str) -> Result<Self> {
        let client = config::oauth_client(client_id, LIBRARY_SCOPES)?;
        let token_path = token_path()?;

        let refreshed = match refresh_stored(&client, &token_path).await? {
            Stored::Token(token) if has_library_scopes(&token.scopes) => Some(token),
            // The token was saved before likes existed: the library scopes can only be
            // had by signing in again.
            Stored::Token(token) => {
                log::warn!(
                    "the Web API token lacks the library scopes: {:?}",
                    token.scopes
                );
                eprintln!("Deck needs access to your Liked Songs.");
                None
            }
            Stored::SignInNeeded(reason) => {
                log::warn!("{reason}");
                None
            }
        };
        let token = match refreshed {
            Some(token) => token,
            None => {
                eprintln!("Sign in to the Deck Spotify app in the browser that opens…");
                client
                    .get_access_token_async()
                    .await
                    .context("Web API sign-in failed")?
            }
        };
        Self::with_token(client, token_path, token).await
    }

    /// Like `connect`, but the browser is never opened: without a working saved
    /// token, the result is an error.
    pub(super) async fn stored(client_id: &str) -> Result<Self> {
        let client = config::oauth_client(client_id, LIBRARY_SCOPES)?;
        let token_path = token_path()?;
        let token = match refresh_stored(&client, &token_path).await? {
            Stored::Token(token) => token,
            Stored::SignInNeeded(reason) => {
                bail!("{reason}: no usable Spotify sign-in, start `deck` once to sign in")
            }
        };
        Self::with_token(client, token_path, token).await
    }

    async fn with_token(
        client: OAuthClient,
        token_path: PathBuf,
        token: OAuthToken,
    ) -> Result<Self> {
        let auth = Self {
            client,
            token_path,
            token: Mutex::new(token),
        };
        auth.save(&*auth.token.lock().await)?;
        Ok(auth)
    }

    pub(super) async fn token(&self) -> Result<String> {
        let mut token = self.token.lock().await;
        if token.expires_at <= Instant::now() + Duration::from_secs(60) {
            let fresh = refresh(&self.client, token.refresh_token.clone()).await?;
            self.save(&fresh)?;
            *token = fresh;
        }
        Ok(token.access_token.clone())
    }

    fn save(&self, token: &OAuthToken) -> Result<()> {
        let stored = StoredToken {
            refresh_token: token.refresh_token.clone(),
        };
        write_private(&self.token_path, serde_json::to_string(&stored)?.as_bytes())
            .with_context(|| format!("cannot save {}", self.token_path.display()))
    }
}

/// The refresh token of the user's own app (`~/.cache/deck/webapi.json`).
fn token_path() -> Result<PathBuf> {
    Ok(config::private_cache_dir()?.join("webapi.json"))
}

/// Writes the file atomically (`.tmp` and rename), readable by the owner only
/// (0600), so that the token is never readable by others, not even for a moment.
pub(super) fn write_private(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    // A crash leftover may have other permissions, and `mode` only applies to new files.
    match fs::remove_file(&tmp) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(contents)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)
}

/// The result of refreshing the saved token when the network worked.
enum Stored {
    Token(OAuthToken),
    /// A new sign-in is needed: there is no token, the file is broken or Spotify
    /// rejected the token. Carries the reason for the log.
    SignInNeeded(String),
}

/// Reads the saved refresh token and uses it to renew the access token. An error means
/// something other than [`Stored::SignInNeeded`], e.g. a network problem.
async fn refresh_stored(client: &OAuthClient, token_path: &Path) -> Result<Stored> {
    let path = token_path.display();
    let text = match fs::read_to_string(token_path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Stored::SignInNeeded(format!("{path} is missing")));
        }
        Err(e) => return Err(e).with_context(|| format!("cannot read {path}")),
    };
    // The file is Deck's own cache, not a user file, so a broken one may be replaced
    // by signing in again.
    let stored: StoredToken = match serde_json::from_str(&text) {
        Ok(stored) => stored,
        Err(e) => return Ok(Stored::SignInNeeded(format!("{path} is broken: {e}"))),
    };
    match refresh(client, stored.refresh_token).await {
        Ok(token) => Ok(Stored::Token(token)),
        Err(e) if rejected(&e) => Ok(Stored::SignInNeeded(format!(
            "Spotify rejected the saved Web API token: {e:#}"
        ))),
        Err(e) => Err(e.context("cannot reach Spotify to renew the sign-in")),
    }
}

/// Whether Spotify rejected the refresh token (e.g. `invalid_grant`), or the request
/// failed otherwise (network, server error). librespot passes on only the text of the
/// oauth2 error, so the check is based on it: a server error response starts with
/// "Server returned error response", a network error is "Request failed" and an
/// unparseable response "Failed to parse server response". An unclear case does not
/// open the browser.
pub(super) fn rejected(error: &anyhow::Error) -> bool {
    match error.downcast_ref::<OAuthError>() {
        Some(OAuthError::ExchangeCode { e }) => {
            e.starts_with("Server returned error response")
                && !e.contains("server_error")
                && !e.contains("temporarily_unavailable")
        }
        _ => false,
    }
}

/// Renews the access token. Spotify usually returns no new refresh token, in which
/// case the old one is still valid and is kept.
async fn refresh(client: &OAuthClient, refresh_token: String) -> Result<OAuthToken> {
    let mut token = client
        .refresh_token_async(&refresh_token)
        .await
        .context("could not refresh the Web API token")?;
    if token.refresh_token.is_empty() {
        token.refresh_token = refresh_token;
    }
    Ok(token)
}
