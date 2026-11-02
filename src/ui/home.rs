//! The home views: the shelf with its list rows above the artists, the Genres catalog and
//! the Curated list.

use super::*;

/// Mark of the list rows above the shelf.
const LIST_MARK: &str = "✦";

/// Mark of a genre (radio) row.
const GENRE_MARK: &str = "≋";

/// Maximum height of the reason in the list view.
const REASON_LINES: usize = 2;

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

/// Shelf: at the top the group of lists (Curated, favourite genres and Genres) and an
/// empty row, below that the artists. The selection moves over the list rows and the
/// artists; the empty row cannot be selected. The list group always has at least the
/// Genres row.
pub(super) fn shelf(
    frame: &mut Frame,
    state: &State,
    selected: usize,
    scroll: &Scroll,
    area: Rect,
) {
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

fn subtitle_color(list: &AlbumList) -> Color {
    if list.stale { WARNING } else { MUTED }
}

/// Genres: all catalog genres in alphabetical order, each favourite followed by a
/// yellow ★.
pub(super) fn genre_list(
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

/// List view: the list's name and subtitle as the heading, albums in curation order,
/// and the selected album's reason below the list. The list scrolls, the reason stays
/// in place. A stale list's subtitle is followed by a hint about the curation run's log.
pub(super) fn curated(
    frame: &mut Frame,
    state: &State,
    selected: usize,
    scroll: &Scroll,
    area: Rect,
) {
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
