//! Play order: an album from a track, Pick for me, the next and previous track, and the
//! queue, which plays after the current track before the album continues.

use super::*;

/// Where the next track comes from (`State::upcoming_source`).
enum Upcoming {
    Queue,
    /// An album track.
    Album(usize),
    /// A radio track.
    Radio(usize),
}

/// Album track `index`, to be added to the queue.
pub(super) fn queued(album: &Album, tracks: &[Track], index: usize) -> QueuedTrack {
    let track = &tracks[index];
    QueuedTrack {
        uri: track.uri.clone(),
        name: track.name.clone(),
        artist: track.artist.clone(),
        duration: track.duration,
        album_uri: album.uri.clone(),
        index: Some(index),
        album: Some(Box::new(album.clone())),
    }
}

/// A song from the search to be added to the queue. Its place on the album is not known.
pub(super) fn song_queued(song: &Song) -> QueuedTrack {
    QueuedTrack {
        uri: song.track.uri.clone(),
        name: song.track.name.clone(),
        artist: song.track.artist.clone(),
        duration: song.track.duration,
        album_uri: song.album.uri.clone(),
        index: None,
        album: Some(Box::new(song.album.clone())),
    }
}

/// A radio track to be added to the queue. The station does not report the duration, so
/// it is zero (the queue view leaves it out).
pub(super) fn radio_queued(track: &radio::Track) -> QueuedTrack {
    QueuedTrack {
        uri: track.uri.clone(),
        name: track.name.clone(),
        artist: track.artist.clone(),
        duration: Duration::ZERO,
        album_uri: track.album.uri.clone(),
        index: None,
        album: Some(Box::new(Album {
            id: spotify_id(&track.album.uri),
            uri: track.album.uri.clone(),
            name: track.album.name.clone(),
            artist: track.artist.clone(),
            artist_id: spotify_id(&track.artist_uri),
            year: None,
        })),
    }
}

impl State {
    /// Plays an album (`len` tracks) from track `track`. The queue is kept and plays after
    /// this track.
    pub(super) fn play_album(&mut self, album: Album, len: usize, track: usize) -> Vec<Action> {
        let uri = album.uri.clone();
        // Every album that starts, picked or not, counts as played for Pick for me.
        if !self.picked.contains(&uri) {
            self.picked.push(uri.clone());
            self.picked_dirty = true;
        }
        // An album replaces the radio, including a pending fetch.
        self.radio_fetch = RadioFetch::default();
        let playback = &mut self.playback;
        playback.current = Some(Current::Album);
        playback.played.clear();
        playback.radio = None;
        playback.album = Some(AlbumPosition {
            uri: uri.clone(),
            len,
            index: track,
            album: Some(album),
        });
        vec![Action::Play(Item::Album { uri, track })]
    }

    /// Pick for me: an album from the shelf or Curated (or both) that hasn't played in this
    /// round and isn't playing now, which then plays as `deck ctl play-album` would.
    pub(super) fn pick(&mut self, from: PickFrom) -> Vec<Action> {
        let shelf = self.shelf.iter().map(|album| album.uri.clone());
        let curated = self
            .curated
            .iter()
            .flat_map(|list| list.albums.iter().map(|entry| entry.album.uri.clone()));
        let mut candidates: Vec<String> = match from {
            PickFrom::Shelf => shelf.collect(),
            PickFrom::Curated => curated.collect(),
            PickFrom::Any => shelf.chain(curated).collect(),
        };
        candidates.dedup();
        let playing = match self.playback.current.as_ref() {
            Some(Current::Album) => self.playback.album.as_ref().map(|a| a.uri.clone()),
            Some(Current::Queued(track)) => Some(track.album_uri.clone()),
            Some(Current::Radio) | None => None,
        };
        let random = self.random();
        let Some(uri) = picks::choose(&candidates, &mut self.picked, playing.as_deref(), random)
        else {
            self.error = Some(match from {
                PickFrom::Curated => "nothing to pick from: there is no Curated list".into(),
                _ => "nothing to pick from: your shelf is empty".into(),
            });
            return Vec::new();
        };
        // A new round may have taken the source's albums out of the history.
        self.picked_dirty = true;
        let album = self
            .shelf
            .iter()
            .chain(
                self.curated
                    .iter()
                    .flat_map(|list| list.albums.iter().map(|e| &e.album)),
            )
            .find(|album| album.uri == uri)
            .cloned();
        album.map(Action::PickAlbum).into_iter().collect()
    }

    /// `Event::AlbumPicked`: the picked album's tracks go to the front of the queue, all of
    /// them, and the first plays now; what was queued plays after the album.
    pub(super) fn album_picked(
        &mut self,
        album: &Album,
        result: Result<Vec<Track>, String>,
    ) -> Vec<Action> {
        let tracks = match result {
            Ok(tracks) if !tracks.is_empty() => tracks,
            Ok(_) => {
                self.error = Some(format!("{} has no tracks", album.name));
                return Vec::new();
            }
            Err(message) => {
                self.error = Some(message);
                return Vec::new();
            }
        };
        self.error = None;
        self.learn_length(&album.uri, tracks.len());
        if !self.picked.contains(&album.uri) {
            self.picked.push(album.uri.clone());
            self.picked_dirty = true;
        }
        let mut actions = self.likes.check(tracks.iter().map(|t| t.uri.as_str()));
        let queued: Vec<QueuedTrack> = (0..tracks.len())
            .map(|i| queued(album, &tracks, i))
            .collect();
        self.playback.queue.splice(0..0, queued);
        // Nothing playing: the queue's first track starts as `next` would start it.
        let play = if self.playback.current.is_some() {
            self.next()
        } else {
            let track = self.playback.queue.remove(0);
            let item = Item::Track(track.uri.clone());
            self.playback.current = Some(Current::Queued(track));
            Some(Action::Play(item))
        };
        actions.splice(0..0, play);
        actions
    }

    /// Where the next track comes from: the queue first, then the album or radio where it
    /// left off. `None` if nothing is playing or there is nothing left to play.
    fn upcoming_source(&self) -> Option<Upcoming> {
        let playback = &self.playback;
        playback.current.as_ref()?;
        if !playback.queue.is_empty() {
            return Some(Upcoming::Queue);
        }
        if let Some(album) = &playback.album {
            return (album.index + 1 < album.len).then_some(Upcoming::Album(album.index + 1));
        }
        let radio = playback.radio.as_ref()?;
        (radio.index + 1 < radio.tracks.len()).then_some(Upcoming::Radio(radio.index + 1))
    }

    pub(super) fn upcoming(&self) -> Option<Item> {
        let playback = &self.playback;
        Some(match self.upcoming_source()? {
            Upcoming::Queue => Item::Track(playback.queue[0].uri.clone()),
            Upcoming::Album(track) => Item::Album {
                uri: playback.album.as_ref()?.uri.clone(),
                track,
            },
            Upcoming::Radio(index) => {
                Item::Track(playback.radio.as_ref()?.tracks[index].uri.clone())
            }
        })
    }

    /// → and the end of a track: moves to the next one (`upcoming`).
    pub(super) fn next(&mut self) -> Option<Action> {
        let source = self.upcoming_source()?;
        let item = self.upcoming()?;
        let playback = &mut self.playback;
        match source {
            Upcoming::Queue => {
                let track = playback.queue.remove(0);
                if let Some(Current::Queued(previous)) = playback.current.take() {
                    playback.played.push(previous);
                }
                playback.current = Some(Current::Queued(track));
            }
            Upcoming::Album(track) => {
                if let Some(album) = &mut playback.album {
                    album.index = track;
                }
                playback.played.clear();
                playback.current = Some(Current::Album);
            }
            Upcoming::Radio(index) => {
                if let Some(radio) = &mut playback.radio {
                    radio.index = index;
                }
                playback.played.clear();
                playback.current = Some(Current::Radio);
            }
        }
        Some(Action::Play(item))
    }

    /// ←: on an album or radio, the previous track (the first one starts over). From a
    /// queue track, goes back to the previous queue track, or to the album or radio track
    /// after which the queue started, and the track goes back to the front of the queue.
    pub(super) fn previous(&mut self) -> Option<Action> {
        let playback = &mut self.playback;
        // The playback state changes only once the previous track has been found.
        let item = match playback.current.as_ref()? {
            Current::Album => {
                let album = playback.album.as_mut()?;
                album.index = album.index.saturating_sub(1);
                Item::Album {
                    uri: album.uri.clone(),
                    track: album.index,
                }
            }
            Current::Radio => {
                let radio = playback.radio.as_mut()?;
                let index = radio.index.saturating_sub(1);
                let item = Item::Track(radio.tracks.get(index)?.uri.clone());
                radio.index = index;
                item
            }
            Current::Queued(_) => {
                // Every branch below sets the playing track again.
                let Some(Current::Queued(track)) = playback.current.take() else {
                    return None;
                };
                if let Some(previous) = playback.played.pop() {
                    playback.queue.insert(0, track);
                    let item = Item::Track(previous.uri.clone());
                    playback.current = Some(Current::Queued(previous));
                    item
                } else if let Some(album) = &playback.album {
                    playback.queue.insert(0, track);
                    playback.current = Some(Current::Album);
                    Item::Album {
                        uri: album.uri.clone(),
                        track: album.index,
                    }
                } else if let Some(radio_track) = playback
                    .radio
                    .as_ref()
                    .and_then(|radio| radio.tracks.get(radio.index))
                {
                    let item = Item::Track(radio_track.uri.clone());
                    playback.queue.insert(0, track);
                    playback.current = Some(Current::Radio);
                    item
                } else {
                    // The queue started from nothing: the first track starts over.
                    let item = Item::Track(track.uri.clone());
                    playback.current = Some(Current::Queued(track));
                    item
                }
            }
        };
        Some(Action::Play(item))
    }

    /// Adds the tracks to the end of the queue and shows a confirmation. If nothing is
    /// playing, the queue starts playing.
    pub(super) fn enqueue(
        &mut self,
        tracks: Vec<QueuedTrack>,
        name: &str,
        now: Instant,
    ) -> Vec<Action> {
        if tracks.is_empty() {
            return Vec::new();
        }
        self.playback.queue.extend(tracks);
        self.notice = Some(Notice {
            kind: NoticeKind::Queued,
            name: name.to_owned(),
            until: now + NOTICE_TIME,
        });
        if self.playback.current.is_some() {
            return Vec::new();
        }
        let track = self.playback.queue.remove(0);
        let action = Action::Play(Item::Track(track.uri.clone()));
        self.playback.current = Some(Current::Queued(track));
        vec![action]
    }

    /// Playback ended (album and queue played, or the player stopped).
    pub(super) fn stopped(&mut self) {
        let playback = &mut self.playback;
        playback.current = None;
        playback.album = None;
        playback.radio = None;
        playback.played.clear();
        self.now_playing = None;
    }

    /// A song from the search: its album opens with the song selected, and plays from it when
    /// the tracks arrive (`album_tracks`), so the album goes on after the song.
    pub(super) fn play_song(&mut self, song: Song) -> Vec<Action> {
        self.play_on_load = Some(song.album.uri.clone());
        self.play_from = Some(song.track.uri);
        self.open_album_to_play(song.album)
    }
}
