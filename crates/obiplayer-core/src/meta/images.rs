//! Downloaded images: artist avatars and album covers from online sources.
//! Both are cached on disk; failures leave a dated marker so they are retried
//! after a while instead of on every view (DrPlayer never retried avatars).

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::Service;
use super::services::Services;
use super::titles;
use crate::{paths, text};

const RETRY_AFTER: Duration = Duration::from_secs(7 * 24 * 3600);
/// Real images are bigger than error pages and 1×1 placeholders.
const MIN_BYTES: usize = 1000;
/// Last.fm's generic "no image" star.
const LASTFM_PLACEHOLDER: &str = "2a96cbd8b46e442fc41c2b86b821562f";

fn marker_fresh(p: &Path) -> bool {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|a| a < RETRY_AFTER)
}

fn store(img: &Path, marker: &Path, bytes: Option<&[u8]>, remember_miss: bool) {
    if let Some(d) = img.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    match bytes {
        Some(b) => {
            let _ = std::fs::write(img, b);
            let _ = std::fs::remove_file(marker);
        }
        None if remember_miss => {
            let _ = std::fs::write(marker, b"");
        }
        None => {}
    }
}

async fn fetch(s: &Services, service: Service, url: &str) -> Option<Vec<u8>> {
    s.http
        .get_bytes(service, url)
        .await
        .filter(|b| b.len() > MIN_BYTES)
}

// -----------------------------------------------------------------------------
// Artist avatars
// -----------------------------------------------------------------------------

fn avatar_paths(name: &str) -> (PathBuf, PathBuf) {
    let dir = paths::cache_dir().join("artist-avatars");
    let key = text::djb2_hex(&text::fold(name));
    (
        dir.join(format!("{key}.img")),
        dir.join(format!("{key}.notfound")),
    )
}

/// Cached avatar file, if any (no network).
pub fn cached_avatar(name: &str) -> Option<PathBuf> {
    let (img, _) = avatar_paths(name);
    img.is_file().then_some(img)
}

/// Artist photo: Wikipedia thumbnail, then Last.fm image. Returns the cached
/// file path.
pub async fn avatar(s: &Services, name: &str) -> Option<PathBuf> {
    let (img, marker) = avatar_paths(name);
    if img.is_file() {
        return Some(img);
    }
    if marker_fresh(&marker) {
        return None;
    }
    let mut bytes = None;
    if let Some(url) = s
        .wiki
        .search_artist(name)
        .await
        .and_then(|w| w.thumbnail_url)
    {
        bytes = fetch(s, Service::Other, &url).await;
    }
    if bytes.is_none()
        && s.lastfm.is_configured()
        && let Some(first) = super::wikipedia::artist_search_queries(name)
            .into_iter()
            .next()
        && let Some(url) = s
            .lastfm
            .artist(&first)
            .await
            .and_then(|a| a.image_url)
            .filter(|u| !u.contains(LASTFM_PLACEHOLDER))
    {
        bytes = fetch(s, Service::Other, &url).await;
    }
    store(&img, &marker, bytes.as_deref(), true);
    bytes.map(|_| img)
}

// -----------------------------------------------------------------------------
// Album covers (when neither the folder nor MPD has one)
// -----------------------------------------------------------------------------

fn cover_paths(artist: &str, album: &str) -> (PathBuf, PathBuf) {
    let dir = paths::cache_dir().join("covers");
    let key = text::djb2_hex(&format!(
        "{}|{}",
        artist.to_lowercase(),
        album.to_lowercase()
    ));
    (
        dir.join(format!("{key}.img")),
        dir.join(format!("{key}.notfound")),
    )
}

const CAA: &str = "https://coverartarchive.org/release";

/// Front cover from Discogs (if configured) or the Cover Art Archive.
pub async fn online_cover(
    s: &Services,
    artist: &str,
    album: &str,
    mbid: Option<&str>,
) -> Option<Vec<u8>> {
    let (img, marker) = cover_paths(artist, album);
    if let Ok(b) = std::fs::read(&img) {
        return Some(b);
    }
    if marker_fresh(&marker) {
        return None;
    }
    let clean = titles::clean_title_for_search(album);
    let mbid = mbid.filter(|m| !m.is_empty());
    let mut bytes = None;

    if s.discogs.is_configured() {
        let release = match mbid {
            Some(id) => match s.mb.fetch_release(id).await {
                Some(r) if !r.catalog_number.is_empty() => {
                    s.discogs
                        .search_by_catalog(&r.catalog_number, &Default::default())
                        .await
                }
                _ => None,
            },
            None => s.discogs.search(artist, &clean).await,
        };
        if let Some(url) = release.and_then(|r| r.image_url) {
            bytes = fetch(s, Service::Discogs, &url).await;
        }
    }
    if bytes.is_none()
        && let Some(id) = mbid
    {
        bytes = fetch(s, Service::Other, &format!("{CAA}/{id}/front")).await;
    }
    if bytes.is_none()
        && let Some(r) =
            s.mb.search_release(artist, &clean, &Default::default())
                .await
    {
        bytes = fetch(s, Service::Other, &format!("{CAA}/{}/front", r.id)).await;
    }
    store(&img, &marker, bytes.as_deref(), true);
    bytes
}

/// Forget a cached online cover (Retry cover art).
pub fn clear_online_cover(artist: &str, album: &str) {
    let (img, marker) = cover_paths(artist, album);
    let _ = std::fs::remove_file(img);
    let _ = std::fs::remove_file(marker);
}
