//! Lyrics from LRCLIB (<https://lrclib.net>): exact match first, then search.
//! Not cached (port of `LyricsService.swift`).

use std::sync::Arc;

use serde_json::Value;

use super::{Http, Service};

const BASE: &str = "https://lrclib.net/api";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lyrics {
    pub plain: Option<String>,
    /// LRC text; parse with [`parse_lrc`].
    pub synced: Option<String>,
    pub instrumental: bool,
}

/// One timed line of synced lyrics.
#[derive(Debug, Clone, PartialEq)]
pub struct LrcLine {
    /// Seconds from the start of the track.
    pub time: f64,
    pub text: String,
}

/// Parse an LRCLIB result object.
pub fn parse_lyrics_json(v: &Value) -> Lyrics {
    let s = |k: &str| v[k].as_str().map(str::to_owned);
    Lyrics {
        plain: s("plainLyrics"),
        synced: s("syncedLyrics"),
        instrumental: v["instrumental"].as_bool().unwrap_or(false),
    }
}

/// `mm:ss`, `mm:ss.ff` or `mm:ss.fff` → seconds. Metadata tags such as
/// `ar:Queen` or `offset:+100` yield `None`.
fn parse_time(s: &str) -> Option<f64> {
    let digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    let (mins, rest) = s.split_once(':')?;
    let (secs, frac) = match rest.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (rest, None),
    };
    if !digits(mins) || !digits(secs) {
        return None;
    }
    let frac = match frac {
        Some(f) if digits(f) => format!("0.{f}").parse().ok()?,
        Some(_) => return None,
        None => 0.0,
    };
    Some(mins.parse::<f64>().ok()? * 60.0 + secs.parse::<f64>().ok()? + frac)
}

/// Parse LRC text into timed lines, sorted by time. A line may carry several
/// leading timestamps (`[00:10.00][01:20.00]chorus`); metadata tags
/// (`[ar:…]`, `[ti:…]`, …) and untimed lines are dropped.
pub fn parse_lrc(lrc: &str) -> Vec<LrcLine> {
    let mut lines = Vec::new();
    for line in lrc.lines() {
        let mut rest = line.trim();
        let mut times = Vec::new();
        while let Some(inner) = rest.strip_prefix('[') {
            let Some(close) = inner.find(']') else { break };
            match parse_time(&inner[..close]) {
                Some(t) => times.push(t),
                None => break,
            }
            rest = &inner[close + 1..];
        }
        let text = rest.trim();
        lines.extend(times.into_iter().map(|time| LrcLine {
            time,
            text: text.to_owned(),
        }));
    }
    lines.sort_by(|a, b| a.time.total_cmp(&b.time));
    lines
}

pub struct LyricsService {
    http: Arc<Http>,
}

impl LyricsService {
    pub fn new(http: Arc<Http>) -> LyricsService {
        LyricsService { http }
    }

    /// Exact match (`/get`, needs album and duration), then `/search` by
    /// artist + title taking the first hit.
    pub async fn fetch(
        &self,
        artist: &str,
        title: &str,
        album: &str,
        duration_secs: f64,
    ) -> Option<Lyrics> {
        let duration = (duration_secs.max(0.0) as u64).to_string();
        let exact = [
            ("artist_name", artist),
            ("track_name", title),
            ("album_name", album),
            ("duration", duration.as_str()),
        ];
        if let Some((v, _)) = self
            .http
            .get_json(Service::Other, &format!("{BASE}/get"), &exact)
            .await
            && v.is_object()
        {
            return Some(parse_lyrics_json(&v));
        }
        let search = [("track_name", title), ("artist_name", artist)];
        let (v, _) = self
            .http
            .get_json(Service::Other, &format!("{BASE}/search"), &search)
            .await?;
        v.as_array()?.first().map(parse_lyrics_json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lrc() {
        let lrc = "[ar:Queen]\n[ti:Bohemian Rhapsody]\n[offset:+100]\n\
                   [00:00.50] Is this the real life?\r\n\
                   [00:04.123]Is this just fantasy?\n\
                   [01:02]\n\
                   no timestamp\n\
                   [00:10.00][00:02.00] Repeated\n";
        let l = parse_lrc(lrc);
        let got: Vec<(f64, &str)> = l.iter().map(|x| (x.time, x.text.as_str())).collect();
        assert_eq!(
            got,
            vec![
                (0.5, "Is this the real life?"),
                (2.0, "Repeated"),
                (4.123, "Is this just fantasy?"),
                (10.0, "Repeated"),
                (62.0, ""),
            ]
        );
    }

    #[test]
    fn rejects_non_times() {
        assert_eq!(parse_time("ar:Queen"), None);
        assert_eq!(parse_time("length:03:45"), None);
        assert_eq!(parse_time("00:1x"), None);
        assert_eq!(parse_time("02:03.5"), Some(123.5));
    }

    #[test]
    fn parses_result_json() {
        let v: Value = serde_json::from_str(
            r#"{"id":1,"plainLyrics":"a\nb","syncedLyrics":null,"instrumental":false}"#,
        )
        .unwrap();
        assert_eq!(
            parse_lyrics_json(&v),
            Lyrics {
                plain: Some("a\nb".into()),
                synced: None,
                instrumental: false
            }
        );
        let v: Value = serde_json::from_str(r#"{"instrumental":true}"#).unwrap();
        assert!(parse_lyrics_json(&v).instrumental);
    }
}
