//! The key help row at the bottom: the open view's keys, shortened to fit Deck's width.

use super::*;

/// The bottom row: `[key]` in the accent colour, the label dimmed. ↑↓ and Enter (unless
/// it plays) are left out as self-evident. Outside the search field, Ctrl keys are shown
/// as plain letters (`[f] search`, for the queue `[u] show queue`, because q quits).
/// The Ctrl+A label tells whether it adds the selected album to the shelf (or the genre
/// to home) or removes it from there, and like shows only while something is playing:
/// `like playing` or `unlike playing`, because l applies to the playing track and not
/// to the selected row.
///
/// `[n] now playing` shows when n leads somewhere, and only if the row fits with it
/// even with the longest labels, at most by leaving out quit: otherwise it would
/// appear and disappear as the selection moves. If the row does not fit, quit is left
/// out (q, Ctrl+C in the search field); if that is not enough, Space and ←→, which work
/// everywhere, are left out instead, except on the shelf, whose help always shows
/// them; as a last resort both.
///
/// The shelf's `[d] remove from home` shows only while a genre row is selected. It
/// does not affect whether `[n]` shows, so that n does not disappear as the selection
/// moves; if the row does not fit with it, Space and ←→ may be left out on the shelf
/// too.
pub(super) fn help_line(state: &State, view: &View, width: u16) -> Line<'static> {
    let entry_width = |(key, label): &(&str, &str)| key.width() + label.width() + 3;
    let total = |entries: &[(&str, &str)]| {
        entries.iter().map(entry_width).sum::<usize>() + 2 * entries.len().saturating_sub(1)
    };
    let mut entries = key_help(view).to_vec();
    let remove = if selected_on_shelf(state, view) {
        Some("remove from shelf")
    } else if selected_favorite(state, view) {
        Some("remove from home")
    } else {
        None
    };
    let playing = state
        .now_playing
        .as_ref()
        .filter(|playing| !playing.uri.is_empty());
    let like = playing.map(|playing| {
        if state.liked(&playing.uri) {
            "unlike playing"
        } else {
            "like playing"
        }
    });
    let now_playing = state.has_now_playing();
    let shelf = matches!(view, View::Shelf { .. });
    let home_row = selected_home_genre(state, view);
    entries.retain(|(key, _)| match *key {
        "Ctrl+L" => like.is_some(),
        "Ctrl+N" => now_playing,
        "Ctrl+D" if shelf => home_row,
        _ => true,
    });
    let typing = matches!(view, View::Search(view) if !view.in_results);
    for entry in &mut entries {
        match entry.0 {
            "Ctrl+A" => entry.1 = remove.unwrap_or(entry.1),
            "Ctrl+L" => entry.1 = like.unwrap_or(entry.1),
            _ => {}
        }
        if !typing {
            entry.0 = short_key(entry.0);
        }
    }
    let quit = |entry: &(&str, &str)| entry.1 == "quit";
    let home = |entry: &(&str, &str)| shelf && entry.1 == "remove from home";
    let longest: Vec<_> = entries
        .iter()
        .filter(|entry| !quit(entry) && !home(entry))
        .map(|&(key, label)| (key, longest_label(label)))
        .collect();
    if total(&longest) > usize::from(width) {
        entries.retain(|entry| entry.1 != "now playing");
    }
    if total(&entries) > usize::from(width) {
        let player =
            |entry: &(&str, &str)| (!shelf || home_row) && matches!(entry.0, "Space" | "←→");
        let without = |drop: &dyn Fn(&(&str, &str)) -> bool| -> Vec<(&'static str, &'static str)> {
            entries
                .iter()
                .copied()
                .filter(|entry| !drop(entry))
                .collect()
        };
        let both = |entry: &(&str, &str)| quit(entry) || player(entry);
        entries = [without(&quit), without(&player)]
            .into_iter()
            .find(|candidate| total(candidate) <= usize::from(width))
            .unwrap_or_else(|| without(&both));
    }
    let mut spans = Vec::new();
    for (index, (key, label)) in entries.iter().enumerate() {
        if index > 0 {
            spans.push("  ".into());
        }
        spans.push(format!("[{key}]").fg(ACCENT).bold());
        spans.push(format!(" {label}").fg(MUTED));
    }
    Line::from(spans)
}

/// Outside the search field, a Ctrl key also works as a plain letter.
fn short_key(key: &'static str) -> &'static str {
    match key {
        "Ctrl+F" => "f",
        "Ctrl+L" => "l",
        "Ctrl+N" => "n",
        "Ctrl+E" => "e",
        "Ctrl+A" => "a",
        "Ctrl+D" => "d",
        "Ctrl+R" => "r",
        "Ctrl+Q" => "u",
        key => key,
    }
}

/// Whether the album that Ctrl+A applies to is already on the shelf.
fn selected_on_shelf(state: &State, view: &View) -> bool {
    let uri = match view {
        View::Album(view) => Some(view.album.uri.as_str()),
        View::Artist(view) => view
            .albums()
            .get(view.selected)
            .map(|album| album.uri.as_str()),
        View::Search(view) => match view.items().get(view.selected) {
            Some(SearchItem::Album(album)) => Some(album.uri.as_str()),
            _ => None,
        },
        View::Curated { selected, .. } => state
            .curated()
            .and_then(|list| list.albums.get(*selected))
            .map(|entry| entry.album.uri.as_str()),
        View::Radio(view) => state
            .radio_tracks(&view.genre.id)
            .and_then(|tracks| tracks.get(view.selected))
            .map(|track| track.album.uri.as_str()),
        View::Shelf { .. } | View::Queue { .. } | View::Genres { .. } => None,
    };
    uri.is_some_and(|uri| state.on_shelf(uri))
}

/// Whether the selected shelf row is a genre on home (d removes it from home).
fn selected_home_genre(state: &State, view: &View) -> bool {
    match view {
        View::Shelf { selected, .. } => {
            matches!(state.shelf_lists().get(*selected), Some(ShelfRow::Genre(_)))
        }
        _ => false,
    }
}

/// Whether the genre selected in the Genres view is on home.
fn selected_favorite(state: &State, view: &View) -> bool {
    match view {
        View::Genres { selected, .. } => genres::all()
            .get(*selected)
            .is_some_and(|genre| state.favorite(&genre.id)),
        _ => false,
    }
}

fn key_help(view: &View) -> &'static [(&'static str, &'static str)] {
    match view {
        View::Shelf { .. } => &[
            ("Space", "pause"),
            ("←→", "track"),
            ("Ctrl+D", "remove from home"),
            ("Ctrl+N", "now playing"),
            ("Ctrl+L", "like playing"),
            ("Ctrl+F", "search"),
            ("Ctrl+Q", "show queue"),
            ("q", "quit"),
        ],
        View::Curated { .. } => &[
            ("Ctrl+E", "add to queue"),
            ("Ctrl+A", "add to shelf"),
            ("Ctrl+D", "not for me"),
            ("Ctrl+N", "now playing"),
            ("Ctrl+L", "like playing"),
            ("Esc", "back"),
        ],
        View::Artist(ArtistView {
            source: Source::Shelf(_),
            ..
        }) => &[
            ("Ctrl+E", "add to queue"),
            ("Ctrl+D", "remove from shelf"),
            ("Ctrl+N", "now playing"),
            ("Ctrl+L", "like playing"),
            ("Space", "pause"),
            ("←→", "track"),
            ("Esc", "back"),
            ("q", "quit"),
        ],
        View::Artist(_) => &[
            ("Ctrl+E", "add to queue"),
            ("Ctrl+A", "add to shelf"),
            ("Ctrl+N", "now playing"),
            ("Ctrl+L", "like playing"),
            ("Space", "pause"),
            ("←→", "track"),
            ("Esc", "back"),
            ("q", "quit"),
        ],
        View::Album(_) => &[
            ("Enter", "play from here"),
            ("Ctrl+E", "add to queue"),
            ("Ctrl+A", "add to shelf"),
            ("Ctrl+N", "now playing"),
            ("Ctrl+L", "like playing"),
            ("Space", "pause"),
            ("←→", "track"),
            ("Esc", "back"),
            ("q", "quit"),
        ],
        // In the search field letters type, so only Ctrl keys work there.
        View::Search(SearchView {
            in_results: false, ..
        }) => &[
            ("↓", "results"),
            ("Ctrl+N", "now playing"),
            ("Ctrl+L", "like playing"),
            ("Esc", "back"),
            ("Ctrl+C", "quit"),
        ],
        View::Search(_) => &[
            ("Ctrl+E", "add to queue"),
            ("Ctrl+A", "add to shelf"),
            ("Ctrl+N", "now playing"),
            ("Ctrl+L", "like playing"),
            ("Ctrl+F", "search"),
            ("Space", "pause"),
            ("←→", "track"),
            ("Esc", "back"),
            ("q", "quit"),
        ],
        View::Genres { .. } => &[
            ("Enter", "play radio"),
            ("Ctrl+A", "add to home"),
            ("Ctrl+N", "now playing"),
            ("Ctrl+L", "like playing"),
            ("Esc", "back"),
        ],
        // Enter's label is short so that the row fits in 100 columns even with the label
        // "remove from shelf".
        View::Radio(_) => &[
            ("Enter", "play"),
            ("Ctrl+E", "add to queue"),
            ("Ctrl+A", "add to shelf"),
            ("Ctrl+R", "new radio"),
            ("Ctrl+N", "now playing"),
            ("Ctrl+L", "like playing"),
            ("Esc", "back"),
        ],
        View::Queue { .. } => &[
            ("Ctrl+D", "remove"),
            ("Ctrl+N", "now playing"),
            ("Ctrl+L", "like playing"),
            ("Space", "pause"),
            ("←→", "track"),
            ("Esc", "back"),
            ("q", "quit"),
        ],
    }
}

/// The longest form of a label: remove for Ctrl+A and unlike for l.
fn longest_label(label: &'static str) -> &'static str {
    match label {
        "add to shelf" => "remove from shelf",
        "add to home" => "remove from home",
        "like playing" => "unlike playing",
        label => label,
    }
}
