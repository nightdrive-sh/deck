//! The shelf file: saving, reloading, a broken file, a read-only shelf and edits made by
//! hand.

use super::*;

#[test]
fn missing_file_is_empty_shelf() {
    let dir = tempfile::tempdir().unwrap();
    let (path, shelf) = shelf_in(&dir);
    assert!(artists_of(shelf.albums()).is_empty());
    assert!(!path.exists());
}

#[test]
fn add_and_remove_survive_reload() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut shelf) = shelf_in(&dir);
    let colour = album(
        "c0l0ur",
        "The Colour And The Shape",
        "Foo Fighters",
        Some(1997),
    );
    let wasting = album("w4st1ng", "Wasting Light", "Foo Fighters", Some(2011));

    assert!(shelf.add(colour.clone()).unwrap());
    assert!(shelf.add(wasting.clone()).unwrap());

    let mut reloaded = Shelf::load(&path).unwrap();
    assert_eq!(
        albums_of(reloaded.albums(), &colour.artist_id),
        vec![colour.clone(), wasting]
    );

    assert!(reloaded.remove(&colour.uri).unwrap());
    let reloaded = Shelf::load(&path).unwrap();
    assert_eq!(
        names(&albums_of(reloaded.albums(), &colour.artist_id)),
        ["Wasting Light"]
    );
    assert!(!reloaded.contains(&colour.uri));
}

#[test]
fn file_has_spec_fields() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut shelf) = shelf_in(&dir);
    shelf
        .add(album("abc", "Nevermind", "Nirvana", Some(1991)))
        .unwrap();

    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        json,
        serde_json::json!([{
            "uri": "spotify:album:abc",
            "name": "Nevermind",
            "artist": "Nirvana",
            "artist_id": "id-nirvana",
            "year": 1991,
        }])
    );
    let files: Vec<_> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(files, ["shelf.json"]);
}

#[test]
fn adding_twice_keeps_one_copy() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut shelf) = shelf_in(&dir);
    let a = album("abc", "Nevermind", "Nirvana", Some(1991));
    assert!(shelf.add(a.clone()).unwrap());
    assert!(!shelf.add(a.clone()).unwrap());
    assert_eq!(
        albums_of(Shelf::load(&path).unwrap().albums(), &a.artist_id).len(),
        1
    );
}

#[test]
fn removing_missing_album_does_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut shelf) = shelf_in(&dir);
    assert!(!shelf.remove("spotify:album:nope").unwrap());
    assert!(!path.exists());
}

#[test]
fn album_id_comes_from_uri_on_load() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut shelf) = shelf_in(&dir);
    shelf
        .add(album("4bc", "Nevermind", "Nirvana", Some(1991)))
        .unwrap();
    let loaded = albums_of(Shelf::load(&path).unwrap().albums(), "id-nirvana");
    assert_eq!(loaded[0].id, "4bc");
}

#[test]
fn broken_file_is_error_and_left_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shelf.json");
    let broken = "[{\"uri\": \"spotify:album:abc\", \"name\": ";
    fs::write(&path, broken).unwrap();

    let err = Shelf::load(&path).unwrap_err();
    assert!(format!("{err:#}").contains("is broken"), "{err:#}");
    assert_eq!(fs::read_to_string(&path).unwrap(), broken);
}

#[test]
fn wrong_shape_is_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shelf.json");
    fs::write(&path, r#"{"albums": []}"#).unwrap();
    assert!(Shelf::load(&path).is_err());
}

#[test]
fn unavailable_shelf_is_read_only() {
    let mut shelf = Shelf::unavailable();
    let a = album("abc", "Nevermind", "Nirvana", Some(1991));
    assert!(shelf.add(a.clone()).is_err());
    assert!(!shelf.contains(&a.uri));
    assert!(artists_of(shelf.albums()).is_empty());
    assert!(!shelf.remove(&a.uri).unwrap());
}

#[test]
fn failed_save_rolls_back() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let (path, mut shelf) = shelf_in(&dir);
    let a = album("abc", "Nevermind", "Nirvana", Some(1991));
    shelf.add(a.clone()).unwrap();
    // The directory becomes read-only: the file can be read but not saved.
    let deck = path.parent().unwrap();
    fs::set_permissions(deck, fs::Permissions::from_mode(0o500)).unwrap();
    let b = album("def", "Bleach", "Nirvana", Some(1989));
    let added = shelf.add(b.clone());
    let removed = shelf.remove(&a.uri);
    fs::set_permissions(deck, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(added.is_err());
    assert!(removed.is_err());
    assert!(!shelf.contains(&b.uri));
    assert!(shelf.contains(&a.uri));
}

#[test]
fn changes_keep_edits_made_by_hand() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut shelf) = shelf_in(&dir);
    let nevermind = album("abc", "Nevermind", "Nirvana", Some(1991));
    let bleach = album("def", "Bleach", "Nirvana", Some(1989));
    shelf.add(nevermind.clone()).unwrap();

    // The user adds an album by hand while Deck is open.
    let mut by_hand = Shelf::load(&path).unwrap();
    by_hand.add(bleach.clone()).unwrap();

    shelf
        .add(album("ghi", "In Utero", "Nirvana", Some(1993)))
        .unwrap();
    assert_eq!(names(shelf.albums()), ["Nevermind", "Bleach", "In Utero"]);
    assert!(shelf.remove(&nevermind.uri).unwrap());
    assert_eq!(
        names(Shelf::load(&path).unwrap().albums()),
        ["Bleach", "In Utero"]
    );
}

#[test]
fn file_broken_by_hand_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut shelf) = shelf_in(&dir);
    let nevermind = album("abc", "Nevermind", "Nirvana", Some(1991));
    shelf.add(nevermind.clone()).unwrap();
    fs::write(&path, "[{").unwrap();

    let error = shelf
        .add(album("def", "Bleach", "Nirvana", Some(1989)))
        .unwrap_err();
    assert!(format!("{error:#}").contains("is broken"), "{error:#}");
    assert!(shelf.remove(&nevermind.uri).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "[{");
    assert_eq!(names(shelf.albums()), ["Nevermind"]);
}
