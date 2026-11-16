//! Album cover loading and in-memory texture cache.
//!
//! Covers come from the local music directory when it is configured (same
//! search rules as DrPlayer), otherwise from MPD itself (`albumart` /
//! `readpicture`), which also works for remote servers without mounts.
//! Images are decoded and down-scaled off the main thread.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use obiplayer_core::covers::find_cover;
use obiplayer_core::library::Album;
use obiplayer_core::meta::{Services, images};
use obiplayer_core::mpd::Client;
use obiplayer_core::{paths, text};
use relm4::gtk::{self, gdk, gdk_pixbuf, gio, glib, prelude::TextureExt};
use tokio::sync::Semaphore;

/// Edge in pixels of decoded thumbnails (2× the grid cell for HiDPI).
pub const THUMB_SIZE: i32 = 400;

type Callback = Box<dyn FnOnce(Option<&gdk::Texture>)>;

pub struct CoverLoader {
    client: Client,
    services: Arc<Services>,
    music_dir: Option<PathBuf>,
    cache: RefCell<HashMap<String, Option<gdk::Texture>>>,
    pending: RefCell<HashMap<String, Vec<Callback>>>,
    limit: Arc<Semaphore>,
}

impl CoverLoader {
    pub fn new(
        client: Client,
        services: Arc<Services>,
        music_dir: Option<PathBuf>,
    ) -> Rc<CoverLoader> {
        Rc::new(CoverLoader {
            client,
            services,
            music_dir,
            cache: RefCell::default(),
            pending: RefCell::default(),
            limit: Arc::new(Semaphore::new(4)),
        })
    }

    /// Cached result for an album folder: `Some(None)` = known to have no cover.
    pub fn cached(&self, folder: &str) -> Option<Option<gdk::Texture>> {
        self.cache.borrow().get(folder).cloned()
    }

    /// Forget a cached cover (memory and disk) so the next request loads it again.
    pub fn invalidate(&self, folder: &str) {
        self.cache.borrow_mut().remove(folder);
        let (png, none) = disk_paths(folder);
        let _ = std::fs::remove_file(png);
        let _ = std::fs::remove_file(none);
    }

    /// Get the cover, calling `cb` on the main thread (immediately if cached).
    pub fn request(
        self: &Rc<Self>,
        album: &Album,
        cb: impl FnOnce(Option<&gdk::Texture>) + 'static,
    ) {
        if let Some(t) = self.cached(&album.folder) {
            cb(t.as_ref());
            return;
        }
        let key = album.folder.clone();
        {
            let mut pending = self.pending.borrow_mut();
            if let Some(waiters) = pending.get_mut(&key) {
                waiters.push(Box::new(cb));
                return;
            }
            pending.insert(key.clone(), vec![Box::new(cb)]);
        }

        let client = self.client.clone();
        let local_dir = self.music_dir.as_ref().map(|d| d.join(&album.folder));
        let first_file = album.first_file().map(str::to_owned);
        let online = OnlineQuery {
            services: self.services.clone(),
            artist: album.artist.clone(),
            title: album.title.clone(),
            mbid: album.musicbrainz_album_id.clone(),
        };
        let limit = self.limit.clone();
        let this = Rc::clone(self);
        let disk = disk_paths(&key);
        glib::spawn_future_local(async move {
            let texture = match load_disk(disk.clone()).await {
                DiskHit::Texture(t) => Some(t),
                DiskHit::NoCover => None,
                DiskHit::Miss => {
                    let bytes = relm4::spawn(async move {
                        let _permit = limit.acquire_owned().await.ok()?;
                        fetch_bytes(client, local_dir, first_file, online).await
                    })
                    .await
                    .ok()
                    .flatten();
                    let texture = match bytes {
                        Some(b) => gio::spawn_blocking(move || decode(b, THUMB_SIZE))
                            .await
                            .ok()
                            .flatten(),
                        None => None,
                    };
                    store_disk(disk, texture.clone());
                    texture
                }
            };

            this.cache.borrow_mut().insert(key.clone(), texture.clone());
            let waiters = this.pending.borrow_mut().remove(&key).unwrap_or_default();
            for w in waiters {
                w(texture.as_ref());
            }
        });
    }
}

/// How long "this album has no cover" is remembered on disk.
const NO_COVER_TTL: Duration = Duration::from_secs(24 * 3600);

/// Thumbnail and "no cover" marker paths for an album folder.
fn disk_paths(folder: &str) -> (PathBuf, PathBuf) {
    let dir = paths::cache_dir().join("thumbs");
    let key = text::djb2_hex(folder);
    (
        dir.join(format!("{key}.png")),
        dir.join(format!("{key}.none")),
    )
}

enum DiskHit {
    Texture(gdk::Texture),
    NoCover,
    Miss,
}

async fn load_disk((png, none): (PathBuf, PathBuf)) -> DiskHit {
    gio::spawn_blocking(move || {
        if let Ok(t) = gdk::Texture::from_filename(&png) {
            return DiskHit::Texture(t);
        }
        let fresh = std::fs::metadata(&none)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age < NO_COVER_TTL);
        if fresh {
            DiskHit::NoCover
        } else {
            DiskHit::Miss
        }
    })
    .await
    .unwrap_or(DiskHit::Miss)
}

fn store_disk((png, none): (PathBuf, PathBuf), texture: Option<gdk::Texture>) {
    gio::spawn_blocking(move || {
        if let Some(dir) = png.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let r = match texture {
            Some(t) => std::fs::write(&png, t.save_to_png_bytes()),
            None => std::fs::write(&none, b""),
        };
        if let Err(e) = r {
            tracing::debug!(error = %e, "could not write thumbnail cache");
        }
    });
}

/// Last resort: Discogs / Cover Art Archive.
struct OnlineQuery {
    services: Arc<Services>,
    artist: String,
    title: String,
    mbid: Option<String>,
}

async fn fetch_bytes(
    client: Client,
    local_dir: Option<PathBuf>,
    first_file: Option<String>,
    online: OnlineQuery,
) -> Option<Vec<u8>> {
    if let Some(dir) = local_dir {
        let found = tokio::task::spawn_blocking(move || {
            find_cover(&dir).and_then(|p| std::fs::read(p).ok())
        })
        .await
        .ok()
        .flatten();
        if found.is_some() {
            return found;
        }
    }
    if let Some(file) = first_file {
        match client.cover_art(&file).await {
            Ok(Some(b)) => return Some(b),
            Ok(None) => {}
            Err(e) => tracing::debug!(%file, error = %e, "cover art from MPD failed"),
        }
    }
    if online.artist.is_empty() {
        return None;
    }
    images::online_cover(
        &online.services,
        &online.artist,
        &online.title,
        online.mbid.as_deref(),
    )
    .await
}

/// Decode and scale to fit `size`×`size`, keeping the aspect ratio.
fn decode(bytes: Vec<u8>, size: i32) -> Option<gdk::Texture> {
    let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(bytes));
    let pb =
        gdk_pixbuf::Pixbuf::from_stream_at_scale(&stream, size, size, true, gio::Cancellable::NONE)
            .ok()?;
    Some(to_texture(&pb))
}

/// Load a local image scaled to fit `size`, off the main thread.
pub async fn load_file_texture(path: PathBuf, size: i32) -> Option<gdk::Texture> {
    gio::spawn_blocking(move || {
        let pb = gdk_pixbuf::Pixbuf::from_file_at_scale(&path, size, size, true).ok()?;
        Some(to_texture(&pb))
    })
    .await
    .ok()
    .flatten()
}

fn to_texture(pb: &gdk_pixbuf::Pixbuf) -> gdk::Texture {
    let format = if pb.has_alpha() {
        gdk::MemoryFormat::R8g8b8a8
    } else {
        gdk::MemoryFormat::R8g8b8
    };
    gdk::MemoryTexture::new(
        pb.width(),
        pb.height(),
        format,
        &pb.read_pixel_bytes(),
        pb.rowstride() as usize,
    )
    .upcast()
}

use gtk::prelude::Cast;
