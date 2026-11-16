//! Wikipedia article summaries (REST `page/summary`) with search fallback.
//! English first, then Spanish (port of `WikipediaService.swift`).

use std::sync::Arc;

use reqwest::Url;
use serde_json::Value;

use super::{Cache, Http, Service};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub title: String,
    pub extract: String,
    pub thumbnail_url: Option<String>,
    /// Original (full-resolution) image.
    pub image_url: Option<String>,
    pub page_url: String,
    /// Wiki language code ("en", "es").
    pub lang: String,
}

/// Lookup result that separates "nothing there" (cacheable) from a failed
/// request (not cacheable).
enum Outcome<T> {
    Found(T),
    NotFound,
    Failed,
}

/// Search variants for an artist name: the exact name; the first artist of a
/// compound name ("A & B", "A feat. B", …); `/` replaced by a space.
pub fn artist_search_queries(name: &str) -> Vec<String> {
    const SEPARATORS: [&str; 9] = [
        " & ", " and ", " with ", " feat. ", " feat ", " ft. ", " ft ", " + ", " y ",
    ];
    let mut queries = vec![name.to_owned()];
    let lower = name.to_lowercase();
    // Lowercasing can change byte offsets for some scripts; only use the
    // offset when it is a valid boundary of the original string.
    for sep in SEPARATORS {
        if let Some(i) = lower.find(sep) {
            if lower.len() == name.len() && name.is_char_boundary(i) {
                let first = name[..i].trim();
                if !first.is_empty() {
                    queries.push(first.to_owned());
                }
            }
            break;
        }
    }
    if name.contains('/') {
        queries.push(name.replace('/', " "));
    }
    queries
}

/// Parse a REST summary. Disambiguation pages and empty extracts are `None`.
pub fn parse_summary(v: &Value, lang: &str) -> Option<Summary> {
    if v["type"].as_str() == Some("disambiguation") {
        return None;
    }
    let extract = v["extract"].as_str().unwrap_or_default();
    if extract.is_empty() {
        return None;
    }
    let s = |v: &Value| v.as_str().map(str::to_owned);
    Some(Summary {
        title: v["title"].as_str().unwrap_or_default().to_owned(),
        extract: extract.to_owned(),
        thumbnail_url: s(&v["thumbnail"]["source"]),
        image_url: s(&v["originalimage"]["source"]),
        page_url: v["content_urls"]["desktop"]["page"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        lang: lang.to_owned(),
    })
}

/// Decode `%XX` escapes (slugs from MusicBrainz URLs are often encoded).
/// Invalid sequences or non-UTF-8 results leave the input unchanged.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_owned())
}

/// Normalized slug: percent-decoded, spaces as underscores.
fn clean_slug(slug: &str) -> String {
    percent_decode(slug).replace(' ', "_")
}

/// Summary endpoint with the slug encoded as a single path segment.
fn summary_url(lang: &str, slug: &str) -> Option<Url> {
    let mut url = Url::parse(&format!(
        "https://{lang}.wikipedia.org/api/rest_v1/page/summary/"
    ))
    .ok()?;
    url.path_segments_mut().ok()?.pop_if_empty().push(slug);
    Some(url)
}

pub struct Wikipedia {
    http: Arc<Http>,
    cache: Arc<Cache>,
}

impl Wikipedia {
    pub fn new(http: Arc<Http>, cache: Arc<Cache>) -> Wikipedia {
        Wikipedia { http, cache }
    }

    /// Summary for an article slug: English, then Spanish.
    pub async fn fetch_summary(&self, slug: &str) -> Option<Summary> {
        for lang in ["en", "es"] {
            if let Outcome::Found(s) = self.summary(lang, slug).await {
                return Some(s);
            }
        }
        None
    }

    /// Artist article: every name variant on English Wikipedia, then on
    /// Spanish.
    pub async fn search_artist(&self, name: &str) -> Option<Summary> {
        let queries = artist_search_queries(name);
        for lang in ["en", "es"] {
            for q in &queries {
                if let Some(s) = self.search(q, lang).await {
                    return Some(s);
                }
            }
        }
        None
    }

    /// Topic lookup: exact slug, then the search API's best title. Only a
    /// genuine "no article" is cached as negative; network failures are not.
    pub async fn search(&self, query: &str, lang: &str) -> Option<Summary> {
        let key = format!("wiki_search_{lang}_{}", query.to_lowercase());
        if let Some(slug) = self.cache.get(&key) {
            if slug == Cache::NONE {
                return None;
            }
            if let Outcome::Found(s) = self.summary(lang, &slug).await {
                return Some(s);
            }
            // Cached slug no longer resolves: fall through to a fresh lookup.
        }

        let mut failed = false;
        let slug = query.replace(' ', "_");
        match self.summary(lang, &slug).await {
            Outcome::Found(s) => {
                self.cache.set(&key, &slug);
                return Some(s);
            }
            Outcome::NotFound => {}
            Outcome::Failed => failed = true,
        }
        match self.search_title(query, lang).await {
            Outcome::Found(title) => {
                let slug = title.replace(' ', "_");
                match self.summary(lang, &slug).await {
                    Outcome::Found(s) => {
                        self.cache.set(&key, &slug);
                        return Some(s);
                    }
                    Outcome::NotFound => {}
                    Outcome::Failed => failed = true,
                }
            }
            Outcome::NotFound => {}
            Outcome::Failed => failed = true,
        }
        if !failed {
            self.cache.set(&key, Cache::NONE);
        }
        None
    }

    /// Best article title from the search API (`srlimit=1`).
    async fn search_title(&self, query: &str, lang: &str) -> Outcome<String> {
        let key = format!("wiki_title_{lang}_{}", query.to_lowercase());
        if let Some(t) = self.cache.get(&key) {
            return if t == Cache::NONE {
                Outcome::NotFound
            } else {
                Outcome::Found(t)
            };
        }
        let url = format!("https://{lang}.wikipedia.org/w/api.php");
        let params = [
            ("action", "query"),
            ("list", "search"),
            ("srsearch", query),
            ("format", "json"),
            ("srlimit", "1"),
        ];
        let Some((v, _)) = self.http.get_json(Service::Other, &url, &params).await else {
            return Outcome::Failed;
        };
        let Some(results) = v["query"]["search"].as_array() else {
            // 200 without the expected shape (e.g. an API error object).
            return Outcome::Failed;
        };
        match results.first().and_then(|r| r["title"].as_str()) {
            Some(t) => {
                self.cache.set(&key, t);
                Outcome::Found(t.to_owned())
            }
            None => {
                self.cache.set(&key, Cache::NONE);
                Outcome::NotFound
            }
        }
    }

    /// REST summary for one wiki. Cached only when it yields a usable
    /// summary (as DrPlayer does).
    async fn summary(&self, lang: &str, slug: &str) -> Outcome<Summary> {
        let slug = clean_slug(slug);
        if slug.is_empty() {
            return Outcome::NotFound;
        }
        let Some(url) = summary_url(lang, &slug) else {
            return Outcome::NotFound;
        };
        // Same key shape as DrPlayer: the unencoded URL.
        let key =
            format!("wiki_{lang}_https://{lang}.wikipedia.org/api/rest_v1/page/summary/{slug}");
        if let Some(v) = self.cache.get_json(&key)
            && let Some(s) = parse_summary(&v, lang)
        {
            return Outcome::Found(s);
        }
        match self.http.get_json(Service::Other, url.as_str(), &[]).await {
            Some((v, raw)) => match parse_summary(&v, lang) {
                Some(s) => {
                    self.cache.set(&key, &raw);
                    Outcome::Found(s)
                }
                None => Outcome::NotFound,
            },
            // Tell "no such article" from a failed request. Only paid on
            // the failure path.
            None => match self.http.status(Service::Other, url.as_str(), &[]).await {
                Some(404) => Outcome::NotFound,
                _ => Outcome::Failed,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_variants() {
        assert_eq!(artist_search_queries("Queen"), vec!["Queen"]);
        assert_eq!(
            artist_search_queries("Simon & Garfunkel"),
            vec!["Simon & Garfunkel", "Simon"]
        );
        assert_eq!(
            artist_search_queries("Santana FEAT. Rob Thomas"),
            vec!["Santana FEAT. Rob Thomas", "Santana"]
        );
        // First separator in list order wins, not first in the string.
        assert_eq!(
            artist_search_queries("Ella with Louis & Band"),
            vec!["Ella with Louis & Band", "Ella with Louis"]
        );
        assert_eq!(artist_search_queries("AC/DC"), vec!["AC/DC", "AC DC"]);
    }

    #[test]
    fn summary_parsing_and_disambiguation() {
        let v: Value = serde_json::from_str(
            r#"{"type":"standard","title":"Queen (band)","extract":"Queen are a British rock band.",
                "thumbnail":{"source":"https://upload/t.jpg"},
                "originalimage":{"source":"https://upload/o.jpg"},
                "content_urls":{"desktop":{"page":"https://en.wikipedia.org/wiki/Queen_(band)"}}}"#,
        )
        .unwrap();
        let s = parse_summary(&v, "en").unwrap();
        assert_eq!(s.title, "Queen (band)");
        assert_eq!(s.thumbnail_url.as_deref(), Some("https://upload/t.jpg"));
        assert_eq!(s.image_url.as_deref(), Some("https://upload/o.jpg"));
        assert_eq!(s.page_url, "https://en.wikipedia.org/wiki/Queen_(band)");
        assert_eq!(s.lang, "en");

        let dis: Value = serde_json::from_str(
            r#"{"type":"disambiguation","title":"Queen","extract":"Queen may refer to:"}"#,
        )
        .unwrap();
        assert!(parse_summary(&dis, "en").is_none());
        let empty: Value = serde_json::from_str(r#"{"type":"standard","extract":""}"#).unwrap();
        assert!(parse_summary(&empty, "en").is_none());
    }

    #[test]
    fn slugs_and_urls() {
        assert_eq!(
            clean_slug("A_Night_at_the_Opera_%28Queen_album%29"),
            "A_Night_at_the_Opera_(Queen_album)"
        );
        assert_eq!(clean_slug("Mot%C3%B6rhead"), "Motörhead");
        assert_eq!(clean_slug("100%"), "100%");
        assert_eq!(clean_slug("Sigur Rós"), "Sigur_Rós");
        assert_eq!(
            summary_url("en", "AC/DC").unwrap().as_str(),
            "https://en.wikipedia.org/api/rest_v1/page/summary/AC%2FDC"
        );
        assert_eq!(
            summary_url("es", "Queen_(banda)").unwrap().as_str(),
            "https://es.wikipedia.org/api/rest_v1/page/summary/Queen_(banda)"
        );
        assert_eq!(
            summary_url("en", "Motörhead").unwrap().as_str(),
            "https://en.wikipedia.org/api/rest_v1/page/summary/Mot%C3%B6rhead"
        );
    }
}
