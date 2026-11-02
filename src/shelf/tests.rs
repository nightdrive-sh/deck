//! Tests of the shelf, one file per topic in `tests/`. The fixtures shared by both
//! topics are here.

use std::fs;

use super::*;

fn album(id: &str, name: &str, artist: &str, year: Option<u16>) -> Album {
    Album {
        id: id.to_owned(),
        uri: format!("spotify:album:{id}"),
        name: name.to_owned(),
        artist: artist.to_owned(),
        artist_id: format!("id-{}", artist.to_lowercase().replace(' ', "-")),
        year,
    }
}

fn names(albums: &[Album]) -> Vec<&str> {
    albums.iter().map(|a| a.name.as_str()).collect()
}

fn shelf_in(dir: &tempfile::TempDir) -> (PathBuf, Shelf) {
    let path = dir.path().join("deck/shelf.json");
    let shelf = Shelf::load(&path).unwrap();
    (path, shelf)
}

mod file;
mod order;
