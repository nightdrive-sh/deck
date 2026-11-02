//! Draws the app state with ratatui. No logic: decisions are made in `app`.
//!
//! The top (title, queue length and pause mark, now playing or an error, progress
//! bar) is always visible. Below it is the current view and on the bottom row the
//! view's key help. There is always an empty row between the view and the help row:
//! a long list scrolls and never reaches the help row.
//!
//! The UI text is in English.
//!
//! Everything is drawn in Deck's area, which is at most `MAX_WIDTH` columns wide and
//! `MAX_HEIGHT` rows high and centred in the window. In a small window it fills the window.
//!
//! The colours are fixed RGB colours that stand out on both dark and light
//! backgrounds; the selected row sets both the text and the background colour, so it
//! does not depend on the terminal theme.

use std::time::Instant;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    app::{
        AlbumView, ArtistView, NoticeKind, QueuedTrack, RadioView, Scroll, SearchItem, SearchView,
        ShelfRow, Source, State, View,
    },
    catalog::Album,
    genres,
    lists::{AlbumList, ListAlbum},
    radio,
};

mod key_help;
mod list;

use key_help::help_line;
use list::{count, hint, lines, list, shelf_mark, spread, truncate, wrap};

const SEARCH_PROMPT: &str = "Search: ";

/// Mark after the name of a liked track.
const LIKED: &str = " ♥";

/// Mark of the list rows above the shelf.
const LIST_MARK: &str = "✦";
/// Mark of a genre (radio) row.
const GENRE_MARK: &str = "≋";

/// Maximum height of the reason in the list view.
const REASON_LINES: usize = 2;
/// Maximum height of the reason under the track list heading.
const ALBUM_REASON_LINES: usize = 3;
/// After a stale list's subtitle: why the weekly update did not happen.
const STALE_HINT: &str = " – see ~/.cache/deck/curate.log";

/// The Curated view before there is a list: what Curated is and how to set it up.
const CURATED_SETUP: &[&str] = &[
    "Curated is a weekly list of 20 albums you have not heard yet, picked for your taste \
     by Claude Code from your Last.fm history. Every album comes with a reason.",
    "To set it up:",
    "1. Add lastfm_api_key and lastfm_user to ~/.config/deck/config.toml and check them \
     with: deck taste",
    "2. Make the first list in a terminal (needs Claude Code): deck curate",
    "3. To get a new list every week, see \"Setting up Curated\" in Deck's README.",
];

/// Accent colour: DECK, keys, progress bar, search field underline, the playing
/// track's ♪, the liked ♥, the queue's + and the pause ■.
const ACCENT: Color = Color::Rgb(0xD0, 0x7A, 0x10);
/// Secondary text: labels, years, durations, reasons. Its contrast on a typical dark
/// terminal background is over 4.5:1, and it stays clearly dimmer than normal
/// text.
const MUTED: Color = Color::Rgb(0x8A, 0x8F, 0x99);
/// The remaining part of the progress bar and the rule above the help row: darker
/// than text, so that the full-width rule does not stand out.
const RULE: Color = Color::Rgb(0x4A, 0x4E, 0x58);
/// Group headings of the search results (Artists, Albums, Songs).
const HEADING: Color = Color::Rgb(0x6B, 0x80, 0xA0);
/// Subtitle of a stale list: the weekly update has been missed.
const WARNING: Color = Color::Rgb(0xE6, 0xC2, 0x4A);
/// The favourite genre's ★ in the Genres view.
const STAR: Color = Color::Rgb(0xE6, 0xC2, 0x4A);
/// Dim background and light text of the selected row.
const SELECTED_BG: Color = Color::Rgb(0x3A, 0x41, 0x50);
const SELECTED_FG: Color = Color::Rgb(0xF2, 0xF2, 0xF2);
/// Secondary text on the selected row: `MUTED` does not stand out from `SELECTED_BG`.
const SELECTED_MUTED: Color = Color::Rgb(0xA8, 0xB0, 0xC0);

/// Maximum size of Deck's area. A smaller window is used in full.
const MAX_WIDTH: u16 = 100;
const MAX_HEIGHT: u16 = 30;

pub fn draw(frame: &mut Frame, state: &State, now: Instant) {
    let [title, status, bar, _, body, _, rule, help] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(deck_area(frame.area()));

    let view = state.view();
    frame.render_widget("DECK".fg(ACCENT).bold(), title);
    frame.render_widget(title_right(state, now, title.width), title);
    status_line(frame, state, now, status);
    frame.render_widget(progress_bar(state, now, bar.width), bar);

    match view {
        View::Shelf { selected, scroll } => shelf(frame, state, *selected, scroll, body),
        View::Curated { selected, scroll } => curated(frame, state, *selected, scroll, body),
        View::Artist(view) => artist(frame, state, view, body),
        View::Search(view) => search(frame, state, view, body),
        View::Album(view) => album(frame, state, view, body),
        View::Queue { selected, scroll } => queue(frame, state, *selected, scroll, body),
        View::Genres { selected, scroll } => genre_list(frame, state, *selected, scroll, body),
        View::Radio(view) => radio_view(frame, state, view, body),
    }
    frame.render_widget(help_rule(rule.width), rule);
    frame.render_widget(help_line(state, view, help.width), help);
}

/// Deck's area centred in the window.
fn deck_area(window: Rect) -> Rect {
    let width = window.width.min(MAX_WIDTH);
    let height = window.height.min(MAX_HEIGHT);
    Rect {
        x: window.x + (window.width - width) / 2,
        y: window.y + (window.height - height) / 2,
        width,
        height,
    }
}

/// Row 2: an error in red, otherwise artist / album / track on the left and the
/// elapsed time on the right edge. The track name is bold and the separators dim, and
/// a liked track is followed by ♥. A long row is cut before the time, so the time is
/// always visible.
fn status_line(frame: &mut Frame, state: &State, now: Instant, area: Rect) {
    if let Some(error) = &state.error {
        frame.render_widget(Line::from(error.as_str()).red(), area);
        return;
    }
    let Some(playing) = &state.now_playing else {
        frame.render_widget(Line::from("—").fg(MUTED), area);
        return;
    };
    let time = clock(playing.elapsed(now).as_secs());
    let [left, _, right] = Layout::horizontal([
        Constraint::Min(0),
        Constraint::Length(2),
        Constraint::Length(time.width() as u16),
    ])
    .areas(area);
    let mut track = Line::from(vec![
        playing.artist.as_str().into(),
        " / ".fg(MUTED),
        playing.album.as_str().into(),
        " / ".fg(MUTED),
        playing.track.as_str().bold(),
    ]);
    if state.liked(&playing.uri) {
        track.push_span(LIKED.fg(ACCENT));
    }
    frame.render_widget(track, left);
    frame.render_widget(Line::from(time), right);
}

/// Right edge of the top row: the queue length (`+3 queued`), briefly replaced by a
/// confirmation after queueing or liking (`+ queued: Nevermind`, `♥ liked: Sunset`,
/// `unliked: Sunset`), and `■ stopped` last when paused. Marks in the accent colour,
/// text dimmed. A long name is truncated so that DECK stays visible.
fn title_right(state: &State, now: Instant, width: u16) -> Line<'_> {
    let paused = state
        .now_playing
        .as_ref()
        .is_some_and(|playing| playing.paused);
    let mut spans = Vec::new();
    if let Some((kind, name)) = state.notice(now) {
        let (mark, label) = match kind {
            NoticeKind::Queued => ("+ ", "queued: "),
            NoticeKind::Liked => ("♥ ", "liked: "),
            NoticeKind::Unliked => ("", "unliked: "),
        };
        // "DECK" and a space on the left, the mark, label and pause mark on the right.
        let used = 6 + mark.width() + label.width() + if paused { 12 } else { 0 };
        let name = truncate(name, usize::from(width).saturating_sub(used));
        spans.extend([mark.fg(ACCENT), label.fg(MUTED), name.into()]);
    } else if !state.queue().is_empty() {
        spans.push(format!("+{}", state.queue().len()).fg(ACCENT));
        spans.push(" queued".fg(MUTED));
    }
    if paused {
        if !spans.is_empty() {
            spans.push("   ".into());
        }
        spans.extend(["■".fg(ACCENT), " stopped".fg(MUTED)]);
    }
    Line::from(spans).right_aligned()
}

/// Row 3: the progress bar across Deck's area. When nothing is playing, ● is at the start.
fn progress_bar(state: &State, now: Instant, width: u16) -> Line<'static> {
    let width = usize::from(width);
    if width == 0 {
        return Line::default();
    }
    let ratio = match &state.now_playing {
        Some(playing) if !playing.duration.is_zero() => {
            playing.elapsed(now).as_secs_f64() / playing.duration.as_secs_f64()
        }
        _ => 0.0,
    };
    let head = ((ratio.clamp(0.0, 1.0) * (width - 1) as f64) as usize).min(width - 1);
    Line::from(vec![
        Span::raw("─".repeat(head)).fg(ACCENT),
        Span::raw("●").fg(ACCENT),
        Span::raw("─".repeat(width - head - 1)).fg(RULE),
    ])
}

/// The rule above the help row: the unplayed part of the progress bar without ●, so
/// that the bottom edge mirrors the top.
fn help_rule(width: u16) -> Line<'static> {
    Line::from("─".repeat(usize::from(width))).fg(RULE)
}

/// Shelf: at the top the group of lists (Curated, favourite genres and Genres) and an
/// empty row, below that the artists. The selection moves over the list rows and the
/// artists; the empty row cannot be selected. The list group always has at least the
/// Genres row.
fn shelf(frame: &mut Frame, state: &State, selected: usize, scroll: &Scroll, area: Rect) {
    let lists = state.shelf_lists();
    let width = usize::from(area.width);
    let mut rows: Vec<Line> = lists
        .iter()
        .map(|row| match row {
            ShelfRow::Curated(Some(list)) => list_row(list, width),
            ShelfRow::Curated(None) => spread(
                vec![format!("{LIST_MARK} Curated").into()],
                vec![if state.curated_broken() {
                    "file broken".fg(WARNING)
                } else {
                    "not set up".fg(MUTED)
                }],
                width,
            ),
            ShelfRow::Genre(genre) => {
                let right = if state.playing_genre() == Some(genre.id.as_str()) {
                    vec!["♪".fg(ACCENT), " playing".fg(MUTED)]
                } else {
                    vec!["radio".fg(MUTED)]
                };
                spread(
                    vec![format!("{GENRE_MARK} {}", genre.name).into()],
                    right,
                    width,
                )
            }
            ShelfRow::Genres => spread(
                vec![format!("{GENRE_MARK} Genres").fg(MUTED)],
                vec![count(genres::all().len(), "genre").fg(MUTED)],
                width,
            ),
        })
        .collect();
    rows.push(Line::default());
    let selected_row = if selected < lists.len() {
        selected
    } else {
        selected + rows.len() - lists.len()
    };

    let artists = state.shelf_artists();
    if artists.is_empty() {
        // The hint for an empty shelf is shown below the lists.
        let [top, rest] =
            Layout::vertical([Constraint::Length(rows.len() as u16), Constraint::Min(0)])
                .areas(area);
        lines(frame, rows, Some(selected_row), scroll, top);
        frame.render_widget(
            hint("The shelf is empty – search for albums with Ctrl+F"),
            rest,
        );
        return;
    }
    rows.extend(
        artists
            .iter()
            .map(|artist| Line::from(artist.name.as_str())),
    );
    lines(frame, rows, Some(selected_row), scroll, area);
}

/// "✦ Curated" on the left, the album count and the week dimmed on the right edge.
/// A stale list's subtitle (with its age) is in the warning colour.
fn list_row(list: &AlbumList, width: usize) -> Line<'static> {
    spread(
        vec![format!("{LIST_MARK} {}", list.name).into()],
        vec![list.subtitle.clone().fg(subtitle_color(list))],
        width,
    )
}

/// Genres: all catalog genres in alphabetical order, each favourite followed by a
/// yellow ★.
fn genre_list(frame: &mut Frame, state: &State, selected: usize, scroll: &Scroll, area: Rect) {
    let [title, meta, _, rows] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(area);

    let all = genres::all();
    frame.render_widget("Genres".bold(), title);
    let home = state.favorite_genres().len();
    frame.render_widget(
        Line::from(format!("{} · {home} on home", count(all.len(), "genre"))).fg(MUTED),
        meta,
    );
    let row = |index: usize| {
        let genre = &all[index];
        let mut line = Line::from(genre.name.as_str());
        if state.favorite(&genre.id) {
            line.push_span(" ★".fg(STAR));
        }
        line
    };
    list(frame, all.len(), row, Some(selected), scroll, rows);
}

/// Radio view: the genre and a dim "radio" as the heading, tracks in the order they
/// arrived at the station.
fn radio_view(frame: &mut Frame, state: &State, view: &RadioView, area: Rect) {
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

fn subtitle_color(list: &AlbumList) -> Color {
    if list.stale { WARNING } else { MUTED }
}

/// List view: the list's name and subtitle as the heading, albums in curation order,
/// and the selected album's reason below the list. The list scrolls, the reason stays
/// in place. A stale list's subtitle is followed by a hint about the curation run's log.
fn curated(frame: &mut Frame, state: &State, selected: usize, scroll: &Scroll, area: Rect) {
    let [title, meta, _, rows, _, reason] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(REASON_LINES as u16),
    ])
    .areas(area);

    let shown = state.curated();
    let name = shown.map_or("Curated", |list| list.name.as_str());
    frame.render_widget(name.bold(), title);
    if shown.is_none() {
        let text = if state.curated_broken() {
            vec!["The list file cannot be read – see the error above.".to_owned()]
        } else {
            let width = usize::from(rows.width);
            CURATED_SETUP
                .iter()
                .flat_map(|paragraph| {
                    let mut lines = wrap(paragraph, width, usize::MAX);
                    lines.push(String::new());
                    lines
                })
                .collect()
        };
        frame.render_widget(
            Paragraph::new(text.into_iter().map(Line::from).collect::<Vec<_>>()).fg(MUTED),
            rows.union(reason),
        );
        return;
    }
    let Some(current) = shown.filter(|list| !list.albums.is_empty()) else {
        frame.render_widget(
            hint("Nothing left – a new list arrives with the next weekly run"),
            rows,
        );
        return;
    };
    let mut subtitle = vec![current.subtitle.as_str().fg(subtitle_color(current))];
    if current.stale {
        subtitle.push(STALE_HINT.fg(MUTED));
    }
    frame.render_widget(Line::from(subtitle), meta);

    let width = usize::from(rows.width);
    let row = |index: usize| list_album_row(state, &current.albums[index], width);
    list(
        frame,
        current.albums.len(),
        row,
        Some(selected),
        scroll,
        rows,
    );

    if let Some(text) = current
        .albums
        .get(selected)
        .and_then(|entry| entry.reason.as_deref())
    {
        let lines = wrap(text, usize::from(reason.width), REASON_LINES);
        frame.render_widget(
            Paragraph::new(lines.into_iter().map(Line::from).collect::<Vec<_>>()).fg(MUTED),
            reason,
        );
    }
}

/// "Artist · Album  1974 ●": names are truncated so that the year and the shelf mark
/// show. The artist is truncated only if not even the start of the album would show
/// otherwise.
fn list_album_row(state: &State, entry: &ListAlbum, width: usize) -> Line<'static> {
    let album = &entry.album;
    let year = album
        .year
        .map(|year| format!("  {year}"))
        .unwrap_or_default();
    let on_shelf = state.on_shelf(&album.uri);
    let fixed = year.width() + if on_shelf { 2 } else { 0 };
    let room = width.saturating_sub(fixed);
    let album_start = album.name.width().min(6);
    let artist_room = (room / 2).max(room.saturating_sub(3 + album_start));
    let artist = truncate(&album.artist, artist_room);
    let mut spans = vec![artist.clone().into()];
    let rest = room.saturating_sub(artist.width() + 3);
    if rest > 0 {
        spans.push(" · ".fg(MUTED));
        spans.push(truncate(&album.name, rest).into());
    }
    spans.push(year.fg(MUTED));
    let mut line = Line::from(spans);
    shelf_mark(&mut line, state, album);
    line
}

fn artist(frame: &mut Frame, state: &State, view: &ArtistView, area: Rect) {
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
fn album(frame: &mut Frame, state: &State, view: &AlbumView, area: Rect) {
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
fn queue(frame: &mut Frame, state: &State, selected: usize, scroll: &Scroll, area: Rect) {
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

fn search(frame: &mut Frame, state: &State, view: &SearchView, area: Rect) {
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

fn clock(secs: u64) -> String {
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

/// A track's duration in a list: 4:10.
fn minutes(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

#[cfg(test)]
mod tests;
