//! MusicBrainz web service (`/ws/2`): release and artist lookups with credits,
//! Wikipedia links and genres (port of `MusicBrainzService.swift`).
//!
//! Every request goes through [`Service::MusicBrainz`], so the 1 req/s policy
//! applies to searches as well as lookups.

use std::sync::{Arc, LazyLock};

use regex::Regex;
use serde_json::Value;

use super::{Cache, Http, Service};

const BASE: &str = "https://musicbrainz.org/ws/2";
// Space-separated: form-encoded as `+`, the separator MusicBrainz documents.
const RELEASE_INC: &str = "artist-credits labels recordings release-groups url-rels artist-rels recording-level-rels genres";
const ARTIST_INC: &str = "url-rels artist-rels genres";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credit {
    pub name: String,
    /// Relationship type, with attributes: `"producer"`, `"instrument (piano)"`.
    pub role: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MbRelease {
    pub id: String,
    pub title: String,
    pub date: String,
    pub country: String,
    pub status: String,
    pub barcode: String,
    /// Artist credit names joined with ", ".
    pub artist: String,
    /// From the first `label-info` entry.
    pub label: String,
    pub catalog_number: String,
    /// Release and recording relationships, deduplicated by (name, role).
    pub credits: Vec<Credit>,
    pub producers: Vec<String>,
    pub engineers: Vec<String>,
    pub mastering_engineers: Vec<String>,
    pub wikipedia_slug: Option<String>,
    /// Release-group genres.
    pub genres: Vec<String>,
    /// One entry per medium (`["CD", "CD"]` for a double CD).
    pub media_formats: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    /// Relationship attributes ("original", "lead vocals", …), ", "-joined.
    pub role: String,
    /// `"1970–1991"`, `"1970–present"`, or empty when unknown.
    pub period: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MbArtist {
    pub id: String,
    pub name: String,
    /// "Group", "Person", …
    pub kind: String,
    pub begin: String,
    pub end: String,
    pub area: String,
    pub wikipedia_slug: Option<String>,
    pub members: Vec<Member>,
}

/// Preferences for picking one release among search results.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReleaseHints {
    /// Medium format substring ("CD", "SACD", "Vinyl"), case-insensitive.
    pub format: Option<String>,
    /// ISO country code, case-insensitive.
    pub country: Option<String>,
    pub label: Option<String>,
}

impl ReleaseHints {
    /// Cache-key suffix, as in DrPlayer: `fmt_x_cc_y_lbl_z`, lowercased.
    fn key_suffix(&self) -> String {
        [
            self.format.as_ref().map(|f| format!("fmt_{f}")),
            self.country.as_ref().map(|c| format!("cc_{c}")),
            self.label.as_ref().map(|l| format!("lbl_{l}")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("_")
        .to_lowercase()
    }
}

// --- Lucene query building -------------------------------------------------

/// A Lucene phrase: `"…"` with `\` and `"` escaped.
fn lucene_phrase(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// Loose terms for a field: `(t1 t2 …)` with every Lucene special character
/// escaped and the operator words lowercased (the index is case-insensitive).
/// Matches like DrPlayer's unquoted query, but bound to the field.
fn lucene_terms(s: &str) -> String {
    const SPECIAL: &str = "+-&|!(){}[]^\"~*?:\\/";
    let words: Vec<String> = s
        .split_whitespace()
        .map(|w| {
            let w = match w {
                "AND" | "OR" | "NOT" | "TO" => w.to_lowercase(),
                _ => w.to_owned(),
            };
            let mut out = String::with_capacity(w.len());
            for c in w.chars() {
                if SPECIAL.contains(c) {
                    out.push('\\');
                }
                out.push(c);
            }
            out
        })
        .collect();
    format!("({})", words.join(" "))
}

fn catalog_query(artist: &str, catno: &str, label: Option<&str>) -> String {
    let mut q = format!(
        "catno:{} AND artist:{}",
        lucene_phrase(catno),
        lucene_phrase(artist)
    );
    if let Some(l) = label.filter(|l| !l.is_empty()) {
        q += &format!(" AND label:{}", lucene_phrase(l));
    }
    q
}

fn release_query(artist: &str, album: &str, label: Option<&str>) -> String {
    let mut q = format!(
        "release:{} AND artist:{}",
        lucene_terms(album),
        lucene_phrase(artist)
    );
    if let Some(l) = label.filter(|l| !l.is_empty()) {
        q += &format!(" AND label:{}", lucene_phrase(l));
    }
    q
}

// --- Parsing ---------------------------------------------------------------

fn str_of(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

fn arr(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or_default()
}

/// Article slug from a Wikipedia URL (text after the last `/wiki/`).
fn wiki_slug(resource: &str) -> Option<String> {
    resource.contains("wikipedia.org").then(|| {
        resource
            .rsplit("/wiki/")
            .next()
            .unwrap_or(resource)
            .to_owned()
    })
}

fn relation_credit(rel: &Value) -> Option<Credit> {
    let artist = rel.get("artist")?;
    let kind = rel["type"].as_str().unwrap_or_default();
    let attrs: Vec<&str> = arr(&rel["attributes"])
        .iter()
        .filter_map(Value::as_str)
        .filter(|a| !a.is_empty())
        .collect();
    let role = if attrs.is_empty() {
        kind.to_owned()
    } else {
        format!("{kind} ({})", attrs.join(", "))
    };
    Some(Credit {
        name: str_of(&artist["name"]),
        role,
    })
}

/// Unique names whose role contains any keyword (case-insensitive).
fn credit_names(credits: &[Credit], keywords: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in credits {
        let role = c.role.to_lowercase();
        if keywords.iter().any(|k| role.contains(k)) && !out.contains(&c.name) {
            out.push(c.name.clone());
        }
    }
    out
}

/// Parse a `/release/<id>` lookup.
pub fn parse_release(id: &str, v: &Value) -> MbRelease {
    let artist = arr(&v["artist-credit"])
        .iter()
        .filter_map(|c| c["name"].as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let li = &v["label-info"][0];

    let mut credits: Vec<Credit> = Vec::new();
    let recording_rels = arr(&v["media"])
        .iter()
        .flat_map(|m| arr(&m["tracks"]))
        .flat_map(|t| arr(&t["recording"]["relations"]));
    for rel in arr(&v["relations"]).iter().chain(recording_rels) {
        if let Some(c) = relation_credit(rel)
            && !credits.contains(&c)
        {
            credits.push(c);
        }
    }

    let url_slug = |rels: &Value| {
        arr(rels)
            .iter()
            .rev()
            .find_map(|r| r["url"]["resource"].as_str().and_then(wiki_slug))
    };
    let wikipedia_slug =
        url_slug(&v["release-group"]["relations"]).or_else(|| url_slug(&v["relations"]));

    MbRelease {
        id: id.to_owned(),
        title: str_of(&v["title"]),
        date: str_of(&v["date"]),
        country: str_of(&v["country"]),
        status: str_of(&v["status"]),
        barcode: str_of(&v["barcode"]),
        artist,
        label: str_of(&li["label"]["name"]),
        catalog_number: str_of(&li["catalog-number"]),
        producers: credit_names(&credits, &["producer", "executive producer"]),
        engineers: credit_names(&credits, &["engineer", "recording", "mix", "balance"]),
        mastering_engineers: credit_names(&credits, &["mastering"]),
        credits,
        wikipedia_slug,
        genres: arr(&v["release-group"]["genres"])
            .iter()
            .filter_map(|g| g["name"].as_str().map(str::to_owned))
            .collect(),
        media_formats: arr(&v["media"])
            .iter()
            .filter_map(|m| m["format"].as_str().map(str::to_owned))
            .collect(),
    }
}

/// Parse an `/artist/<id>` lookup.
pub fn parse_artist(id: &str, v: &Value) -> MbArtist {
    let rels = arr(&v["relations"]);
    let wikipedia_slug = rels
        .iter()
        .find_map(|r| r["url"]["resource"].as_str().and_then(wiki_slug));

    let mut members: Vec<Member> = Vec::new();
    for rel in rels {
        if rel["type"].as_str() != Some("member of band") || rel.get("artist").is_none() {
            continue;
        }
        let begin = rel["begin"].as_str().unwrap_or_default();
        let end = rel["end"].as_str().unwrap_or_default();
        let period = if begin.is_empty() {
            String::new()
        } else {
            format!("{begin}–{}", if end.is_empty() { "present" } else { end })
        };
        let m = Member {
            name: str_of(&rel["artist"]["name"]),
            role: arr(&rel["attributes"])
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", "),
            period,
        };
        if !members
            .iter()
            .any(|x| x.name == m.name && x.period == m.period)
        {
            members.push(m);
        }
    }

    MbArtist {
        id: id.to_owned(),
        name: str_of(&v["name"]),
        kind: str_of(&v["type"]),
        begin: str_of(&v["life-span"]["begin"]),
        end: str_of(&v["life-span"]["end"]),
        area: str_of(&v["area"]["name"]),
        wikipedia_slug,
        members,
    }
}

/// Pick a release id from search results: with a format or country hint,
/// the first release matching both; otherwise (or if none matches) the first.
pub fn pick_release(search: &Value, hints: &ReleaseHints) -> Option<String> {
    let releases = arr(&search["releases"]);
    let first = releases.first()?["id"].as_str()?.to_owned();
    if hints.format.is_none() && hints.country.is_none() {
        return Some(first);
    }
    let fmt = hints.format.as_deref().map(str::to_lowercase);
    let cc = hints.country.as_deref().map(str::to_uppercase);
    let hit = releases.iter().find(|r| {
        let fmt_ok = fmt.as_ref().is_none_or(|f| {
            arr(&r["media"])
                .iter()
                .filter_map(|m| m["format"].as_str())
                .any(|mf| mf.to_lowercase().contains(f.as_str()))
        });
        let cc_ok = cc
            .as_ref()
            .is_none_or(|c| r["country"].as_str().unwrap_or_default().to_uppercase() == *c);
        fmt_ok && cc_ok
    });
    Some(
        hit.and_then(|r| r["id"].as_str())
            .map(str::to_owned)
            .unwrap_or(first),
    )
}

static RE_TRAILING_PARENS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*\(.*\)\s*$").expect("regex"));

// --- Service -----------------------------------------------------------------

pub struct MusicBrainz {
    http: Arc<Http>,
    cache: Arc<Cache>,
}

impl MusicBrainz {
    pub fn new(http: Arc<Http>, cache: Arc<Cache>) -> MusicBrainz {
        MusicBrainz { http, cache }
    }

    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Option<(Value, String)> {
        let mut q = query.to_vec();
        q.push(("fmt", "json"));
        self.http
            .get_json(Service::MusicBrainz, &format!("{BASE}/{path}"), &q)
            .await
    }

    /// Search by catalog number (most precise for a pressing), then fetch
    /// the first hit.
    pub async fn search_by_catalog(
        &self,
        artist: &str,
        catno: &str,
        label: Option<&str>,
    ) -> Option<MbRelease> {
        let suffix = label
            .map(|l| format!("_lbl_{}", l.to_lowercase()))
            .unwrap_or_default();
        let key = format!(
            "mb_search_catno_{}_{}{suffix}",
            artist.to_lowercase(),
            catno.to_lowercase()
        );
        let id = match self.cache.get(&key) {
            Some(id) => id,
            None => {
                let query = catalog_query(artist, catno, label);
                let (v, _) = self
                    .get("release/", &[("query", &query), ("limit", "5")])
                    .await?;
                let id = v["releases"][0]["id"].as_str().map(str::to_owned);
                self.cache.set(&key, id.as_deref().unwrap_or(Cache::NONE));
                id?
            }
        };
        if id == Cache::NONE {
            return None;
        }
        self.fetch_release(&id).await
    }

    /// Search by artist + album, preferring releases matching the hints.
    pub async fn search_release(
        &self,
        artist: &str,
        album: &str,
        hints: &ReleaseHints,
    ) -> Option<MbRelease> {
        let mut key = format!(
            "mb_search_release_{}_{}",
            artist.to_lowercase(),
            album.to_lowercase()
        );
        let suffix = hints.key_suffix();
        if !suffix.is_empty() {
            key += "_";
            key += &suffix;
        }
        let id = match self.cache.get(&key) {
            Some(id) => id,
            None => {
                let query = release_query(artist, album, hints.label.as_deref());
                let limit = if hints.format.is_some() || hints.country.is_some() {
                    "25"
                } else {
                    "5"
                };
                let (v, _) = self
                    .get("release/", &[("query", &query), ("limit", limit)])
                    .await?;
                let id = pick_release(&v, hints);
                self.cache.set(&key, id.as_deref().unwrap_or(Cache::NONE));
                id?
            }
        };
        if id == Cache::NONE {
            return None;
        }
        self.fetch_release(&id).await
    }

    /// Release from the cache only (no network).
    pub fn release_from_cache(&self, id: &str) -> Option<MbRelease> {
        let v = self.cache.get_json(&format!("mb_release_{id}"))?;
        Some(parse_release(id, &v))
    }

    /// Cached release for an artist + album found by an earlier hint-less
    /// `search_release`; retries without a trailing `(…)` in the title.
    pub fn cached_release(&self, artist: &str, album: &str) -> Option<MbRelease> {
        let lookup = |album: &str| {
            let key = format!(
                "mb_search_release_{}_{}",
                artist.to_lowercase(),
                album.to_lowercase()
            );
            self.cache
                .get(&key)
                .filter(|id| id != Cache::NONE)
                .and_then(|id| self.release_from_cache(&id))
        };
        if let Some(r) = lookup(album) {
            return Some(r);
        }
        let cleaned = RE_TRAILING_PARENS.replace(album, "");
        if cleaned != album {
            return lookup(&cleaned);
        }
        None
    }

    /// Full release lookup with credits; cached as raw JSON.
    pub async fn fetch_release(&self, id: &str) -> Option<MbRelease> {
        if let Some(r) = self.release_from_cache(id) {
            return Some(r);
        }
        let (v, raw) = self
            .get(&format!("release/{id}"), &[("inc", RELEASE_INC)])
            .await?;
        self.cache.set(&format!("mb_release_{id}"), &raw);
        Some(parse_release(id, &v))
    }

    /// Best artist MBID for a name.
    pub async fn search_artist(&self, name: &str) -> Option<String> {
        let key = format!("mb_search_artist_{}", name.to_lowercase());
        if let Some(id) = self.cache.get(&key) {
            return (id != Cache::NONE).then_some(id);
        }
        let query = format!("artist:{}", lucene_phrase(name));
        let (v, _) = self
            .get("artist/", &[("query", &query), ("limit", "3")])
            .await?;
        let id = v["artists"][0]["id"].as_str().map(str::to_owned);
        self.cache.set(&key, id.as_deref().unwrap_or(Cache::NONE));
        id
    }

    /// Artist lookup with members and Wikipedia link; cached as raw JSON.
    pub async fn fetch_artist(&self, id: &str) -> Option<MbArtist> {
        let key = format!("mb_artist_{id}");
        if let Some(v) = self.cache.get_json(&key) {
            return Some(parse_artist(id, &v));
        }
        let (v, raw) = self
            .get(&format!("artist/{id}"), &[("inc", ARTIST_INC)])
            .await?;
        self.cache.set(&key, &raw);
        Some(parse_artist(id, &v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE: &str = r#"{
      "id": "r1", "title": "A Night at the Opera", "date": "1975-11-21", "country": "GB",
      "status": "Official", "barcode": "",
      "artist-credit": [{"name": "Queen", "joinphrase": ""}],
      "label-info": [{"catalog-number": "EMTC 103", "label": {"name": "EMI"}},
                     {"catalog-number": "X", "label": {"name": "Other"}}],
      "relations": [
        {"type": "producer", "attributes": [], "artist": {"name": "Roy Thomas Baker"}},
        {"type": "producer", "attributes": [], "artist": {"name": "Queen"}},
        {"type": "mastering", "attributes": [], "artist": {"name": "Bob Ludwig"}},
        {"type": "wikipedia", "url": {"resource": "https://en.wikipedia.org/wiki/Ignored_release_link"}}
      ],
      "media": [
        {"format": "Vinyl", "tracks": [
          {"recording": {"relations": [
            {"type": "engineer", "attributes": ["recording"], "artist": {"name": "Mike Stone"}},
            {"type": "producer", "attributes": [], "artist": {"name": "Queen"}},
            {"type": "instrument", "attributes": ["piano", ""], "artist": {"name": "Freddie Mercury"}},
            {"type": "mix", "attributes": [], "artist": {"name": "Mike Stone"}}
          ]}},
          {"recording": {}}
        ]},
        {"format": "Vinyl", "tracks": []}
      ],
      "release-group": {
        "genres": [{"name": "rock"}, {"name": "progressive rock"}],
        "relations": [
          {"type": "wikipedia", "url": {"resource": "https://de.wikipedia.org/wiki/First"}},
          {"type": "wikidata", "url": {"resource": "https://www.wikidata.org/wiki/Q1"}},
          {"type": "wikipedia", "url": {"resource": "https://en.wikipedia.org/wiki/A_Night_at_the_Opera_(Queen_album)"}}
        ]
      }
    }"#;

    #[test]
    fn parses_release() {
        let v: Value = serde_json::from_str(RELEASE).unwrap();
        let r = parse_release("r1", &v);
        assert_eq!(r.title, "A Night at the Opera");
        assert_eq!(r.artist, "Queen");
        assert_eq!(
            (r.label.as_str(), r.catalog_number.as_str()),
            ("EMI", "EMTC 103")
        );
        assert_eq!(r.credits.len(), 6, "{:?}", r.credits);
        assert!(r.credits.contains(&Credit {
            name: "Mike Stone".into(),
            role: "engineer (recording)".into()
        }));
        assert!(r.credits.contains(&Credit {
            name: "Freddie Mercury".into(),
            role: "instrument (piano)".into()
        }));
        assert_eq!(r.producers, vec!["Roy Thomas Baker", "Queen"]);
        assert_eq!(r.engineers, vec!["Mike Stone"]);
        assert_eq!(r.mastering_engineers, vec!["Bob Ludwig"]);
        // Last Wikipedia link of the release group wins.
        assert_eq!(
            r.wikipedia_slug.as_deref(),
            Some("A_Night_at_the_Opera_(Queen_album)")
        );
        assert_eq!(r.genres, vec!["rock", "progressive rock"]);
        assert_eq!(r.media_formats, vec!["Vinyl", "Vinyl"]);
    }

    #[test]
    fn release_wiki_fallback_to_release_rels() {
        let v: Value = serde_json::from_str(
            r#"{"relations":[{"url":{"resource":"https://en.wikipedia.org/wiki/X"}}],
                "release-group":{"relations":[]}}"#,
        )
        .unwrap();
        assert_eq!(parse_release("x", &v).wikipedia_slug.as_deref(), Some("X"));
        assert_eq!(
            parse_release("x", &Value::Null),
            MbRelease {
                id: "x".into(),
                ..Default::default()
            }
        );
    }

    #[test]
    fn parses_artist_members() {
        let v: Value = serde_json::from_str(
            r#"{"name":"Queen","type":"Group","life-span":{"begin":"1970","end":""},
                "area":{"name":"United Kingdom"},
                "relations":[
                  {"type":"wikidata","url":{"resource":"https://www.wikidata.org/wiki/Q15862"}},
                  {"type":"wikipedia","url":{"resource":"https://en.wikipedia.org/wiki/Queen_(band)"}},
                  {"type":"wikipedia","url":{"resource":"https://es.wikipedia.org/wiki/Queen"}},
                  {"type":"member of band","attributes":["original","lead vocals"],"begin":"1970","end":"1991",
                   "artist":{"name":"Freddie Mercury"}},
                  {"type":"member of band","attributes":["original"],"begin":"1970","end":"1991",
                   "artist":{"name":"Freddie Mercury"}},
                  {"type":"member of band","attributes":[],"begin":"1970","end":null,
                   "artist":{"name":"Brian May"}},
                  {"type":"member of band","attributes":["bass guitar"],
                   "artist":{"name":"Unknown Era"}},
                  {"type":"collaboration","artist":{"name":"Not a member"}}
                ]}"#,
        )
        .unwrap();
        let a = parse_artist("a1", &v);
        assert_eq!(a.kind, "Group");
        assert_eq!(a.begin, "1970");
        assert_eq!(a.area, "United Kingdom");
        // First Wikipedia link wins for artists.
        assert_eq!(a.wikipedia_slug.as_deref(), Some("Queen_(band)"));
        assert_eq!(
            a.members,
            vec![
                Member {
                    name: "Freddie Mercury".into(),
                    role: "original, lead vocals".into(),
                    period: "1970–1991".into()
                },
                Member {
                    name: "Brian May".into(),
                    role: "".into(),
                    period: "1970–present".into()
                },
                Member {
                    name: "Unknown Era".into(),
                    role: "bass guitar".into(),
                    period: "".into()
                },
            ]
        );
    }

    #[test]
    fn picks_release_by_hints() {
        let v: Value = serde_json::from_str(
            r#"{"releases":[
                {"id":"a","country":"GB","media":[{"format":"12\" Vinyl"}]},
                {"id":"b","country":"JP","media":[{"format":"CD"}]},
                {"id":"c","country":"JP","media":[{"format":"Hybrid SACD"},{"format":"CD"}]}
            ]}"#,
        )
        .unwrap();
        let h = |f: Option<&str>, c: Option<&str>| ReleaseHints {
            format: f.map(Into::into),
            country: c.map(Into::into),
            label: None,
        };
        assert_eq!(pick_release(&v, &h(None, None)).as_deref(), Some("a"));
        assert_eq!(
            pick_release(&v, &h(Some("sacd"), None)).as_deref(),
            Some("c")
        );
        assert_eq!(
            pick_release(&v, &h(Some("CD"), Some("jp"))).as_deref(),
            Some("b")
        );
        assert_eq!(
            pick_release(&v, &h(Some("Cassette"), None)).as_deref(),
            Some("a")
        );
        assert_eq!(
            pick_release(&serde_json::json!({"releases": []}), &h(None, None)),
            None
        );
    }

    #[test]
    fn cache_key_suffix() {
        let h = ReleaseHints {
            format: Some("SACD".into()),
            country: Some("JP".into()),
            label: Some("Sony".into()),
        };
        assert_eq!(h.key_suffix(), "fmt_sacd_cc_jp_lbl_sony");
        assert_eq!(ReleaseHints::default().key_suffix(), "");
    }

    #[test]
    fn lucene_escaping() {
        assert_eq!(lucene_phrase(r#"Say "Hi" \o/"#), r#""Say \"Hi\" \\o/""#);
        assert_eq!(
            catalog_query("AC/DC", "CDP 7 46207 2", Some("EMI")),
            r#"catno:"CDP 7 46207 2" AND artist:"AC/DC" AND label:"EMI""#
        );
        assert_eq!(
            release_query("Queen", "Live Killers (Disc 1) AND more: OK?", Some("")),
            r#"release:(Live Killers \(Disc 1\) and more\: OK\?) AND artist:"Queen""#
        );
    }

    #[test]
    fn cached_release_strips_trailing_parens() {
        let dir = std::env::temp_dir().join(format!("obi-mb-{}", std::process::id()));
        let cache = Arc::new(Cache::new(&dir));
        let mb = MusicBrainz::new(Arc::new(Http::new()), cache.clone());
        cache.set("mb_search_release_queen_a night at the opera", "r1");
        cache.set("mb_release_r1", RELEASE);
        let r = mb
            .cached_release("Queen", "A Night at the Opera (2011 Remaster)")
            .unwrap();
        assert_eq!(r.id, "r1");
        assert_eq!(r.title, "A Night at the Opera");
        cache.set("mb_search_release_queen_jazz", Cache::NONE);
        assert!(mb.cached_release("Queen", "Jazz").is_none());
        cache.purge();
    }
}
