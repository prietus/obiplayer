//! Last.fm: artist info (bio, stats, tags, similar), similar-artist scores
//! and album tags.
//!
//! Port of DrPlayer's `LastFMService.swift`. Cache keys are identical.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use regex::Regex;
use serde_json::Value;

use super::{Cache, Http, Service};

const BASE_URL: &str = "https://ws.audioscrobbler.com/2.0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LfmArtist {
    pub name: String,
    /// Full biography, HTML stripped.
    pub bio: String,
    /// Short biography, HTML stripped.
    pub summary: String,
    pub listeners: String,
    pub playcount: String,
    pub tags: Vec<String>,
    pub similar: Vec<String>,
    pub url: String,
    /// Largest image (last of `image[]`), if non-empty.
    pub image_url: Option<String>,
}

pub struct LastFm {
    http: Arc<Http>,
    cache: Arc<Cache>,
    key: Option<String>,
}

impl LastFm {
    pub fn new(http: Arc<Http>, cache: Arc<Cache>, key: Option<String>) -> LastFm {
        let key = key.filter(|k| !k.is_empty());
        LastFm { http, cache, key }
    }

    pub fn is_configured(&self) -> bool {
        self.key.is_some()
    }

    async fn call(&self, key: &str, params: &[(&str, &str)]) -> Option<(Value, String)> {
        let mut q = params.to_vec();
        q.push(("api_key", key));
        q.push(("format", "json"));
        self.http.get_json(Service::Other, BASE_URL, &q).await
    }

    /// Validate an API key with a lightweight call.
    pub async fn validate_key(&self, key: &str) -> Result<(), String> {
        if key.is_empty() {
            return Err("Empty key".into());
        }
        let params = [("method", "artist.getinfo"), ("artist", "Radiohead")];
        match self.call(key, &params).await {
            Some((json, _)) => {
                if json.get("error").is_some()
                    && let Some(m) = json["message"].as_str()
                {
                    return Err(m.to_owned());
                }
                if json.get("artist").is_some() {
                    Ok(())
                } else {
                    Err("Unexpected response".into())
                }
            }
            // Last.fm answers a bad key with HTTP 403; the client only
            // returns 200 bodies, so ask for the status to tell them apart.
            None => {
                let mut q = params.to_vec();
                q.push(("api_key", key));
                q.push(("format", "json"));
                match self.http.status(Service::Other, BASE_URL, &q).await {
                    None => Err("Network error".into()),
                    Some(403) => Err("Invalid API key".into()),
                    Some(n) => Err(format!("HTTP {n}")),
                }
            }
        }
    }

    /// Similar artists via `artist.getSimilar`:
    /// `{lowercased name: match 0..1}`, up to `limit` entries.
    pub async fn similar_artists(&self, name: &str, limit: u32) -> HashMap<String, f64> {
        let Some(key) = self.key.as_deref() else {
            return HashMap::new();
        };
        if name.is_empty() {
            return HashMap::new();
        }
        let cache_key = format!("lfm_similar_{}_{limit}", name.to_lowercase());
        if let Some(m) = self
            .cache
            .get_json(&cache_key)
            .and_then(|j| parse_similar(&j))
        {
            return m;
        }
        let limit_s = limit.to_string();
        let params = [
            ("method", "artist.getsimilar"),
            ("artist", name),
            ("limit", &limit_s),
            ("autocorrect", "1"),
        ];
        let Some((json, raw)) = self.call(key, &params).await else {
            return HashMap::new();
        };
        let Some(m) = parse_similar(&json) else {
            return HashMap::new();
        };
        self.cache.set(&cache_key, &raw);
        m
    }

    /// Tags of an album (genre-like).
    pub async fn album_tags(&self, artist: &str, album: &str) -> Vec<String> {
        let Some(key) = self.key.as_deref() else {
            return Vec::new();
        };
        let cache_key = format!(
            "lfm_album_{}_{}",
            artist.to_lowercase(),
            album.to_lowercase()
        );
        if let Some(c) = self.cache.get(&cache_key) {
            if c == Cache::EMPTY {
                return Vec::new();
            }
            return c
                .lines()
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .collect();
        }
        let params = [
            ("method", "album.getinfo"),
            ("artist", artist),
            ("album", album),
        ];
        // Network/HTTP failure: don't cache, retry next time.
        let Some((json, _)) = self.call(key, &params).await else {
            return Vec::new();
        };
        let tags = parse_album_tags(&json);
        let value = if tags.is_empty() {
            Cache::EMPTY.to_owned()
        } else {
            tags.join("\n")
        };
        self.cache.set(&cache_key, &value);
        tags
    }

    /// Artist info via `artist.getinfo`.
    pub async fn artist(&self, name: &str) -> Option<LfmArtist> {
        let key = self.key.as_deref()?;
        let cache_key = format!("lfm_artist_{}", name.to_lowercase());
        if let Some(j) = self.cache.get_json(&cache_key)
            && j["artist"].is_object()
        {
            return Some(parse_artist(name, &j["artist"]));
        }
        let (json, raw) = self
            .call(key, &[("method", "artist.getinfo"), ("artist", name)])
            .await?;
        if !json["artist"].is_object() {
            return None;
        }
        self.cache.set(&cache_key, &raw);
        Some(parse_artist(name, &json["artist"]))
    }
}

/// Last.fm collapses one-element lists into a bare object; accept both.
fn list(v: &Value) -> Vec<&Value> {
    match v {
        Value::Array(a) => a.iter().collect(),
        Value::Object(_) => vec![v],
        _ => Vec::new(),
    }
}

fn names(v: &Value) -> Vec<String> {
    list(v)
        .into_iter()
        .filter_map(|x| x["name"].as_str().map(str::to_owned))
        .collect()
}

/// `artist.getsimilar` → `{lowercased name: match}`. `None` if malformed.
pub fn parse_similar(json: &Value) -> Option<HashMap<String, f64>> {
    let arr = &json["similarartists"]["artist"];
    if !(arr.is_array() || arr.is_object()) {
        return None;
    }
    let mut out = HashMap::new();
    for item in list(arr) {
        let Some(name) = item["name"].as_str() else {
            continue;
        };
        let m = &item["match"];
        let score = m
            .as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| m.as_f64())
            .unwrap_or(0.0);
        out.insert(name.to_lowercase(), score);
    }
    Some(out)
}

/// `album.getinfo` → tag names (empty if none or not found).
pub fn parse_album_tags(json: &Value) -> Vec<String> {
    names(&json["album"]["tags"]["tag"])
}

static HTML_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").expect("regex"));

fn strip_html(s: &str) -> String {
    HTML_TAG.replace_all(s, "").trim().to_owned()
}

/// Parse the `artist` object of `artist.getinfo`.
pub fn parse_artist(name: &str, a: &Value) -> LfmArtist {
    let image_url = a["image"]
        .as_array()
        .and_then(|i| i.last())
        .and_then(|i| i["#text"].as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let s = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    LfmArtist {
        name: a["name"].as_str().unwrap_or(name).to_owned(),
        bio: strip_html(a["bio"]["content"].as_str().unwrap_or_default()),
        summary: strip_html(a["bio"]["summary"].as_str().unwrap_or_default()),
        listeners: s(&a["stats"]["listeners"]),
        playcount: s(&a["stats"]["playcount"]),
        tags: names(&a["tags"]["tag"]),
        similar: names(&a["similar"]["artist"]),
        url: s(&a["url"]),
        image_url,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_artist() {
        let j = json!({"artist": {
            "name": "Queen",
            "url": "https://www.last.fm/music/Queen",
            "image": [
                {"#text": "https://img/s.png", "size": "small"},
                {"#text": "https://img/xl.png", "size": "extralarge"}
            ],
            "stats": {"listeners": "5000000", "playcount": "300000000"},
            "similar": {"artist": [{"name": "Freddie Mercury"}, {"name": "David Bowie"}]},
            "tags": {"tag": [{"name": "classic rock"}, {"name": "rock"}]},
            "bio": {
                "summary": "  Queen are a <b>British</b> band. <a href=\"https://www.last.fm/music/Queen\">Read more on Last.fm</a> ",
                "content": "<p>Long bio</p>\n"
            }
        }});
        let a = parse_artist("queen", &j["artist"]);
        assert_eq!(a.name, "Queen");
        assert_eq!(a.summary, "Queen are a British band. Read more on Last.fm");
        assert_eq!(a.bio, "Long bio");
        assert_eq!(a.listeners, "5000000");
        assert_eq!(a.playcount, "300000000");
        assert_eq!(a.tags, ["classic rock", "rock"]);
        assert_eq!(a.similar, ["Freddie Mercury", "David Bowie"]);
        assert_eq!(a.image_url.as_deref(), Some("https://img/xl.png"));
        assert_eq!(a.url, "https://www.last.fm/music/Queen");
    }

    #[test]
    fn artist_fallbacks() {
        let a = parse_artist("Nobody", &json!({"image": [{"#text": "x"}, {"#text": ""}]}));
        assert_eq!(a.name, "Nobody");
        assert_eq!(a.image_url, None);
        assert!(a.bio.is_empty() && a.tags.is_empty());
        // Single tag collapsed to an object.
        let a = parse_artist("X", &json!({"tags": {"tag": {"name": "jazz"}}}));
        assert_eq!(a.tags, ["jazz"]);
    }

    #[test]
    fn similar_map() {
        let j = json!({"similarartists": {"artist": [
            {"name": "Freddie Mercury", "match": "1"},
            {"name": "David Bowie", "match": "0.42"},
            {"name": "Odd", "match": 0.5},
            {"name": "Broken"},
            {"match": "0.3"}
        ]}});
        let m = parse_similar(&j).unwrap();
        assert_eq!(m.len(), 4);
        assert_eq!(m["freddie mercury"], 1.0);
        assert_eq!(m["david bowie"], 0.42);
        assert_eq!(m["odd"], 0.5);
        assert_eq!(m["broken"], 0.0);
        assert!(parse_similar(&json!({"error": 6})).is_none());
        assert!(
            parse_similar(&json!({"similarartists": {"artist": []}}))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn album_tags() {
        let j = json!({"album": {"tags": {"tag": [{"name": "rock"}, {"name": "70s"}]}}});
        assert_eq!(parse_album_tags(&j), ["rock", "70s"]);
        assert!(parse_album_tags(&json!({"album": {"tags": ""}})).is_empty());
        assert!(parse_album_tags(&json!({"error": 6, "message": "Album not found"})).is_empty());
    }

    #[tokio::test]
    async fn unconfigured_does_nothing() {
        let dir = std::env::temp_dir().join(format!("obi-lfm-{}", std::process::id()));
        let l = LastFm::new(
            Arc::new(Http::new()),
            Arc::new(Cache::new(&dir)),
            Some(String::new()),
        );
        assert!(!l.is_configured());
        assert!(l.artist("Queen").await.is_none());
        assert!(l.album_tags("a", "b").await.is_empty());
        assert!(l.similar_artists("a", 10).await.is_empty());
        assert_eq!(l.validate_key("").await, Err("Empty key".into()));
    }
}
