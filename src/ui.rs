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

mod home;
mod key_help;
mod list;
mod views;

use home::{curated, genre_list, shelf};
use key_help::help_line;
use list::{count, hint, lines, list, shelf_mark, spread, truncate, wrap};
use views::{album, artist, queue, radio_view, search};

/// Mark after the name of a liked track.
const LIKED: &str = " ♥";

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

fn clock(secs: u64) -> String {
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

/// A track's duration in a list: 4:10.
fn minutes(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

#[cfg(test)]
mod tests;
