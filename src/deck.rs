//! [`Deck`] carries out the actions `State::update` returns: player, Web API, radio,
//! shelf, favourite genres and lists.

use anyhow::Result;
use jiff::Timestamp;
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    app::{self, Action},
    catalog::Catalog,
    genres::Favorites,
    history, listened,
    lists::{AlbumList, CuratedFiles},
    now_playing, picks,
    playback::PlaybackFile,
    player::{Command, Player},
    radio,
    shelf::Shelf,
};

/// Carries out actions: player, Web API, radio, shelf, favourite genres and lists.
/// Results return to the loop as events through the `results` channel.
pub(super) struct Deck {
    pub(super) player: Player,
    pub(super) catalog: Catalog,
    pub(super) radio: radio::Client,
    pub(super) shelf: Shelf,
    pub(super) favorites: Favorites,
    pub(super) lists: CuratedFiles,
    pub(super) playback: PlaybackFile,
    pub(super) picked: picks::PickedFile,
    pub(super) listened: listened::ListenedFile,
    pub(super) history: history::HistoryFile,
    pub(super) results: UnboundedSender<app::Event>,
    /// macOS Now Playing; `None` elsewhere or if the helper process did not start.
    pub(super) media: Option<now_playing::Session>,
}

impl Deck {
    pub(super) fn perform(&mut self, action: Action) {
        match action {
            Action::Play(item) => self.player.send(Command::Play(item)),
            Action::Preload(item) => self.player.send(Command::Preload(item)),
            Action::Restore { item, position } => {
                self.player.send(Command::Restore { item, position });
            }
            Action::TogglePause => self.player.send(Command::TogglePause),
            Action::Seek(position) => self.player.send(Command::Seek(position)),
            Action::Stop => self.player.send(Command::Stop),
            Action::Search(query) => {
                let catalog = self.catalog.clone();
                self.spawn(async move {
                    let result = catalog
                        .search(&query)
                        .await
                        .map_err(failed("search failed"));
                    app::Event::Searched { query, result }
                });
            }
            Action::LoadArtistAlbums(artist_id) => {
                let catalog = self.catalog.clone();
                self.spawn(async move {
                    let result = catalog
                        .artist_albums(&artist_id)
                        .await
                        .map_err(failed("could not load the artist's albums"));
                    app::Event::ArtistAlbums { artist_id, result }
                });
            }
            Action::FetchAlbum(uri) => {
                let catalog = self.catalog.clone();
                self.spawn(async move {
                    let id = uri.strip_prefix("spotify:album:").unwrap_or(&uri);
                    let result = catalog
                        .album(id)
                        .await
                        .map_err(failed("could not load the album"));
                    app::Event::AlbumFetched { uri, result }
                });
            }
            Action::LoadAlbumTracks(uri) => {
                let catalog = self.catalog.clone();
                self.spawn(async move {
                    let id = uri.strip_prefix("spotify:album:").unwrap_or(&uri);
                    let result = catalog
                        .album_tracks(id)
                        .await
                        .map_err(failed("could not load the album's tracks"));
                    app::Event::AlbumTracks { uri, result }
                });
            }
            Action::QueueAlbum(album) => {
                let catalog = self.catalog.clone();
                self.spawn(async move {
                    let result = catalog
                        .album_tracks(&album.id)
                        .await
                        .map_err(failed("could not queue the album"));
                    app::Event::AlbumQueued {
                        album,
                        track: None,
                        result,
                    }
                });
            }
            Action::RecordHistory { uri, name, artist } => {
                let at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                let played = history::Played {
                    uri,
                    name,
                    artist,
                    at,
                };
                if let Err(e) = self.history.record(played) {
                    log::warn!("{e:#}");
                }
            }
            Action::PickAlbum(album) => {
                let catalog = self.catalog.clone();
                self.spawn(async move {
                    let result = catalog
                        .album_tracks(&album.id)
                        .await
                        .map_err(failed("could not play the picked album"));
                    app::Event::AlbumPicked { album, result }
                });
            }
            Action::QueueRemote { album: uri, track } => {
                let catalog = self.catalog.clone();
                self.spawn(async move {
                    let id = uri
                        .strip_prefix("spotify:album:")
                        .unwrap_or(&uri)
                        .to_owned();
                    let what = if track.is_some() {
                        "could not queue the song"
                    } else {
                        "could not queue the album"
                    };
                    let album = match catalog.album(&id).await {
                        Ok(album) => album,
                        Err(e) => return app::Event::Error(failed(what)(e)),
                    };
                    let result = catalog.album_tracks(&id).await.map_err(failed(what));
                    app::Event::AlbumQueued {
                        album,
                        track,
                        result,
                    }
                });
            }
            Action::AddToShelf(album) => {
                let result = self.shelf.add(album);
                self.shelf_changed(result);
            }
            Action::RemoveFromShelf(uri) => {
                let result = self.shelf.remove(&uri);
                self.shelf_changed(result);
            }
            Action::LoadLists => {
                let _ = self
                    .results
                    .send(app::Event::ListsLoaded(curated_list(&self.lists)));
            }
            Action::RejectCurated(uri) => {
                let _ = self.results.send(reject_curated(&self.lists, &uri));
            }
            Action::LoadRadio(request) => {
                let radio = self.radio.clone();
                self.spawn(async move {
                    let result = radio
                        .page(&request.seed, &request.previous)
                        .await
                        .map_err(failed("could not load the radio"));
                    app::Event::RadioLoaded { request, result }
                });
            }
            Action::AddRadioAlbumToShelf(uri) => {
                let radio = self.radio.clone();
                self.spawn(async move {
                    let result = radio
                        .album_for_shelf(&uri)
                        .await
                        .map_err(failed("could not add the album to the shelf"));
                    app::Event::RadioAlbum(result)
                });
            }
            Action::AddFavorite(id) => {
                let result = self.favorites.add(&id);
                self.favorites_changed(result);
            }
            Action::RemoveFavorite(id) => {
                let result = self.favorites.remove(&id);
                self.favorites_changed(result);
            }
            Action::CheckLiked(uris) => {
                let catalog = self.catalog.clone();
                self.spawn(async move {
                    let result = catalog
                        .liked(&uris)
                        .await
                        .map_err(failed("could not check Liked Songs"));
                    app::Event::LikedChecked { uris, result }
                });
            }
            Action::SetLiked { uri, liked } => {
                let catalog = self.catalog.clone();
                self.spawn(async move {
                    let result = catalog
                        .set_liked(&uri, liked)
                        .await
                        .map_err(failed("could not change Liked Songs"));
                    app::Event::LikeChanged { uri, liked, result }
                });
            }
            Action::SavePlayback { path, playback } => {
                if let Err(e) = PlaybackFile::at(path).save(playback.as_deref()) {
                    log::warn!("deck ctl save: {e:#}");
                }
            }
            Action::Quit => {}
        }
    }

    /// Runs a task in the background and sends its result back to the loop as an event.
    fn spawn(&self, task: impl Future<Output = app::Event> + Send + 'static) {
        let results = self.results.clone();
        tokio::spawn(async move {
            let _ = results.send(task.await);
        });
    }

    /// Saves the playback state, or removes it when nothing is playing.
    pub(super) fn save_playback(&self, playback: Option<app::SavedPlayback>) {
        if let Err(e) = self.playback.save(playback.as_ref()) {
            log::warn!("{e:#}");
            let _ = self.results.send(app::Event::Error(format!("{e:#}")));
        }
    }

    fn favorites_changed(&self, result: Result<bool>) {
        let event = match result {
            Ok(_) => app::Event::FavoritesChanged(favorite_ids(&self.favorites)),
            Err(e) => app::Event::Error(format!("{e:#}")),
        };
        let _ = self.results.send(event);
    }

    fn shelf_changed(&self, result: Result<bool>) {
        let event = match result {
            Ok(_) => app::Event::ShelfChanged(self.shelf.albums().to_vec()),
            Err(e) => app::Event::Error(format!("{e:#}")),
        };
        let _ = self.results.send(event);
    }
}

/// An error as a UI message: `<what was attempted>: <cause>`.
fn failed(what: &'static str) -> impl Fn(anyhow::Error) -> String {
    move |e| format!("{what}: {e:#}")
}

/// IDs of the favourite genres for the view (catalog genres).
pub(super) fn favorite_ids(favorites: &Favorites) -> Vec<String> {
    favorites.genres().iter().map(|g| g.id.clone()).collect()
}

/// The Curated list for the view. A broken file is an error.
pub(super) fn curated_list(files: &CuratedFiles) -> Result<Option<AlbumList>, String> {
    files
        .load()
        .map(|curated| curated.map(|curated| curated.to_list(Timestamp::now())))
        .map_err(|e| format!("{e:#}"))
}

/// Rejects an album: removes it from `curated.json` and marks it in the history.
fn reject_curated(files: &CuratedFiles, uri: &str) -> app::Event {
    match files.reject(uri) {
        Ok(curated) => {
            app::Event::CuratedChanged(curated.map(|curated| curated.to_list(Timestamp::now())))
        }
        Err(e) => app::Event::Error(format!("{e:#}")),
    }
}
#[cfg(test)]
mod tests;
