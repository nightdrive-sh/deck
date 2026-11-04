//! The rough genre of each Curated album ("jazz", "soul"), in coarse words
//! ([`crate::coarse_genre`]). The curator gives each candidate a genre (`genre` in
//! `deck curate submit`'s input), since Spotify no longer gives its own apps the
//! artists' genres. `deck curate genres` fills them into a list written before that,
//! from the artists' Last.fm tags. Either way they are kept in `curated.json`, so each
//! list is looked up once.

use std::{collections::HashMap, time::Duration};

use anyhow::{Result, bail};

use super::taste::lastfm_client;
use crate::{
    coarse_genre::coarse,
    config::Config,
    lists::{CuratedFiles, ListAlbum},
};

/// Pause between Last.fm requests, to stay well within its limits.
const LASTFM_PAUSE: Duration = Duration::from_millis(250);

/// The curator's genre as coarse words: "soft rock, jazz" is read as two genres.
pub(super) fn from_curator(genre: &str) -> Vec<String> {
    let genres: Vec<String> = genre
        .split([',', '/', ';'])
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map(str::to_owned)
        .collect();
    words(&genres)
}

fn words(genres: &[String]) -> Vec<String> {
    coarse(genres).into_iter().map(str::to_owned).collect()
}

/// The artists (id and name) of the albums that have no genre yet, each once, in list
/// order.
pub(super) fn artists_without_genres(albums: &[ListAlbum]) -> Vec<(String, String)> {
    let mut artists: Vec<(String, String)> = Vec::new();
    for entry in albums.iter().filter(|e| e.genres.is_empty()) {
        let album = &entry.album;
        if !album.artist_id.is_empty() && !artists.iter().any(|(id, _)| *id == album.artist_id) {
            artists.push((album.artist_id.clone(), album.artist.clone()));
        }
    }
    artists
}

/// `deck curate genres`: the genres of the current list's albums that have none, from
/// their artists' Last.fm tags. An artist whose tags give no coarse word stays without.
pub async fn fill_current() -> Result<()> {
    let files = CuratedFiles::default_paths()?;
    let Some(curated) = files.load()? else {
        println!("No Curated list yet");
        return Ok(());
    };
    let artists = artists_without_genres(&curated.albums);
    if artists.is_empty() {
        println!("Every album on the list has its genre");
        return Ok(());
    }
    let Some(lastfm) = lastfm_client(&Config::load()?)? else {
        bail!("deck curate genres needs Last.fm: lastfm_api_key in ~/.config/deck/config.toml");
    };
    let mut by_artist = HashMap::new();
    for (index, (id, name)) in artists.iter().enumerate() {
        if index > 0 {
            tokio::time::sleep(LASTFM_PAUSE).await;
        }
        let found = words(&lastfm.artist_tags(name).await?);
        if !found.is_empty() {
            by_artist.insert(id.clone(), found);
        }
    }
    files.set_genres(&by_artist)?;
    let known = curated
        .albums
        .iter()
        .filter(|e| !e.genres.is_empty() || by_artist.contains_key(&e.album.artist_id))
        .count();
    println!("{known} of {} albums have a genre", curated.albums.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Album;

    fn album(artist_id: &str, genres: &[&str]) -> ListAlbum {
        ListAlbum {
            album: Album {
                id: "a".into(),
                uri: "spotify:album:a".into(),
                name: "A".into(),
                artist: format!("Artist {artist_id}"),
                artist_id: artist_id.into(),
                year: None,
            },
            reason: None,
            genres: genres.iter().map(|g| g.to_string()).collect(),
        }
    }

    #[test]
    fn the_curators_genre_is_split_and_made_coarse() {
        assert_eq!(from_curator("vocal jazz"), ["jazz"]);
        assert_eq!(from_curator("soft rock, jazz"), ["rock", "jazz"]);
        assert_eq!(from_curator(" neo soul / r&b "), ["soul"]);
        assert_eq!(from_curator("indie / dream pop"), ["indie", "dream pop"]);
        assert!(from_curator("").is_empty());
    }

    #[test]
    fn only_albums_without_genres_are_looked_up_and_each_artist_once() {
        let albums = [
            album("x", &[]),
            album("y", &["jazz"]),
            album("x", &[]),
            album("", &[]),
            album("z", &[]),
        ];
        let ids: Vec<String> = artists_without_genres(&albums)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(ids, ["x", "z"]);
        assert_eq!(artists_without_genres(&albums)[0].1, "Artist x");
    }
}
