//! State shared by all pages, and the actions pages send back to the app.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use obiplayer_core::catalog::Library;
use obiplayer_core::library::Album;

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
    pub fn matches(&self, a: &Album) -> bool {
        use obiplayer_core::text::fold;
        match self {
            AlbumFilter::Genre(g) => a.genres.iter().any(|x| fold(x) == fold(g)),
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
    tx: relm4::Sender<AppMsg>,
}

impl Ctx {
    pub fn new(
        covers: Rc<CoverLoader>,
        music_dir: Option<PathBuf>,
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
            tx,
        })
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
