//! Likes (Liked Songs): Ctrl+L on the playing track, and what is known of which tracks
//! are liked, queried in batches and kept for the session.

use super::*;

/// Ctrl+L without your own Spotify app.
pub(super) const LIKES_NEED_APP: &str =
    "liking songs needs your own Spotify app: set client_id in ~/.config/deck/config.toml";

/// Liked tracks for the session. Not saved.
#[derive(Debug, Default)]
pub(super) struct Likes {
    /// Likes need your own Spotify app (`Catalog::can_like`).
    pub(super) enabled: bool,
    /// Known state by track URI.
    known: HashMap<String, bool>,
    /// URIs asked about, including pending and failed ones: they are not asked again.
    asked: HashSet<String>,
    /// A query failed (e.g. 429): no new queries are made during the session.
    stopped: bool,
    /// Pending saves and removals: URI → track name for the confirmation.
    changing: HashMap<String, String>,
}

impl Likes {
    /// Asks about the tracks whose state is neither known nor asked, `LIKED_BATCH`
    /// at a time.
    pub(super) fn check<'a>(&mut self, uris: impl IntoIterator<Item = &'a str>) -> Vec<Action> {
        if !self.enabled || self.stopped {
            return Vec::new();
        }
        let mut new: Vec<String> = Vec::new();
        for uri in uris {
            // `insert` is false if the URI was already asked or is already in this batch.
            if uri.starts_with("spotify:track:")
                && !self.known.contains_key(uri)
                && self.asked.insert(uri.to_owned())
            {
                new.push(uri.to_owned());
            }
        }
        new.chunks(LIKED_BATCH)
            .map(|uris| Action::CheckLiked(uris.to_vec()))
            .collect()
    }
}

impl State {
    /// Enables likes: they need your own Spotify app.
    pub fn with_likes(mut self, enabled: bool) -> Self {
        self.likes.enabled = enabled;
        self
    }

    /// Whether the track (URI) is liked. An unknown state is `false`.
    pub fn liked(&self, uri: &str) -> bool {
        self.likes.known.get(uri).copied().unwrap_or(false)
    }

    /// Ctrl+L: likes the playing track or removes the like. An unknown state counts as
    /// not liked, and while a change is pending the key does nothing.
    pub(super) fn toggle_like(&mut self) -> Vec<Action> {
        let Some(playing) = self.now_playing.as_ref().filter(|p| !p.uri.is_empty()) else {
            return Vec::new();
        };
        if !self.likes.enabled {
            self.error = Some(LIKES_NEED_APP.to_owned());
            return Vec::new();
        }
        if self.likes.changing.contains_key(&playing.uri) {
            return Vec::new();
        }
        let uri = playing.uri.clone();
        let liked = !self.liked(&uri);
        self.likes
            .changing
            .insert(uri.clone(), playing.track.clone());
        vec![Action::SetLiked { uri, liked }]
    }

    pub(super) fn liked_checked(&mut self, uris: Vec<String>, result: Result<Vec<bool>, String>) {
        match result {
            Ok(flags) => {
                // A like made during the query is newer information than the answer.
                for (uri, liked) in uris.into_iter().zip(flags) {
                    self.likes.known.entry(uri).or_insert(liked);
                }
            }
            Err(message) => {
                self.likes.stopped = true;
                self.error = Some(message);
            }
        }
    }

    pub(super) fn like_changed(
        &mut self,
        uri: String,
        liked: bool,
        result: Result<(), String>,
        now: Instant,
    ) {
        let name = self.likes.changing.remove(&uri).unwrap_or_default();
        match result {
            Ok(()) => {
                self.error = None;
                self.likes.known.insert(uri, liked);
                self.notice = Some(Notice {
                    kind: if liked {
                        NoticeKind::Liked
                    } else {
                        NoticeKind::Unliked
                    },
                    name,
                    until: now + NOTICE_TIME,
                });
            }
            Err(message) => self.error = Some(message),
        }
    }
}
