//! Opening views on the view stack: an album, the playing album or radio (n), the search
//! and the queue.

use super::*;

/// Where n leads (`State::playing_target`).
enum PlayingTarget {
    /// The album's track list. The track's position, or its URI if the position is missing.
    Album {
        album: Album,
        index: Option<usize>,
        uri: String,
    },
    Radio {
        genre: &'static Genre,
        index: usize,
    },
}

impl State {
    /// Opens the album that `play_on_load` waits for, with `play_from`'s track selected: it plays
    /// from the selected track when the tracks arrive.
    pub(super) fn open_album_to_play(&mut self, album: Album) -> Vec<Action> {
        let actions = self.open_album(album);
        if let Some(View::Album(view)) = self.views.last_mut() {
            view.select_uri = self.play_from.take();
        }
        actions
    }

    /// Opens the album's track list without playing it. The tracks arrive as
    /// `Event::AlbumTracks`.
    pub(super) fn open_album(&mut self, album: Album) -> Vec<Action> {
        let action = Action::LoadAlbumTracks(album.uri.clone());
        self.views.push(View::Album(AlbumView {
            album,
            tracks: None,
            selected: 0,
            scroll: Scroll::default(),
            select_uri: None,
        }));
        vec![action]
    }

    /// Where n leads: the playing track's album or radio. `None` when nothing is playing or
    /// the album details are missing from a restored old state.
    fn playing_target(&self) -> Option<PlayingTarget> {
        let playback = &self.playback;
        // An album from old saved state can still be found on the shelf.
        let album = |album: Option<&Album>, uri: &str| {
            album
                .or_else(|| self.shelf.iter().find(|a| a.uri == uri))
                .cloned()
        };
        match playback.current.as_ref()? {
            Current::Album => {
                let position = playback.album.as_ref()?;
                Some(PlayingTarget::Album {
                    album: album(position.album.as_ref(), &position.uri)?,
                    index: Some(position.index),
                    uri: String::new(),
                })
            }
            Current::Radio => {
                let radio = playback.radio.as_ref()?;
                Some(PlayingTarget::Radio {
                    genre: genres::find(&radio.genre)?,
                    index: radio.index,
                })
            }
            Current::Queued(track) => Some(PlayingTarget::Album {
                album: album(track.album.as_deref(), &track.album_uri)?,
                index: track.index,
                uri: track.uri.clone(),
            }),
        }
    }

    /// Whether n leads anywhere (`[n] now playing` in the help row).
    pub fn has_now_playing(&self) -> bool {
        self.playing_target().is_some()
    }

    /// n: opens the playing album's track list or the playing radio, selecting the playing
    /// track. If it is already visible, only moves the selection; Esc returns to where n
    /// was pressed.
    pub(super) fn open_playing(&mut self) -> Vec<Action> {
        match self.playing_target() {
            None => Vec::new(),
            Some(PlayingTarget::Radio { genre, index }) => {
                match self.views.last_mut() {
                    Some(View::Radio(view)) if view.genre.id == genre.id => view.selected = index,
                    _ => self.views.push(View::Radio(RadioView {
                        genre,
                        selected: index,
                        scroll: Scroll::default(),
                    })),
                }
                Vec::new()
            }
            Some(PlayingTarget::Album { album, index, uri }) => {
                if let Some(View::Album(view)) = self.views.last_mut()
                    && view.album.uri == album.uri
                {
                    view.select_uri = None;
                    match (index, &view.tracks) {
                        (Some(index), Some(tracks)) => {
                            view.selected = index;
                            clamp(&mut view.selected, tracks.len());
                        }
                        (Some(index), None) => view.selected = index,
                        (None, Some(tracks)) => {
                            if let Some(index) = tracks.iter().position(|t| t.uri == uri) {
                                view.selected = index;
                            }
                        }
                        (None, None) => view.select_uri = Some(uri),
                    }
                    return Vec::new();
                }
                let action = Action::LoadAlbumTracks(album.uri.clone());
                self.views.push(View::Album(AlbumView {
                    album,
                    tracks: None,
                    selected: index.unwrap_or(0),
                    scroll: Scroll::default(),
                    select_uri: index.is_none().then_some(uri),
                }));
                vec![action]
            }
        }
    }

    /// Opens search in the search field. If search is already on the stack, returns to
    /// it (the text and results are kept).
    pub(super) fn open_search(&mut self) {
        match self.views.iter().position(|v| matches!(v, View::Search(_))) {
            Some(index) => {
                self.views.truncate(index + 1);
                if let Some(search) = search_view(&mut self.views) {
                    search.in_results = false;
                }
            }
            None => self.views.push(View::Search(SearchView::default())),
        }
    }

    /// Opens the queue view. If it is already on the stack, returns to it.
    pub(super) fn open_queue(&mut self) {
        match self
            .views
            .iter()
            .position(|v| matches!(v, View::Queue { .. }))
        {
            Some(index) => self.views.truncate(index + 1),
            None => self.views.push(View::Queue {
                selected: 0,
                scroll: Scroll::default(),
            }),
        }
    }
}
