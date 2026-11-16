//! Genre enrichment for albums with few genre tags (MusicBrainz release
//! genres, Last.fm album tags, falling back to Last.fm artist tags).

use std::path::PathBuf;

use super::services::Services;
use super::titles;
use crate::library::Album;
use crate::{paths, text};

const EMPTY: &str = "(empty)";

/// Album artist variants to try ("AC-DC" ↔ "AC/DC").
pub fn artist_search_names(name: &str) -> Vec<String> {
    let mut v = vec![name.to_owned()];
    if name.contains('-') {
        v.push(name.replace('-', "/"));
    }
    if name.contains('/') {
        v.push(name.replace('/', "-"));
    }
    v
}

/// Tags Last.fm users attach that aren't genres.
const NOT_GENRES: &[&str] = &[
    "seen live",
    "favorite",
    "favorites",
    "favourite",
    "favourites",
    "albums i own",
    "owned",
    "own",
    "awesome",
    "amazing",
    "good",
    "great",
    "best",
    "love",
    "loved",
];

fn cache_path(album: &Album) -> PathBuf {
    let key = format!(
        "{}|{}",
        album.artist.to_lowercase(),
        titles::clean_title_for_search(&album.title).to_lowercase()
    );
    paths::cache_dir()
        .join("genres")
        .join(format!("{}.genres", text::djb2_hex(&key)))
}

/// Cached enrichment for an album (`Some(empty)` = looked up, nothing new).
pub fn cached(album: &Album) -> Option<Vec<String>> {
    let s = std::fs::read_to_string(cache_path(album)).ok()?;
    Some(if s.trim() == EMPTY {
        Vec::new()
    } else {
        s.lines()
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect()
    })
}

pub fn cache_count() -> usize {
    std::fs::read_dir(paths::cache_dir().join("genres"))
        .map(|d| d.count())
        .unwrap_or(0)
}

/// Keep real, new genres: trimmed, capitalised, not already on the album,
/// deduplicated case-insensitively.
fn normalize(found: Vec<String>, existing: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for g in found {
        let g = g.trim();
        if g.is_empty() || NOT_GENRES.contains(&g.to_lowercase().as_str()) {
            continue;
        }
        let g = text::capitalize_words(g);
        let f = text::fold(&g);
        if existing
            .iter()
            .chain(out.iter())
            .any(|x| text::fold(x) == f)
        {
            continue;
        }
        out.push(g);
    }
    out
}

/// New genres for `album` (network; cached).
pub async fn enrich(s: &Services, album: &Album) -> Vec<String> {
    if let Some(c) = cached(album) {
        return c;
    }
    let mut found: Vec<String> = Vec::new();
    if let Some(id) = album.musicbrainz_album_id.as_deref()
        && let Some(r) = s.mb.fetch_release(id).await
    {
        found.extend(r.genres);
    }
    let clean = titles::clean_title_for_search(&album.title);
    let names = artist_search_names(&album.artist);
    if s.lastfm.is_configured() && !album.artist.is_empty() {
        'outer: for n in &names {
            for t in [clean.as_str(), album.title.as_str()] {
                let tags = s.lastfm.album_tags(n, t).await;
                if !tags.is_empty() {
                    found.extend(tags);
                    break 'outer;
                }
                if clean == album.title {
                    break;
                }
            }
        }
        if found.is_empty() {
            for n in &names {
                if let Some(a) = s.lastfm.artist(n).await
                    && !a.tags.is_empty()
                {
                    found.extend(a.tags);
                    break;
                }
            }
        }
    }
    let new = normalize(found, &album.genres);

    // Only remember "nothing" when a service that could answer is configured,
    // so adding keys later triggers a retry.
    if !new.is_empty() || s.lastfm.is_configured() || s.discogs.is_configured() {
        let p = cache_path(album);
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let body = if new.is_empty() {
            EMPTY.to_owned()
        } else {
            new.join("\n")
        };
        let _ = std::fs::write(p, body);
    }
    new
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(artist_search_names("AC-DC"), ["AC-DC", "AC/DC"]);
        assert_eq!(artist_search_names("Queen"), ["Queen"]);
    }

    #[test]
    fn normalization() {
        let n = normalize(
            vec![
                "classic rock".into(),
                "Rock".into(),
                "seen live".into(),
                "CLASSIC ROCK".into(),
                " ".into(),
                "UK".into(),
            ],
            &["rock".into()],
        );
        assert_eq!(n, ["Classic Rock", "UK"]);
    }
}
