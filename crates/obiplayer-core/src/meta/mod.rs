//! Online metadata services (MusicBrainz, Discogs, Wikipedia, Last.fm,
//! LRCLIB, Loudness War DB) and what is built on them.
//!
//! Shared infrastructure lives here: an HTTP client with per-service rate
//! limiting ([`Http`]) and a disk cache with TTL ([`Cache`]). Services hold an
//! `Arc` to both and return `Option`s: a lookup that fails for any reason is
//! simply "no data", never shown as an error to the user.

pub mod cache;
pub mod discogs;
pub mod genres;
pub mod http;
pub mod images;
pub mod lastfm;
pub mod loudnesswar;
pub mod lyrics;
pub mod musicbrainz;
pub mod producers;
pub mod release;
pub mod services;
pub mod titles;
pub mod wikipedia;

pub use cache::Cache;
pub use http::{Http, Service};
pub use services::Services;

/// API credentials from the user's configuration.
#[derive(Debug, Clone, Default)]
pub struct Keys {
    pub lastfm: Option<String>,
    /// (consumer key, consumer secret)
    pub discogs: Option<(String, String)>,
}

impl Keys {
    pub fn from_config(c: &crate::config::ApiConfig) -> Keys {
        let some = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_owned());
        Keys {
            lastfm: some(&c.lastfm_api_key),
            discogs: some(&c.discogs_key).zip(some(&c.discogs_secret)),
        }
    }
}
