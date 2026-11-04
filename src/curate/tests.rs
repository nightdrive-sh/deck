//! Tests of `deck curate submit` and `deck taste`, one file per topic in `tests/`. The
//! fixtures shared by several topics are here; a fixture used by one topic stays in its
//! file.

use super::*;

fn album(id: &str, artist: &str, name: &str, year: Option<u16>) -> Album {
    Album {
        id: id.to_owned(),
        uri: format!("spotify:album:{id}"),
        name: name.to_owned(),
        artist: artist.to_owned(),
        artist_id: format!("id-{artist}"),
        year,
    }
}

fn found(id: &str, artist: &str, name: &str, year: u16, album_type: &str) -> FoundAlbum {
    FoundAlbum {
        album: album(id, artist, name, Some(year)),
        artists: vec![artist.to_owned()],
        album_type: album_type.to_owned(),
    }
}

fn candidate(artist: &str, album: &str) -> Candidate {
    Candidate {
        artist: artist.to_owned(),
        album: album.to_owned(),
        reason: format!("Because {album}."),
        genre: String::new(),
    }
}

fn history(uri: &str, artist: &str, name: &str) -> HistoryEntry {
    HistoryEntry {
        uri: uri.to_owned(),
        name: name.to_owned(),
        artist: artist.to_owned(),
        round: "2026-W40".to_owned(),
        rejected: false,
    }
}

/// One candidate whose only hits are `hit`.
fn check_one(
    hit: Vec<FoundAlbum>,
    shelf: &[Album],
    history: &[HistoryEntry],
    listened: &[TopAlbum],
) -> Report {
    check(
        &[candidate("Gene Clark", "No Other")],
        &[Some(hit)],
        shelf,
        history,
        listened,
    )
    .0
}

fn reasons(report: &Report) -> Vec<Rejection> {
    report.rejected.iter().map(|r| r.reason).collect()
}

fn no_other() -> Vec<FoundAlbum> {
    vec![found("noother", "Gene Clark", "No Other", 1974, "album")]
}

const NOW: u64 = 1_800_000_000;

mod rules;
mod searches;
mod taste;
