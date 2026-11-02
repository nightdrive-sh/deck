//! The order of the shelf's artists and albums: by name as the user's locale sorts it
//! (Finnish and Swedish put å, ä and ö after z), and albums by year.

use std::sync::OnceLock;

use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

use crate::catalog::{Album, Artist};

/// The albums' artists in alphabetical order ([`sort_key`]), each once.
pub fn artists_of(albums: &[Album]) -> Vec<Artist> {
    let mut artists: Vec<Artist> = Vec::new();
    for album in albums {
        if !artists.iter().any(|a| a.id == album.artist_id) {
            artists.push(Artist {
                id: album.artist_id.clone(),
                name: album.artist.clone(),
            });
        }
    }
    artists.sort_by_cached_key(|a| (sort_key(&a.name), a.name.to_lowercase(), a.id.clone()));
    artists
}

/// The artist's albums from the list, sorted by year (see [`sort_by_year`]).
pub fn albums_of(albums: &[Album], artist_id: &str) -> Vec<Album> {
    let mut albums: Vec<Album> = albums
        .iter()
        .filter(|a| a.artist_id == artist_id)
        .cloned()
        .collect();
    sort_by_year(&mut albums);
    albums
}

/// Oldest first, albums without a year last, albums from the same year by name
/// ([`sort_key`]).
pub fn sort_by_year(albums: &mut [Album]) {
    albums.sort_by_cached_key(|a| (a.year.is_none(), a.year, sort_key(&a.name)));
}

/// Sort key for a name according to the user's locale ([`nordic_collation`], see
/// [`sort_key_with`]).
fn sort_key(name: &str) -> String {
    sort_key_with(name, nordic_collation())
}

/// Whether å, ä and ö sort as letters of their own after z. Decided once from
/// the environment variables ([`is_nordic_locale`]).
fn nordic_collation() -> bool {
    static NORDIC: OnceLock<bool> = OnceLock::new();
    *NORDIC.get_or_init(|| is_nordic_locale(|name| std::env::var(name).ok()))
}

/// Whether the collation locale is Finnish or Swedish. As in POSIX, the locale is the
/// first non-empty one of `LC_ALL`, `LC_COLLATE` and `LANG`. `var` reads a variable, so
/// the decision can be tested without touching the environment.
pub(super) fn is_nordic_locale(var: impl Fn(&str) -> Option<String>) -> bool {
    ["LC_ALL", "LC_COLLATE", "LANG"]
        .into_iter()
        .filter_map(var)
        .find(|value| !value.is_empty())
        .is_some_and(|locale| locale.starts_with("fi") || locale.starts_with("sv"))
}

/// Sort key for a name; the displayed name stays as it is. A leading "The " is skipped,
/// case does not matter and accents are removed (é → e, ü → u, ñ → n).
/// `nordic`: as in Finnish and Swedish, å, ä and ö are letters of their own after z
/// (æ = ä, ø = ö). Otherwise they too fold to their base letter (å, ä → a,
/// ö, ø → o, æ → ae).
pub(super) fn sort_key_with(name: &str, nordic: bool) -> String {
    // NFC first, so that an ä stored decomposed (a + ¨) is recognised too.
    let lower = name.trim().nfc().collect::<String>().to_lowercase();
    let lower = match lower.strip_prefix("the ") {
        Some(rest) if !rest.trim().is_empty() => rest.trim_start(),
        _ => &lower,
    };
    let mut key = String::with_capacity(lower.len());
    for c in lower.chars() {
        // Characters from a Private Use plane sort after everything else.
        match c {
            'å' if nordic => key.push('\u{F0001}'),
            'ä' | 'æ' if nordic => key.push('\u{F0002}'),
            'ö' | 'ø' if nordic => key.push('\u{F0003}'),
            'æ' => key.push_str("ae"),
            'ø' => key.push('o'),
            _ => key.extend(c.nfd().filter(|&c| !is_combining_mark(c))),
        }
    }
    key
}
