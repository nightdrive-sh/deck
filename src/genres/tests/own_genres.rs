//! The own genres file: missing, saved, broken, and a failed save.

use super::*;

#[test]
fn missing_my_genres_file_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    let my = MyGenres::load(&dir.path().join("my-genres.json")).unwrap();
    assert!(my.genres().is_empty());
}

#[test]
fn my_genres_put_replaces_and_remove_saves() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deck/my-genres.json");
    let mut my = MyGenres::load(&path).unwrap();
    my.put(own("60s rock", 3)).unwrap();
    my.put(own("lofi", 3)).unwrap();
    my.put(own("60s rock", 4)).unwrap();

    let reloaded = MyGenres::load(&path).unwrap();
    assert_eq!(reloaded.genres().len(), 2);
    assert_eq!(reloaded.find("60s-rock").unwrap().seeds.len(), 4);

    assert!(my.remove("lofi").unwrap());
    assert!(!my.remove("lofi").unwrap());
    let reloaded = MyGenres::load(&path).unwrap();
    assert!(reloaded.find("lofi").is_none());
    assert_eq!(dir_files(&path), ["my-genres.json"]);
}

#[test]
fn broken_my_genres_file_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("my-genres.json");
    let cases = [
        ("{ not json", "my genres file"),
        (
            r#"{"genres": [{"id": "x", "name": "60s rock", "seeds": [{"uri": "spotify:artist:5FsfZj0Mp6YwEWytuJUcWt", "name": "x"}]}]}"#,
            "id must be 60s-rock",
        ),
        (
            r#"{"genres": [{"id": "a", "name": "a", "seeds": []}]}"#,
            "no seeds",
        ),
    ];
    for (text, error) in cases {
        std::fs::write(&path, text).unwrap();
        let e = format!("{:#}", MyGenres::load(&path).unwrap_err());
        assert!(e.contains("my genres file") && e.contains(error), "{e}");
    }
}

#[test]
fn failed_put_keeps_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("my-genres.json");
    let mut my = MyGenres::load(&path).unwrap();
    // A directory where the file should be: saving fails.
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("keep"), "").unwrap();
    assert!(my.put(own("60s rock", 3)).is_err());
    assert!(my.genres().is_empty());
}
