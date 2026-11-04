use std::fs;

use super::*;

fn entry(id: &str, name: &str, artist: &str, reason: &str) -> ListAlbum {
    ListAlbum {
        album: Album {
            id: id.to_owned(),
            uri: format!("spotify:album:{id}"),
            name: name.to_owned(),
            artist: artist.to_owned(),
            artist_id: format!("id-{}", artist.to_lowercase().replace(' ', "-")),
            year: Some(1974),
        },
        reason: Some(reason.to_owned()),
        genres: Vec::new(),
    }
}

fn round(albums: Vec<ListAlbum>) -> Curated {
    Curated {
        round: "2026-W41".to_owned(),
        created: "2026-10-05T07:00:00+03:00".to_owned(),
        albums,
    }
}

#[test]
fn missing_files_are_empty() {
    let dir = tempfile::tempdir().unwrap();
    let files = CuratedFiles::in_dir(dir.path());
    assert_eq!(files.load().unwrap(), None);
    assert!(files.history().unwrap().is_empty());
    assert_eq!(files.reject("spotify:album:x").unwrap(), None);
    assert!(!dir.path().join("curated-history.json").exists());
}

#[test]
fn publish_writes_spec_format_and_appends_history() {
    let dir = tempfile::tempdir().unwrap();
    let files = CuratedFiles::in_dir(&dir.path().join("deck"));
    let first = round(vec![entry("a1", "No Other", "Gene Clark", "Cosmic.")]);
    files.publish(&first).unwrap();

    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("deck/curated.json")).unwrap())
            .unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "round": "2026-W41",
            "created": "2026-10-05T07:00:00+03:00",
            "albums": [{
                "uri": "spotify:album:a1",
                "name": "No Other",
                "artist": "Gene Clark",
                "artist_id": "id-gene-clark",
                "year": 1974,
                "reason": "Cosmic.",
            }],
        })
    );
    assert_eq!(files.load().unwrap(), Some(first));

    let mut second = round(vec![entry("b2", "#1 Record", "Big Star", "Power pop.")]);
    second.round = "2026-W42".to_owned();
    files.publish(&second).unwrap();
    assert_eq!(files.load().unwrap(), Some(second));
    assert_eq!(
        files.history().unwrap(),
        vec![
            HistoryEntry {
                uri: "spotify:album:a1".to_owned(),
                name: "No Other".to_owned(),
                artist: "Gene Clark".to_owned(),
                round: "2026-W41".to_owned(),
                rejected: false,
            },
            HistoryEntry {
                uri: "spotify:album:b2".to_owned(),
                name: "#1 Record".to_owned(),
                artist: "Big Star".to_owned(),
                round: "2026-W42".to_owned(),
                rejected: false,
            },
        ]
    );
    let mut names: Vec<String> = fs::read_dir(dir.path().join("deck"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["curated-history.json", "curated.json"]);
}

#[test]
fn reject_removes_from_list_and_marks_history() {
    let dir = tempfile::tempdir().unwrap();
    let files = CuratedFiles::in_dir(dir.path());
    files
        .publish(&round(vec![
            entry("a1", "No Other", "Gene Clark", "x"),
            entry("b2", "#1 Record", "Big Star", "y"),
        ]))
        .unwrap();

    let updated = files.reject("spotify:album:a1").unwrap().unwrap();
    assert_eq!(updated.albums.len(), 1);
    assert_eq!(files.load().unwrap(), Some(updated));
    let history = files.history().unwrap();
    assert!(history[0].rejected);
    assert!(!history[1].rejected);

    // An unknown URI changes nothing.
    let same = files.reject("spotify:album:nope").unwrap().unwrap();
    assert_eq!(same.albums.len(), 1);
    assert_eq!(files.history().unwrap(), history);
}

#[test]
fn reject_adds_missing_history_entry() {
    let dir = tempfile::tempdir().unwrap();
    let files = CuratedFiles::in_dir(dir.path());
    save_json(
        &dir.path().join("curated.json"),
        &CuratedFile::from(&round(vec![entry("a1", "No Other", "Gene Clark", "x")])),
    )
    .unwrap();

    files.reject("spotify:album:a1").unwrap();
    let history = files.history().unwrap();
    assert_eq!(history.len(), 1);
    assert!(history[0].rejected);
    assert_eq!(history[0].round, "2026-W41");
}

#[test]
fn broken_files_are_errors_and_left_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let files = CuratedFiles::in_dir(dir.path());
    let broken = "{\"round\": ";
    fs::write(dir.path().join("curated.json"), broken).unwrap();
    let err = files.load().unwrap_err();
    assert!(format!("{err:#}").contains("is broken"), "{err:#}");
    assert!(files.reject("spotify:album:a1").is_err());
    assert_eq!(
        fs::read_to_string(dir.path().join("curated.json")).unwrap(),
        broken
    );

    // A broken history blocks both publishing and rejecting.
    let dir = tempfile::tempdir().unwrap();
    let files = CuratedFiles::in_dir(dir.path());
    files
        .publish(&round(vec![entry("a1", "No Other", "Gene Clark", "x")]))
        .unwrap();
    fs::write(dir.path().join("curated-history.json"), broken).unwrap();
    assert!(files.history().is_err());
    assert!(files.reject("spotify:album:a1").is_err());
    assert!(files.publish(&round(Vec::new())).is_err());
    assert_eq!(files.load().unwrap().unwrap().albums.len(), 1);
    assert_eq!(
        fs::read_to_string(dir.path().join("curated-history.json")).unwrap(),
        broken
    );
}

#[test]
fn reason_is_optional_in_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("curated.json"),
        r#"{"round":"2026-W41","created":"x","albums":[
            {"uri":"spotify:album:a1","name":"No Other","artist":"Gene Clark",
             "artist_id":"g","year":null}]}"#,
    )
    .unwrap();
    let curated = CuratedFiles::in_dir(dir.path()).load().unwrap().unwrap();
    assert_eq!(curated.albums[0].reason, None);
    assert_eq!(curated.albums[0].album.id, "a1");
}

#[test]
fn list_title_and_subtitle() {
    let list = round(vec![entry("a1", "No Other", "Gene Clark", "x")]).to_list(TEST_NOW);
    assert_eq!(list.name, "Curated");
    assert_eq!(list.subtitle, "1 album · week 41");

    let mut curated = round(vec![
        entry("a1", "No Other", "Gene Clark", "x"),
        entry("b2", "#1 Record", "Big Star", "y"),
    ]);
    curated.round = "2027-W03".to_owned();
    assert_eq!(curated.to_list(TEST_NOW).subtitle, "2 albums · week 3");
    assert_eq!(curated.to_list(TEST_NOW).albums, curated.albums);
    assert!(!curated.to_list(TEST_NOW).stale);
}

#[test]
fn list_older_than_a_week_is_stale_and_shows_its_age() {
    // The list was written at 2026-10-05 07:00 +03:00 = 04:00 UTC.
    let curated = round(vec![entry("a1", "No Other", "Gene Clark", "x")]);
    let at = |rfc3339: &str| curated.to_list(rfc3339.parse().unwrap());

    let week = at("2026-10-12T04:00:00Z");
    assert!(!week.stale);
    assert_eq!(week.subtitle, "1 album · week 41");

    let over_week = at("2026-10-12T04:00:01Z");
    assert!(over_week.stale);
    assert_eq!(over_week.subtitle, "1 album · week 41 · 7 days old");

    let nine_days = at("2026-10-14T09:00:00Z");
    assert_eq!(nine_days.subtitle, "1 album · week 41 · 9 days old");

    // An unparsable time does not make the list stale.
    let mut unknown = curated.clone();
    unknown.created = "x".to_owned();
    let list = unknown.to_list("2026-10-14T09:00:00Z".parse().unwrap());
    assert!(!list.stale);
    assert_eq!(list.subtitle, "1 album · week 41");
}

#[test]
fn normalize_rules() {
    assert_eq!(normalize("OK Computer"), "ok computer");
    assert_eq!(normalize("Abbey Road (Remastered 2009)"), "abbey road");
    assert_eq!(normalize("Pet Sounds [Deluxe Edition]"), "pet sounds");
    assert_eq!(
        normalize("Sgt. Pepper's Lonely Hearts Club Band"),
        "sgt peppers lonely hearts club band"
    );
    assert_eq!(normalize("Hüsker Dü"), "husker du");
    assert_eq!(normalize("Sigur Rós"), "sigur ros");
    assert_eq!(normalize("Simon & Garfunkel"), "simon and garfunkel");
    assert_eq!(normalize("AC/DC"), normalize("AC DC"));
    assert_eq!(normalize("  #1   Record "), "1 record");
    assert_eq!(normalize("Älymystö"), "alymysto");
    // Only brackets: their content stays in the comparison.
    assert_eq!(normalize("(What's the Story)"), "whats the story");
}

#[test]
fn same_album_compares_both_names() {
    assert!(same_album(
        "Radiohead",
        "OK Computer",
        "radiohead",
        "OK Computer (Collector's Edition)"
    ));
    assert!(same_album(
        "Gene Clark",
        "No Other",
        "Gene Clark",
        "No Other"
    ));
    assert!(!same_album(
        "Gene Clark",
        "No Other",
        "Gene Clark",
        "White Light"
    ));
    assert!(!same_album(
        "Radiohead",
        "OK Computer",
        "Radiohead",
        "OK Computer OKNOTOK 1997 2017"
    ));
    assert!(!same_album(
        "The Byrds",
        "No Other",
        "Gene Clark",
        "No Other"
    ));
}

#[test]
fn set_genres_fills_by_artist_and_keeps_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let files = CuratedFiles::in_dir(dir.path());
    files
        .publish(&round(vec![
            entry("a1", "No Other", "Gene Clark", "x"),
            entry("b2", "#1 Record", "Big Star", "y"),
        ]))
        .unwrap();
    let by_artist = HashMap::from([("id-gene-clark".to_owned(), vec!["country".to_owned()])]);
    files.set_genres(&by_artist).unwrap();

    let albums = files.load().unwrap().unwrap().albums;
    assert_eq!(albums[0].genres, ["country"]);
    assert!(albums[1].genres.is_empty());
    let text = fs::read_to_string(dir.path().join("curated.json")).unwrap();
    assert!(text.contains(r#""genres": ["#));
    // An album without genres writes none: older lists look as they did
    assert_eq!(text.matches("genres").count(), 1);
}

#[test]
fn set_genres_without_a_list_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let files = CuratedFiles::in_dir(dir.path());
    files.set_genres(&HashMap::new()).unwrap();
    assert!(!dir.path().join("curated.json").exists());
}
