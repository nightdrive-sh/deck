//! Genre radio: starting a station, taking in its pages, refilling the playing radio, and
//! moving on to the next seed once a station only repeats itself.

use super::*;

/// A radio refill starts when fewer tracks than this remain after the playing one.
const RADIO_REFILL: usize = 5;

/// If a refill brings fewer new tracks than this, the station has started repeating itself
/// and the radio switches seeds. In testing a station repeated itself after about 300
/// tracks, and pages could then bring only a couple of new ones at a time for a long while.
const RADIO_FRESH_MIN: usize = 10;

/// The radio keeps at most this many played tracks before the playing one: otherwise the
/// track list would grow for the whole session, and every save would copy it.
pub(super) const RADIO_HISTORY: usize = 500;

impl State {
    /// Plays the radio from track `index`. The queue is kept and plays after this track.
    pub(super) fn play_radio(&mut self, index: usize) -> Vec<Action> {
        let playback = &mut self.playback;
        let Some(radio) = &mut playback.radio else {
            return Vec::new();
        };
        let Some(track) = radio.tracks.get(index) else {
            return Vec::new();
        };
        radio.index = index;
        let item = Item::Track(track.uri.clone());
        playback.current = Some(Current::Radio);
        playback.played.clear();
        vec![Action::Play(item)]
    }

    /// Enter on a genre: starts the genre's radio from a random seed. The radio view opens
    /// once the station has been fetched. If the genre's radio is already playing, only
    /// opens the view.
    pub(super) fn start_radio(&mut self, genre: &'static Genre) -> Vec<Action> {
        if self.playing_genre() == Some(genre.id.as_str()) {
            let selected = self.playback.radio.as_ref().map_or(0, |radio| radio.index);
            self.views.push(View::Radio(RadioView {
                genre,
                selected,
                scroll: Scroll::default(),
            }));
            return Vec::new();
        }
        self.request_station(genre, None)
    }

    /// Fetches the genre's station from a seed that is not `current`.
    pub(super) fn request_station(&mut self, genre: &Genre, current: Option<&str>) -> Vec<Action> {
        let random = self.random();
        let seed = genre.pick_seed(current, &[], random).uri.clone();
        let request = RadioRequest {
            genre: genre.id.clone(),
            seed,
            previous: Vec::new(),
            kind: RadioLoad::Station,
        };
        self.radio_fetch.pending = Some(request.clone());
        vec![Action::LoadRadio(request)]
    }

    /// A station page arrived. A new station replaces the playback context and opens the
    /// radio view (with Ctrl+R the view is already open); a failed fetch changes nothing.
    pub(super) fn radio_loaded(
        &mut self,
        request: RadioRequest,
        result: Result<Vec<radio::Track>, String>,
    ) -> Vec<Action> {
        if self.radio_fetch.pending.as_ref() != Some(&request) {
            return Vec::new();
        }
        self.radio_fetch.pending = None;
        let page = match result {
            Ok(page) => page,
            Err(message) => {
                self.error = Some(message);
                if request.kind == RadioLoad::More {
                    self.radio_fetch.failed_at = self.playback.radio.as_ref().map(|r| r.index);
                }
                return Vec::new();
            }
        };
        match request.kind {
            RadioLoad::Station => self.new_station(request, page),
            RadioLoad::More => {
                let actions = self.more_radio(page);
                self.trim_radio_history();
                actions
            }
        }
    }

    fn new_station(&mut self, request: RadioRequest, page: Vec<radio::Track>) -> Vec<Action> {
        let Some(genre) = genres::find(&request.genre) else {
            return Vec::new();
        };
        let tracks = radio::new_tracks(page, &[]);
        let Some(first) = tracks.first() else {
            self.error = Some(format!("the {} radio station is empty", genre.name));
            return Vec::new();
        };
        let item = Item::Track(first.uri.clone());
        let mut actions = self
            .likes
            .check(tracks.iter().map(|track| track.uri.as_str()));
        self.error = None;
        self.radio_fetch = RadioFetch::default();
        let playback = &mut self.playback;
        playback.album = None;
        playback.played.clear();
        playback.current = Some(Current::Radio);
        playback.radio = Some(RadioPosition {
            genre: request.genre,
            seed: request.seed.clone(),
            seeds: vec![request.seed],
            seed_start: 0,
            tracks,
            index: 0,
        });
        match self.views.last_mut() {
            Some(View::Radio(view)) => {
                view.genre = genre;
                view.selected = 0;
            }
            _ => self.views.push(View::Radio(RadioView {
                genre,
                selected: 0,
                scroll: Scroll::default(),
            })),
        }
        actions.insert(0, Action::Play(item));
        actions
    }

    /// A refill page: new tracks go to the end of the radio. If little new came in, the
    /// station is repeating itself, and the next fetch uses the genre's next unplayed seed.
    fn more_radio(&mut self, page: Vec<radio::Track>) -> Vec<Action> {
        let random = self.random();
        let Some(radio) = &mut self.playback.radio else {
            return Vec::new();
        };
        let added = radio::new_tracks(page, &radio.tracks);
        let fresh = added.len();
        let actions = self
            .likes
            .check(added.iter().map(|track| track.uri.as_str()));
        radio.tracks.extend(added);
        self.error = None;
        self.radio_fetch.failed_at = None;
        self.radio_fetch.empty = if fresh == 0 {
            self.radio_fetch.empty + 1
        } else {
            0
        };
        if fresh >= RADIO_FRESH_MIN {
            return actions;
        }
        let Some(genre) = genres::find(&radio.genre) else {
            return actions;
        };
        let seed = genre
            .pick_seed(Some(&radio.seed), &radio.seeds, random)
            .uri
            .clone();
        if !radio.seeds.contains(&seed) {
            radio.seeds.push(seed.clone());
        }
        radio.seed = seed;
        radio.seed_start = radio.tracks.len();
        actions
    }

    /// Drops played tracks from the start of the radio when more than `RADIO_HISTORY`
    /// precede the playing one. Indices into the radio (the playing track, the seed's
    /// start, and the radio view's selection and scroll position) shift along with them.
    fn trim_radio_history(&mut self) {
        let Some(radio) = &mut self.playback.radio else {
            return;
        };
        let drop = radio.index.saturating_sub(RADIO_HISTORY);
        if drop == 0 {
            return;
        }
        radio.tracks.drain(..drop);
        radio.index -= drop;
        radio.seed_start = radio.seed_start.saturating_sub(drop);
        self.radio_fetch.failed_at = self
            .radio_fetch
            .failed_at
            .map(|index| index.saturating_sub(drop));
        for view in &mut self.views {
            if let View::Radio(view) = view
                && view.genre.id == radio.genre
            {
                view.selected = view.selected.saturating_sub(drop);
                view.scroll.set(view.scroll.get().saturating_sub(drop));
            }
        }
    }

    /// A refill when fewer than `RADIO_REFILL` tracks remain after the playing radio track
    /// and no fetch is already running or has failed during this track.
    pub(super) fn radio_refill(&mut self) -> Option<Action> {
        self.playback.current.as_ref()?;
        let radio = self.playback.radio.as_ref()?;
        let fetch = &self.radio_fetch;
        if fetch.pending.is_some()
            || fetch.failed_at == Some(radio.index)
            || radio.tracks.len() >= radio.index + 1 + RADIO_REFILL
        {
            return None;
        }
        let seeds = genres::find(&radio.genre).map_or(0, |genre| genre.seeds.len());
        if fetch.empty > seeds {
            return None;
        }
        let request = RadioRequest {
            genre: radio.genre.clone(),
            seed: radio.seed.clone(),
            previous: radio.tracks[radio.seed_start.min(radio.tracks.len())..]
                .iter()
                .map(|track| track.uri.clone())
                .collect(),
            kind: RadioLoad::More,
        };
        self.radio_fetch.pending = Some(request.clone());
        Some(Action::LoadRadio(request))
    }

    /// The next random number (xorshift).
    pub(super) fn random(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }
}
