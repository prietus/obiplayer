//! System tray icon (`org.kde.StatusNotifierItem`), so the window can be
//! closed while the app keeps running next to the clock.
//!
//! The item (`ksni`) lives on Relm4's tokio runtime; its callbacks only push
//! a [`TrayCommand`] through an `async_channel` that is drained on the GTK
//! main thread, so `on_command` always runs there. State pushes go the other
//! way through a second channel.
//!
//! Not every desktop has a tray host (GNOME needs an extension): closing to
//! the tray is only honoured while [`Tray::is_available`] says a host is
//! showing the icon, otherwise the window could not be brought back.
//!
//! On anything other than Linux the handle is a no-op stub.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use obiplayer_core::mpd::{Song, Status};

/// User preference (`[ui] close_to_tray`), live-updated from Preferences.
static CLOSE_TO_TRAY: AtomicBool = AtomicBool::new(true);

pub fn set_close_to_tray(on: bool) {
    CLOSE_TO_TRAY.store(on, Ordering::Relaxed);
}

/// A request coming from the tray icon or its menu.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub enum TrayCommand {
    ToggleWindow,
    /// The tray host went away: a hidden window must come back.
    ShowWindow,
    PlayPause,
    Next,
    Previous,
    Quit,
}

/// Handle to the tray icon. Cheap to clone.
#[derive(Clone)]
pub struct Tray {
    available: Arc<AtomicBool>,
    #[cfg(target_os = "linux")]
    tx: async_channel::Sender<imp::Update>,
}

impl Tray {
    /// True while a tray host is showing the icon.
    pub fn is_available(&self) -> bool {
        self.available.load(Ordering::Relaxed)
    }

    /// Whether closing the window should hide it instead of quitting.
    pub fn closes_to_tray(&self) -> bool {
        CLOSE_TO_TRAY.load(Ordering::Relaxed) && self.is_available()
    }
}

#[cfg(not(target_os = "linux"))]
impl Tray {
    pub fn start(on_command: impl Fn(TrayCommand) + 'static) -> Tray {
        let _ = on_command;
        Tray {
            available: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn update(&self, _status: &Status, _song: Option<&Song>) {}

    pub fn set_window_visible(&self, _visible: bool) {}
}

#[cfg(target_os = "linux")]
impl Tray {
    /// Register the tray icon. Never fails hard: without a session bus or a
    /// tray host the handle just reports itself as unavailable. Must be
    /// called on the GTK main thread.
    pub fn start(on_command: impl Fn(TrayCommand) + 'static) -> Tray {
        let available = Arc::new(AtomicBool::new(false));
        Tray {
            tx: imp::start(on_command, available.clone()),
            available,
        }
    }

    /// Push the current MPD state (tooltip and Play/Pause label).
    pub fn update(&self, status: &Status, song: Option<&Song>) {
        use obiplayer_core::mpd::PlayState;
        let track = song
            .map(|s| {
                let title = s.display_title();
                match s.tag_values("artist").iter().find(|a| !a.is_empty()) {
                    Some(artist) => format!("{artist} — {title}"),
                    None => title.to_string(),
                }
            })
            .unwrap_or_default();
        let _ = self.tx.try_send(imp::Update::Track {
            track,
            playing: status.state == PlayState::Play,
        });
    }

    pub fn set_window_visible(&self, visible: bool) {
        let _ = self.tx.try_send(imp::Update::WindowVisible(visible));
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use ksni::TrayMethods;
    use relm4::gtk::{self, gdk, gdk_pixbuf, gio, glib};

    use super::TrayCommand;
    use crate::tr;

    #[derive(Debug, PartialEq)]
    pub enum Update {
        Track { track: String, playing: bool },
        WindowVisible(bool),
    }

    /// Menu labels, translated up front on the main thread.
    struct Labels {
        show: String,
        hide: String,
        play: String,
        pause: String,
        previous: String,
        next: String,
        quit: String,
    }

    struct Item {
        cmd: async_channel::Sender<TrayCommand>,
        available: Arc<AtomicBool>,
        icon_name: String,
        icon_pixmap: Vec<ksni::Icon>,
        labels: Labels,
        track: String,
        playing: bool,
        window_visible: bool,
    }

    impl Item {
        fn entry(&self, label: &str, icon: &str, cmd: TrayCommand) -> ksni::MenuItem<Self> {
            ksni::menu::StandardItem {
                // Underscores would be taken as access-key markers.
                label: label.replace('_', "__"),
                icon_name: icon.into(),
                activate: Box::new(move |this: &mut Self| this.send(cmd)),
                ..Default::default()
            }
            .into()
        }

        fn send(&self, cmd: TrayCommand) {
            // Unbounded: never blocks the D-Bus task.
            let _ = self.cmd.try_send(cmd);
        }
    }

    impl ksni::Tray for Item {
        fn id(&self) -> String {
            crate::APP_ID.into()
        }

        fn title(&self) -> String {
            "ObiPlayer".into()
        }

        fn icon_name(&self) -> String {
            self.icon_name.clone()
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            self.icon_pixmap.clone()
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                title: "ObiPlayer".into(),
                description: self.track.clone(),
                ..Default::default()
            }
        }

        fn activate(&mut self, _x: i32, _y: i32) {
            self.send(TrayCommand::ToggleWindow);
        }

        fn secondary_activate(&mut self, _x: i32, _y: i32) {
            self.send(TrayCommand::PlayPause);
        }

        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            let l = &self.labels;
            let (toggle, play, play_icon) = (
                if self.window_visible {
                    &l.hide
                } else {
                    &l.show
                },
                if self.playing { &l.pause } else { &l.play },
                if self.playing {
                    "media-playback-pause-symbolic"
                } else {
                    "media-playback-start-symbolic"
                },
            );
            vec![
                self.entry(toggle, "", TrayCommand::ToggleWindow),
                ksni::MenuItem::Separator,
                self.entry(play, play_icon, TrayCommand::PlayPause),
                self.entry(
                    &l.previous,
                    "media-skip-backward-symbolic",
                    TrayCommand::Previous,
                ),
                self.entry(&l.next, "media-skip-forward-symbolic", TrayCommand::Next),
                ksni::MenuItem::Separator,
                self.entry(&l.quit, "application-exit-symbolic", TrayCommand::Quit),
            ]
        }

        fn watcher_online(&self) {
            tracing::info!("tray host is back");
            self.available.store(true, Ordering::Relaxed);
        }

        fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
            tracing::info!("no tray host: {reason:?}");
            self.available.store(false, Ordering::Relaxed);
            self.send(TrayCommand::ShowWindow);
            // Keep waiting: the bar may (re)start later.
            true
        }
    }

    pub fn start(
        on_command: impl Fn(TrayCommand) + 'static,
        available: Arc<AtomicBool>,
    ) -> async_channel::Sender<Update> {
        let (cmd_tx, cmd_rx) = async_channel::unbounded();
        glib::spawn_future_local(async move {
            while let Ok(cmd) = cmd_rx.recv().await {
                on_command(cmd);
            }
        });

        let (icon_name, icon_pixmap) = icon();
        let item = Item {
            cmd: cmd_tx,
            available: available.clone(),
            icon_name,
            icon_pixmap,
            labels: Labels {
                show: tr("Show ObiPlayer"),
                hide: tr("Hide ObiPlayer"),
                play: tr("Play"),
                pause: tr("Pause"),
                previous: tr("Previous"),
                next: tr("Next"),
                quit: tr("Quit"),
            },
            track: String::new(),
            playing: false,
            window_visible: true,
        };

        let (tx, rx) = async_channel::unbounded::<Update>();
        relm4::spawn(async move {
            // `watcher_offline` flips this back during `spawn` when there is
            // no host yet (soft error, see `assume_sni_available`).
            available.store(true, Ordering::Relaxed);
            let handle = match item
                // A sandboxed app cannot own `org.kde.StatusNotifierItem-*`.
                .disable_dbus_name(std::path::Path::new("/.flatpak-info").exists())
                .assume_sni_available(true)
                .spawn()
                .await
            {
                Ok(h) => h,
                Err(e) => {
                    available.store(false, Ordering::Relaxed);
                    tracing::warn!("tray icon unavailable: {e}");
                    return;
                }
            };
            let (mut last_track, mut last_visible) = (None, None);
            while let Ok(update) = rx.recv().await {
                // The same MPD state is pushed on every idle event.
                let seen = match &update {
                    Update::Track { .. } => &mut last_track,
                    Update::WindowVisible(_) => &mut last_visible,
                };
                if seen.as_ref() == Some(&update) {
                    continue;
                }
                let alive = handle
                    .update(|item| match &update {
                        Update::Track { track, playing } => {
                            item.track = track.clone();
                            item.playing = *playing;
                        }
                        Update::WindowVisible(v) => item.window_visible = *v,
                    })
                    .await
                    .is_some();
                if !alive {
                    break;
                }
                *seen = Some(update);
            }
        });
        tx
    }

    /// The themed icon when the app is installed; otherwise (running from the
    /// build tree) the embedded SVG rendered to ARGB32.
    fn icon() -> (String, Vec<ksni::Icon>) {
        let themed = gdk::Display::default()
            .is_some_and(|d| gtk::IconTheme::for_display(&d).has_icon(crate::APP_ID));
        if themed {
            return (crate::APP_ID.into(), Vec::new());
        }
        const SVG: &[u8] = include_bytes!("../../../data/us.priet.ObiPlayer.svg");
        let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_static(SVG));
        let pixbuf =
            gdk_pixbuf::Pixbuf::from_stream_at_scale(&stream, 64, 64, true, gio::Cancellable::NONE)
                .ok()
                .and_then(|p| p.add_alpha(false, 0, 0, 0).ok());
        let Some(p) = pixbuf else {
            return ("audio-x-generic".into(), Vec::new());
        };
        let (w, h, stride) = (
            p.width() as usize,
            p.height() as usize,
            p.rowstride() as usize,
        );
        let bytes = p.read_pixel_bytes();
        let mut data = Vec::with_capacity(w * h * 4);
        for row in bytes.chunks(stride).take(h) {
            for px in row[..w * 4].as_chunks::<4>().0 {
                data.extend_from_slice(&[px[3], px[0], px[1], px[2]]); // RGBA -> ARGB
            }
        }
        let icon = ksni::Icon {
            width: w as i32,
            height: h as i32,
            data,
        };
        (String::new(), vec![icon])
    }
}
