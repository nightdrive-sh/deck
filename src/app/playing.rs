//! What comes from outside the keyboard: the player's events, media keys and `deck ctl`,
//! and what the system's Now Playing shows.

use super::*;

impl NowPlaying {
    /// Elapsed time: the player's latest position plus the time passed since.
    pub fn elapsed(&self, now: Instant) -> Duration {
        if self.paused {
            return self.position;
        }
        (self.position + now.saturating_duration_since(self.position_at)).min(self.duration)
    }
}

impl State {
    /// The playing track for Now Playing; `None` when nothing is playing.
    pub fn now_playing_info(&self, now: Instant) -> Option<now_playing::Info> {
        let playing = self.now_playing.as_ref()?;
        Some(now_playing::Info {
            title: playing.track.clone(),
            artist: playing.artist.clone(),
            album: playing.album.clone(),
            duration_ms: millis(playing.duration),
            position_ms: millis(playing.elapsed(now)),
            playing: !playing.paused,
            cover: playing.cover.clone(),
            liked: (self.likes.enabled && !playing.uri.is_empty())
                .then(|| self.liked(&playing.uri)),
            track_uri: (!playing.uri.is_empty()).then(|| playing.uri.clone()),
            album_uri: match self.playback.current.as_ref() {
                Some(Current::Album) => self.playback.album.as_ref().map(|a| a.uri.clone()),
                Some(Current::Queued(track)) => Some(track.album_uri.clone()),
                Some(Current::Radio) | None => None,
            },
            queued: matches!(self.playback.current, Some(Current::Queued(_))),
            queue: self
                .playback
                .queue
                .iter()
                .map(|track| now_playing::QueuedInfo {
                    uri: track.uri.clone(),
                    title: track.name.clone(),
                    artist: track.artist.clone(),
                    duration_ms: millis(track.duration),
                    album_uri: track.album_uri.clone(),
                })
                .collect(),
            radio: self.radio_info(),
            error: self.error.clone(),
        })
    }

    /// The playing radio for Now Playing: its tracks from the playing one on.
    fn radio_info(&self) -> Option<now_playing::RadioInfo> {
        let radio = self.playback.radio.as_ref()?;
        let genre = genres::find(&radio.genre)?;
        Some(now_playing::RadioInfo {
            genre: genre.id.clone(),
            name: genre.name.clone(),
            tracks: radio
                .tracks
                .iter()
                .skip(radio.index)
                .map(|track| now_playing::RadioTrackInfo {
                    uri: track.uri.clone(),
                    title: track.name.clone(),
                    artist: track.artist.clone(),
                    album: track.album.name.clone(),
                    album_uri: track.album.uri.clone(),
                })
                .collect(),
        })
    }

    /// A jump in the playing track, shown at once; the player confirms it with its position.
    pub(super) fn seek(&mut self, position: Duration, now: Instant) -> Vec<Action> {
        let Some(playing) = &mut self.now_playing else {
            return Vec::new();
        };
        let position = position.min(playing.duration);
        playing.position = position;
        playing.position_at = now;
        vec![Action::Seek(position)]
    }

    pub(super) fn media(&mut self, command: MediaCommand) -> Vec<Action> {
        let paused = self.now_playing.as_ref().map(|playing| playing.paused);
        match command {
            MediaCommand::Toggle => vec![Action::TogglePause],
            MediaCommand::Play if paused == Some(true) => vec![Action::TogglePause],
            MediaCommand::Pause if paused == Some(false) => vec![Action::TogglePause],
            MediaCommand::Play | MediaCommand::Pause => Vec::new(),
            MediaCommand::Next => self.next().into_iter().collect(),
            MediaCommand::Previous => self.previous().into_iter().collect(),
            MediaCommand::ToggleLike => self.toggle_like(),
        }
    }

    /// `deck ctl play-album <uri>`: opens the album and plays it from the first track once the
    /// tracks have arrived. The details come from the shelf or Curated, otherwise from Spotify.
    pub(super) fn play_album_remote(&mut self, uri: String) -> Vec<Action> {
        let curated = self.curated.as_ref().map_or(&[][..], |list| &list.albums);
        let album = self
            .shelf
            .iter()
            .find(|album| album.uri == uri)
            .cloned()
            .or_else(|| {
                let entry = curated.iter().find(|entry| entry.album.uri == uri)?;
                Some(entry.album.clone())
            });
        self.play_on_load = Some(uri.clone());
        match album {
            Some(album) => self.open_album_to_play(album),
            // Not on the shelf or in Curated: fetch its details first (`album_fetched`).
            None => vec![Action::FetchAlbum(uri)],
        }
    }

    /// `Event::AlbumFetched`: an album asked for by `deck ctl play-album` opens, and it plays
    /// when its tracks arrive (`album_tracks`).
    pub(super) fn album_fetched(
        &mut self,
        uri: &str,
        result: Result<Album, String>,
    ) -> Vec<Action> {
        if self.play_on_load.as_deref() != Some(uri) {
            return Vec::new();
        }
        match result {
            Ok(album) => self.open_album_to_play(album),
            Err(message) => {
                self.play_on_load = None;
                self.play_from = None;
                self.error = Some(message);
                Vec::new()
            }
        }
    }

    /// `deck ctl search <query>`: opens the search view with the query and searches right away,
    /// without the typing debounce.
    pub(super) fn search_remote(&mut self, query: &str) -> Vec<Action> {
        let query = query.trim().to_owned();
        if query.is_empty() {
            return Vec::new();
        }
        self.open_search();
        let Some(search) = search_view(&mut self.views) else {
            return Vec::new();
        };
        search.cursor = query.chars().count();
        search.query = query.clone();
        search.in_results = false;
        search.due = None;
        search.requested = Some(query.clone());
        search.pending = true;
        vec![Action::Search(query)]
    }

    pub(super) fn player(&mut self, event: player::Event, now: Instant) -> Vec<Action> {
        match event {
            player::Event::TrackChanged {
                uri,
                artist,
                album,
                track,
                duration,
                cover,
            } => {
                // Usually the state has already been asked along with the track list or
                // radio; e.g. the tracks of a restored album have not.
                let actions = self.likes.check([uri.as_str()]);
                self.now_playing = Some(NowPlaying {
                    uri,
                    artist,
                    album,
                    track,
                    duration,
                    cover,
                    paused: false,
                    position: Duration::ZERO,
                    position_at: now,
                });
                // Loading the restored track is not a user action: a startup error
                // (e.g. a broken shelf) stays visible.
                if !self.restoring {
                    self.error = None;
                }
                return actions;
            }
            player::Event::EndOfTrack => {
                if let Some(action) = self.next() {
                    return vec![action];
                }
                self.stopped();
                return vec![Action::Stop];
            }
            player::Event::PreloadNext => {
                return self.upcoming().map(Action::Preload).into_iter().collect();
            }
            player::Event::Position(ms) => {
                self.restoring = false;
                if let Some(playing) = &mut self.now_playing {
                    playing.position = Duration::from_millis(ms.into());
                    playing.position_at = now;
                    playing.paused = false;
                }
            }
            player::Event::Paused(ms) => {
                if let Some(playing) = &mut self.now_playing {
                    playing.position = Duration::from_millis(ms.into()).min(playing.duration);
                    playing.position_at = now;
                    playing.paused = true;
                }
                if self.restoring {
                    self.restoring = false;
                } else {
                    self.error = None;
                }
            }
            player::Event::Stopped => self.stopped(),
            // The restored track could not be loaded (network, removed album): Deck
            // continues empty.
            player::Event::Error(message) if self.restoring => {
                self.restoring = false;
                self.playback = Playback::default();
                self.now_playing = None;
                self.error = Some(format!("could not restore playback: {message}"));
            }
            player::Event::Error(message) => self.error = Some(message),
        }
        Vec::new()
    }
}
