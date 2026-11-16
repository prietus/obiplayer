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

use obiplayer_core::covers::find_cover;
use obiplayer_core::library::Album;
use obiplayer_core::mpd::Client;
use relm4::gtk::{self, gdk, gdk_pixbuf, gio, glib};
use tokio::sync::Semaphore;

/// Edge in pixels of decoded thumbnails (2× the grid cell for HiDPI).
pub const THUMB_SIZE: i32 = 400;

type Callback = Box<dyn FnOnce(Option<&gdk::Texture>)>;

pub struct CoverLoader {
    client: Client,
    music_dir: Option<PathBuf>,
    cache: RefCell<HashMap<String, Option<gdk::Texture>>>,
    pending: RefCell<HashMap<String, Vec<Callback>>>,
    limit: Arc<Semaphore>,
}

impl CoverLoader {
    pub fn new(client: Client, music_dir: Option<PathBuf>) -> Rc<CoverLoader> {
        Rc::new(CoverLoader {
            client,
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
        let limit = self.limit.clone();
        let this = Rc::clone(self);
        glib::spawn_future_local(async move {
            let bytes = relm4::spawn(async move {
                let _permit = limit.acquire_owned().await.ok()?;
                fetch_bytes(client, local_dir, first_file).await
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

            this.cache.borrow_mut().insert(key.clone(), texture.clone());
            let waiters = this.pending.borrow_mut().remove(&key).unwrap_or_default();
            for w in waiters {
                w(texture.as_ref());
            }
        });
    }
}

async fn fetch_bytes(
    client: Client,
    local_dir: Option<PathBuf>,
    first_file: Option<String>,
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
    let file = first_file?;
    match client.cover_art(&file).await {
        Ok(b) => b,
        Err(e) => {
            tracing::debug!(%file, error = %e, "cover art from MPD failed");
            None
        }
    }
}

/// Decode and scale to fit `size`×`size`, keeping the aspect ratio.
fn decode(bytes: Vec<u8>, size: i32) -> Option<gdk::Texture> {
    let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(bytes));
    let pb =
        gdk_pixbuf::Pixbuf::from_stream_at_scale(&stream, size, size, true, gio::Cancellable::NONE)
            .ok()?;
    let format = if pb.has_alpha() {
        gdk::MemoryFormat::R8g8b8a8
    } else {
        gdk::MemoryFormat::R8g8b8
    };
    let texture = gdk::MemoryTexture::new(
        pb.width(),
        pb.height(),
        format,
        &pb.read_pixel_bytes(),
        pb.rowstride() as usize,
    );
    Some(texture.upcast())
}

use gtk::prelude::Cast;
