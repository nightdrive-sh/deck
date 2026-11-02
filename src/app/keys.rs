//! Keys: `State::key` sends a key to the open view's handler; each view has its own.

use super::*;

/// The selected shelf row, for handling a key.
enum ShelfTarget {
    Curated,
    Genre(&'static Genre),
    Genres,
    Artist,
}

/// Ctrl+A: the album onto the shelf, or off it if already there (`shelf_uris`).
fn toggle_shelf(shelf_uris: &HashSet<String>, album: &Album) -> Vec<Action> {
    if shelf_uris.contains(&album.uri) {
        vec![Action::RemoveFromShelf(album.uri.clone())]
    } else {
        vec![Action::AddToShelf(album.clone())]
    }
}

/// Moves the selection within the list with ↑/↓. Returns `true` if the key was an arrow.
fn step(selected: &mut usize, key: Key, len: usize) -> bool {
    match key {
        Key::Up => *selected = selected.saturating_sub(1),
        Key::Down => *selected = (*selected + 1).min(len.saturating_sub(1)),
        _ => return false,
    }
    true
}

/// The byte offset of a character index in a string.
fn byte_index(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map_or(text.len(), |(i, _)| i)
}

impl State {
    pub(super) fn key(&mut self, key: Key, now: Instant) -> Vec<Action> {
        // Outside the search field, Ctrl keys also work as plain letters. The queue's
        // letter is u, because q quits.
        let typing = matches!(self.view(), View::Search(search) if !search.in_results);
        let key = match key {
            Key::Char(c @ ('f' | 'l' | 'e' | 'a' | 'd' | 'r' | 'n')) if !typing => Key::Ctrl(c),
            Key::Char('u') if !typing => Key::Ctrl('q'),
            key => key,
        };
        match key {
            Key::Ctrl('c') => return vec![Action::Quit],
            Key::Ctrl('f') => {
                self.open_search();
                return Vec::new();
            }
            Key::Ctrl('q') => {
                self.open_queue();
                return Vec::new();
            }
            Key::Ctrl('l') => return self.toggle_like(),
            Key::Ctrl('n') => return self.open_playing(),
            Key::Esc => {
                if self.views.len() > 1 {
                    self.views.pop();
                    // The shelf came into view: a new list shows up without a restart.
                    if let View::Shelf { .. } = self.view() {
                        return vec![Action::LoadLists];
                    }
                }
                return Vec::new();
            }
            _ => {}
        }
        if typing {
            return self.search_key(key, now);
        }
        match key {
            Key::Char(' ') => vec![Action::TogglePause],
            Key::Left => self.previous().into_iter().collect(),
            Key::Right => self.next().into_iter().collect(),
            Key::Char('q') => vec![Action::Quit],
            _ => match self.view() {
                View::Shelf { .. } => self.shelf_key(key),
                View::Curated { .. } => self.curated_key(key),
                View::Artist(_) => self.artist_key(key),
                View::Album(_) => self.album_key(key, now),
                View::Queue { .. } => self.queue_key(key),
                View::Genres { .. } => self.genres_key(key),
                View::Radio(_) => self.radio_key(key, now),
                View::Search(_) => self.search_results_key(key, now),
            },
        }
    }

    /// Queue view: Ctrl+D removes the selected track from the queue.
    fn queue_key(&mut self, key: Key) -> Vec<Action> {
        let queue = &mut self.playback.queue;
        let Some(View::Queue { selected, .. }) = self.views.last_mut() else {
            return Vec::new();
        };
        if key == Key::Ctrl('d') && *selected < queue.len() {
            queue.remove(*selected);
            clamp(selected, queue.len());
        } else {
            step(selected, key, queue.len());
        }
        Vec::new()
    }

    /// Shelf: Enter on a list row opens the list, on a genre starts its radio, on the
    /// Genres row opens the genre list, and on an artist opens the artist's albums. Ctrl+D
    /// on a genre removes it from the home page.
    fn shelf_key(&mut self, key: Key) -> Vec<Action> {
        let lists = self.shelf_lists();
        let target = match lists.get(self.shelf_selected()) {
            Some(ShelfRow::Curated(_)) => ShelfTarget::Curated,
            Some(ShelfRow::Genre(genre)) => ShelfTarget::Genre(genre),
            Some(ShelfRow::Genres) => ShelfTarget::Genres,
            None => ShelfTarget::Artist,
        };
        let lists = lists.len();
        let artists = self.shelf_artists.len();
        let Some(View::Shelf { selected, .. }) = self.views.last_mut() else {
            return Vec::new();
        };
        if step(selected, key, lists + artists) {
            return Vec::new();
        }
        let selected = *selected;
        match (key, target) {
            (Key::Enter, ShelfTarget::Curated) => {
                self.views.push(View::Curated {
                    selected: 0,
                    scroll: Scroll::default(),
                });
                vec![Action::LoadLists]
            }
            (Key::Enter, ShelfTarget::Genre(genre)) => self.start_radio(genre),
            (Key::Ctrl('d'), ShelfTarget::Genre(genre)) => {
                vec![Action::RemoveFavorite(genre.id.clone())]
            }
            (Key::Enter, ShelfTarget::Genres) => {
                self.views.push(View::Genres {
                    selected: 0,
                    scroll: Scroll::default(),
                });
                Vec::new()
            }
            (Key::Enter, ShelfTarget::Artist) => {
                if let Some(artist) = self.shelf_artists.get(selected - lists) {
                    let albums = albums_of(&self.shelf, &artist.id);
                    self.views.push(View::Artist(ArtistView {
                        artist: artist.clone(),
                        source: Source::Shelf(albums),
                        selected: 0,
                        scroll: Scroll::default(),
                    }));
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn shelf_selected(&self) -> usize {
        match self.view() {
            View::Shelf { selected, .. } => *selected,
            _ => 0,
        }
    }

    /// Genres: Enter starts the genre's radio and Ctrl+A adds the genre to the home page or
    /// removes it from there.
    fn genres_key(&mut self, key: Key) -> Vec<Action> {
        let all = genres::all();
        let Some(View::Genres { selected, .. }) = self.views.last_mut() else {
            return Vec::new();
        };
        if step(selected, key, all.len()) {
            return Vec::new();
        }
        let Some(genre) = all.get(*selected) else {
            return Vec::new();
        };
        match key {
            Key::Enter => self.start_radio(genre),
            Key::Ctrl('a') if self.favorite(&genre.id) => {
                vec![Action::RemoveFavorite(genre.id.clone())]
            }
            Key::Ctrl('a') => vec![Action::AddFavorite(genre.id.clone())],
            _ => Vec::new(),
        }
    }

    /// Radio view: Enter plays from the selected track, Ctrl+E adds the track to the queue,
    /// Ctrl+A adds the track's album to the shelf or removes it from there, and Ctrl+R
    /// starts the genre's radio from a new seed.
    fn radio_key(&mut self, key: Key, now: Instant) -> Vec<Action> {
        let View::Radio(view) = self.view() else {
            return Vec::new();
        };
        let genre = view.genre;
        let selected = view.selected;
        if key == Key::Ctrl('r') {
            let current = self
                .playback
                .radio
                .as_ref()
                .filter(|radio| radio.genre == genre.id)
                .map(|radio| radio.seed.clone());
            return self.request_station(genre, current.as_deref());
        }
        let tracks = self.radio_tracks(&genre.id).unwrap_or_default();
        let len = tracks.len();
        match (key, tracks.get(selected)) {
            (Key::Enter, Some(_)) => self.play_radio(selected),
            (Key::Ctrl('e'), Some(track)) => {
                let track = radio_queued(track);
                let name = track.name.clone();
                self.enqueue(vec![track], &name, now)
            }
            (Key::Ctrl('a'), Some(track)) if self.on_shelf(&track.album.uri) => {
                vec![Action::RemoveFromShelf(track.album.uri.clone())]
            }
            (Key::Ctrl('a'), Some(track)) => {
                vec![Action::AddRadioAlbumToShelf(track.album.uri.clone())]
            }
            _ => {
                if let Some(View::Radio(view)) = self.views.last_mut() {
                    step(&mut view.selected, key, len);
                }
                Vec::new()
            }
        }
    }

    /// Curated list: Enter opens the album's tracks, Ctrl+E adds the album to the queue,
    /// Ctrl+A adds it to the shelf or removes it from there, and Ctrl+D rejects it.
    fn curated_key(&mut self, key: Key) -> Vec<Action> {
        let View::Curated { selected, .. } = *self.view() else {
            return Vec::new();
        };
        let albums = self.curated.as_ref().map_or(&[][..], |list| &list.albums);
        let len = albums.len();
        let Some(album) = albums.get(selected).map(|entry| entry.album.clone()) else {
            return Vec::new();
        };
        match key {
            Key::Enter => self.open_album(album),
            Key::Ctrl('e') => vec![Action::QueueAlbum(album)],
            Key::Ctrl('a') => toggle_shelf(&self.shelf_uris, &album),
            Key::Ctrl('d') => vec![Action::RejectCurated(album.uri)],
            _ => {
                if let Some(View::Curated { selected, .. }) = self.views.last_mut() {
                    step(selected, key, len);
                }
                Vec::new()
            }
        }
    }

    fn artist_key(&mut self, key: Key) -> Vec<Action> {
        let View::Artist(view) = self.view() else {
            return Vec::new();
        };
        let albums = view.albums();
        let len = albums.len();
        let from_shelf = matches!(view.source, Source::Shelf(_));
        let Some(album) = albums.get(view.selected) else {
            return Vec::new();
        };
        match key {
            Key::Enter => self.open_album(album.clone()),
            Key::Ctrl('e') => vec![Action::QueueAlbum(album.clone())],
            Key::Ctrl('d') if from_shelf => vec![Action::RemoveFromShelf(album.uri.clone())],
            Key::Ctrl('a') => toggle_shelf(&self.shelf_uris, album),
            _ => {
                if let Some(View::Artist(view)) = self.views.last_mut() {
                    step(&mut view.selected, key, len);
                }
                Vec::new()
            }
        }
    }

    /// Track list: Enter plays the album from the selected track, Ctrl+E adds the track to
    /// the queue, and Ctrl+A adds the album to the shelf or removes it from there.
    fn album_key(&mut self, key: Key, now: Instant) -> Vec<Action> {
        let View::Album(view) = self.view() else {
            return Vec::new();
        };
        let tracks = view.tracks.as_deref().unwrap_or_default();
        let len = tracks.len();
        match key {
            Key::Enter if view.selected < len => {
                let album = view.album.clone();
                let track = view.selected;
                self.play_album(album, len, track)
            }
            Key::Ctrl('e') if view.selected < len => {
                let track = queued(&view.album, tracks, view.selected);
                let name = track.name.clone();
                self.enqueue(vec![track], &name, now)
            }
            // A radio track's album (n from the queue) has no year: when adding it to the
            // shelf, the year and the album's artist are fetched as in the radio view.
            Key::Ctrl('a') if view.album.year.is_none() && !self.on_shelf(&view.album.uri) => {
                vec![Action::AddRadioAlbumToShelf(view.album.uri.clone())]
            }
            Key::Ctrl('a') => toggle_shelf(&self.shelf_uris, &view.album),
            _ => {
                if let Some(View::Album(view)) = self.views.last_mut() {
                    step(&mut view.selected, key, len);
                }
                Vec::new()
            }
        }
    }

    /// A key in the search field.
    fn search_key(&mut self, key: Key, now: Instant) -> Vec<Action> {
        let Some(View::Search(search)) = self.views.last_mut() else {
            return Vec::new();
        };
        match key {
            Key::Char(c) => {
                let at = byte_index(&search.query, search.cursor);
                search.query.insert(at, c);
                search.cursor += 1;
                search.edited(now);
            }
            Key::Backspace if search.cursor > 0 => {
                search.cursor -= 1;
                let at = byte_index(&search.query, search.cursor);
                search.query.remove(at);
                search.edited(now);
            }
            Key::Left => search.cursor = search.cursor.saturating_sub(1),
            Key::Right => search.cursor = (search.cursor + 1).min(search.query.chars().count()),
            Key::Down | Key::Enter if !search.items().is_empty() => {
                search.in_results = true;
                search.selected = 0;
            }
            _ => {}
        }
        Vec::new()
    }

    /// A key in the search results; the keys every view shares are handled already.
    fn search_results_key(&mut self, key: Key, now: Instant) -> Vec<Action> {
        let Some(View::Search(search)) = self.views.last_mut() else {
            return Vec::new();
        };
        match key {
            Key::Up if search.selected == 0 => search.in_results = false,
            Key::Up | Key::Down => {
                let len = search.items().len();
                step(&mut search.selected, key, len);
            }
            Key::Enter => match search.items().get(search.selected) {
                Some(SearchItem::Artist(artist)) => {
                    let artist = (*artist).clone();
                    let action = Action::LoadArtistAlbums(artist.id.clone());
                    self.views.push(View::Artist(ArtistView {
                        artist,
                        source: Source::Spotify(None),
                        selected: 0,
                        scroll: Scroll::default(),
                    }));
                    return vec![action];
                }
                Some(SearchItem::Album(album)) => {
                    let album = (*album).clone();
                    return self.open_album(album);
                }
                Some(SearchItem::Song(song)) => {
                    let song = (*song).clone();
                    return self.play_song(song);
                }
                None => {}
            },
            Key::Ctrl('a') => match search.items().get(search.selected) {
                Some(SearchItem::Album(album)) => return toggle_shelf(&self.shelf_uris, album),
                Some(SearchItem::Song(song)) => {
                    return toggle_shelf(&self.shelf_uris, &song.album);
                }
                _ => {}
            },
            Key::Ctrl('e') => match search.items().get(search.selected) {
                Some(SearchItem::Album(album)) => {
                    return vec![Action::QueueAlbum((*album).clone())];
                }
                Some(SearchItem::Song(song)) => {
                    let track = song_queued(song);
                    let name = track.name.clone();
                    return self.enqueue(vec![track], &name, now);
                }
                _ => {}
            },
            _ => {}
        }
        Vec::new()
    }
}
