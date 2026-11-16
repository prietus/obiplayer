//! All metadata clients, built once from the user's keys and shared.

use std::sync::Arc;

use super::discogs::Discogs;
use super::lastfm::LastFm;
use super::loudnesswar::LoudnessWar;
use super::lyrics::LyricsService;
use super::musicbrainz::MusicBrainz;
use super::wikipedia::Wikipedia;
use super::{Cache, Http, Keys};

pub struct Services {
    pub http: Arc<Http>,
    pub cache: Arc<Cache>,
    pub mb: MusicBrainz,
    pub wiki: Wikipedia,
    pub lastfm: LastFm,
    pub discogs: Discogs,
    pub lyrics: LyricsService,
    pub loudness: LoudnessWar,
}

impl Services {
    pub fn new(keys: &Keys) -> Arc<Services> {
        let http = Arc::new(Http::new());
        let cache = Arc::new(Cache::default());
        Arc::new(Services {
            mb: MusicBrainz::new(http.clone(), cache.clone()),
            wiki: Wikipedia::new(http.clone(), cache.clone()),
            lastfm: LastFm::new(http.clone(), cache.clone(), keys.lastfm.clone()),
            discogs: Discogs::new(http.clone(), cache.clone(), keys.discogs.clone()),
            lyrics: LyricsService::new(http.clone()),
            loudness: LoudnessWar::new(http.clone()),
            http,
            cache,
        })
    }
}
