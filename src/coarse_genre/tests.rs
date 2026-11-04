use super::*;

fn of(genres: &[&str]) -> Vec<&'static str> {
    coarse(&genres.iter().map(|g| g.to_string()).collect::<Vec<_>>())
}

#[test]
fn fine_genres_become_one_coarse_word() {
    assert_eq!(of(&["nu jazz", "jazz fusion", "vocal jazz"]), ["jazz"]);
    assert_eq!(of(&["neo soul", "classic soul", "motown"]), ["soul"]);
    assert_eq!(of(&["synthwave", "retrowave", "outrun"]), ["synthwave"]);
}

#[test]
fn the_specific_word_wins_over_the_broad_one() {
    // "jazz funk" is jazz, "indie rock" indie, "dream pop" not pop
    assert_eq!(of(&["jazz funk"]), ["jazz"]);
    assert_eq!(of(&["indie rock"]), ["indie"]);
    assert_eq!(of(&["dream pop"]), ["dream pop"]);
}

#[test]
fn a_narrow_first_word_is_shown_alone() {
    assert_eq!(of(&["shoegaze", "dream pop", "noise pop"]), ["shoegaze"]);
    assert_eq!(of(&["emo", "indie rock"]), ["emo"]);
    assert_eq!(of(&["britpop", "indie", "rock"]), ["britpop"]);
}

#[test]
fn a_broad_first_word_takes_the_next_ones_with_it() {
    // Three rocks against one psychedelic
    assert_eq!(
        of(&[
            "rock",
            "classic rock",
            "psychedelic rock",
            "mod",
            "british rock"
        ]),
        ["rock", "psych"]
    );
    assert_eq!(
        of(&["pop", "rock", "new wave"]),
        ["pop", "rock", "new wave"]
    );
    assert_eq!(
        of(&["indie", "folk", "emo", "rock"]),
        ["indie", "folk", "emo"]
    );
}

#[test]
fn the_most_common_word_comes_first() {
    // Three rocks against one jazz ("jazz rock" is jazz)
    assert_eq!(
        of(&["soft rock", "yacht rock", "jazz rock", "album rock"]),
        ["rock", "jazz"]
    );
    assert_eq!(of(&["neo soul", "jazz", "soul", "r&b"]), ["soul"]);
}

#[test]
fn a_tie_goes_to_the_earlier_genre() {
    assert_eq!(of(&["bossa nova", "vocal jazz"]), ["bossa nova"]);
    assert_eq!(of(&["vocal jazz", "bossa nova"]), ["jazz"]);
    assert_eq!(of(&["rock", "jazz"]), ["rock", "jazz"]);
}

#[test]
fn patterns_match_whole_words_only() {
    // "rockabilly" is not rock, "dubstep" not reggae, "popwave" not pop
    assert!(of(&["rockabilly", "dubstep", "popwave"]).is_empty());
    assert_eq!(of(&["post-punk revival"]), ["post-punk"]);
}

#[test]
fn case_does_not_matter() {
    assert_eq!(of(&["Vocal Jazz"]), ["jazz"]);
}

#[test]
fn unknown_or_no_genres_give_nothing() {
    assert!(of(&[]).is_empty());
    assert!(of(&["sea shanties", "chiptune"]).is_empty());
    // Only the known ones count
    assert_eq!(of(&["chiptune", "synthwave"]), ["synthwave"]);
}

#[test]
fn finnish_genres_have_their_own_words() {
    assert_eq!(of(&["finnish rock", "suomirock"]), ["suomirock"]);
    assert_eq!(of(&["iskelmä", "finnish pop"]), ["suomipop"]);
}

#[test]
fn psychedelic_and_progressive_rock_are_their_own() {
    assert_eq!(of(&["psychedelic rock", "neo-psychedelia"]), ["psych"]);
    assert_eq!(of(&["progressive rock", "art rock", "prog"]), ["prog"]);
}
