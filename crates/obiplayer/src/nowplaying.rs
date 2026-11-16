//! Full-window "now playing" view: artwork (cycling every 8 s) or visualizer
//! over a blurred cover, with waveform and transport.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use obiplayer_core::covers::all_artwork;
use obiplayer_core::library::Album;
use obiplayer_core::mpd::{PlayState, Single, Song, Status};
use relm4::gtk::{self, gdk, glib, prelude::*};

use crate::covers::load_file_texture;
use crate::ctx::{Action, Ctx};
use crate::player_bar::Transport;
use crate::tr;
use crate::ui;
use crate::viz::{Visualizer, VizMode, Waveform};

pub struct NowPlaying {
    pub root: gtk::Overlay,
    ctx: Rc<Ctx>,
    background: gtk::Picture,
    art: gtk::Picture,
    art_counter: gtk::Label,
    art_nav: gtk::Box,
    stack: gtk::Stack,
    title: gtk::Label,
    subtitle: gtk::Label,
    audio: gtk::Label,
    elapsed: gtk::Label,
    total: gtk::Label,
    play: gtk::Button,
    shuffle: gtk::Button,
    repeat: gtk::Button,
    pub waveform: Waveform,
    images: Rc<RefCell<Vec<PathBuf>>>,
    textures: Rc<RefCell<Vec<Option<gdk::Texture>>>>,
    index: Rc<Cell<usize>>,
    folder: RefCell<Option<String>>,
    viz_toggle: RefCell<Option<gtk::ToggleButton>>,
}

fn round_button(icon: &str, tip: &str, f: impl Fn() + 'static) -> gtk::Button {
    let b = ui::icon_button(icon, tip, f);
    b.add_css_class("osd");
    b
}

impl NowPlaying {
    pub fn new(ctx: &Rc<Ctx>) -> Rc<NowPlaying> {
        let background = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Cover)
            .css_classes(["np-background"])
            .build();
        let shade = gtk::Box::builder().css_classes(["np-shade"]).build();

        // -- Top bar -------------------------------------------------------------
        let c = ctx.clone();
        let close = round_button("go-down-symbolic", "Close (Esc)", move || {
            c.act(Action::ShowNowPlaying(false))
        });
        let modes = gtk::Box::builder()
            .css_classes(["linked"])
            .halign(gtk::Align::Center)
            .hexpand(true)
            .build();
        let artwork_btn = gtk::ToggleButton::builder()
            .icon_name("image-x-generic-symbolic")
            .tooltip_text(tr("Artwork"))
            .active(true)
            .build();
        let viz_btn = gtk::ToggleButton::builder()
            .icon_name("utilities-system-monitor-symbolic")
            .tooltip_text(tr("Visualizer"))
            .group(&artwork_btn)
            .build();
        modes.append(&artwork_btn);
        modes.append(&viz_btn);
        let audio = ui::label("", &["monospace", "caption", "np-dim"]);
        audio.set_xalign(1.0);
        let top = ui::hbox(12);
        top.append(&close);
        top.append(&modes);
        top.append(&audio);

        // -- Centre: artwork / visualizer --------------------------------------------
        let art = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Contain)
            .vexpand(true)
            .hexpand(true)
            .css_classes(["np-art"])
            .build();
        let art_counter = ui::label("", &["caption", "np-dim"]);
        let index = Rc::new(Cell::new(0usize));
        let images: Rc<RefCell<Vec<PathBuf>>> = Rc::default();
        let textures: Rc<RefCell<Vec<Option<gdk::Texture>>>> = Rc::default();

        let art_nav = ui::hbox(12);
        art_nav.set_halign(gtk::Align::Center);
        let art_box = ui::vbox(8);
        art_box.append(&art);
        art_box.append(&art_nav);

        let viz = Visualizer::new(ctx, VizMode::Spectrum);
        let viz_modes = gtk::Box::builder()
            .css_classes(["linked"])
            .halign(gtk::Align::Center)
            .build();
        let mut first: Option<gtk::ToggleButton> = None;
        for m in VizMode::ALL {
            let b = gtk::ToggleButton::builder()
                .label(tr(m.label()))
                .active(m == VizMode::Spectrum)
                .build();
            if let Some(f) = &first {
                b.set_group(Some(f));
            } else {
                first = Some(b.clone());
            }
            let v = viz.clone();
            b.connect_toggled(move |b| {
                if b.is_active() {
                    v.set_mode(m);
                }
            });
            viz_modes.append(&b);
        }
        let viz_box = ui::vbox(8);
        viz_box.append(&viz.area);
        viz_box.append(&viz_modes);

        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .vexpand(true)
            .build();
        stack.add_named(&art_box, Some("artwork"));
        stack.add_named(&viz_box, Some("visualizer"));

        // -- Bottom: waveform, text, transport ----------------------------------------
        let c = ctx.clone();
        let waveform = Waveform::new(ctx, 40, move |f| {
            let d = c.clock.get().duration;
            if d > 0.0 {
                c.act(Action::Seek(f * d));
            }
        });
        let elapsed = ui::label("", &["monospace", "caption", "np-dim"]);
        let total = ui::label("", &["monospace", "caption", "np-dim"]);
        total.set_xalign(1.0);
        let times = ui::hbox(0);
        elapsed.set_hexpand(true);
        times.append(&elapsed);
        times.append(&total);
        let title = gtk::Label::builder()
            .css_classes(["title-2"])
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        let subtitle = gtk::Label::builder()
            .css_classes(["np-dim"])
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        let tbtn = |icon: &str, tip: &str, t: Transport| {
            let c = ctx.clone();
            round_button(icon, tip, move || c.act(Action::Transport(t)))
        };
        let shuffle = tbtn(
            "media-playlist-shuffle-symbolic",
            "Shuffle",
            Transport::ToggleShuffle,
        );
        let prev = tbtn(
            "media-skip-backward-symbolic",
            "Previous",
            Transport::Previous,
        );
        let play = tbtn(
            "media-playback-start-symbolic",
            "Play",
            Transport::TogglePause,
        );
        play.add_css_class("np-play");
        let next = tbtn("media-skip-forward-symbolic", "Next", Transport::Next);
        let repeat = tbtn(
            "media-playlist-repeat-symbolic",
            "Repeat",
            Transport::CycleRepeat,
        );
        let transport = ui::hbox(12);
        transport.set_halign(gtk::Align::Center);
        for b in [&shuffle, &prev, &play, &next, &repeat] {
            transport.append(b);
        }
        let bottom = ui::vbox(6);
        bottom.append(&waveform.area);
        bottom.append(&times);
        bottom.append(&title);
        bottom.append(&subtitle);
        bottom.append(&transport);

        let content = ui::vbox(18);
        content.set_margin_top(18);
        content.set_margin_bottom(24);
        content.set_margin_start(24);
        content.set_margin_end(24);
        content.append(&top);
        content.append(&stack);
        content.append(&bottom);

        let root = gtk::Overlay::builder().css_classes(["now-playing"]).build();
        root.set_child(Some(&background));
        root.add_overlay(&shade);
        root.add_overlay(&content);

        let np = Rc::new(NowPlaying {
            root,
            ctx: ctx.clone(),
            background,
            art,
            art_counter,
            art_nav,
            stack,
            title,
            subtitle,
            audio,
            elapsed,
            total,
            play,
            shuffle,
            repeat,
            waveform,
            images,
            textures,
            index,
            folder: RefCell::default(),
            viz_toggle: RefCell::new(Some(viz_btn.clone())),
        });

        {
            let n = np.clone();
            artwork_btn.connect_toggled(move |b| {
                if b.is_active() {
                    n.stack.set_visible_child_name("artwork");
                    n.background.remove_css_class("np-blur");
                }
            });
            let n = np.clone();
            viz_btn.connect_toggled(move |b| {
                if b.is_active() {
                    n.stack.set_visible_child_name("visualizer");
                    n.background.add_css_class("np-blur");
                }
            });
        }
        np.build_art_nav();

        // Times update and artwork cycling.
        {
            let weak = Rc::downgrade(&np);
            glib::timeout_add_local(Duration::from_millis(500), move || {
                let Some(n) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                let c = n.ctx.clock.get();
                n.elapsed.set_label(&ui::mmss(c.now()));
                n.total.set_label(&if c.duration > 0.0 {
                    ui::mmss(c.duration)
                } else {
                    String::new()
                });
                glib::ControlFlow::Continue
            });
            let weak = Rc::downgrade(&np);
            glib::timeout_add_seconds_local(8, move || {
                let Some(n) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                if n.root.is_mapped() && n.stack.visible_child_name().as_deref() == Some("artwork")
                {
                    n.step(1);
                }
                glib::ControlFlow::Continue
            });
        }
        np
    }

    fn build_art_nav(self: &Rc<Self>) {
        let n = self.clone();
        let prev = round_button("go-previous-symbolic", "Previous image", move || n.step(-1));
        let n = self.clone();
        let next = round_button("go-next-symbolic", "Next image", move || n.step(1));
        self.art_nav.append(&prev);
        self.art_nav.append(&self.art_counter);
        self.art_nav.append(&next);
    }

    fn step(&self, delta: isize) {
        let n = self.images.borrow().len();
        if n < 2 {
            return;
        }
        let i = (self.index.get() as isize + delta).rem_euclid(n as isize) as usize;
        self.show_index(i);
    }

    fn show_index(&self, i: usize) {
        self.index.set(i);
        let n = self.images.borrow().len();
        self.art_counter.set_label(&format!("{} / {}", i + 1, n));
        self.art_nav.set_visible(n > 1);
        if let Some(Some(t)) = self.textures.borrow().get(i) {
            self.art.set_paintable(Some(t));
            return;
        }
        let Some(path) = self.images.borrow().get(i).cloned() else {
            return;
        };
        let (art, textures) = (self.art.clone(), self.textures.clone());
        let idx = self.index.clone();
        relm4::spawn_local(async move {
            let t = load_file_texture(path, 1600).await;
            if let Some(slot) = textures.borrow_mut().get_mut(i) {
                *slot = t.clone();
            }
            if idx.get() == i {
                art.set_paintable(t.as_ref());
            }
        });
    }

    /// Switch to the visualizer mode.
    pub fn show_visualizer(&self) {
        self.stack.set_visible_child_name("visualizer");
        self.background.add_css_class("np-blur");
        if let Some(b) = self.viz_toggle.borrow().as_ref() {
            b.set_active(true);
        }
    }

    pub fn update(&self, status: &Status, song: Option<&Song>, album: Option<&Arc<Album>>) {
        match song {
            Some(s) => {
                self.title.set_label(s.display_title());
                self.subtitle.set_label(
                    &[s.artist(), s.album()]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" — "),
                );
            }
            None => {
                self.title.set_label(&tr("Not playing"));
                self.subtitle.set_label("");
            }
        }
        let mut info = status.audio.map(|a| a.to_string()).unwrap_or_default();
        if let Some(b) = status.bitrate {
            info.push_str(&format!("\n{b} kbps"));
        }
        if let Some(d) = song.and_then(|s| self.ctx.dr(&s.file)) {
            info.push_str(&format!("\nDR{d}"));
        }
        self.audio.set_label(&info);

        let playing = status.state == PlayState::Play;
        self.play.set_icon_name(if playing {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        });
        self.shuffle
            .set_opacity(if status.random { 1.0 } else { 0.4 });
        self.repeat
            .set_icon_name(if status.repeat && status.single == Single::On {
                "media-playlist-repeat-song-symbolic"
            } else {
                "media-playlist-repeat-symbolic"
            });
        self.repeat
            .set_opacity(if status.repeat { 1.0 } else { 0.4 });

        // Artwork for a new album.
        let folder = album.map(|a| a.folder.clone());
        if *self.folder.borrow() == folder {
            return;
        }
        *self.folder.borrow_mut() = folder;
        self.images.borrow_mut().clear();
        self.textures.borrow_mut().clear();
        self.art.set_paintable(None::<&gdk::Paintable>);
        self.background.set_paintable(None::<&gdk::Paintable>);
        self.art_nav.set_visible(false);
        let Some(a) = album else { return };

        // Thumbnail first (fast, also used as the background).
        let (bg, art) = (self.background.clone(), self.art.clone());
        self.ctx.covers.request(a, move |t| {
            bg.set_paintable(t);
            if art.paintable().is_none() {
                art.set_paintable(t);
            }
        });
        // Then every local image, full size.
        if let Some(dir) = self.ctx.local_path(&a.folder) {
            let (images, textures) = (self.images.clone(), self.textures.clone());
            let this_folder = a.folder.clone();
            let (counter, nav, art, idx) = (
                self.art_counter.clone(),
                self.art_nav.clone(),
                self.art.clone(),
                self.index.clone(),
            );
            let current = self.folder.clone();
            relm4::spawn_local(async move {
                let found = gtk::gio::spawn_blocking(move || all_artwork(&dir))
                    .await
                    .unwrap_or_default();
                if current.borrow().as_deref() != Some(this_folder.as_str()) || found.is_empty() {
                    return;
                }
                let n = found.len();
                *textures.borrow_mut() = vec![None; n];
                *images.borrow_mut() = found.clone();
                idx.set(0);
                counter.set_label(&format!("1 / {n}"));
                nav.set_visible(n > 1);
                if let Some(t) = load_file_texture(found[0].clone(), 1600).await {
                    art.set_paintable(Some(&t));
                    if let Some(s) = textures.borrow_mut().get_mut(0) {
                        *s = Some(t);
                    }
                }
            });
        }
    }
}
