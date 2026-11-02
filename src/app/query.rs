//! What the drawing (`ui`) and the event loop read from the state: the lists, the radio,
//! the queue, the visible view and the playing track's place.

use super::*;

impl State {
    /// The Curated list for the list view, even when empty.
    pub fn curated(&self) -> Option<&AlbumList> {
        self.curated.as_ref()
    }

    /// The album's (URI) reason, if the album is on the current Curated list.
    pub fn curated_reason(&self, album_uri: &str) -> Option<&str> {
        self.curated
            .as_ref()?
            .albums
            .iter()
            .find(|entry| entry.album.uri == album_uri)?
            .reason
            .as_deref()
    }

    /// Whether the list file could not be read (the error is shown on the top row).
    pub fn curated_broken(&self) -> bool {
        self.lists_broken
    }

    /// The list rows shown above the shelf: always Curated first (without a list it opens
    /// the setup instructions), favourite genres in alphabetical order, and always Genres
    /// last.
    pub fn shelf_lists(&self) -> Vec<ShelfRow<'_>> {
        [ShelfRow::Curated(self.curated.as_ref())]
            .into_iter()
            .chain(self.favorite_genres().into_iter().map(ShelfRow::Genre))
            .chain([ShelfRow::Genres])
            .collect()
    }

    pub fn favorite(&self, id: &str) -> bool {
        self.favorites.iter().any(|f| f == id)
    }

    /// Favourite genres from the catalog by name. Unknown ids are skipped.
    pub fn favorite_genres(&self) -> Vec<&'static Genre> {
        genres::all()
            .iter()
            .filter(|g| self.favorite(&g.id))
            .collect()
    }

    /// The playing radio's genre (id), also when paused and while the queue plays.
    pub fn playing_genre(&self) -> Option<&str> {
        self.playback.current.as_ref()?;
        self.playback
            .radio
            .as_ref()
            .map(|radio| radio.genre.as_str())
    }

    /// The genre's radio tracks, if it is the playback context.
    pub fn radio_tracks(&self, genre: &str) -> Option<&[radio::Track]> {
        self.playback
            .radio
            .as_ref()
            .filter(|radio| radio.genre == genre)
            .map(|radio| radio.tracks.as_slice())
    }

    /// The playing radio track's position in the genre's radio (not when from the queue).
    pub fn playing_radio_track(&self, genre: &str) -> Option<usize> {
        self.now_playing.as_ref()?;
        let radio = self.playback.radio.as_ref()?;
        (self.playback.current == Some(Current::Radio) && radio.genre == genre)
            .then_some(radio.index)
    }

    /// The shelf's selectable rows: lists and artists.
    pub(super) fn shelf_rows(&self) -> usize {
        self.shelf_lists().len() + self.shelf_artists.len()
    }

    /// The queue's tracks in play order (the playing track is not included).
    pub fn queue(&self) -> &[QueuedTrack] {
        &self.playback.queue
    }

    /// The latest confirmation and its name, until `NOTICE_TIME` has passed.
    pub fn notice(&self, now: Instant) -> Option<(NoticeKind, &str)> {
        self.notice
            .as_ref()
            .filter(|notice| now < notice.until)
            .map(|notice| (notice.kind, notice.name.as_str()))
    }

    /// The visible view.
    pub fn view(&self) -> &View {
        self.views.last().expect("the view stack is never empty")
    }

    /// The shelf's artists in alphabetical order.
    pub fn shelf_artists(&self) -> &[Artist] {
        &self.shelf_artists
    }

    /// Whether the album (URI) is on the shelf.
    pub fn on_shelf(&self, uri: &str) -> bool {
        self.shelf_uris.contains(uri)
    }

    /// The playing track's position in the album's (URI) track list, if it plays from the
    /// album or from the queue.
    pub fn playing_track(&self, album_uri: &str) -> Option<usize> {
        self.now_playing.as_ref()?;
        let playback = &self.playback;
        match playback.current.as_ref()? {
            Current::Album => playback
                .album
                .as_ref()
                .filter(|album| album.uri == album_uri)
                .map(|album| album.index),
            Current::Radio => None,
            Current::Queued(track) => track.index.filter(|_| track.album_uri == album_uri),
        }
    }
}
