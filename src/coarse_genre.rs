//! A rough genre for an album on a list: its genres or its artist's ("nu jazz",
//! "jazz fusion", "neo soul") reduced to one to three coarse words ("jazz", "soul"),
//! short enough to sit at the end of a Curated row. Its sources are the curator's genre and
//! Last.fm's tags (`curate::genres`); Spotify no longer gives its own apps genres.
//!
//! The coarse words are the genre radios' names where they are a genre of their own
//! (synthwave, shoegaze, bossa nova, see `genres/genres.json`) and the broad families
//! around them (jazz, soul, rock). The table is ordered from the specific to the broad:
//! a genre takes the first word whose pattern it contains, so "jazz funk" is jazz
//! and "indie rock" indie, not rock.

/// A coarse word and the patterns (whole words of a genre) that lead to it.
const TABLE: &[(&str, &[&str])] = &[
    (
        "synthwave",
        &[
            "synthwave",
            "retrowave",
            "outrun",
            "darksynth",
            "chillsynth",
        ],
    ),
    ("chillwave", &["chillwave"]),
    ("shoegaze", &["shoegaze", "nu gaze"]),
    ("dream pop", &["dream pop"]),
    ("post-rock", &["post-rock", "post rock"]),
    ("post-punk", &["post-punk", "post punk"]),
    ("trip hop", &["trip hop"]),
    ("bossa nova", &["bossa nova", "mpb", "samba"]),
    ("britpop", &["britpop"]),
    ("emo", &["emo"]),
    ("grunge", &["grunge"]),
    ("k-pop", &["k-pop"]),
    ("lofi", &["lo-fi", "lofi"]),
    ("suomirock", &["suomirock", "finnish rock"]),
    ("suomipop", &["suomipop", "finnish pop", "iskelmä"]),
    ("hip hop", &["hip hop", "rap", "boom bap", "trap"]),
    ("jazz", &["jazz", "bebop", "hard bop", "swing", "big band"]),
    ("soul", &["soul", "motown"]),
    ("r&b", &["r&b"]),
    ("funk", &["funk"]),
    ("disco", &["disco"]),
    ("house", &["house"]),
    ("techno", &["techno"]),
    ("ambient", &["ambient", "drone"]),
    ("new wave", &["new wave", "synthpop", "synth-pop"]),
    (
        "electronic",
        &["electronic", "electronica", "idm", "downtempo", "edm"],
    ),
    ("metal", &["metal"]),
    ("punk", &["punk", "hardcore"]),
    ("folk", &["folk", "singer-songwriter"]),
    ("country", &["country", "americana", "bluegrass"]),
    ("blues", &["blues"]),
    ("reggae", &["reggae", "dub", "ska"]),
    (
        "classical",
        &["classical", "baroque", "opera", "orchestral"],
    ),
    ("psych", &["psychedelic", "neo-psychedelia", "psych"]),
    ("prog", &["progressive", "prog"]),
    ("indie", &["indie"]),
    ("pop", &["pop"]),
    ("rock", &["rock"]),
];

/// At most this many words.
const MAX_WORDS: usize = 3;

/// Words too broad to say much alone: when one of them comes first, the next ones are
/// shown with it ("rock, psych"). A narrower word first is shown alone ("jazz", "emo").
const BROAD: &[&str] = &["rock", "pop", "indie", "electronic"];

/// The coarse words for an artist's genres, most common first: the first alone when it is
/// narrow enough, else with up to two more ([`BROAD`]). Empty when none of the genres is
/// in the table (or there are none).
pub fn coarse(genres: &[String]) -> Vec<&'static str> {
    // (word, how many of the genres lead to it, the first genre's index)
    let mut votes: Vec<(&'static str, usize, usize)> = Vec::new();
    for (index, genre) in genres.iter().enumerate() {
        let Some(word) = word_for(genre) else {
            continue;
        };
        match votes.iter_mut().find(|(w, ..)| *w == word) {
            Some((_, count, _)) => *count += 1,
            None => votes.push((word, 1, index)),
        }
    }
    // Spotify and Last.fm list the most typical genres first: a tie goes to the earlier one.
    votes.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
    let alone = votes.first().is_some_and(|v| !BROAD.contains(&v.0));
    let shown = if alone { 1 } else { MAX_WORDS };
    votes.into_iter().take(shown).map(|v| v.0).collect()
}

/// The first coarse word whose pattern is in the genre as whole words.
fn word_for(genre: &str) -> Option<&'static str> {
    let words: Vec<String> = genre.split_whitespace().map(str::to_lowercase).collect();
    TABLE
        .iter()
        .find(|(_, patterns)| patterns.iter().any(|p| contains_words(&words, p)))
        .map(|(word, _)| *word)
}

/// Whether the pattern's words come one after another in `words`.
fn contains_words(words: &[String], pattern: &str) -> bool {
    let pattern: Vec<&str> = pattern.split_whitespace().collect();
    words
        .windows(pattern.len())
        .any(|window| window.iter().zip(&pattern).all(|(w, p)| w == p))
}

#[cfg(test)]
mod tests;
