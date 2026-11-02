//! Searching: artists, albums and songs for the UI, and the narrow album, artist and
//! track searches of `deck curate submit` and `deck genre add`.

use anyhow::Result;

use super::{
    Album, Catalog, FoundAlbum, FoundSeed, SearchResults,
    api::{AlbumSearchPage, ArtistSearchPage, SearchPage, TrackSearchPage, search_results},
};

impl Catalog {
    /// Searches for artists, albums and songs, in one request. The result for the same
    /// query comes from memory, so that e.g. backspace does not search again.
    pub async fn search(&self, query: &str) -> Result<SearchResults> {
        self.shared
            .searches
            .get_or_fetch(query, || self.fetch_search(query))
            .await
    }

    async fn fetch_search(&self, query: &str) -> Result<SearchResults> {
        let page: SearchPage = self
            .get(
                "/search",
                &[
                    ("q", query),
                    ("type", "artist,album,track"),
                    ("limit", "10"),
                ],
            )
            .await?;
        Ok(search_results(page))
    }

    /// Searches for an album by name and artist (an `album:` + `artist:` search). Hits
    /// come in Spotify's order and are not filtered.
    pub async fn search_albums(&self, artist: &str, album: &str) -> Result<Vec<FoundAlbum>> {
        let query = format!("album:{album} artist:{artist}");
        let page: AlbumSearchPage = self
            .get(
                "/search",
                &[("q", query.as_str()), ("type", "album"), ("limit", "10")],
            )
            .await?;
        Ok(page
            .albums
            .items
            .into_iter()
            .map(|a| FoundAlbum {
                artists: a.artists.iter().map(|ar| ar.name.clone()).collect(),
                album_type: a.album_type.clone().unwrap_or_default(),
                album: Album::from(a),
            })
            .collect())
    }

    /// Searches for an artist by name. Hits come in Spotify's order.
    pub async fn search_artists(&self, artist: &str) -> Result<Vec<FoundSeed>> {
        let page: ArtistSearchPage = self
            .get(
                "/search",
                &[("q", artist), ("type", "artist"), ("limit", "10")],
            )
            .await?;
        Ok(page
            .artists
            .items
            .into_iter()
            .map(|a| FoundSeed {
                uri: format!("spotify:artist:{}", a.id),
                artists: vec![a.name.clone()],
                name: a.name,
            })
            .collect())
    }

    /// Searches for a track by name and artist (a `track:` + `artist:` search). Hits
    /// come in Spotify's order.
    pub async fn search_tracks(&self, artist: &str, track: &str) -> Result<Vec<FoundSeed>> {
        let query = format!("track:{track} artist:{artist}");
        let page: TrackSearchPage = self
            .get(
                "/search",
                &[("q", query.as_str()), ("type", "track"), ("limit", "10")],
            )
            .await?;
        Ok(page
            .tracks
            .items
            .into_iter()
            .map(|t| FoundSeed {
                uri: t.uri,
                name: t.name,
                artists: t.artists.into_iter().map(|a| a.name).collect(),
            })
            .collect())
    }
}
