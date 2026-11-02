//! The Web API's JSON responses, and how they become Deck's own types.

use std::time::Duration;

use serde::Deserialize;

use super::{Album, Artist, SearchResults, Song, Track};

/// A search response as artists, albums and songs.
pub(super) fn search_results(page: SearchPage) -> SearchResults {
    SearchResults {
        artists: page
            .artists
            .items
            .into_iter()
            .map(|a| Artist {
                id: a.id,
                name: a.name,
            })
            .collect(),
        albums: page.albums.items.into_iter().map(Album::from).collect(),
        songs: page
            .tracks
            .map(|tracks| tracks.items)
            .unwrap_or_default()
            .into_iter()
            .map(|t| Song {
                track: Track::from(ApiTrack {
                    uri: t.uri,
                    name: t.name,
                    artists: t.artists,
                    duration_ms: t.duration_ms,
                }),
                album: Album::from(t.album),
            })
            .collect(),
    }
}

#[derive(Deserialize)]
pub(super) struct SearchPage {
    pub(super) artists: Page<ApiArtist>,
    pub(super) albums: Page<ApiAlbum>,
    /// Missing from a saved response of an older Deck, which searched artists and albums only.
    #[serde(default)]
    pub(super) tracks: Option<Page<ApiSearchTrack>>,
}

/// A track as search finds it: with its album.
#[derive(Deserialize)]
pub(super) struct ApiSearchTrack {
    pub(super) uri: String,
    pub(super) name: String,
    pub(super) artists: Vec<ApiArtist>,
    pub(super) duration_ms: u64,
    pub(super) album: ApiAlbum,
}

#[derive(Deserialize)]
pub(super) struct ArtistSearchPage {
    pub(super) artists: Page<ApiArtist>,
}

#[derive(Deserialize)]
pub(super) struct TrackSearchPage {
    pub(super) tracks: Page<ApiTrack>,
}

#[derive(Deserialize)]
pub(super) struct AlbumSearchPage {
    pub(super) albums: Page<ApiAlbum>,
}

#[derive(Deserialize)]
pub(super) struct Page<T> {
    pub(super) items: Vec<T>,
    pub(super) next: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct ApiArtist {
    pub(super) id: String,
    pub(super) name: String,
}

#[derive(Deserialize)]
pub(super) struct ApiAlbum {
    pub(super) id: String,
    pub(super) uri: String,
    pub(super) name: String,
    pub(super) artists: Vec<ApiArtist>,
    pub(super) release_date: Option<String>,
    pub(super) album_type: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct ApiTrack {
    pub(super) uri: String,
    pub(super) name: String,
    pub(super) artists: Vec<ApiArtist>,
    pub(super) duration_ms: u64,
}

impl From<ApiTrack> for Track {
    fn from(t: ApiTrack) -> Self {
        Self {
            uri: t.uri,
            name: t.name,
            artist: t
                .artists
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            duration: Duration::from_millis(t.duration_ms),
        }
    }
}

impl From<ApiAlbum> for Album {
    fn from(a: ApiAlbum) -> Self {
        let (artist, artist_id) = a
            .artists
            .into_iter()
            .next()
            .map(|ar| (ar.name, ar.id))
            .unwrap_or_default();
        Self {
            year: a
                .release_date
                .as_deref()
                .and_then(|d| d.get(..4))
                .and_then(|y| y.parse().ok()),
            id: a.id,
            uri: a.uri,
            name: a.name,
            artist,
            artist_id,
        }
    }
}
