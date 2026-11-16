//! Producers/engineers across the library, from MusicBrainz releases already
//! in the cache (no network: albums gain credits as they are browsed).

use std::collections::HashMap;
use std::path::Path;

use super::discogs::PressingInfo;
use super::release::folder_key;
use super::services::Services;
use super::titles;
use crate::catalog::Library;
use crate::text;

/// Credit roles that make someone show up here.
const ROLES: &[&str] = &[
    "producer",
    "executive producer",
    "mastering",
    "engineer",
    "recording",
    "mix",
    "balance",
    "remastered",
    "remixer",
];

#[derive(Debug, Clone, PartialEq)]
pub struct Producer {
    pub name: String,
    /// (album index, roles on that album)
    pub albums: Vec<(u32, Vec<String>)>,
}

impl Producer {
    /// Distinct base roles ("producer", "engineer"…), text before " (".
    pub fn base_roles(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for (_, roles) in &self.albums {
            for r in roles {
                let base = r.split(" (").next().unwrap_or(r).to_lowercase();
                if !v.contains(&base) {
                    v.push(base);
                }
            }
        }
        v
    }
}

/// Blocking (reads the metadata cache from disk).
pub fn collect(s: &Services, lib: &Library) -> Vec<Producer> {
    let mut map: HashMap<String, Producer> = HashMap::new();
    for (i, a) in lib.albums.iter().enumerate() {
        let release = s
            .cache
            .get(&folder_key(&a.folder))
            .and_then(|id| s.mb.release_from_cache(&id))
            .or_else(|| {
                a.musicbrainz_album_id
                    .as_deref()
                    .and_then(|id| s.mb.release_from_cache(id))
            })
            .or_else(|| {
                s.mb.cached_release(&a.artist, &titles::clean_title_for_search(&a.title))
            });
        let Some(r) = release else { continue };
        for c in &r.credits {
            let role = c.role.to_lowercase();
            if !ROLES.iter().any(|x| role.contains(x)) {
                continue;
            }
            let p = map.entry(text::fold(&c.name)).or_insert_with(|| Producer {
                name: c.name.clone(),
                albums: Vec::new(),
            });
            match p.albums.iter_mut().find(|(ai, _)| *ai == i as u32) {
                Some((_, roles)) => {
                    if !roles.contains(&c.role) {
                        roles.push(c.role.clone());
                    }
                }
                None => p.albums.push((i as u32, vec![c.role.clone()])),
            }
        }
    }
    let mut v: Vec<Producer> = map.into_values().collect();
    v.sort_by(|a, b| {
        b.albums
            .len()
            .cmp(&a.albums.len())
            .then_with(|| text::compare(&a.name, &b.name))
    });
    v
}

/// `.drmobile-pressing.json` sidecar (DrPlayer/DrMobile pressing certification).
pub fn read_pressing(album_dir: &Path) -> Option<PressingInfo> {
    let text = std::fs::read_to_string(album_dir.join(".drmobile-pressing.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    serde_json::from_value(v.get("info")?.clone()).ok()
}
