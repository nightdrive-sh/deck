//! The views opened from home: an artist's albums, an album's tracks, the queue, a genre's
//! radio and the search.

use super::*;

const SEARCH_PROMPT: &str = "Search: ";

/// Maximum height of the reason under the track list heading.
const ALBUM_REASON_LINES: usize = 3;

pub(super) fn artist(frame: &mut Frame, state: &State, view: &ArtistView, area: Rect) {
    if matches!(view.source, Source::Spotify(None)) {
        frame.render_widget(hint("Loading albums…"), area);
        return;
    }
    let albums = view.albums();
    if albums.is_empty() {
        frame.render_widget(hint("No albums"), area);
        return;
    }
    let from_shelf = matches!(view.source, Source::Shelf(_));
    let row = |index: usize| {
        let album = &albums[index];
        let year = album.year.map_or("    ".to_owned(), |y| y.to_string());
        let mut line = Line::from(vec![year.fg(MUTED), "  ".into(), album.name.clone().into()]);
        if !from_shelf {
            shelf_mark(&mut line, state, album);
        }
        line
    };
    list(
        frame,
        albums.len(),
        row,
        Some(view.selected),
        &view.scroll,
        area,
    );
}

/// Track list: the album, the artist and the year as the heading. For an album on the
/// Curated list, its reason is under the heading between empty rows, wherever the
/// album was opened from; the list scrolls below it. Tracks are numbered, with the
/// duration on the right edge, ♪ before the playing track and ♥ after a liked one.
pub(super) fn album(frame: &mut Frame, state: &State, view: &AlbumView, area: Rect) {
    let reason = state
        .curated_reason(&view.album.uri)
        .map(|text| wrap(text, usize::from(area.width), ALBUM_REASON_LINES))
        .unwrap_or_default();
    let reason_height = if reason.is_empty() {
        0
    } else {
        reason.len() as u16 + 1
    };
    let [title, meta, _, reason_area, rows] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(reason_height),
        Constraint::Min(0),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(reason.into_iter().map(Line::from).collect::<Vec<_>>()).fg(MUTED),
        reason_area,
    );

    let album = &view.album;
    frame.render_widget(album.name.as_str().bold(), title);
    let mut line = Line::from(album.artist.as_str().fg(MUTED));
    if let Some(year) = album.year {
        line.push_span(format!(" · {year}").fg(MUTED));
    }
    shelf_mark(&mut line, state, album);
    frame.render_widget(line, meta);

    let Some(tracks) = &view.tracks else {
        frame.render_widget(hint("Loading tracks…"), rows);
        return;
    };
    if tracks.is_empty() {
        frame.render_widget(hint("No tracks"), rows);
        return;
    }
    let playing = state.playing_track(&album.uri);
    let digits = tracks.len().to_string().len();
    let width = usize::from(rows.width);
    let row = |index: usize| {
        let track = &tracks[index];
        let mark = if playing == Some(index) {
            "♪".fg(ACCENT)
        } else {
            " ".into()
        };
        // "♪ 12  " + name + ♥ + at least two spaces + duration.
        let number = format!(" {:>digits$}  ", index + 1);
        let duration = minutes(track.duration.as_secs());
        let liked = if state.liked(&track.uri) { LIKED } else { "" };
        let fixed = 1 + number.len() + liked.width() + duration.len();
        let name = truncate(&track.name, width.saturating_sub(fixed + 2));
        let gap = width.saturating_sub(fixed + name.width()).max(2);
        Line::from(vec![
            mark,
            number.fg(MUTED),
            name.into(),
            liked.fg(ACCENT),
            " ".repeat(gap).into(),
            duration.fg(MUTED),
        ])
    };
    list(
        frame,
        tracks.len(),
        row,
        Some(view.selected),
        &view.scroll,
        rows,
    );
}

/// The queue in playing order: heading, number of tracks and total duration, and the
/// tracks numbered (name, artist dimmed, duration on the right edge).
pub(super) fn queue(
    frame: &mut Frame,
    state: &State,
    selected: usize,
    scroll: &Scroll,
    area: Rect,
) {
    let [title, meta, _, rows] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(area);

    frame.render_widget("Queue".bold(), title);
    let tracks = state.queue();
    if tracks.is_empty() {
        frame.render_widget(
            hint("The queue is empty – add tracks or albums with Ctrl+E"),
            rows,
        );
        return;
    }
    let total = tracks.iter().map(|t| t.duration.as_secs()).sum();
    frame.render_widget(
        Line::from(format!(
            "{} · {}",
            count(tracks.len(), "track"),
            minutes(total)
        ))
        .fg(MUTED),
        meta,
    );

    let digits = tracks.len().to_string().len();
    let width = usize::from(rows.width);
    let row = |index: usize| queue_row(&tracks[index], index, digits, width);
    list(frame, tracks.len(), row, Some(selected), scroll, rows);
}

/// "  3  Name  Artist      4:10": the name is truncated last.
fn queue_row(track: &QueuedTrack, index: usize, digits: usize, width: usize) -> Line<'static> {
    let number = format!("  {:>digits$}  ", index + 1);
    // A radio track's duration is unknown (zero).
    let duration = if track.duration.is_zero() {
        String::new()
    } else {
        minutes(track.duration.as_secs())
    };
    let room = width.saturating_sub(number.len() + duration.len() + 2);
    let name = truncate(&track.name, room);
    let artist = truncate(&track.artist, room.saturating_sub(name.width() + 2));
    let mut spans = vec![number.fg(MUTED), name.clone().into()];
    let mut used = name.width();
    if !artist.is_empty() {
        used += 2 + artist.width();
        spans.push("  ".into());
        spans.push(artist.fg(MUTED));
    }
    let gap = room.saturating_sub(used) + 2;
    spans.push(" ".repeat(gap).into());
    spans.push(duration.fg(MUTED));
    Line::from(spans)
}

/// Radio view: the genre and a dim "radio" as the heading, tracks in the order they
/// arrived at the station.
pub(super) fn radio_view(frame: &mut Frame, state: &State, view: &RadioView, area: Rect) {
    let [title, meta, _, rows] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(area);

    let genre = &view.genre.id;
    frame.render_widget(view.genre.name.as_str().bold(), title);
    frame.render_widget("radio".fg(MUTED), meta);
    let Some(tracks) = state.radio_tracks(genre) else {
        frame.render_widget(hint("The radio has stopped – Ctrl+R starts it again"), rows);
        return;
    };
    let playing = state.playing_radio_track(genre);
    let width = usize::from(rows.width);
    let row = |index: usize| radio_row(state, &tracks[index], playing == Some(index), width);
    list(
        frame,
        tracks.len(),
        row,
        Some(view.selected),
        &view.scroll,
        rows,
    );
}

/// "♪ Artist · Track ♥  Album ●": album dimmed. A long row truncates the album first,
/// then the artist and the track.
fn radio_row(state: &State, track: &radio::Track, playing: bool, width: usize) -> Line<'static> {
    let mark = if playing {
        "♪ ".fg(ACCENT)
    } else {
        "  ".into()
    };
    let on_shelf = state.on_shelf(&track.album.uri);
    let liked = state.liked(&track.uri);
    let room = width
        .saturating_sub(2 + if on_shelf { 2 } else { 0 } + if liked { LIKED.width() } else { 0 });
    let name = truncate(&format!("{} · {}", track.artist, track.name), room);
    let rest = room.saturating_sub(name.width() + 2);
    let mut spans = vec![mark, name.into()];
    if liked {
        spans.push(LIKED.fg(ACCENT));
    }
    // Show the album only if at least its beginning fits.
    if rest > 0 && rest >= track.album.name.width().min(4) {
        spans.push("  ".into());
        spans.push(truncate(&track.album.name, rest).fg(MUTED));
    }
    if on_shelf {
        spans.push(" ●".green());
    }
    Line::from(spans)
}

pub(super) fn search(frame: &mut Frame, state: &State, view: &SearchView, area: Rect) {
    let [input, _, results] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(area);

    // The field is underlined in the accent colour up to the cursor, so even an empty
    // field is visible. The text shown while searching starts after a space.
    let field = Style::new().underlined().underline_color(ACCENT);
    let mut line = Line::from(vec![
        SEARCH_PROMPT.fg(MUTED),
        Span::styled(format!("{} ", view.query), field),
    ]);
    if view.searching() {
        line.push_span(" searching…".fg(MUTED));
    }
    frame.render_widget(line, input);
    // The cursor shows only while typing; in the results a row is selected instead.
    if !view.in_results {
        let before_cursor: String = view.query.chars().take(view.cursor).collect();
        let x = input.x
            + Span::raw(SEARCH_PROMPT).width() as u16
            + Span::raw(before_cursor).width() as u16;
        frame.set_cursor_position((x.min(input.right().saturating_sub(1)), input.y));
    }

    let Some(found) = &view.results else {
        return;
    };
    if found.artists.is_empty() && found.albums.is_empty() && found.songs.is_empty() {
        frame.render_widget(hint("No results"), results);
        return;
    }

    // Group headings and the gap are list rows that cannot be selected: the selection's
    // index within the results is converted to a row index.
    let mut rows = Vec::new();
    let mut selected_row = None;
    let mut group = None;
    for (index, item) in view.items().into_iter().enumerate() {
        let (name, line) = match item {
            SearchItem::Artist(artist) => ("Artists", Line::from(artist.name.as_str())),
            SearchItem::Album(album) => {
                let mut line = Line::from(vec![
                    album.name.as_str().into(),
                    "  ".into(),
                    album.artist.as_str().fg(MUTED),
                ]);
                if let Some(year) = album.year {
                    line.push_span(format!(" · {year}").fg(MUTED));
                }
                shelf_mark(&mut line, state, album);
                ("Albums", line)
            }
            SearchItem::Song(song) => {
                let line = Line::from(vec![
                    song.track.name.as_str().into(),
                    "  ".into(),
                    song.track.artist.as_str().fg(MUTED),
                    format!(" · {}", song.album.name).fg(MUTED),
                ]);
                ("Songs", line)
            }
        };
        if group != Some(name) {
            if group.is_some() {
                rows.push(Line::default());
            }
            rows.push(Line::from(name).fg(HEADING).bold());
            group = Some(name);
        }
        if view.in_results && index == view.selected {
            selected_row = Some(rows.len());
        }
        rows.push(line);
    }
    lines(frame, rows, selected_row, &view.scroll, results);
}
