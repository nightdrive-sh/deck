//! `deck taste`: the taste profile the curator reads, from Last.fm when the config has
//! it, and otherwise from what Deck itself knows and your own words in `taste.md`.

use std::{
    io,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::Serialize;

use super::{LISTENED_MIN_PLAYS, SearchCache};
use crate::{
    catalog::Album,
    config::{self, Config},
    history::{HistoryFile, Played},
    lastfm::{self, LastFm, Period, TopAlbum, TopArtist},
    lists::{CuratedFiles, same_album},
    shelf::Shelf,
    store::unix_now,
};

/// The period of the `3year` artist list in `deck taste`, in seconds.
const THREE_YEARS: u64 = 3 * 365 * 24 * 60 * 60;

/// `deck taste`: prints the taste profile as JSON.
pub async fn taste() -> Result<()> {
    let config = &Config::load()?;
    let shelf = load_shelf()?;
    let files = CuratedFiles::default_paths()?;
    let history = files.history()?;
    let played = HistoryFile::default_path()?.load();
    let notes = taste_notes(&taste_notes_path()?)?;

    let lastfm = match lastfm_client(config)? {
        Some(lastfm) => Some(LastFmTaste {
            user: lastfm_user(config)?.to_owned(),
            top_artists: TopArtists {
                overall: lastfm.top_artists(Period::Overall, 200).await?,
                three_years: lastfm
                    .top_artists_since(unix_now().saturating_sub(THREE_YEARS), 100)
                    .await?,
                twelve_months: lastfm.top_artists(Period::TwelveMonths, 100).await?,
            },
            top_albums: lastfm.top_albums(Period::Overall, 200).await?,
            listened: lastfm
                .listened_albums(&lastfm::albums_cache_path()?, LISTENED_MIN_PLAYS)
                .await?
                .iter()
                .map(|a| format!("{} – {}", a.artist, a.album))
                .collect(),
        }),
        None => None,
    };
    if lastfm.is_none() && notes.is_none() && shelf.is_empty() && played.is_empty() {
        bail!(
            "Curated has nothing to go on yet. Add Last.fm to ~/.config/deck/config.toml \
             (lastfm_api_key and lastfm_user), or write a few lines about the music you \
             like in ~/.config/deck/taste.md, or put some albums on your shelf"
        );
    }

    let profile = Taste {
        lastfm,
        notes,
        played_in_deck: played
            .iter()
            .map(|p| format!("{} – {}", p.artist, p.name))
            .collect(),
        not_on_spotify: SearchCache::load(&SearchCache::default_path()?, unix_now())
            .not_found
            .iter()
            .map(|n| format!("{} – {}", n.artist, n.album))
            .collect(),
        shelf: shelf
            .iter()
            .map(|a| ShelfAlbum {
                artist: a.artist.clone(),
                album: a.name.clone(),
                year: a.year,
            })
            .collect(),
        history: history
            .iter()
            .map(|h| PastSuggestion {
                artist: h.artist.clone(),
                album: h.name.clone(),
                round: h.round.clone(),
                rejected: h.rejected,
                on_shelf: shelf
                    .iter()
                    .any(|a| a.uri == h.uri || same_album(&a.artist, &a.name, &h.artist, &h.name)),
            })
            .collect(),
    };
    println!("{}", serde_json::to_string_pretty(&profile)?);
    Ok(())
}

/// The Last.fm user from the config. There is no default: the error says what to
/// write in the file.
pub(super) fn lastfm_user(config: &Config) -> Result<&str> {
    match config.lastfm_user.as_deref().map(str::trim) {
        Some(user) if !user.is_empty() => Ok(user),
        _ => bail!(
            "lastfm_user is missing from ~/.config/deck/config.toml. Add the line \
             lastfm_user = \"<your Last.fm username>\" (Curated needs your Last.fm \
             listening history)"
        ),
    }
}

/// Last.fm, if the config has it. Curated works without it, but half of it (a key without
/// a user, or a user without a key) is a mistake to point out.
pub(super) fn lastfm_client(config: &Config) -> Result<Option<LastFm>> {
    let given = |value: &Option<String>| value.as_deref().is_some_and(|v| !v.trim().is_empty());
    if !given(&config.lastfm_api_key) && !given(&config.lastfm_user) {
        return Ok(None);
    }
    let Some(key) = config
        .lastfm_api_key
        .as_deref()
        .filter(|k| !k.trim().is_empty())
    else {
        bail!(
            "lastfm_api_key is missing from ~/.config/deck/config.toml. Create an API \
             account at https://www.last.fm/api/account/create and add the line \
             lastfm_api_key = \"<your API key>\""
        );
    };
    Ok(Some(LastFm::new(key, lastfm_user(config)?)?))
}

/// The albums played in Deck lately (its History), as listened: without Last.fm they are
/// the albums Deck knows you have heard.
pub(super) fn played_in_deck(played: &[Played]) -> Vec<TopAlbum> {
    played
        .iter()
        .map(|p| TopAlbum {
            artist: p.artist.clone(),
            album: p.name.clone(),
            plays: LISTENED_MIN_PLAYS,
        })
        .collect()
}

/// Your own words about your taste, for the curator: Deck's setup writes them with you,
/// and you can edit them any time.
fn taste_notes_path() -> Result<PathBuf> {
    Ok(config::config_dir()?.join("taste.md"))
}

/// The notes, or `None` if there are none (no file, or an empty one).
pub(super) fn taste_notes(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text.trim().to_owned()).filter(|t| !t.is_empty())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
    }
}

/// The shelf, read-only. A broken shelf is an error: the check does not work without it.
pub(super) fn load_shelf() -> Result<Vec<Album>> {
    Ok(Shelf::load(&Shelf::default_path()?)?.albums().to_vec())
}

#[derive(Serialize)]
struct Taste {
    /// Without Last.fm in the config, left out.
    #[serde(skip_serializing_if = "Option::is_none")]
    lastfm: Option<LastFmTaste>,
    /// Your own words about your taste (`taste.md`), if you have written any.
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
    /// The albums played in Deck in the last 30 days, newest first ("Artist – Album").
    played_in_deck: Vec<String>,
    /// Candidates of earlier runs that were not found on Spotify ("Artist – Album").
    not_on_spotify: Vec<String>,
    shelf: Vec<ShelfAlbum>,
    history: Vec<PastSuggestion>,
}

#[derive(Serialize)]
struct LastFmTaste {
    user: String,
    top_artists: TopArtists,
    /// The most played albums of all time.
    top_albums: Vec<TopAlbum>,
    /// Every album with at least [`LISTENED_MIN_PLAYS`] plays ("Artist – Album"):
    /// `deck curate submit` rejects these.
    listened: Vec<String>,
}

#[derive(Serialize)]
struct TopArtists {
    overall: Vec<TopArtist>,
    #[serde(rename = "3year")]
    three_years: Vec<TopArtist>,
    #[serde(rename = "12month")]
    twelve_months: Vec<TopArtist>,
}

#[derive(Serialize)]
struct ShelfAlbum {
    artist: String,
    album: String,
    year: Option<u16>,
}

#[derive(Serialize)]
struct PastSuggestion {
    artist: String,
    album: String,
    round: String,
    rejected: bool,
    on_shelf: bool,
}
