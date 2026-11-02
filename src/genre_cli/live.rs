//! The real Spotify for the check: a librespot session for metadata and stations, the
//! Web API only for search.

use anyhow::{Context, Result, bail};
use librespot_core::SpotifyUri;
use librespot_metadata::{Artist as SpotifyArtist, Metadata, Track as SpotifyTrack};

use super::check::Spotify;
use crate::{
    catalog::{Catalog, FoundSeed},
    config::Config,
    player::{self, SharedSession},
    radio,
};

/// The real Spotify: a librespot session for metadata and stations, the Web API only
/// for search.
pub(super) struct Live {
    config: Config,
    session: SharedSession,
    radio: radio::Client,
    /// Opened only for the first search.
    catalog: Option<Catalog>,
}

impl Live {
    pub(super) async fn connect(config: Config) -> Result<Self> {
        let session = SharedSession::new(player::connect().await?);
        Ok(Self {
            config,
            radio: radio::Client::new(session.clone()),
            session,
            catalog: None,
        })
    }
}

impl Spotify for Live {
    async fn search(&mut self, artist: &str, track: Option<&str>) -> Result<Vec<FoundSeed>> {
        if self.catalog.is_none() {
            self.catalog = Some(Catalog::connect(self.session.clone(), &self.config).await?);
        }
        let catalog = self.catalog.as_ref().expect("connected above");
        match track {
            Some(track) => catalog.search_tracks(artist, track).await,
            None => catalog.search_artists(artist).await,
        }
    }

    async fn names(&mut self, uri: &str) -> Result<FoundSeed> {
        let parsed = SpotifyUri::from_uri(uri).context("invalid seed URI")?;
        match parsed {
            SpotifyUri::Artist { .. } => {
                let artist = SpotifyArtist::get(&self.session.get().await?, &parsed).await?;
                Ok(FoundSeed {
                    uri: uri.to_owned(),
                    artists: vec![artist.name.clone()],
                    name: artist.name,
                })
            }
            SpotifyUri::Track { .. } => {
                let track = SpotifyTrack::get(&self.session.get().await?, &parsed).await?;
                Ok(FoundSeed {
                    uri: uri.to_owned(),
                    name: track.name,
                    artists: track.artists.iter().map(|a| a.name.clone()).collect(),
                })
            }
            _ => bail!("seed must be an artist or track URI"),
        }
    }

    async fn station(&mut self, uri: &str) -> Result<Vec<radio::Track>> {
        self.radio.page(uri, &[]).await
    }

    async fn year(&mut self, album_uri: &str) -> Option<u16> {
        self.radio
            .album_for_shelf(album_uri)
            .await
            .ok()
            .and_then(|album| album.year)
    }
}
