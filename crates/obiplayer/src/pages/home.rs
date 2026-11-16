//! Home: recently played, favourite genres, not played yet.

use std::rc::Rc;

use obiplayer_core::history::History;
use obiplayer_core::home::{self, ShelfKind};
use relm4::gtk::{self, prelude::*};

use crate::ctx::{Action, Ctx, Route};
use crate::tr;
use crate::ui;

pub struct HomeView {
    pub root: gtk::Box,
    body: gtk::Box,
}

fn shelf_cell(ctx: &Rc<Ctx>, album: &obiplayer_core::library::Album, playing: bool) -> gtk::Button {
    let col = ui::vbox(4);
    col.set_width_request(140);
    let cover = ui::cover(ctx, album, 140);
    if playing {
        cover.add_overlay(
            &gtk::Image::builder()
                .icon_name("audio-speakers-symbolic")
                .halign(gtk::Align::End)
                .valign(gtk::Align::End)
                .margin_end(6)
                .margin_bottom(6)
                .css_classes(["playing-badge"])
                .build(),
        );
    }
    col.append(&cover);
    let t = gtk::Label::builder()
        .label(&album.title)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(1)
        .css_classes(["caption-heading"])
        .build();
    col.append(&t);
    let a = gtk::Label::builder()
        .label(&album.artist)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(1)
        .css_classes(["caption", "dim-label"])
        .build();
    col.append(&a);
    let b = gtk::Button::builder()
        .child(&col)
        .css_classes(["flat", "shelf-cell"])
        .build();
    let (c, f) = (ctx.clone(), album.folder.clone());
    b.connect_clicked(move |_| c.act(Action::Open(Route::Album(f.clone()))));
    b
}

impl HomeView {
    pub fn new() -> Rc<HomeView> {
        let body = ui::vbox(18);
        body.set_margin_top(12);
        body.set_margin_bottom(24);
        body.set_margin_start(18);
        body.set_margin_end(18);
        let root = ui::vbox(0);
        root.append(
            &gtk::ScrolledWindow::builder()
                .child(&body)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .vexpand(true)
                .build(),
        );
        Rc::new(HomeView { root, body })
    }

    pub fn reload(self: &Rc<Self>, ctx: &Rc<Ctx>) {
        let history = History::new(History::default_path()).load();
        if !history.is_empty() {
            self.render(ctx, history);
            return;
        }
        // No local history yet: approximate it from MPD's play stickers
        // (written by DrPlayer and by us), one entry per played file.
        let (me, c, client) = (self.clone(), ctx.clone(), ctx.client.clone());
        relm4::spawn_local(async move {
            let (last, counts) = relm4::spawn(async move {
                (
                    client
                        .sticker_find("", "last_played")
                        .await
                        .unwrap_or_default(),
                    client
                        .sticker_find("", "play_count")
                        .await
                        .unwrap_or_default(),
                )
            })
            .await
            .unwrap_or_default();
            let counts: std::collections::HashMap<String, u32> = counts
                .into_iter()
                .filter_map(|(f, v)| v.parse().ok().map(|n| (f, n)))
                .collect();
            let lib = c.lib();
            let mut synth = Vec::new();
            for (file, ts) in last {
                let Ok(ts) = ts.parse::<i64>() else { continue };
                let Some(r) = lib.find_file(&file) else {
                    continue;
                };
                let (a, s) = (lib.album(r.album), lib.song(r));
                // Weight by play count so favourite genres rank correctly.
                for _ in 0..counts.get(&file).copied().unwrap_or(1).clamp(1, 50) {
                    synth.push(obiplayer_core::history::Play {
                        ts,
                        file: file.clone(),
                        artist: s.artist().unwrap_or(&a.artist).to_owned(),
                        album: a.title.clone(),
                        title: s.display_title().to_owned(),
                        duration: s.duration.unwrap_or(0.0),
                    });
                }
            }
            synth.sort_by_key(|p| p.ts);
            me.render(&c, synth);
        });
    }

    fn render(&self, ctx: &Rc<Ctx>, history: Vec<obiplayer_core::history::Play>) {
        ui::clear(&self.body);
        let lib = ctx.lib();
        if lib.albums.is_empty() {
            return;
        }
        let extra = ctx.extra_genres.borrow().clone();
        let seed = relm4::gtk::glib::random_int() as u64;
        let current = ctx
            .current_file
            .borrow()
            .as_deref()
            .map(|f| obiplayer_core::library::album_key(f).to_owned());
        for shelf in home::shelves(&lib, &history, &extra, seed) {
            if shelf.albums.is_empty() {
                continue;
            }
            let title = match &shelf.kind {
                ShelfKind::RecentlyPlayed => tr("Recently played"),
                ShelfKind::Genre(g) => g.clone(),
                ShelfKind::NotPlayed => tr("Not played yet"),
            };
            self.body.append(&ui::label(&title, &["title-3"]));
            let row = ui::hbox(12);
            for &i in &shelf.albums {
                let a = lib.album(i);
                row.append(&shelf_cell(
                    ctx,
                    a,
                    current.as_deref() == Some(a.folder.as_str()),
                ));
            }
            self.body.append(
                &gtk::ScrolledWindow::builder()
                    .child(&row)
                    .vscrollbar_policy(gtk::PolicyType::Never)
                    .build(),
            );
        }
        if history.is_empty() && self.body.first_child().is_none() {
            self.body.append(&ui::label(
                &tr("Play some music: recently played albums and your favourite genres will appear here."),
                &["dim-label"],
            ));
        }
    }
}
