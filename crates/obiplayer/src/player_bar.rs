//! Now-playing bar: cover, track info, audio format, progress and transport.

use std::rc::Rc;
use std::time::{Duration, Instant};

use obiplayer_core::library::Album;
use obiplayer_core::mpd::{PlayState, Single, Song, Status};
use relm4::gtk::{self, glib, prelude::*};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use crate::covers::CoverLoader;
use crate::tr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    TogglePause,
    Stop,
    Next,
    Previous,
    ToggleShuffle,
    CycleRepeat,
}

#[derive(Debug)]
pub enum BarInput {
    Update {
        status: Box<Status>,
        song: Option<Box<Song>>,
        album: Option<std::sync::Arc<Album>>,
    },
    QueueShown(bool),
    Tick,
}

#[derive(Debug)]
pub enum BarOutput {
    Transport(Transport),
    ShowQueue(bool),
    OpenCurrentAlbum,
}

pub struct PlayerBar {
    status: Status,
    song: Option<Song>,
    album_folder: Option<String>,
    /// When `status.elapsed` was sampled, for local interpolation between
    /// server events (MPD's idle doesn't report time progress).
    sampled_at: Instant,
    queue_shown: bool,
    covers: Rc<CoverLoader>,
}

pub struct BarWidgets {
    cover: gtk::Picture,
    cover_placeholder: gtk::Image,
    title: gtk::Label,
    artist_album: gtk::Label,
    album_button: gtk::Button,
    format: gtk::Label,
    bitrate: gtk::Label,
    progress: gtk::ProgressBar,
    elapsed: gtk::Label,
    total: gtk::Label,
    play: gtk::Button,
    shuffle: gtk::Button,
    repeat: gtk::Button,
    queue: gtk::ToggleButton,
    queue_handler: glib::SignalHandlerId,
    cover_folder: Rc<std::cell::RefCell<Option<String>>>,
}

fn mmss(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

impl PlayerBar {
    fn elapsed_now(&self) -> f64 {
        let base = self.status.elapsed.unwrap_or(0.0);
        if self.status.state == PlayState::Play {
            base + self.sampled_at.elapsed().as_secs_f64()
        } else {
            base
        }
        .min(self.status.duration.unwrap_or(f64::MAX))
    }
}

impl SimpleComponent for PlayerBar {
    type Init = Rc<CoverLoader>;
    type Input = BarInput;
    type Output = BarOutput;
    type Root = gtk::Box;
    type Widgets = BarWidgets;

    fn init_root() -> gtk::Box {
        gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_start(12)
            .margin_end(12)
            .margin_top(8)
            .margin_bottom(8)
            .css_classes(["player-bar"])
            .build()
    }

    fn init(
        covers: Rc<CoverLoader>,
        root: gtk::Box,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // -- Row 1: cover · text · audio info ----------------------------------
        let cover = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Cover)
            .width_request(48)
            .height_request(48)
            .build();
        let cover_placeholder = gtk::Image::builder()
            .icon_name("audio-x-generic-symbolic")
            .pixel_size(24)
            .css_classes(["dim-label"])
            .build();
        let cover_frame = gtk::Overlay::builder()
            .width_request(48)
            .height_request(48)
            .overflow(gtk::Overflow::Hidden)
            .valign(gtk::Align::Center)
            .css_classes(["bar-cover"])
            .build();
        cover_frame.set_child(Some(&cover_placeholder));
        cover_frame.add_overlay(&cover);

        let title = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["heading"])
            .build();
        let artist_album = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["dim-label"])
            .build();
        let album_button = gtk::Button::builder()
            .child(&artist_album)
            .halign(gtk::Align::Start)
            .css_classes(["flat", "link-like"])
            .tooltip_text(tr("Show album"))
            .build();
        {
            let s = sender.clone();
            album_button.connect_clicked(move |_| {
                let _ = s.output(BarOutput::OpenCurrentAlbum);
            });
        }
        let text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        text.append(&title);
        text.append(&album_button);

        let format = gtk::Label::builder()
            .xalign(1.0)
            .css_classes(["monospace", "caption"])
            .build();
        let bitrate = gtk::Label::builder()
            .xalign(1.0)
            .css_classes(["monospace", "caption", "dim-label"])
            .build();
        let info = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Center)
            .build();
        info.append(&format);
        info.append(&bitrate);

        let row1 = gtk::Box::builder().spacing(12).build();
        row1.append(&cover_frame);
        row1.append(&text);
        row1.append(&info);

        // -- Row 2: progress ----------------------------------------------------
        let elapsed = gtk::Label::builder()
            .css_classes(["monospace", "caption", "dim-label"])
            .width_chars(6)
            .xalign(0.0)
            .build();
        let total = gtk::Label::builder()
            .css_classes(["monospace", "caption", "dim-label"])
            .width_chars(6)
            .xalign(1.0)
            .build();
        let progress = gtk::ProgressBar::builder()
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        let row2 = gtk::Box::builder().spacing(8).build();
        row2.append(&elapsed);
        row2.append(&progress);
        row2.append(&total);

        // -- Row 3: transport · queue ------------------------------------------
        let button = |icon: &str, tip: &str, t: Transport| {
            let b = gtk::Button::builder()
                .icon_name(icon)
                .tooltip_text(tr(tip))
                .css_classes(["flat", "circular"])
                .build();
            let s = sender.clone();
            b.connect_clicked(move |_| {
                let _ = s.output(BarOutput::Transport(t));
            });
            b
        };
        let shuffle = button(
            "media-playlist-shuffle-symbolic",
            "Shuffle",
            Transport::ToggleShuffle,
        );
        let prev = button(
            "media-skip-backward-symbolic",
            "Previous",
            Transport::Previous,
        );
        let play = button(
            "media-playback-start-symbolic",
            "Play",
            Transport::TogglePause,
        );
        play.set_css_classes(&["circular", "suggested-action", "play-button"]);
        let stop = button("media-playback-stop-symbolic", "Stop", Transport::Stop);
        let next = button("media-skip-forward-symbolic", "Next", Transport::Next);
        let repeat = button(
            "media-playlist-repeat-symbolic",
            "Repeat: off",
            Transport::CycleRepeat,
        );

        let transport = gtk::Box::builder()
            .spacing(6)
            .halign(gtk::Align::Center)
            .hexpand(true)
            .build();
        for b in [&shuffle, &prev, &play, &stop, &next, &repeat] {
            transport.append(b);
        }

        let queue = gtk::ToggleButton::builder()
            .icon_name("view-list-symbolic")
            .css_classes(["flat"])
            .tooltip_text(tr("Play queue"))
            .build();
        let queue_handler = {
            let s = sender.clone();
            queue.connect_toggled(move |b| {
                let _ = s.output(BarOutput::ShowQueue(b.is_active()));
            })
        };
        // Balance the queue button so the transport stays centred.
        let spacer = gtk::Box::builder().width_request(34).build();
        let row3 = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        row3.append(&spacer);
        row3.append(&transport);
        row3.append(&queue);

        root.append(&row1);
        root.append(&row2);
        root.append(&row3);

        {
            let s = sender.clone();
            glib::timeout_add_local(Duration::from_millis(500), move || {
                s.input(BarInput::Tick);
                glib::ControlFlow::Continue
            });
        }

        let model = PlayerBar {
            status: Status::default(),
            song: None,
            album_folder: None,
            sampled_at: Instant::now(),
            queue_shown: false,
            covers,
        };
        let widgets = BarWidgets {
            cover,
            cover_placeholder,
            title,
            artist_album,
            album_button,
            format,
            bitrate,
            progress,
            elapsed,
            total,
            play,
            shuffle,
            repeat,
            queue,
            queue_handler,
            cover_folder: Rc::default(),
        };
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: BarInput, _sender: ComponentSender<Self>) {
        match msg {
            BarInput::Update {
                status,
                song,
                album,
            } => {
                self.status = *status;
                self.song = song.map(|s| *s);
                self.sampled_at = Instant::now();
                if let Some(a) = &album {
                    // Kick off loading; the view picks it up from the cache.
                    let folder = a.folder.clone();
                    self.covers.request(a, |_| {});
                    self.album_folder = Some(folder);
                } else {
                    self.album_folder = None;
                }
            }
            BarInput::QueueShown(v) => self.queue_shown = v,
            BarInput::Tick => {}
        }
    }

    fn update_view(&self, w: &mut BarWidgets, _sender: ComponentSender<Self>) {
        let st = &self.status;
        let stopped = st.state == PlayState::Stop && self.song.is_none();

        // Text
        match &self.song {
            Some(s) if !stopped => {
                w.title.set_label(s.display_title());
                let line = [s.artist(), s.album()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" — ");
                w.artist_album.set_label(&line);
                w.album_button.set_sensitive(self.album_folder.is_some());
            }
            _ => {
                w.title.set_label(&tr("Not playing"));
                w.artist_album.set_label("");
                w.album_button.set_sensitive(false);
            }
        }

        // Audio info
        w.format
            .set_label(&st.audio.map(|a| a.to_string()).unwrap_or_default());
        w.bitrate.set_label(
            &st.bitrate
                .filter(|_| st.state != PlayState::Stop)
                .map(|b| format!("{b} kbps"))
                .unwrap_or_default(),
        );

        // Progress
        let dur = st.duration.unwrap_or(0.0);
        let el = self.elapsed_now();
        w.progress.set_visible(dur > 0.0);
        w.progress.set_fraction(if dur > 0.0 {
            (el / dur).clamp(0.0, 1.0)
        } else {
            0.0
        });
        w.elapsed
            .set_label(if dur > 0.0 { mmss(el) } else { String::new() }.as_str());
        w.total
            .set_label(if dur > 0.0 { mmss(dur) } else { String::new() }.as_str());

        // Transport state
        let playing = st.state == PlayState::Play;
        w.play.set_icon_name(if playing {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        });
        w.play
            .set_tooltip_text(Some(&tr(if playing { "Pause" } else { "Play" })));
        set_accent(&w.shuffle, st.random);
        w.shuffle.set_tooltip_text(Some(&tr(if st.random {
            "Shuffle: on"
        } else {
            "Shuffle: off"
        })));
        let (icon, tip, on) = match (st.repeat, st.single) {
            (true, Single::On) => ("media-playlist-repeat-song-symbolic", "Repeat: track", true),
            (true, _) => ("media-playlist-repeat-symbolic", "Repeat: queue", true),
            (false, _) => ("media-playlist-repeat-symbolic", "Repeat: off", false),
        };
        w.repeat.set_icon_name(icon);
        w.repeat.set_tooltip_text(Some(&tr(tip)));
        set_accent(&w.repeat, on);

        w.queue.block_signal(&w.queue_handler);
        w.queue.set_active(self.queue_shown);
        w.queue.unblock_signal(&w.queue_handler);

        // Cover (cached by the loader; refresh only when the album changes or
        // the load has completed).
        let mut bound = w.cover_folder.borrow_mut();
        let want = self.album_folder.clone().filter(|_| !stopped);
        let cached = want.as_deref().and_then(|f| self.covers.cached(f));
        if *bound != want {
            // Not loaded yet: show the placeholder and retry on the next tick.
            let loaded = cached.is_some();
            let tex = cached.flatten();
            w.cover.set_paintable(tex.as_ref());
            w.cover_placeholder.set_visible(tex.is_none());
            if loaded || want.is_none() {
                *bound = want;
            }
        }
    }
}

fn set_accent(b: &gtk::Button, on: bool) {
    if on {
        b.add_css_class("accent");
        b.remove_css_class("dim-label");
    } else {
        b.remove_css_class("accent");
        b.add_css_class("dim-label");
    }
}
