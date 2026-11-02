//! Tests of the genres, one file per topic in `tests/`. The fixtures shared by several
//! topics are here; a fixture used by one topic stays in its file.

use std::path::Path;

use super::*;

fn seed(n: u32) -> Seed {
    Seed {
        uri: format!("spotify:artist:{n:022}"),
        name: format!("Artist {n}"),
    }
}

/// The files in `path`'s directory: saving leaves no temporary files behind.
fn dir_files(path: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn own(name: &str, seeds: u32) -> Genre {
    Genre {
        id: slug(name),
        name: name.into(),
        seeds: (0..seeds).map(seed).collect(),
    }
}

mod catalog;
mod favorites_file;
mod own_genres;
