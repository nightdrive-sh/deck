//! Drawing shared by the views: a scrolling list, a hint in place of an empty one, and
//! the text helpers (shortening, wrapping, spreading a row, counts, the shelf mark).

use super::*;

/// A row with the left part at the start and the right part at the right edge, with at
/// least two spaces between them.
pub(super) fn spread(
    mut left: Vec<Span<'static>>,
    right: Vec<Span<'static>>,
    width: usize,
) -> Line<'static> {
    let used: usize = left.iter().chain(&right).map(Span::width).sum();
    left.push(" ".repeat(width.saturating_sub(used).max(2)).into());
    left.extend(right);
    Line::from(left)
}

/// "1 genre", "18 genres".
pub(super) fn count(n: usize, noun: &str) -> String {
    match n {
        1 => format!("1 {noun}"),
        n => format!("{n} {noun}s"),
    }
}

/// Wraps text by words into at most `max_lines` lines. If text is left over, the
/// last line ends in "…". A word that is too long is truncated.
pub(super) fn wrap(text: &str, width: usize, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.width() + 1 + word.width() > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    let more = lines.len() > max_lines;
    lines.truncate(max_lines);
    if more && let Some(last) = lines.last_mut() {
        last.push('…');
    }
    lines.iter().map(|line| truncate(line, width)).collect()
}

/// Truncates text to at most `width` columns, ending truncated text with "…".
pub(super) fn truncate(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    let mut short = String::new();
    let mut used = 1; // "…"
    for c in text.chars() {
        used += c.width().unwrap_or(0);
        if used > width {
            break;
        }
        short.push(c);
    }
    if width > 0 {
        short.push('…');
    }
    short
}

/// A green ● after an album that is on the shelf.
pub(super) fn shelf_mark(line: &mut Line, state: &State, album: &Album) {
    if state.on_shelf(&album.uri) {
        line.push_span(" ●".green());
    }
}

/// A list whose selected row is highlighted across the full width with a dim background
/// and bold text. The row's parts keep their own colours (the shelf ● green), so the
/// highlight uses the row's style and not the list's highlight_style, which would
/// cover them. Dim parts (year, artist) turn lighter on the selected row. The list
/// scrolls only when the selection would go past the visible area, and the new scroll
/// position is kept in the view state.
///
/// The list has `len` rows, and `row` builds a row from its index. Only the visible
/// rows are built, so drawing even a long list (e.g. radio) does not depend on its
/// length.
pub(super) fn list<'a>(
    frame: &mut Frame,
    len: usize,
    mut row: impl FnMut(usize) -> Line<'a>,
    selected: Option<usize>,
    scroll: &Scroll,
    area: Rect,
) {
    let height = usize::from(area.height);
    if len == 0 || height == 0 || area.width == 0 {
        return;
    }
    // A selection past the end of the list lands on the last row, as in ratatui.
    let selected = selected.map(|selected| selected.min(len - 1));
    let first = first_visible(len, height, selected, scroll.get());
    scroll.set(first);
    let selected_style = Style::new().fg(SELECTED_FG).bg(SELECTED_BG).bold();
    let items = (first..len.min(first + height)).map(|index| {
        let line = row(index);
        if Some(index) == selected {
            ListItem::new(brighten_muted(line)).style(selected_style)
        } else {
            ListItem::new(line)
        }
    });
    frame.render_widget(List::new(items), area);
}

/// [`list`] from ready-made rows (shelf and search, which have heading and gap rows).
pub(super) fn lines(
    frame: &mut Frame,
    mut rows: Vec<Line>,
    selected: Option<usize>,
    scroll: &Scroll,
    area: Rect,
) {
    let len = rows.len();
    list(
        frame,
        len,
        |index| std::mem::take(&mut rows[index]),
        selected,
        scroll,
        area,
    );
}

/// The top visible row, computed as ratatui's `List` does for single-line rows: the
/// previous position stays until the selection would go past the visible area.
/// `selected` is within the list.
pub(super) fn first_visible(
    len: usize,
    height: usize,
    selected: Option<usize>,
    offset: usize,
) -> usize {
    let offset = offset.min(len - 1);
    let Some(selected) = selected else {
        return offset;
    };
    if selected < offset {
        selected
    } else if selected >= offset + height {
        selected + 1 - height
    } else {
        offset
    }
}

fn brighten_muted(mut line: Line) -> Line {
    for span in &mut line.spans {
        if span.style.fg == Some(MUTED) {
            span.style.fg = Some(SELECTED_MUTED);
        }
    }
    line
}

pub(super) fn hint(text: &str) -> Paragraph<'_> {
    Paragraph::new(text).style(Style::new().fg(MUTED))
}
