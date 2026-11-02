//! The check rules: what is accepted, and why the rest is rejected.

use super::*;

#[test]
fn accepts_matching_album_with_reason() {
    let (report, albums) = check(
        &[candidate("gene clark", "No Other (Remastered)")],
        &[Some(no_other())],
        &[],
        &[],
        &[],
    );
    assert_eq!(
        report.accepted,
        vec![Accepted {
            artist: "Gene Clark".to_owned(),
            album: "No Other".to_owned(),
            year: Some(1974),
            uri: "spotify:album:noother".to_owned(),
        }]
    );
    assert!(report.rejected.is_empty());
    assert_eq!(report.missing, 19);
    assert_eq!(albums[0].album.uri, "spotify:album:noother");
    assert_eq!(
        albums[0].reason.as_deref(),
        Some("Because No Other (Remastered).")
    );
}

#[test]
fn not_found_when_no_hit_matches() {
    let report = check_one(Vec::new(), &[], &[], &[]);
    assert_eq!(reasons(&report), [Rejection::NotFound]);
    assert_eq!(report.rejected[0].uri, None);

    // A wrong artist or a wrong album is not accepted.
    let wrong = vec![
        found("x", "Gene Clark", "White Light", 1971, "album"),
        found("y", "The Byrds", "No Other", 1974, "album"),
    ];
    assert_eq!(
        reasons(&check_one(wrong, &[], &[], &[])),
        [Rejection::NotFound]
    );
}

#[test]
fn on_shelf_by_uri_or_name() {
    let by_uri = [album("noother", "Someone", "Else", None)];
    let report = check_one(no_other(), &by_uri, &[], &[]);
    assert_eq!(reasons(&report), [Rejection::OnShelf]);
    assert_eq!(
        report.rejected[0].uri.as_deref(),
        Some("spotify:album:noother")
    );

    let by_name = [album(
        "other-id",
        "Gene Clark",
        "No Other (Deluxe)",
        Some(1974),
    )];
    assert_eq!(
        reasons(&check_one(no_other(), &by_name, &[], &[])),
        [Rejection::OnShelf]
    );
}

#[test]
fn suggested_before_by_uri_or_name() {
    let by_uri = [history("spotify:album:noother", "x", "y")];
    assert_eq!(
        reasons(&check_one(no_other(), &[], &by_uri, &[])),
        [Rejection::SuggestedBefore]
    );
    let by_name = [history("spotify:album:other", "Gene Clark", "No Other")];
    assert_eq!(
        reasons(&check_one(no_other(), &[], &by_name, &[])),
        [Rejection::SuggestedBefore]
    );
}

#[test]
fn listened_needs_three_plays() {
    let listened = |plays| TopAlbum {
        artist: "Gene Clark".to_owned(),
        album: "No Other".to_owned(),
        plays,
    };
    assert_eq!(
        reasons(&check_one(no_other(), &[], &[], &[listened(3)])),
        [Rejection::Listened]
    );
    assert!(
        check_one(no_other(), &[], &[], &[listened(2)])
            .rejected
            .is_empty()
    );
}

#[test]
fn duplicate_within_the_list() {
    let (report, albums) = check(
        &[
            candidate("Gene Clark", "No Other"),
            candidate("Gene Clark", "No Other (2003 Remaster)"),
        ],
        &[Some(no_other()), Some(no_other())],
        &[],
        &[],
        &[],
    );
    assert_eq!(report.accepted.len(), 1);
    assert_eq!(albums.len(), 1);
    assert_eq!(reasons(&report), [Rejection::Duplicate]);
}

#[test]
fn first_matching_rule_wins() {
    // On the shelf, in the history and listened: the shelf decides.
    let report = check_one(
        no_other(),
        &[album("noother", "Gene Clark", "No Other", None)],
        &[history("spotify:album:noother", "Gene Clark", "No Other")],
        &[TopAlbum {
            artist: "Gene Clark".to_owned(),
            album: "No Other".to_owned(),
            plays: 10,
        }],
    );
    assert_eq!(reasons(&report), [Rejection::OnShelf]);
}

#[test]
fn picks_original_album_over_compilation_and_single() {
    let hits = vec![
        found("single", "Gene Clark", "No Other", 1974, "single"),
        found("reissue", "Gene Clark", "No Other (Deluxe)", 2019, "album"),
        found("comp", "Gene Clark", "No Other", 1970, "compilation"),
        found("orig", "Gene Clark", "No Other", 1974, "album"),
    ];
    let (report, _) = check(
        &[candidate("Gene Clark", "No Other")],
        &[Some(hits)],
        &[],
        &[],
        &[],
    );
    assert_eq!(report.accepted[0].uri, "spotify:album:orig");

    // A single alone is accepted when there is no album.
    let hits = vec![found("single", "Gene Clark", "No Other", 1974, "single")];
    let (report, _) = check(
        &[candidate("Gene Clark", "No Other")],
        &[Some(hits)],
        &[],
        &[],
        &[],
    );
    assert_eq!(report.accepted[0].uri, "spotify:album:single");
}

#[test]
fn matches_any_album_artist() {
    let mut hit = found("csn", "Crosby, Stills & Nash", "CSN", 1977, "album");
    hit.artists = vec!["David Crosby".to_owned(), "Graham Nash".to_owned()];
    let (report, _) = check(
        &[candidate("Graham Nash", "CSN")],
        &[Some(vec![hit])],
        &[],
        &[],
        &[],
    );
    assert_eq!(report.accepted.len(), 1);
}

#[test]
fn keeps_order_and_caps_at_twenty() {
    let candidates: Vec<Candidate> = (0..25)
        .map(|i| candidate(&format!("Artist {i}"), &format!("Album {i}")))
        .collect();
    let hits: Vec<Option<Vec<FoundAlbum>>> = (0..25)
        .map(|i| {
            Some(vec![found(
                &format!("a{i}"),
                &format!("Artist {i}"),
                &format!("Album {i}"),
                2000,
                "album",
            )])
        })
        .collect();
    // The second candidate is on the shelf.
    let shelf = [album("a1", "Artist 1", "Album 1", None)];
    let (report, albums) = check(&candidates, &hits, &shelf, &[], &[]);

    assert_eq!(report.accepted.len(), MAX_ALBUMS);
    assert_eq!(report.missing, 0);
    assert_eq!(albums.len(), MAX_ALBUMS);
    let order: Vec<&str> = report.accepted.iter().map(|a| a.album.as_str()).collect();
    assert_eq!(order[..3], ["Album 0", "Album 2", "Album 3"]);
    assert_eq!(order[19], "Album 20");
    let unused: Vec<&str> = report.unused.iter().map(|a| a.album.as_str()).collect();
    assert_eq!(unused, ["Album 21", "Album 22", "Album 23", "Album 24"]);
    assert_eq!(reasons(&report), [Rejection::OnShelf]);
}

#[test]
fn report_json_shape() {
    let (report, _) = check(
        &[
            candidate("Gene Clark", "No Other"),
            candidate("Nobody", "Nothing"),
        ],
        &[Some(no_other()), Some(Vec::new())],
        &[],
        &[],
        &[],
    );
    assert_eq!(
        serde_json::to_value(&report).unwrap(),
        serde_json::json!({
            "accepted": [{
                "artist": "Gene Clark",
                "album": "No Other",
                "year": 1974,
                "uri": "spotify:album:noother",
            }],
            "rejected": [{"artist": "Nobody", "album": "Nothing", "reason": "not_found"}],
            "missing": 19,
            "searched": 0,
            "searches_left": 0,
        })
    );
}

#[test]
fn candidates_parse_without_reason() {
    let parsed: Vec<Candidate> =
        serde_json::from_str(r#"[{"artist":"Gene Clark","album":"No Other"}]"#).unwrap();
    assert_eq!(parsed[0].reason, "");
    let (_, albums) = check(&parsed, &[Some(no_other())], &[], &[], &[]);
    assert_eq!(albums[0].reason, None);
}
