//! The favourites file: saving, unknown ids, a broken file and edits made by hand.

use std::path::PathBuf;

use super::*;
use crate::genres::favorites::FavoritesFile;

fn favorites_in(dir: &tempfile::TempDir) -> (PathBuf, Favorites) {
    let path = dir.path().join("deck/genres.json");
    let favorites = Favorites::load(&path).unwrap();
    (path, favorites)
}

fn file_ids(path: &Path) -> Vec<String> {
    let file: FavoritesFile =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    file.favorites
}

#[test]
fn missing_file_means_no_favorites() {
    let dir = tempfile::tempdir().unwrap();
    let (path, favorites) = favorites_in(&dir);
    assert!(favorites.genres().is_empty());
    assert!(!path.exists());
}

#[test]
fn add_saves_and_reloads() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut favorites) = favorites_in(&dir);
    assert!(favorites.add("synthwave").unwrap());
    assert!(favorites.add("lofi").unwrap());
    assert!(!favorites.add("lofi").unwrap());
    assert_eq!(file_ids(&path), ["synthwave", "lofi"]);
    assert_eq!(dir_files(&path), ["genres.json"]);

    let reloaded = Favorites::load(&path).unwrap();
    let ids: Vec<&str> = reloaded.genres().iter().map(|g| g.id.as_str()).collect();
    assert_eq!(ids, ["lofi", "synthwave"]);
}

#[test]
fn add_rejects_unknown_genre() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut favorites) = favorites_in(&dir);
    assert!(favorites.add("polka-noir").is_err());
    assert!(!favorites.contains("polka-noir"));
    assert!(!path.exists());
}

#[test]
fn remove_saves() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut favorites) = favorites_in(&dir);
    favorites.add("lofi").unwrap();
    favorites.add("synthwave").unwrap();
    assert!(favorites.remove("lofi").unwrap());
    assert!(!favorites.remove("lofi").unwrap());
    assert_eq!(file_ids(&path), ["synthwave"]);
    assert!(!Favorites::load(&path).unwrap().contains("lofi"));
}

#[test]
fn unknown_id_is_hidden_but_kept_in_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("genres.json");
    std::fs::write(&path, r#"{"favorites": ["polka-noir", "lofi"]}"#).unwrap();
    let mut favorites = Favorites::load(&path).unwrap();
    let ids: Vec<&str> = favorites.genres().iter().map(|g| g.id.as_str()).collect();
    assert_eq!(ids, ["lofi"]);

    favorites.add("synthwave").unwrap();
    assert_eq!(file_ids(&path), ["polka-noir", "lofi", "synthwave"]);
    assert!(favorites.remove("polka-noir").unwrap());
    assert_eq!(file_ids(&path), ["lofi", "synthwave"]);
}

#[test]
fn broken_file_is_an_error_and_never_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("genres.json");
    std::fs::write(&path, "{ not json").unwrap();
    let e = format!("{:#}", Favorites::load(&path).unwrap_err());
    assert!(e.contains("genre favorites file"), "{e}");

    let mut favorites = Favorites::unavailable();
    assert!(favorites.genres().is_empty());
    assert!(favorites.add("lofi").is_err());
    assert!(!favorites.contains("lofi"));
    assert!(!favorites.remove("lofi").unwrap());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
}

#[test]
fn changes_keep_edits_made_by_hand() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut favorites) = favorites_in(&dir);
    favorites.add("lofi").unwrap();
    std::fs::write(&path, r#"{"favorites": ["lofi", "synthwave"]}"#).unwrap();

    favorites.remove("lofi").unwrap();
    assert_eq!(file_ids(&path), ["synthwave"]);
    assert!(favorites.contains("synthwave"));

    // A file broken by hand is an error and is left as it is.
    std::fs::write(&path, "{ not json").unwrap();
    assert!(favorites.add("lofi").is_err());
    assert!(favorites.remove("synthwave").is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
    assert!(favorites.contains("synthwave"));
}

#[test]
fn failed_save_keeps_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("genres.json");
    let mut favorites = Favorites::load(&path).unwrap();
    // A directory where the file should be: saving fails.
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("keep"), "").unwrap();
    assert!(favorites.add("lofi").is_err());
    assert!(!favorites.contains("lofi"));
}
