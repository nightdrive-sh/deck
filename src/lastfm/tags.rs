//! An artist's top tags on Last.fm (`artist.getTopTags`): what listeners call the
//! artist ("jazz", "soul", "seen live"), most used first. Deck reads genres from them
//! ([`crate::coarse_genre`]) for a Curated list written before the curator gave genres.

use anyhow::Result;
use serde::Deserialize;

use super::LastFm;

/// Tags used less than this (Last.fm scales the top tag to 100) say little.
const MIN_COUNT: u32 = 10;

impl LastFm {
    /// The artist's tags, most used first, without the rarely used ones.
    pub async fn artist_tags(&self, artist: &str) -> Result<Vec<String>> {
        let response: TopTagsResponse = self
            .get(
                "artist.gettoptags",
                &[
                    ("artist", artist.to_owned()),
                    ("autocorrect", "1".to_owned()),
                ],
            )
            .await?;
        Ok(tags(response))
    }
}

fn tags(response: TopTagsResponse) -> Vec<String> {
    response
        .toptags
        .tag
        .into_iter()
        .filter(|t| t.count >= MIN_COUNT)
        .map(|t| t.name)
        .collect()
}

#[derive(Deserialize)]
struct TopTagsResponse {
    toptags: TopTags,
}

#[derive(Deserialize)]
struct TopTags {
    /// Missing for an artist nobody has tagged.
    #[serde(default)]
    tag: Vec<ApiTag>,
}

#[derive(Deserialize)]
struct ApiTag {
    name: String,
    count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_top_tags_without_the_rare_ones() {
        let body = r#"{"toptags": {"tag": [
            {"count": 100, "name": "jazz", "url": "x"},
            {"count": 42, "name": "vocal jazz", "url": "x"},
            {"count": 3, "name": "seen live", "url": "x"}
        ], "@attr": {"artist": "Ella Fitzgerald"}}}"#;
        let response: TopTagsResponse = serde_json::from_str(body).unwrap();
        assert_eq!(tags(response), ["jazz", "vocal jazz"]);
    }

    #[test]
    fn an_untagged_artist_has_no_tags() {
        let response: TopTagsResponse =
            serde_json::from_str(r#"{"toptags": {"@attr": {"artist": "x"}}}"#).unwrap();
        assert!(tags(response).is_empty());
    }
}
