//! Lyrics view (side panel and full-window mode): LRCLIB synced lyrics with
//! the current line highlighted and kept centred, or plain text.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use obiplayer_core::meta::lyrics::{LrcLine, parse_lrc};
use obiplayer_core::mpd::Song;
use relm4::gtk::{self, glib, prelude::*};

use crate::ctx::Ctx;
use crate::tr;
use crate::ui;

pub struct LyricsView {
    pub root: gtk::Box,
    status: gtk::Label,
    stack: gtk::Stack,
    lines_box: gtk::Box,
    plain: gtk::Label,
    scroll: gtk::ScrolledWindow,
    lines: RefCell<Vec<(f64, gtk::Label)>>,
    current: Cell<Option<usize>>,
    key: RefCell<String>,
    large: bool,
}

fn state(icon: &str, text: &str) -> gtk::Box {
    let b = ui::vbox(8);
    b.set_valign(gtk::Align::Center);
    b.set_vexpand(true);
    b.append(
        &gtk::Image::builder()
            .icon_name(icon)
            .pixel_size(48)
            .css_classes(["dim-label"])
            .build(),
    );
    let l = ui::label(text, &["dim-label"]);
    l.set_halign(gtk::Align::Center);
    l.set_justify(gtk::Justification::Center);
    b.append(&l);
    b
}

impl LyricsView {
    /// `large`: full-window style (centred, big type).
    pub fn new(ctx: &Rc<Ctx>, large: bool) -> Rc<LyricsView> {
        let status = gtk::Label::builder()
            .css_classes(["caption", "dim-label"])
            .build();
        let lines_box = ui::vbox(if large { 14 } else { 8 });
        lines_box.set_margin_top(12);
        lines_box.set_margin_bottom(12);
        lines_box.set_margin_start(12);
        lines_box.set_margin_end(12);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&lines_box)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let plain = ui::label("", &[]);
        plain.set_margin_top(12);
        plain.set_margin_start(12);
        plain.set_margin_end(12);
        plain.set_selectable(true);
        plain.set_focusable(false);
        if large {
            plain.set_justify(gtk::Justification::Center);
            plain.set_halign(gtk::Align::Center);
        }
        let plain_scroll = gtk::ScrolledWindow::builder()
            .child(&plain)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .vexpand(true)
            .build();
        stack.add_named(
            &gtk::Spinner::builder()
                .spinning(true)
                .halign(gtk::Align::Center)
                .valign(gtk::Align::Center)
                .build(),
            Some("loading"),
        );
        stack.add_named(
            &state("audio-x-generic-symbolic", &tr("Instrumental")),
            Some("instrumental"),
        );
        stack.add_named(
            &state("action-unavailable-symbolic", &tr("No lyrics available")),
            Some("none"),
        );
        stack.add_named(&scroll, Some("synced"));
        stack.add_named(&plain_scroll, Some("plain"));
        stack.set_visible_child_name("none");

        let root = ui::vbox(0);
        if !large {
            let head = ui::hbox(8);
            head.set_margin_top(8);
            head.set_margin_bottom(8);
            head.set_margin_start(12);
            head.set_margin_end(12);
            let t = ui::label(&tr("Lyrics"), &["heading"]);
            t.set_hexpand(true);
            head.append(&t);
            head.append(&status);
            root.append(&head);
            root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        }
        root.append(&stack);

        let v = Rc::new(LyricsView {
            root,
            status,
            stack,
            lines_box,
            plain,
            scroll,
            lines: RefCell::default(),
            current: Cell::new(None),
            key: RefCell::default(),
            large,
        });
        {
            let (w, c) = (Rc::downgrade(&v), ctx.clone());
            glib::timeout_add_local(Duration::from_millis(250), move || {
                let Some(v) = w.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                if v.root.is_mapped() {
                    v.follow(c.clock.get().now());
                }
                glib::ControlFlow::Continue
            });
        }
        v
    }

    /// Load lyrics for `song` (no-op if already shown).
    pub fn load(self: &Rc<Self>, ctx: &Rc<Ctx>, song: Option<&Song>) {
        let Some(song) = song else {
            self.key.borrow_mut().clear();
            self.stack.set_visible_child_name("none");
            return;
        };
        let artist = song.artist().unwrap_or_default().to_owned();
        let title = song.display_title().to_owned();
        let key = format!("{artist}|{title}");
        if *self.key.borrow() == key {
            return;
        }
        *self.key.borrow_mut() = key.clone();
        self.stack.set_visible_child_name("loading");
        self.status.set_label("");
        let album = song.album().unwrap_or_default().to_owned();
        let duration = song.duration.unwrap_or(0.0);
        let (s, me) = (ctx.services.clone(), self.clone());
        relm4::spawn_local(async move {
            let res =
                relm4::spawn(
                    async move { s.lyrics.fetch(&artist, &title, &album, duration).await },
                )
                .await
                .ok()
                .flatten();
            if *me.key.borrow() != key {
                return; // the song changed meanwhile
            }
            me.show(res);
        });
    }

    fn show(&self, res: Option<obiplayer_core::meta::lyrics::Lyrics>) {
        ui::clear(&self.lines_box);
        self.lines.borrow_mut().clear();
        self.current.set(None);
        let Some(l) = res else {
            self.stack.set_visible_child_name("none");
            return;
        };
        if l.instrumental {
            self.stack.set_visible_child_name("instrumental");
            return;
        }
        let synced: Vec<LrcLine> = l.synced.as_deref().map(parse_lrc).unwrap_or_default();
        if !synced.is_empty() {
            self.status.set_label(&tr("Synced"));
            self.status.set_css_classes(&["caption", "lyrics-synced"]);
            for line in synced {
                let lab = gtk::Label::builder()
                    .label(if line.text.is_empty() {
                        "♪"
                    } else {
                        &line.text
                    })
                    .wrap(true)
                    .xalign(if self.large { 0.5 } else { 0.0 })
                    .justify(if self.large {
                        gtk::Justification::Center
                    } else {
                        gtk::Justification::Left
                    })
                    .css_classes(["lyric-line", "lyric-future"])
                    .build();
                if self.large {
                    lab.add_css_class("lyric-large");
                }
                self.lines_box.append(&lab);
                self.lines.borrow_mut().push((line.time, lab));
            }
            self.stack.set_visible_child_name("synced");
            return;
        }
        match l.plain.filter(|p| !p.trim().is_empty()) {
            Some(p) => {
                self.status.set_label(&tr("Unsynced"));
                self.status.set_css_classes(&["caption", "lyrics-unsynced"]);
                self.plain.set_label(&p);
                self.stack.set_visible_child_name("plain");
            }
            None => self.stack.set_visible_child_name("none"),
        }
    }

    /// Highlight the line at playback time `t` and keep it centred.
    fn follow(&self, t: f64) {
        let lines = self.lines.borrow();
        if lines.is_empty() {
            return;
        }
        let idx = lines.iter().rposition(|(time, _)| *time <= t);
        if idx == self.current.get() {
            return;
        }
        self.current.set(idx);
        for (i, (_, l)) in lines.iter().enumerate() {
            let class = match idx {
                Some(c) if i < c => "lyric-past",
                Some(c) if i == c => "lyric-current",
                _ => "lyric-future",
            };
            for c in ["lyric-past", "lyric-current", "lyric-future"] {
                l.remove_css_class(c);
            }
            l.add_css_class(class);
        }
        if let Some(i) = idx
            && let Some(p) = lines[i]
                .1
                .compute_point(&self.lines_box, &gtk::graphene::Point::new(0.0, 0.0))
        {
            let adj = self.scroll.vadjustment();
            let target = p.y() as f64 - adj.page_size() / 2.0 + lines[i].1.height() as f64 / 2.0;
            adj.set_value(target.clamp(adj.lower(), (adj.upper() - adj.page_size()).max(0.0)));
        }
    }
}
