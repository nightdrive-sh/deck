//! Tests of `deck genre`, one file per topic in `tests/`. The fixtures shared by
//! several topics are here; a fixture used by one topic stays in its file.

use super::*;
use crate::genres::Seed;
use check::SeedInput;

const NOW: u64 = 1_800_000_000;
const TROGGS: &str = "spotify:artist:57xdnSVt4ahJCIXYLieQ25";
const TROGSS: &str = "spotify:artist:6oc0uG7r137XS4DUc6UQU9";
const LOVE: &str = "spotify:artist:3Q6OOkfssqoMSTtl11J5Uk";
const ZOMBIES: &str = "spotify:track:5BATmTqGopeifUzHN2bE0f";
const QUIET: &str = "spotify:artist:1YqGsKpdixxSVgpfaL2AEQ";

fn seed(artist: &str, track: Option<&str>, uri: Option<&str>) -> SeedInput {
    SeedInput {
        artist: artist.into(),
        track: track.map(str::to_owned),
        uri: uri.map(str::to_owned),
    }
}

mod commands;
mod seeds;
