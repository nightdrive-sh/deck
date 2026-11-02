//! The order: artists by name, the sort key in each locale, and albums by year.

use super::*;
use crate::shelf::sort::{is_nordic_locale, sort_key_with};

#[test]
fn artists_alphabetical_and_unique() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut shelf) = shelf_in(&dir);
    shelf
        .add(album("1", "In Utero", "Nirvana", Some(1993)))
        .unwrap();
    shelf
        .add(album("2", "Ok Computer", "Radiohead", Some(1997)))
        .unwrap();
    shelf
        .add(album("3", "Nevermind", "Nirvana", Some(1991)))
        .unwrap();
    shelf.add(album("4", "Post", "björk", Some(1995))).unwrap();
    shelf
        .add(album("5", "Wasting Light", "Foo Fighters", Some(2011)))
        .unwrap();

    let artists: Vec<String> = artists_of(shelf.albums())
        .into_iter()
        .map(|a| a.name)
        .collect();
    assert_eq!(artists, ["björk", "Foo Fighters", "Nirvana", "Radiohead"]);
}

#[test]
fn artists_sorted_without_the_and_accents() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut shelf) = shelf_in(&dir);
    let given = ["The Beatles", "Édith Piaf", "abba", "Zen Café", "Motörhead"];
    for (i, artist) in given.iter().enumerate() {
        shelf
            .add(album(&i.to_string(), "Album", artist, Some(2000)))
            .unwrap();
    }

    let artists: Vec<String> = artists_of(shelf.albums())
        .into_iter()
        .map(|a| a.name)
        .collect();
    assert_eq!(
        artists,
        ["abba", "The Beatles", "Édith Piaf", "Motörhead", "Zen Café"]
    );
}

/// The names sorted by the `sort_key_with` key.
fn sorted(names: &[&'static str], nordic: bool) -> Vec<&'static str> {
    let mut names = names.to_vec();
    names.sort_by_cached_key(|name| sort_key_with(name, nordic));
    names
}

#[test]
fn sort_key_rules() {
    for nordic in [false, true] {
        let key = |name| sort_key_with(name, nordic);
        assert_eq!(key("The Beatles"), "beatles");
        assert_eq!(key("THE  Cure"), "cure");
        // "The" alone, or a word starting with "The", is not an article.
        assert_eq!(key("The"), "the");
        assert_eq!(key("Them"), "them");
        assert_eq!(key("Édith Piaf"), "edith piaf");
        assert_eq!(key("Mötley Crüe"), key("Mötley Crue"));
        assert_eq!(key("Señor Coconut"), "senor coconut");
        assert_eq!(key("Sigur Rós"), "sigur ros");
        assert_eq!(key("Røyksopp"), key("Röyksopp"));
        // An ä written decomposed (a + ¨) is the same letter too.
        assert_eq!(key("A\u{308}ly"), key("Äly"));
    }
}

#[test]
fn nordic_collation_puts_a_ring_a_umlaut_and_o_umlaut_after_z() {
    let names = ["Älymystö", "Zen Café", "Åke", "abba", "Öljy", "Motörhead"];
    assert_eq!(
        sorted(&names, true),
        ["abba", "Motörhead", "Zen Café", "Åke", "Älymystö", "Öljy"]
    );
    assert!(sort_key_with("Zz", true) < sort_key_with("Åke", true));
    assert!(sort_key_with("Åke", true) < sort_key_with("Ärsyttävä", true));
    assert!(sort_key_with("Ärsyttävä", true) < sort_key_with("Öljy", true));
    assert_eq!(sort_key_with("Æble", true), sort_key_with("Äble", true));
}

#[test]
fn other_locales_fold_nordic_letters() {
    let names = ["Älymystö", "Zen Café", "Åke", "abba", "Öljy", "Motörhead"];
    assert_eq!(
        sorted(&names, false),
        ["abba", "Åke", "Älymystö", "Motörhead", "Öljy", "Zen Café"]
    );
    assert_eq!(sort_key_with("Älymystö", false), "alymysto");
    assert_eq!(sort_key_with("Åke", false), "ake");
    assert_eq!(sort_key_with("Røyksopp", false), "royksopp");
    assert_eq!(sort_key_with("Mæstro", false), "maestro");
}

#[test]
fn nordic_locale_comes_from_lc_all_lc_collate_or_lang() {
    let env = |vars: &'static [(&'static str, &'static str)]| {
        move |name: &str| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    };
    assert!(is_nordic_locale(env(&[("LANG", "fi_FI.UTF-8")])));
    assert!(is_nordic_locale(env(&[("LANG", "sv_SE.UTF-8")])));
    assert!(!is_nordic_locale(env(&[("LANG", "en_US.UTF-8")])));
    assert!(!is_nordic_locale(env(&[])));
    // LC_ALL overrides the others, LC_COLLATE overrides LANG, and empty values
    // are skipped.
    assert!(!is_nordic_locale(env(&[
        ("LC_ALL", "C"),
        ("LC_COLLATE", "fi_FI.UTF-8"),
        ("LANG", "fi_FI.UTF-8"),
    ])));
    assert!(is_nordic_locale(env(&[
        ("LC_COLLATE", "sv_FI.UTF-8"),
        ("LANG", "en_GB.UTF-8"),
    ])));
    assert!(is_nordic_locale(env(&[
        ("LC_ALL", ""),
        ("LC_COLLATE", ""),
        ("LANG", "fi_FI.UTF-8"),
    ])));
}

#[test]
fn same_year_albums_use_sort_key() {
    let mut albums = vec![
        album("1", "Zuma", "X", Some(2001)),
        album("2", "The Yes Album", "X", Some(2001)),
        album("3", "Ébauche", "X", Some(2001)),
        album("4", "Bleach", "X", Some(1989)),
    ];
    sort_by_year(&mut albums);
    assert_eq!(
        names(&albums),
        ["Bleach", "Ébauche", "The Yes Album", "Zuma"]
    );
}

#[test]
fn albums_by_oldest_first_without_year_last() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut shelf) = shelf_in(&dir);
    shelf
        .add(album("1", "In Utero", "Nirvana", Some(1993)))
        .unwrap();
    shelf.add(album("2", "Unplugged", "Nirvana", None)).unwrap();
    shelf
        .add(album("3", "Nevermind", "Nirvana", Some(1991)))
        .unwrap();
    shelf
        .add(album("4", "Bleach", "Nirvana", Some(1989)))
        .unwrap();
    shelf
        .add(album("5", "Ok Computer", "Radiohead", Some(1997)))
        .unwrap();

    let nirvana = albums_of(shelf.albums(), "id-nirvana");
    assert_eq!(
        names(&nirvana),
        ["Bleach", "Nevermind", "In Utero", "Unplugged"]
    );
    assert!(albums_of(shelf.albums(), "id-unknown").is_empty());
}
