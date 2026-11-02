//! Results of earlier actions coming back as events: search results, track lists, an
//! artist's albums, the shelf and the lists, and the tick that starts a delayed search.

use super::*;

impl State {
    /// Sends the search once typing has paused.
    pub(super) fn tick(&mut self, now: Instant) -> Vec<Action> {
        let Some(search) = search_view(&mut self.views) else {
            return Vec::new();
        };
        match search.due {
            Some(due) if now >= due => {
                search.due = None;
                let query = search.query.trim().to_owned();
                if search.requested.as_ref() == Some(&query) {
                    return Vec::new();
                }
                search.requested = Some(query.clone());
                search.pending = true;
                vec![Action::Search(query)]
            }
            _ => Vec::new(),
        }
    }

    pub(super) fn searched(&mut self, query: String, result: Result<SearchResults, String>) {
        let Some(search) = search_view(&mut self.views) else {
            return;
        };
        if !search.pending || search.requested.as_ref() != Some(&query) {
            return;
        }
        search.pending = false;
        match result {
            Ok(results) => {
                search.results = Some(results);
                search.selected = 0;
                search.in_results &= !search.items().is_empty();
                self.error = None;
            }
            Err(message) => {
                search.requested = None;
                self.error = Some(message);
            }
        }
    }

    pub(super) fn album_tracks(
        &mut self,
        uri: &str,
        result: Result<Vec<Track>, String>,
    ) -> Vec<Action> {
        if let Ok(tracks) = &result {
            self.learn_length(uri, tracks.len());
        }
        let mut actions = match &result {
            Ok(tracks) => self
                .likes
                .check(tracks.iter().map(|track| track.uri.as_str())),
            Err(_) => Vec::new(),
        };
        for view in &mut self.views {
            if let View::Album(view) = view
                && view.album.uri == uri
            {
                let tracks = result.clone().unwrap_or_default();
                if let Some(uri) = view.select_uri.take()
                    && let Some(index) = tracks.iter().position(|track| track.uri == uri)
                {
                    view.selected = index;
                }
                clamp(&mut view.selected, tracks.len());
                view.tracks = Some(tracks);
            }
        }
        if self.play_on_load.as_deref() == Some(uri) {
            self.play_on_load = None;
            let album = self.views.iter().rev().find_map(|view| match view {
                View::Album(view) if view.album.uri == uri => {
                    Some((view.album.clone(), view.selected))
                }
                _ => None,
            });
            if let (Ok(tracks), Some((album, track))) = (&result, album)
                && !tracks.is_empty()
            {
                actions.extend(self.play_album(album, tracks.len(), track));
            }
        }
        match result {
            Ok(_) => self.error = None,
            Err(message) => self.error = Some(message),
        }
        actions
    }

    /// The album's tracks arrived, to be added to the queue.
    pub(super) fn album_queued(
        &mut self,
        album: &Album,
        only: Option<&str>,
        result: Result<Vec<Track>, String>,
        now: Instant,
    ) -> Vec<Action> {
        match result {
            Ok(tracks) => {
                self.error = None;
                self.learn_length(&album.uri, tracks.len());
                let indices: Vec<usize> = (0..tracks.len())
                    .filter(|&index| only.is_none_or(|uri| tracks[index].uri == uri))
                    .collect();
                if indices.is_empty() {
                    self.error = Some("could not queue the song: it is not on its album".into());
                    return Vec::new();
                }
                let mut actions = self
                    .likes
                    .check(indices.iter().map(|&index| tracks[index].uri.as_str()));
                // A song is confirmed by its own name, an album by the album's.
                let name = match only {
                    Some(_) => tracks[indices[0]].name.clone(),
                    None => album.name.clone(),
                };
                let tracks = indices
                    .into_iter()
                    .map(|index| queued(album, &tracks, index))
                    .collect();
                actions.splice(0..0, self.enqueue(tracks, &name, now));
                actions
            }
            Err(message) => {
                self.error = Some(message);
                Vec::new()
            }
        }
    }

    pub(super) fn artist_albums(&mut self, artist_id: &str, result: Result<Vec<Album>, String>) {
        for view in &mut self.views {
            if let View::Artist(ArtistView {
                artist,
                source: source @ Source::Spotify(None),
                ..
            }) = view
                && artist.id == artist_id
            {
                let albums = match &result {
                    Ok(albums) => {
                        let mut albums = albums.clone();
                        sort_by_year(&mut albums);
                        albums
                    }
                    Err(_) => Vec::new(),
                };
                *source = Source::Spotify(Some(albums));
            }
        }
        match result {
            Ok(_) => self.error = None,
            Err(message) => self.error = Some(message),
        }
    }

    /// The shelf changed: an artist view whose last album was removed is closed, and
    /// selections are kept within their lists.
    pub(super) fn shelf_changed(&mut self, albums: Vec<Album>) {
        self.set_shelf(albums);
        self.error = None;
        let shelf = &self.shelf;
        for view in &mut self.views {
            if let View::Artist(ArtistView {
                artist,
                source: Source::Shelf(albums),
                ..
            }) = view
            {
                *albums = albums_of(shelf, &artist.id);
            }
        }
        self.views.retain(|view| match view {
            View::Artist(
                view @ ArtistView {
                    source: Source::Shelf(_),
                    ..
                },
            ) => !view.albums().is_empty(),
            _ => true,
        });
        let rows = self.shelf_rows();
        for view in &mut self.views {
            match view {
                View::Shelf { selected, .. } => clamp(selected, rows),
                View::Artist(view) => {
                    let len = view.albums().len();
                    clamp(&mut view.selected, len);
                }
                View::Curated { .. }
                | View::Search(_)
                | View::Album(_)
                | View::Queue { .. }
                | View::Genres { .. }
                | View::Radio(_) => {}
            }
        }
    }

    /// The lists were read: a broken file is shown as an error and its list row is hidden.
    pub(super) fn lists_loaded(&mut self, result: Result<Option<AlbumList>, String>) {
        match result {
            Ok(list) => {
                if self.lists_broken {
                    self.error = None;
                    self.lists_broken = false;
                }
                self.set_curated(list);
            }
            Err(message) => {
                self.error = Some(message);
                self.lists_broken = true;
                self.set_curated(None);
            }
        }
    }

    /// Replaces the Curated list. The list view's selection stays within the list.
    pub(super) fn set_curated(&mut self, list: Option<AlbumList>) {
        self.set_lists(|state| state.curated = list);
        let len = self.curated.as_ref().map_or(0, |list| list.albums.len());
        for view in &mut self.views {
            if let View::Curated { selected, .. } = view {
                clamp(selected, len);
            }
        }
    }

    /// Changes the list rows (Curated or favourite genres). The shelf's selection stays on
    /// the same artist even as list rows appear or disappear. On a list row the selection
    /// stays at the same position, so the next row takes the removed row's place.
    pub(super) fn set_lists(&mut self, change: impl FnOnce(&mut Self)) {
        let before = self.shelf_lists().len();
        change(self);
        let after = self.shelf_lists().len();
        let rows = self.shelf_rows();
        for view in &mut self.views {
            if let View::Shelf { selected, .. } = view {
                if *selected >= before {
                    *selected = *selected - before + after;
                }
                clamp(selected, rows);
            }
        }
    }
}
