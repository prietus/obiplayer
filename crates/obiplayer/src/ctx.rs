//! State shared by all pages, and the actions pages send back to the app.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use std::sync::Arc;

use obiplayer_core::catalog::Library;
use obiplayer_core::library::Album;
use obiplayer_core::meta::Services;
use obiplayer_core::meta::release::{self, AlbumQuery, ReleaseInfo};

use crate::app::AppMsg;
use crate::covers::CoverLoader;

/// A page that can be pushed on the navigation stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Album(String),
    Track(String),
    Artist(String),
    Label(String),
    Composer(String),
    Producer(String),
    Search(String),
}

/// Restricts the album grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlbumFilter {
    Genre(String),
    Country(String),
    Format(String),
}

impl AlbumFilter {
    /// `extra`: enriched genres for the album.
    pub fn matches(&self, a: &Album, extra: &[String]) -> bool {
        use obiplayer_core::text::fold;
        match self {
            AlbumFilter::Genre(g) => a.genres.iter().chain(extra).any(|x| fold(x) == fold(g)),
            AlbumFilter::Country(c) => {
                a.country
                    .as_deref()
                    .is_some_and(|x| x.eq_ignore_ascii_case(c))
                    || a.tracks.iter().any(|t| {
                        t.tag("releasecountry")
                            .is_some_and(|x| x.eq_ignore_ascii_case(c))
                    })
            }
            AlbumFilter::Format(f) => a.format().to_uppercase().contains(&f.to_uppercase()),
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            AlbumFilter::Genre(_) => "tag-symbolic",
            AlbumFilter::Country(_) => "globe-symbolic",
            AlbumFilter::Format(_) => "audio-x-generic-symbolic",
        }
    }

    pub fn label(&self) -> String {
        match self {
            AlbumFilter::Genre(g) => g.to_uppercase(),
            AlbumFilter::Country(c) => c.clone(),
            AlbumFilter::Format(f) => f.to_uppercase(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Action {
    PlayAlbum(String),
    EnqueueAlbum(String),
    /// Add a file to the queue and play it now.
    PlayFile(String),
    EnqueueFile(String),
    Open(Route),
    Filter(AlbumFilter),
    SetRating(String, u8),
    SetPreferred(String),
    UpdateDb(String),
    RetryCover(String),
    OpenFolder(String),
    Transport(crate::player_bar::Transport),
    /// Seek within the current track (seconds).
    Seek(f64),
    ShowNowPlaying(bool),
}

/// Playback position, sampled from MPD and interpolated locally (MPD's
/// `idle` does not report time progress).
#[derive(Debug, Clone, Copy)]
pub struct Clock {
    pub elapsed: f64,
    pub duration: f64,
    pub playing: bool,
    pub at: Instant,
}

impl Default for Clock {
    fn default() -> Self {
        Clock {
            elapsed: 0.0,
            duration: 0.0,
            playing: false,
            at: Instant::now(),
        }
    }
}

impl Clock {
    pub fn now(&self) -> f64 {
        let e = if self.playing {
            self.elapsed + self.at.elapsed().as_secs_f64()
        } else {
            self.elapsed
        };
        if self.duration > 0.0 {
            e.min(self.duration)
        } else {
            e
        }
    }

    pub fn fraction(&self) -> f64 {
        if self.duration > 0.0 {
            (self.now() / self.duration).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

pub struct Ctx {
    pub library: RefCell<Rc<Library>>,
    pub covers: Rc<CoverLoader>,
    pub music_dir: Option<PathBuf>,
    /// file -> rating 1..=5 (only files whose stickers were loaded).
    pub ratings: RefCell<HashMap<String, u8>>,
    pub preferred: RefCell<HashSet<String>>,
    pub current_file: RefCell<Option<String>>,
    /// MPD URI -> DR value (cached or freshly analysed).
    pub dr: RefCell<HashMap<String, i32>>,
    pub clock: Cell<Clock>,
    /// Mono 44.1 kHz samples of the current track, for the visualizers.
    pub samples: RefCell<crate::viz::Samples>,
    pub services: Arc<Services>,
    /// Online release data per album folder (looked up once per session).
    pub releases: RefCell<HashMap<String, Rc<ReleaseInfo>>>,
    /// Genres added by enrichment, per album folder.
    pub extra_genres: RefCell<HashMap<String, Vec<String>>>,
    tx: relm4::Sender<AppMsg>,
}

impl Ctx {
    pub fn new(
        covers: Rc<CoverLoader>,
        music_dir: Option<PathBuf>,
        services: Arc<Services>,
        tx: relm4::Sender<AppMsg>,
    ) -> Rc<Ctx> {
        Rc::new(Ctx {
            library: RefCell::new(Rc::new(Library::default())),
            covers,
            music_dir,
            ratings: RefCell::default(),
            preferred: RefCell::default(),
            current_file: RefCell::default(),
            dr: RefCell::default(),
            clock: Cell::default(),
            samples: RefCell::default(),
            services,
            releases: RefCell::default(),
            extra_genres: RefCell::default(),
            tx,
        })
    }

    /// Album genres including enriched ones.
    pub fn genres(&self, album: &Album) -> Vec<String> {
        let mut g = album.genres.clone();
        if let Some(extra) = self.extra_genres.borrow().get(&album.folder) {
            g.extend(extra.iter().cloned());
        }
        g
    }

    /// Online release data for an album (cached for the session). Runs the
    /// network cascade on the Tokio runtime; await from the GTK main loop.
    pub async fn release_info(&self, album: &Album) -> Rc<ReleaseInfo> {
        if let Some(r) = self.releases.borrow().get(&album.folder) {
            return r.clone();
        }
        let q = AlbumQuery {
            folder: album.folder.clone(),
            title: album.title.clone(),
            artist: album.artist.clone(),
            format: album.format(),
            label: album.label.clone().unwrap_or_default(),
            catalog: album.catalog_number.clone().unwrap_or_default(),
            country: album.country.clone().unwrap_or_default(),
            barcode: String::new(),
            musicbrainz_id: album.musicbrainz_album_id.clone(),
            first_track_path: album.first_file().and_then(|f| self.local_path(f)),
        };
        let s = self.services.clone();
        let info = relm4::spawn(async move { release::lookup(&s, &q).await })
            .await
            .unwrap_or_default();
        let info = Rc::new(info);
        self.releases
            .borrow_mut()
            .insert(album.folder.clone(), info.clone());
        info
    }

    pub fn act(&self, a: Action) {
        self.tx.emit(AppMsg::Act(a));
    }

    pub fn lib(&self) -> Rc<Library> {
        self.library.borrow().clone()
    }

    pub fn is_current(&self, file: &str) -> bool {
        self.current_file.borrow().as_deref() == Some(file)
    }

    pub fn dr(&self, file: &str) -> Option<i32> {
        self.dr.borrow().get(file).copied()
    }

    /// Album DR from whatever track values are known.
    pub fn album_dr(&self, album: &Album) -> Option<i32> {
        let d = self.dr.borrow();
        obiplayer_core::analysis::dr14::album_dr(
            album.tracks.iter().filter_map(|t| d.get(&t.file).copied()),
        )
    }

    pub fn rating(&self, file: &str) -> u8 {
        self.ratings.borrow().get(file).copied().unwrap_or(0)
    }

    /// Local path of an album/track, when the music directory is known.
    pub fn local_path(&self, rel: &str) -> Option<PathBuf> {
        self.music_dir.as_ref().map(|d| d.join(rel))
    }
}
