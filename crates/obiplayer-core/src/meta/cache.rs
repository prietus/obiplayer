//! Disk cache for metadata responses: `$XDG_CACHE_HOME/obiplayer/metadata/
//! <djb2(key)>.cache`, expiring by file age (default 30 days).
//!
//! Negative results are stored as [`Cache::NONE`] / [`Cache::EMPTY`] so a
//! miss is not re-queried on every view.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{paths, text};

pub const DEFAULT_TTL: Duration = Duration::from_secs(30 * 24 * 3600);

pub struct Cache {
    dir: PathBuf,
}

impl Default for Cache {
    fn default() -> Self {
        Cache::new(paths::cache_dir().join("metadata"))
    }
}

impl Cache {
    /// "Looked up, nothing found."
    pub const NONE: &'static str = "(none)";
    /// "Looked up, empty list."
    pub const EMPTY: &'static str = "(empty)";

    pub fn new(dir: impl Into<PathBuf>) -> Cache {
        Cache { dir: dir.into() }
    }

    fn path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{}.cache", text::djb2_hex(key)))
    }

    fn fresh(path: &Path, ttl: Duration) -> bool {
        std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age < ttl)
    }

    pub fn get_ttl(&self, key: &str, ttl: Duration) -> Option<String> {
        let p = self.path(key);
        if !Self::fresh(&p, ttl) {
            return None;
        }
        std::fs::read_to_string(p).ok()
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.get_ttl(key, DEFAULT_TTL)
    }

    pub fn set(&self, key: &str, value: &str) {
        if let Err(e) = std::fs::create_dir_all(&self.dir)
            .and_then(|_| std::fs::write(self.path(key), value))
        {
            tracing::debug!(error = %e, "metadata cache write failed");
        }
    }

    pub fn get_json(&self, key: &str) -> Option<serde_json::Value> {
        serde_json::from_str(&self.get(key)?).ok()
    }

    /// Delete every entry (Settings › Purge metadata caches).
    pub fn purge(&self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_ttl() {
        let dir = std::env::temp_dir().join(format!("obi-meta-{}", std::process::id()));
        let c = Cache::new(&dir);
        assert_eq!(c.get("k"), None);
        c.set("k", "v");
        assert_eq!(c.get("k").as_deref(), Some("v"));
        assert_eq!(c.get_ttl("k", Duration::ZERO), None);
        c.set("j", r#"{"a":1}"#);
        assert_eq!(c.get_json("j").unwrap()["a"], 1);
        c.purge();
        assert_eq!(c.get("k"), None);
    }
}
