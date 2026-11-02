//! The subcommands `deck genre add | remove | list | prompt`: the user's AI adds own
//! genres ([`MyGenres`]) following the instructions in `genres/prompt.md`.
//!
//! The check is [`check()`]: seeds → accepted, and rejected with reasons. Spotify is
//! behind the [`Spotify`](check::Spotify) trait so the rules can be tested without the
//! network. URIs are checked with librespot (metadata and the apollo station), which
//! does not use the Web API. Only a seed given by name is searched on the Web API, with
//! Curated's search cap and cache ([`SearchCache`]); the first 429 ends the run.

use std::{
    fmt,
    io::{IsTerminal, Read},
};

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::{
    config::Config,
    curate::{SEARCH_PAUSE, SearchCache},
    genres::{self, Genre, MyGenres, slug},
    store::unix_now,
};

mod check;
mod live;

use check::{Accepted, Input, Rejected, check};
use live::Live;

/// Instructions for the AI, `deck genre prompt`.
pub const PROMPT: &str = include_str!("../genres/prompt.md");
/// Usage of `deck genre`: shown for a bare `deck genre` or an unknown subcommand.
pub const USAGE: &str = "\
usage: deck genre add [--dry-run] < genre.json
       deck genre remove <name>
       deck genre list
       deck genre prompt

Own genres are put together with an AI: `deck genre prompt` prints its instructions.
Run `deck genre add` without input to see an example genre.json.
";
/// Usage of `deck genre add` when stdin is a terminal and no input is redirected.
pub const ADD_USAGE: &str = r#"usage: deck genre add [--dry-run] < genre.json

Reads a genre as JSON from stdin, checks its seeds on Spotify and prints a report.
Without --dry-run the genre is saved if at least 3 seeds are accepted.

Example genre.json, one seed with a Spotify URI and one by the artist's name:

  {"name": "60s rock", "seeds": [
    {"artist": "The Troggs", "uri": "spotify:artist:57xdnSVt4ahJCIXYLieQ25"},
    {"artist": "Love"}
  ]}

Tip: `deck genre prompt` prints instructions for an AI that puts a genre together.
`deck genre list` shows all genres, `deck genre remove <name>` removes an own genre.
"#;

/// A command-line usage error: `main` prints the usage as it is and exits with code 2.
#[derive(Debug)]
pub struct Usage(pub &'static str);

impl fmt::Display for Usage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Usage {}

/// A genre is saved only once this many seeds have been accepted.
pub const MIN_SEEDS: usize = 3;
/// At most this many seeds in one genre.
pub const MAX_SEEDS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Replaces {
    /// A genre shipped with Deck.
    BuiltIn,
    /// An earlier own genre.
    Own,
}

/// Output of `deck genre add`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaces: Option<Replaces>,
    pub accepted: Vec<Accepted>,
    pub rejected: Vec<Rejected>,
    /// [`MIN_SEEDS`] − the number of accepted seeds.
    pub missing: usize,
    /// Web API searches in this run (cache misses).
    pub searched: usize,
    pub searches_left: usize,
    pub written: bool,
}
/// The genre's name and id from the input: the name in lowercase, as in the catalog.
fn genre_name(input: &Input) -> Result<(String, String)> {
    let name = input.name.trim().to_lowercase();
    let id = slug(&name);
    if id.is_empty() {
        bail!("the genre needs a name");
    }
    if input.seeds.is_empty() || input.seeds.len() > MAX_SEEDS {
        bail!("give 1–{MAX_SEEDS} seeds, not {}", input.seeds.len());
    }
    Ok((id, name))
}

/// Whether the genre replaces a catalog genre or an earlier own genre.
fn replaces(id: &str, my: &MyGenres, catalog: &[Genre]) -> Option<Replaces> {
    if my.find(id).is_some() {
        Some(Replaces::Own)
    } else if catalog.iter().any(|g| g.id == id) {
        Some(Replaces::BuiltIn)
    } else {
        None
    }
}

/// `deck genre add [--dry-run]`: reads a genre from stdin, prints a report and
/// (without `--dry-run`) saves the genre if at least [`MIN_SEEDS`] seeds are accepted.
pub async fn add(dry_run: bool) -> Result<()> {
    let stdin = std::io::stdin();
    let text = read_input(stdin.is_terminal(), stdin)?;
    let input: Input = serde_json::from_str(&text).context(
        "stdin must be JSON {\"name\", \"seeds\": [{\"artist\", \"track\"?, \"uri\"?}]}",
    )?;
    let (id, name) = genre_name(&input)?;

    // A broken own-genres file stops the run before Spotify is contacted.
    let my_path = MyGenres::default_path()?;
    let mut my = MyGenres::load(&my_path)?;
    let replaces = replaces(&id, &my, &genres::catalog());

    let cache_path = SearchCache::default_path()?;
    let mut cache = SearchCache::load(&cache_path, unix_now());
    let mut spotify = Live::connect(Config::load()?).await?;
    let checked = check(
        &input.seeds,
        &mut cache,
        unix_now(),
        SEARCH_PAUSE,
        &mut spotify,
    )
    .await;
    // Keep the searches and the 429 block of an interrupted run too.
    cache.save(&cache_path)?;
    let checked = checked?;

    let report_seeds = checked.seeds.len();
    let enough = report_seeds >= MIN_SEEDS;
    let written = !dry_run && enough;
    if written {
        my.put(Genre {
            id: id.clone(),
            name: name.clone(),
            seeds: checked.seeds,
        })?;
    }
    let report = Report {
        id,
        name: name.clone(),
        replaces,
        accepted: checked.accepted,
        rejected: checked.rejected,
        missing: MIN_SEEDS.saturating_sub(report_seeds),
        searched: checked.searched,
        searches_left: cache.searches_left(),
        written,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    if written {
        eprintln!(
            "Saved {name} to {}. Restart Deck to see it in Genres.",
            my_path.display()
        );
    } else if !dry_run {
        bail!("at least {MIN_SEEDS} seeds must be accepted, the genre was not saved");
    }
    Ok(())
}

/// Reads the input of `deck genre add`. A terminal is not read: without redirection
/// the command would wait for typing, so it returns the usage ([`ADD_USAGE`]).
fn read_input(is_terminal: bool, mut stdin: impl Read) -> Result<String> {
    if is_terminal {
        return Err(Usage(ADD_USAGE).into());
    }
    let mut text = String::new();
    stdin
        .read_to_string(&mut text)
        .context("cannot read stdin")?;
    Ok(text)
}

/// `deck genre remove <name>`: removes an own genre. A catalog genre cannot be removed.
pub fn remove(name: &str) -> Result<()> {
    let id = slug(name);
    let path = MyGenres::default_path()?;
    let mut my = MyGenres::load(&path)?;
    let built_in = genres::catalog().iter().any(|g| g.id == id);
    if my.remove(&id)? {
        let back = if built_in {
            " The built-in version is back."
        } else {
            ""
        };
        eprintln!("Removed {name}.{back} Restart Deck to see the change.");
        Ok(())
    } else if built_in {
        bail!("{name} is a built-in genre and cannot be removed")
    } else {
        bail!("there is no own genre named {name}")
    }
}

#[derive(Serialize)]
struct Listed<'a> {
    #[serde(flatten)]
    genre: &'a Genre,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    own: bool,
}

/// `deck genre list`: all genres with their seeds as JSON, own ones with `"own": true`.
pub fn list() -> Result<()> {
    let my = MyGenres::load(&MyGenres::default_path()?)?;
    let all = genres::merge(genres::catalog(), my.genres());
    let listed: Vec<Listed> = all
        .iter()
        .map(|genre| Listed {
            genre,
            own: my.find(&genre.id).is_some(),
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&listed)?);
    Ok(())
}
#[cfg(test)]
mod tests;
