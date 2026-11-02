//! The views on the view stack and their own state: selection, scroll position, search
//! text and results.

use std::{cell::Cell, time::Instant};

use crate::{
    catalog::{Album, Artist, SearchResults, Song, Track},
    genres::Genre,
    lists::AlbumList,
};

use super::SEARCH_DEBOUNCE;

/// Every view has a list whose scroll position (`Scroll`) is part of the view's state:
/// when you return to the view, the list is where you left it.
#[derive(Debug)]
pub enum View {
    /// The selection moves over the list rows and artists: lists first, then artists.
    Shelf {
        selected: usize,
        scroll: Scroll,
    },
    /// The Curated list. The albums are in the state (`State::curated`).
    Curated {
        selected: usize,
        scroll: Scroll,
    },
    Artist(ArtistView),
    Search(SearchView),
    Album(AlbumView),
    Queue {
        selected: usize,
        scroll: Scroll,
    },
    /// Genres: every genre in the catalog.
    Genres {
        selected: usize,
        scroll: Scroll,
    },
    Radio(RadioView),
}

/// A list's scroll position: the topmost visible row. The list only scrolls when the
/// selection would go past the visible area, so the position depends on the previous draw
/// and the height of the area. That is why the drawing (`ui`) updates it, not
/// `State::update`, and why it is a `Cell` in the `&State` the drawing gets.
#[derive(Debug, Default)]
pub struct Scroll(Cell<usize>);

impl Scroll {
    pub fn get(&self) -> usize {
        self.0.get()
    }

    pub fn set(&self, offset: usize) {
        self.0.set(offset);
    }
}

/// A genre's radio. The tracks are in the playback context (`State::radio_tracks`) if the
/// genre's radio is playing.
#[derive(Debug)]
pub struct RadioView {
    pub genre: &'static Genre,
    pub selected: usize,
    pub scroll: Scroll,
}

/// A list row above the shelf.
#[derive(Debug, PartialEq, Eq)]
pub enum ShelfRow<'a> {
    /// `None`: there is no list yet (Curated is not set up) or its file is broken.
    Curated(Option<&'a AlbumList>),
    Genre(&'static Genre),
    Genres,
}

#[derive(Debug)]
pub struct ArtistView {
    pub artist: Artist,
    pub source: Source,
    pub selected: usize,
    pub scroll: Scroll,
}

#[derive(Debug)]
pub enum Source {
    /// The artist's albums on the shelf by year; updated when the shelf changes.
    Shelf(Vec<Album>),
    /// All of the artist's albums from Spotify; `None` while loading.
    Spotify(Option<Vec<Album>>),
}

/// An album's tracks. Opens with Enter on an album; playback starts only with Enter
/// on a track.
#[derive(Debug)]
pub struct AlbumView {
    pub album: Album,
    /// `None` while the tracks are still being fetched.
    pub tracks: Option<Vec<Track>>,
    pub selected: usize,
    pub scroll: Scroll,
    /// Opened with n when the playing track's position is unknown (queued from a radio):
    /// the selection moves to this track (URI) when the tracks arrive.
    pub(super) select_uri: Option<String>,
}

#[derive(Debug, Default)]
pub struct SearchView {
    pub query: String,
    /// Cursor position in characters.
    pub cursor: usize,
    pub results: Option<SearchResults>,
    /// Selection in the result list: artists first, then albums, then songs.
    pub selected: usize,
    pub scroll: Scroll,
    /// Keys go to the result list instead of the search field: ↓ or Enter moves there,
    /// ↑ on the first row or f goes back. Only then is a row selected.
    pub in_results: bool,
    pub(super) due: Option<Instant>,
    /// The last search sent; only its results are accepted.
    pub(super) requested: Option<String>,
    pub(super) pending: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SearchItem<'a> {
    Artist(&'a Artist),
    Album(&'a Album),
    Song(&'a Song),
}

impl ArtistView {
    /// The artist view's albums by year, oldest first.
    pub fn albums(&self) -> &[Album] {
        match &self.source {
            Source::Shelf(albums) => albums,
            Source::Spotify(albums) => albums.as_deref().unwrap_or_default(),
        }
    }
}

impl SearchView {
    /// The results as one list: artists first, then albums, then songs.
    pub fn items(&self) -> Vec<SearchItem<'_>> {
        let Some(results) = &self.results else {
            return Vec::new();
        };
        results
            .artists
            .iter()
            .map(SearchItem::Artist)
            .chain(results.albums.iter().map(SearchItem::Album))
            .chain(results.songs.iter().map(SearchItem::Song))
            .collect()
    }

    /// The search is waiting for the delay or for results.
    pub fn searching(&self) -> bool {
        self.due.is_some() || self.pending
    }

    /// The search text changed: search after the delay; empty text clears the results.
    pub(super) fn edited(&mut self, now: Instant) {
        if self.query.trim().is_empty() {
            *self = Self {
                query: std::mem::take(&mut self.query),
                cursor: self.cursor,
                ..Self::default()
            };
        } else {
            self.due = Some(now + SEARCH_DEBOUNCE);
        }
    }
}
