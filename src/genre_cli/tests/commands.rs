//! The commands around the check: reading stdin, the genre's name, what it replaces,
//! the JSON in and out, `deck genre list` and the prompt.

use std::io::Read;

use super::*;
use crate::genre_cli::check::Rejection;

/// A reader that must not be read.
struct Untouched;

impl Read for Untouched {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        panic!("a terminal must not be read")
    }
}

#[test]
fn terminal_input_returns_add_usage_without_reading() {
    let error = read_input(true, Untouched).unwrap_err();
    let usage = error.downcast_ref::<Usage>().expect("a usage error");
    assert_eq!(usage.0, ADD_USAGE);
}

#[test]
fn redirected_input_is_read() {
    let text = read_input(false, &b"{\"name\": \"x\"}"[..]).unwrap();
    assert_eq!(text, "{\"name\": \"x\"}");
}

/// The example in the usage is valid input.
#[test]
fn add_usage_example_parses() {
    let start = ADD_USAGE.find("  {").unwrap();
    let end = ADD_USAGE.find("]}").unwrap() + 2;
    let input: Input = serde_json::from_str(&ADD_USAGE[start..end]).unwrap();
    assert_eq!(input.name, "60s rock");
    assert_eq!(input.seeds.len(), 2);
    assert!(input.seeds[0].uri.is_some());
    assert_eq!(input.seeds[1].uri, None);
}

#[test]
fn genre_name_and_seed_count() {
    let input = |name: &str, seeds: usize| Input {
        name: name.into(),
        seeds: vec![seed("x", None, None); seeds],
    };
    assert_eq!(
        genre_name(&input(" 60s Rock ", 3)).unwrap(),
        ("60s-rock".into(), "60s rock".into())
    );
    assert!(genre_name(&input("!!", 3)).is_err());
    assert!(genre_name(&input("a", 0)).is_err());
    assert!(genre_name(&input("a", MAX_SEEDS + 1)).is_err());
}

#[test]
fn replaces_own_before_built_in() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("my-genres.json");
    let mut my = MyGenres::load(&path).unwrap();
    let catalog = genres::catalog();
    assert_eq!(replaces("lofi", &my, &catalog), Some(Replaces::BuiltIn));
    assert_eq!(replaces("60s-surf", &my, &catalog), None);
    my.put(Genre {
        id: "lofi".into(),
        name: "lofi".into(),
        seeds: vec![Seed {
            uri: TROGGS.into(),
            name: "The Troggs".into(),
        }],
    })
    .unwrap();
    assert_eq!(replaces("lofi", &my, &catalog), Some(Replaces::Own));
}

#[test]
fn input_and_report_json() {
    let input: Input = serde_json::from_str(
        r#"{"name": "60s rock", "seeds": [{"artist": "Love"}, {"artist": "The Zombies", "track": "She's Not There", "uri": "spotify:track:5BATmTqGopeifUzHN2bE0f"}]}"#,
    )
    .unwrap();
    assert_eq!(input.seeds[0], seed("Love", None, None));
    assert_eq!(input.seeds[1].uri.as_deref(), Some(ZOMBIES));

    let report = Report {
        id: "60s-rock".into(),
        name: "60s rock".into(),
        replaces: Some(Replaces::BuiltIn),
        accepted: vec![Accepted {
            artist: "Love".into(),
            track: None,
            uri: LOVE.into(),
            station: vec!["Band 1 · Song 1 (1966)".into()],
        }],
        rejected: vec![Rejected {
            artist: "The Troggs".into(),
            track: None,
            uri: Some(TROGSS.into()),
            reason: Rejection::NameMismatch,
            spotify_name: Some("The Trogss".into()),
        }],
        missing: 2,
        searched: 1,
        searches_left: 29,
        written: false,
    };
    assert_eq!(
        serde_json::to_value(&report).unwrap(),
        serde_json::json!({
            "id": "60s-rock", "name": "60s rock", "replaces": "built-in",
            "accepted": [{"artist": "Love", "uri": LOVE, "station": ["Band 1 · Song 1 (1966)"]}],
            "rejected": [{"artist": "The Troggs", "uri": TROGSS, "reason": "name_mismatch", "spotify_name": "The Trogss"}],
            "missing": 2, "searched": 1, "searches_left": 29, "written": false
        })
    );
}

#[test]
fn listed_own_genres_are_marked() {
    let genre = &genres::catalog()[0];
    let own = serde_json::to_value(Listed { genre, own: true }).unwrap();
    let built_in = serde_json::to_value(Listed { genre, own: false }).unwrap();
    assert_eq!(own["own"], true);
    assert!(built_in.get("own").is_none());
    assert_eq!(built_in["id"], genre.id.as_str());
}

#[test]
fn prompt_mentions_the_commands() {
    for command in [
        "deck genre list",
        "deck genre add --dry-run",
        "deck genre remove",
    ] {
        assert!(PROMPT.contains(command), "{command}");
    }
}
