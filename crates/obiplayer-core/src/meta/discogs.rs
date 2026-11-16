//! Discogs: pressing identification (catalog number, barcode, artist+album)
//! and release details with audiophile enrichment (mastering/cutting
//! engineers, pressing plant, studios, matrix/runout).
//!
//! Port of DrPlayer's `DiscogsService.swift`. Cache keys are identical.
//! Authentication is the consumer key/secret pair sent as query parameters.

use std::sync::{Arc, LazyLock};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Cache, Http, Service};

const BASE_URL: &str = "https://api.discogs.com";

/// One tracklist entry of a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DgTrack {
    pub position: String,
    pub title: String,
    pub duration: String,
}

/// A Discogs release (exact pressing).
#[derive(Debug, Clone, PartialEq)]
pub struct DgRelease {
    pub id: u64,
    pub title: String,
    /// `None` when Discogs has no year (0).
    pub year: Option<i32>,
    pub country: String,
    /// Normalised `https://www.discogs.com/...` URL.
    pub url: String,
    pub notes: String,
    /// First label's name and catalog number.
    pub label: String,
    pub catalog_number: String,
    /// `"Name (desc, desc)"`, or just `"Name"` without descriptions.
    pub formats: Vec<String>,
    pub genres: Vec<String>,
    pub styles: Vec<String>,
    pub image_url: Option<String>,
    pub tracklist: Vec<DgTrack>,
    /// Marketplace lowest price, `"%.2f"`.
    pub lowest_price: Option<String>,
    pub mastering_engineer: String,
    pub cut_by: String,
    pub pressed_at: String,
    pub recorded_at: String,
    pub mixed_at: String,
    pub matrix_runout: String,
}

/// Certified pressing info. JSON-compatible with DrPlayer/DrMobile's
/// `.drmobile-pressing.json` sidecar (`info` object).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PressingInfo {
    pub discogs_id: u64,
    pub catalog_number: String,
    pub label: String,
    pub country: String,
    /// Year as text; empty when unknown.
    pub year: String,
    pub formats: Vec<String>,
    pub mastering_engineer: String,
    pub url: String,
    // v2 — audiophile enrichment (optional in older sidecars).
    #[serde(default)]
    pub cut_by: String,
    #[serde(default)]
    pub pressed_at: String,
    #[serde(default)]
    pub recorded_at: String,
    #[serde(default)]
    pub mixed_at: String,
    #[serde(default)]
    pub matrix_runout: String,
}

impl PressingInfo {
    pub fn format_display(&self) -> String {
        self.formats.join(", ")
    }

    pub fn has_detailed_info(&self) -> bool {
        !self.cut_by.is_empty()
            || !self.pressed_at.is_empty()
            || !self.recorded_at.is_empty()
            || !self.mixed_at.is_empty()
            || !self.matrix_runout.is_empty()
    }
}

impl DgRelease {
    pub fn to_pressing_info(&self) -> PressingInfo {
        PressingInfo {
            discogs_id: self.id,
            catalog_number: self.catalog_number.clone(),
            label: self.label.clone(),
            country: self.country.clone(),
            year: self.year.map(|y| y.to_string()).unwrap_or_default(),
            formats: self.formats.clone(),
            mastering_engineer: self.mastering_engineer.clone(),
            url: self.url.clone(),
            cut_by: self.cut_by.clone(),
            pressed_at: self.pressed_at.clone(),
            recorded_at: self.recorded_at.clone(),
            mixed_at: self.mixed_at.clone(),
            matrix_runout: self.matrix_runout.clone(),
        }
    }
}

/// Ranking hints for catalog-number searches ("exact pressing").
#[derive(Debug, Clone, Default)]
pub struct PreferHints {
    /// Preferred release country (e.g. "Japan"). Part of the cache key.
    pub country: Option<String>,
    /// Label filter, sent to the search. Part of the cache key.
    pub label: Option<String>,
    /// Preferred format (e.g. "SACD" for DSF files). Ranking only.
    pub format: Option<String>,
}

pub struct Discogs {
    http: Arc<Http>,
    cache: Arc<Cache>,
    creds: Option<(String, String)>,
}

impl Discogs {
    pub fn new(http: Arc<Http>, cache: Arc<Cache>, creds: Option<(String, String)>) -> Discogs {
        let creds = creds.filter(|(k, s)| !k.is_empty() && !s.is_empty());
        Discogs { http, cache, creds }
    }

    pub fn is_configured(&self) -> bool {
        self.creds.is_some()
    }

    /// Check a key/secret pair with a minimal search.
    pub async fn validate_credentials(&self, key: &str, secret: &str) -> Result<(), String> {
        if key.is_empty() || secret.is_empty() {
            return Err("Key and secret required".into());
        }
        let url = format!("{BASE_URL}/database/search");
        let query = [
            ("q", "test"),
            ("per_page", "1"),
            ("key", key),
            ("secret", secret),
        ];
        match self.http.status(Service::Discogs, &url, &query).await {
            None => Err("Network error".into()),
            Some(200) => Ok(()),
            Some(401) => Err("Invalid key or secret".into()),
            Some(429) => Err("Rate limited — try again later".into()),
            Some(n) => Err(format!("HTTP {n}")),
        }
    }

    /// Search by catalog number (most precise for pressing identification).
    pub async fn search_by_catalog(&self, catno: &str, hints: &PreferHints) -> Option<DgRelease> {
        self.creds.as_ref()?;
        let mut key = format!("discogs_catno_{}", catno.to_lowercase());
        if let Some(c) = &hints.country {
            key.push('_');
            key.push_str(&c.to_lowercase());
        }
        if let Some(l) = &hints.label {
            key.push_str("_lbl_");
            key.push_str(&l.to_lowercase());
        }
        let mut query = vec![("catno", catno)];
        if let Some(l) = hints.label.as_deref().filter(|l| !l.is_empty()) {
            query.push(("label", l));
        }
        self.cached_search(
            &key,
            &query,
            hints.country.as_deref(),
            hints.format.as_deref(),
        )
        .await
    }

    /// Search by barcode.
    pub async fn search_by_barcode(
        &self,
        barcode: &str,
        country: Option<&str>,
    ) -> Option<DgRelease> {
        self.creds.as_ref()?;
        let mut key = format!("discogs_barcode_{barcode}");
        if let Some(c) = country {
            key.push('_');
            key.push_str(&c.to_lowercase());
        }
        self.cached_search(&key, &[("barcode", barcode)], country, None)
            .await
    }

    /// Search by artist + album title.
    pub async fn search(&self, artist: &str, album: &str) -> Option<DgRelease> {
        self.creds.as_ref()?;
        let key = format!(
            "discogs_search_{}_{}",
            artist.to_lowercase(),
            album.to_lowercase()
        );
        let q = format!("{artist} {album}");
        self.cached_search(&key, &[("q", &q), ("type", "release")], None, None)
            .await
    }

    /// Fetch full release details by Discogs ID.
    pub async fn fetch_release(&self, id: u64) -> Option<DgRelease> {
        let (k, s) = self.creds.as_ref()?;
        let key = format!("discogs_release_{id}");
        if let Some(json) = self.cache.get_json(&key) {
            return Some(parse_release(id, &json));
        }
        let url = format!("{BASE_URL}/releases/{id}");
        let (json, raw) = self
            .http
            .get_json(Service::Discogs, &url, &[("key", k), ("secret", s)])
            .await?;
        self.cache.set(&key, &raw);
        Some(parse_release(id, &json))
    }

    /// Run a search (or reuse the cached best ID) and fetch the top-ranked
    /// release. Caches `(none)` when the search succeeded with no results.
    async fn cached_search(
        &self,
        key: &str,
        query: &[(&str, &str)],
        country: Option<&str>,
        format: Option<&str>,
    ) -> Option<DgRelease> {
        if let Some(cached) = self.cache.get(key) {
            if cached == Cache::NONE {
                return None;
            }
            return self.fetch_release(cached.trim().parse().ok()?).await;
        }
        let (k, s) = self.creds.as_ref()?;
        let mut q = query.to_vec();
        q.push(("key", k));
        q.push(("secret", s));
        let url = format!("{BASE_URL}/database/search");
        // A network/HTTP failure is not cached; "no results" is.
        let (json, _) = self.http.get_json(Service::Discogs, &url, &q).await?;
        let Some(&best) = rank_results(&json, country, format).first() else {
            self.cache.set(key, Cache::NONE);
            return None;
        };
        self.cache.set(key, &best.to_string());
        self.fetch_release(best).await
    }
}

/// Rank search results: penalise unofficial/bootleg, prefer the given format
/// and country. Only reorders (stable: ties keep API order), never rejects —
/// except non-release entries (masters, artists, labels), whose IDs are not
/// release IDs.
pub fn rank_results(json: &Value, country: Option<&str>, format: Option<&str>) -> Vec<u64> {
    let Some(results) = json["results"].as_array() else {
        return Vec::new();
    };
    let pf = format.map(str::to_lowercase);
    let pc = country.map(str::to_lowercase);
    let mut ranked: Vec<(u64, i32)> = results
        .iter()
        .filter(|r| r["type"].as_str().is_none_or(|t| t == "release"))
        .filter_map(|r| {
            let id = r["id"].as_u64()?;
            let fmt = r["format"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default()
                .to_lowercase();
            let mut score = 0;
            if fmt.contains("unofficial") || fmt.contains("bootleg") {
                score -= 20;
            }
            if let Some(pf) = &pf {
                if fmt.contains(pf.as_str()) {
                    score += 15;
                } else if pf == "sacd" {
                    // DSF file but the result is a plain CD.
                    score -= 5;
                }
            }
            if let (Some(pc), Some(c)) = (&pc, r["country"].as_str())
                && c.to_lowercase() == *pc
            {
                score += 10;
            }
            Some((id, score))
        })
        .collect();
    ranked.sort_by_key(|&(_, score)| std::cmp::Reverse(score));
    ranked.into_iter().map(|(id, _)| id).collect()
}

static DISAMBIGUATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*\(\d+\)$").expect("regex"));

/// Strip Discogs' `(n)` disambiguation suffix ("John Smith (2)").
pub fn strip_disambiguation(name: &str) -> String {
    DISAMBIGUATION.replace(name, "").into_owned()
}

/// Normalise a release `uri` to an absolute discogs.com URL.
pub fn normalize_url(uri: &str, id: u64) -> String {
    if uri.starts_with("http") {
        uri.to_owned()
    } else if !uri.is_empty() {
        format!("https://www.discogs.com{uri}")
    } else {
        format!("https://www.discogs.com/release/{id}")
    }
}

fn str_of(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

fn str_list(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Join unique names in first-seen order with ", ".
fn join_unique(names: Vec<String>) -> String {
    let mut out: Vec<String> = Vec::new();
    for n in names {
        if !out.contains(&n) {
            out.push(n);
        }
    }
    out.join(", ")
}

/// Parse a `/releases/{id}` response.
pub fn parse_release(id: u64, json: &Value) -> DgRelease {
    let (label, catalog_number) = json["labels"]
        .as_array()
        .and_then(|a| a.first())
        .map(|l| (str_of(&l["name"]), str_of(&l["catno"])))
        .unwrap_or_default();

    let formats = json["formats"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|f| {
                    let name = str_of(&f["name"]);
                    let desc = str_list(&f["descriptions"]);
                    if desc.is_empty() {
                        name
                    } else {
                        format!("{name} ({})", desc.join(", "))
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let image_url = json["images"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|i| i["uri"].as_str())
        .map(str::to_owned);

    let tracklist = json["tracklist"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|t| DgTrack {
                    position: str_of(&t["position"]),
                    title: str_of(&t["title"]),
                    duration: str_of(&t["duration"]),
                })
                .filter(|t| !t.title.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let lowest_price = json["lowest_price"].as_f64().map(|p| format!("{p:.2}"));

    // Engineers from extraartists — mastering vs cutting.
    const MASTERING: [&str; 3] = ["mastered by", "mastered at", "remastered by"];
    const CUTTING: [&str; 4] = [
        "lacquer cut by",
        "cut by",
        "cutting engineer",
        "lacquer cut at",
    ];
    let mut mastering = Vec::new();
    let mut cutting = Vec::new();
    for a in json["extraartists"].as_array().into_iter().flatten() {
        let (Some(role), Some(name)) = (a["role"].as_str(), a["name"].as_str()) else {
            continue;
        };
        let role = role.to_lowercase();
        let name = strip_disambiguation(name);
        if MASTERING.iter().any(|k| role.contains(k)) {
            mastering.push(name);
        } else if CUTTING.iter().any(|k| role.contains(k)) {
            cutting.push(name);
        }
    }

    // Companies: pressing plant, recording/mixing studios.
    let mut pressed = Vec::new();
    let mut recorded = Vec::new();
    let mut mixed = Vec::new();
    for c in json["companies"].as_array().into_iter().flatten() {
        let name = strip_disambiguation(c["name"].as_str().unwrap_or_default());
        if name.is_empty() {
            continue;
        }
        match c["entity_type_name"]
            .as_str()
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "pressed by" => pressed.push(name),
            "recorded at" => recorded.push(name),
            "mixed at" => mixed.push(name),
            _ => {}
        }
    }

    // Matrix / runout identifiers.
    let matrix_runout = json["identifiers"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|i| {
                    let ty = i["type"].as_str()?.to_lowercase();
                    if !(ty.contains("matrix") || ty.contains("runout")) {
                        return None;
                    }
                    let value = i["value"].as_str().filter(|v| !v.is_empty())?;
                    let desc = i["description"].as_str().unwrap_or_default();
                    Some(if desc.is_empty() {
                        value.to_owned()
                    } else {
                        format!("{desc}: {value}")
                    })
                })
                .collect::<Vec<_>>()
                .join(" · ")
        })
        .unwrap_or_default();

    DgRelease {
        id,
        title: str_of(&json["title"]),
        year: json["year"]
            .as_i64()
            .and_then(|y| i32::try_from(y).ok())
            .filter(|&y| y > 0),
        country: str_of(&json["country"]),
        url: normalize_url(json["uri"].as_str().unwrap_or_default(), id),
        notes: str_of(&json["notes"]),
        label,
        catalog_number,
        formats,
        genres: str_list(&json["genres"]),
        styles: str_list(&json["styles"]),
        image_url,
        tracklist,
        lowest_price,
        mastering_engineer: join_unique(mastering),
        cut_by: join_unique(cutting),
        pressed_at: join_unique(pressed),
        recorded_at: join_unique(recorded),
        mixed_at: join_unique(mixed),
        matrix_runout,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({
            "id": 123,
            "title": "A Night At The Opera",
            "year": 2011,
            "country": "Japan",
            "uri": "/release/123-Queen-A-Night-At-The-Opera",
            "notes": "SHM-CD",
            "labels": [
                {"name": "Universal Music", "catno": "UICY-20003"},
                {"name": "Other", "catno": "X"}
            ],
            "formats": [
                {"name": "CD", "qty": "1", "descriptions": ["Album", "Reissue", "Remastered"]},
                {"name": "All Media"}
            ],
            "genres": ["Rock"],
            "styles": ["Pop Rock", "Prog Rock"],
            "images": [{"uri": "https://i.discogs.com/a.jpg"}, {"uri": "https://i.discogs.com/b.jpg"}],
            "tracklist": [
                {"position": "1", "title": "Death On Two Legs", "duration": "3:43"},
                {"position": "", "title": "", "duration": ""},
                {"position": "2", "title": "Lazing On A Sunday Afternoon"}
            ],
            "lowest_price": 12,
            "extraartists": [
                {"name": "Bob Ludwig", "role": "Remastered By"},
                {"name": "Bob Ludwig", "role": "Mastered By"},
                {"name": "Kevin Gray (2)", "role": "Lacquer Cut By"},
                {"name": "Steve Hoffman", "role": "Cutting Engineer"},
                {"name": "Kevin Gray", "role": "Cut By"},
                {"name": "Roy Thomas Baker", "role": "Producer"},
                {"role": "Mastered By"}
            ],
            "companies": [
                {"name": "JVC (3)", "entity_type_name": "Pressed By"},
                {"name": "Sarm East Studios", "entity_type_name": "Recorded At"},
                {"name": "Rockfield Studios", "entity_type_name": "Recorded At"},
                {"name": "Sarm East Studios", "entity_type_name": "Recorded At"},
                {"name": "Scorpio Sound", "entity_type_name": "Mixed At"},
                {"name": "EMI", "entity_type_name": "Phonographic Copyright (p)"},
                {"name": "", "entity_type_name": "Pressed By"}
            ],
            "identifiers": [
                {"type": "Barcode", "value": "4988005640000"},
                {"type": "Matrix / Runout", "value": "UICY-20003 1A1", "description": "Variant 1"},
                {"type": "Matrix / Runout", "value": "UICY-20003 2A1"},
                {"type": "Runout", "value": ""}
            ]
        })
    }

    #[test]
    fn parses_release() {
        let r = parse_release(123, &sample());
        assert_eq!(r.title, "A Night At The Opera");
        assert_eq!(r.year, Some(2011));
        assert_eq!(r.country, "Japan");
        assert_eq!(r.label, "Universal Music");
        assert_eq!(r.catalog_number, "UICY-20003");
        assert_eq!(r.formats, ["CD (Album, Reissue, Remastered)", "All Media"]);
        assert_eq!(r.genres, ["Rock"]);
        assert_eq!(r.styles, ["Pop Rock", "Prog Rock"]);
        assert_eq!(r.image_url.as_deref(), Some("https://i.discogs.com/a.jpg"));
        assert_eq!(r.tracklist.len(), 2);
        assert_eq!(r.tracklist[1].duration, "");
        assert_eq!(r.lowest_price.as_deref(), Some("12.00"));
        assert_eq!(
            r.url,
            "https://www.discogs.com/release/123-Queen-A-Night-At-The-Opera"
        );
    }

    #[test]
    fn engineers_companies_matrix() {
        let r = parse_release(123, &sample());
        assert_eq!(r.mastering_engineer, "Bob Ludwig");
        assert_eq!(r.cut_by, "Kevin Gray, Steve Hoffman");
        assert_eq!(r.pressed_at, "JVC");
        assert_eq!(r.recorded_at, "Sarm East Studios, Rockfield Studios");
        assert_eq!(r.mixed_at, "Scorpio Sound");
        assert_eq!(
            r.matrix_runout,
            "Variant 1: UICY-20003 1A1 · UICY-20003 2A1"
        );
    }

    #[test]
    fn empty_release() {
        let r = parse_release(7, &json!({}));
        assert_eq!(r.year, None);
        assert_eq!(r.url, "https://www.discogs.com/release/7");
        assert!(r.formats.is_empty() && r.mastering_engineer.is_empty());
        assert_eq!(r.to_pressing_info().year, "");
        let r = parse_release(7, &json!({"year": 0}));
        assert_eq!(r.year, None);
    }

    #[test]
    fn disambiguation_and_urls() {
        assert_eq!(strip_disambiguation("John Smith (12)"), "John Smith");
        assert_eq!(strip_disambiguation("Studio (2) East"), "Studio (2) East");
        assert_eq!(strip_disambiguation("Prince"), "Prince");
        assert_eq!(normalize_url("https://x/r/1", 1), "https://x/r/1");
        assert_eq!(
            normalize_url("/release/1", 1),
            "https://www.discogs.com/release/1"
        );
        assert_eq!(normalize_url("", 9), "https://www.discogs.com/release/9");
    }

    #[test]
    fn ranking() {
        let j = json!({"results": [
            {"id": 1, "type": "release", "country": "US", "format": ["CD", "Album"]},
            {"id": 2, "type": "release", "country": "Japan", "format": ["CD", "Unofficial Release"]},
            {"id": 3, "type": "release", "country": "Japan", "format": ["SACD", "Hybrid"]},
            {"id": 4, "type": "master", "country": "Japan", "format": ["SACD"]},
            {"id": 5, "type": "release", "country": "Japan", "format": ["CD"]},
            {"id": 6, "country": "US", "format": ["CD"]},
            {"type": "release"}
        ]});
        // No hints: only bootleg moves; ties keep API order.
        assert_eq!(rank_results(&j, None, None), [1, 3, 5, 6, 2]);
        // Country: Japan +10 (bootleg 2 nets -10).
        assert_eq!(rank_results(&j, Some("japan"), None), [3, 5, 1, 6, 2]);
        // SACD +15, others -5; plus country.
        assert_eq!(
            rank_results(&j, Some("Japan"), Some("SACD")),
            [3, 5, 1, 6, 2]
        );
        assert_eq!(rank_results(&j, None, Some("SACD")), [3, 1, 5, 6, 2]);
        // Non-SACD preferred format: no penalty for mismatches.
        assert_eq!(rank_results(&j, None, Some("vinyl")), [1, 3, 5, 6, 2]);
        assert!(rank_results(&json!({"results": []}), None, None).is_empty());
        assert!(rank_results(&json!({}), None, None).is_empty());
    }

    #[test]
    fn pressing_info_json() {
        let info = parse_release(123, &sample()).to_pressing_info();
        let v = serde_json::to_value(&info).unwrap();
        let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "catalogNumber",
                "country",
                "cutBy",
                "discogsId",
                "formats",
                "label",
                "masteringEngineer",
                "matrixRunout",
                "mixedAt",
                "pressedAt",
                "recordedAt",
                "url",
                "year"
            ]
        );
        assert_eq!(v["discogsId"], 123);
        assert_eq!(v["year"], "2011");
        // v1 sidecar (no enrichment fields) still decodes.
        let v1 = json!({
            "discogsId": 1, "catalogNumber": "C", "label": "L", "country": "UK",
            "year": "", "formats": ["Vinyl"], "masteringEngineer": "", "url": "u"
        });
        let p: PressingInfo = serde_json::from_value(v1).unwrap();
        assert_eq!(p.cut_by, "");
        assert!(!p.has_detailed_info());
        assert_eq!(p.format_display(), "Vinyl");
        assert!(info.has_detailed_info());
    }

    #[tokio::test]
    async fn unconfigured_does_nothing() {
        let dir = std::env::temp_dir().join(format!("obi-dg-{}", std::process::id()));
        let d = Discogs::new(
            Arc::new(Http::new()),
            Arc::new(Cache::new(&dir)),
            Some(("k".into(), String::new())),
        );
        assert!(!d.is_configured());
        assert!(d.search("a", "b").await.is_none());
        assert!(d.fetch_release(1).await.is_none());
        assert_eq!(
            d.validate_credentials("", "x").await,
            Err("Key and secret required".into())
        );
    }
}
