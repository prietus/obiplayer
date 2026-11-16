//! Track detail: technical info, tags and other versions.

use std::rc::Rc;

use obiplayer_core::catalog::TrackRef;
use obiplayer_core::mpd::Song;
use obiplayer_core::probe::{self, Probe};
use relm4::adw;
use relm4::gtk::{self, prelude::*};

use super::Page;
use crate::ctx::{Action, Ctx, Route};
use crate::tr;
use crate::ui::{self, InfoGrid};

/// Tag prefixes that are noise for a reader.
const HIDDEN_TAGS: &[&str] = &[
    "musicbrainz",
    "acoustid",
    "script",
    "replaygain",
    "encoder",
    "barcode",
    "asin",
    "isrc",
    "catalognumber",
    "media",
    "totaldiscs",
    "totaltracks",
    "tracktotal",
    "disctotal",
    "releasestatus",
    "releasetype",
    "releasecountry",
];

pub struct TrackPage {
    nav: adw::NavigationPage,
    route: Route,
}

impl TrackPage {
    pub fn new(ctx: &Rc<Ctx>, file: &str) -> Option<TrackPage> {
        let lib = ctx.lib();
        let r = lib.find_file(file)?;
        let album = lib.album(r.album).clone();
        let song = lib.song(r).clone();
        let body = ui::page_body();

        // -- Header ----------------------------------------------------------------
        let title = ui::label(song.display_title(), &["title-1"]);
        title.set_width_chars(16);
        body.append(&title);
        let who = ui::hbox(6);
        if let Some(a) = song
            .artist()
            .or(Some(album.artist.as_str()))
            .filter(|a| !a.is_empty())
        {
            let (c, a2) = (ctx.clone(), a.to_owned());
            who.append(&ui::link(a, &["title-4"], move || {
                c.act(Action::Open(Route::Artist(a2.clone())))
            }));
            who.append(&ui::label("—", &["dim-label"]));
        }
        let (c, f) = (ctx.clone(), album.folder.clone());
        who.append(&ui::link(
            &album.title,
            &["title-4", "dim-label"],
            move || c.act(Action::Open(Route::Album(f.clone()))),
        ));
        body.append(&who);

        let actions = ui::hbox(6);
        let play = gtk::Button::builder()
            .label(tr("Play"))
            .css_classes(["pill", "suggested-action"])
            .build();
        let (c, f) = (ctx.clone(), song.file.clone());
        play.connect_clicked(move |_| c.act(Action::PlayFile(f.clone())));
        let enqueue = gtk::Button::builder()
            .label(tr("Add to queue"))
            .css_classes(["pill"])
            .build();
        let (c, f) = (ctx.clone(), song.file.clone());
        enqueue.connect_clicked(move |_| c.act(Action::EnqueueFile(f.clone())));
        actions.append(&play);
        actions.append(&enqueue);
        actions.append(&ui::stars(ctx, &song.file));
        body.append(&actions);

        // -- Technical sections: MPD data now, ffprobe data when available -------------
        let tech = ui::vbox(12);
        body.append(&tech);
        fill_tech(&tech, &song, None);
        if let Some(path) = ctx.local_path(&song.file) {
            let (t, s) = (tech.clone(), song.clone());
            relm4::spawn_local(async move {
                let p = relm4::gtk::gio::spawn_blocking(move || probe::probe(&path))
                    .await
                    .ok()
                    .flatten();
                if p.is_some() {
                    ui::clear(&t);
                    fill_tech(&t, &s, p.as_ref());
                }
            });
        }

        // -- Other versions --------------------------------------------------------------
        let versions: Vec<TrackRef> = lib
            .versions(r)
            .iter()
            .copied()
            .filter(|v| *v != r)
            .collect();
        if !versions.is_empty() {
            body.append(&ui::heading(&format!(
                "{} ({})",
                tr("Other versions"),
                versions.len()
            )));
            body.append(&versions_list(ctx, &versions));
        }

        Some(TrackPage {
            nav: ui::page(
                song.display_title(),
                &super::tag(&Route::Track(file.to_owned())),
                &body,
            ),
            route: Route::Track(file.to_owned()),
        })
    }
}

impl Page for TrackPage {
    fn nav(&self) -> &adw::NavigationPage {
        &self.nav
    }
    fn route(&self) -> &Route {
        &self.route
    }
}

fn khz(rate: u32) -> String {
    if rate >= 1000 {
        format!("{:.1} kHz", rate as f64 / 1000.0)
    } else {
        format!("{rate} Hz")
    }
}

fn size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.2} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else {
        format!("{:.0} KB", b / 1e3)
    }
}

fn duration_long(secs: f64) -> String {
    format!("{} ({secs:.3} s)", ui::mmss(secs))
}

fn fill_tech(container: &gtk::Box, song: &Song, probe: Option<&Probe>) {
    // Audio
    let mut audio = InfoGrid::new();
    match probe {
        Some(p) => {
            audio.text(
                "Codec",
                if p.codec_long.is_empty() {
                    &p.codec
                } else {
                    &p.codec_long
                },
            );
            if let Some(r) = p.sample_rate {
                audio.text("Sample rate", &khz(r));
            }
            if let Some(b) = p.bit_depth {
                audio.text("Bit depth", &format!("{b} bit"));
            }
            audio.text("Sample format", &p.sample_format);
            if let Some(c) = p.channels {
                let layout = if p.channel_layout.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", p.channel_layout)
                };
                audio.text("Channels", &format!("{c}{layout}"));
            }
            if let Some(d) = p.duration {
                audio.text("Duration", &duration_long(d));
            }
            if let Some(b) = p.bit_rate {
                audio.text("Bitrate", &format!("{} kbps", b / 1000));
            }
        }
        None => {
            if let Some(f) = song.format {
                audio.text("Format", &f.to_string());
            }
            if let Some(d) = song.duration {
                audio.text("Duration", &duration_long(d));
            }
        }
    }
    if !audio.is_empty() {
        container.append(&ui::heading(&tr("Audio")));
        container.append(&audio.grid);
    }

    // File
    let mut file = InfoGrid::new();
    file.text("Path", &song.file);
    if let Some(p) = probe {
        file.text("Container", &p.format_name);
        if let Some(s) = p.size {
            file.text("Size", &size(s));
        }
    }
    container.append(&ui::heading(&tr("File")));
    container.append(&file.grid);

    // Tags
    let mut tags: Vec<(String, String)> = match probe {
        Some(p) if !p.tags.is_empty() => p
            .tags
            .iter()
            .map(|(k, v)| (k.to_lowercase(), v.clone()))
            .collect(),
        _ => song
            .tags
            .iter()
            .map(|(k, v)| (k.clone(), v.join("; ")))
            .collect(),
    };
    tags.retain(|(k, _)| !HIDDEN_TAGS.iter().any(|h| k.starts_with(h)));
    tags.sort();
    if !tags.is_empty() {
        let mut g = InfoGrid::new();
        for (k, v) in &tags {
            g.text(k, v);
        }
        container.append(&ui::heading(&tr("Tags")));
        container.append(&g.grid);
    }
}

fn versions_list(ctx: &Rc<Ctx>, versions: &[TrackRef]) -> gtk::ListBox {
    let lib = ctx.lib();
    let mut sorted = versions.to_vec();
    sorted.sort_by(|a, b| {
        let (x, y) = (lib.album(a.album), lib.album(b.album));
        x.format()
            .cmp(&y.format())
            .then_with(|| obiplayer_core::text::compare(&x.title, &y.title))
    });
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let mut folders = Vec::new();
    for v in sorted {
        let a = lib.album(v.album);
        let s = lib.song(v);
        let row = ui::hbox(10);
        row.set_margin_top(6);
        row.set_margin_bottom(6);
        row.set_margin_start(6);
        row.set_margin_end(8);
        let (c, f) = (ctx.clone(), s.file.clone());
        row.append(&ui::icon_button(
            "media-playback-start-symbolic",
            "Play",
            move || c.act(Action::PlayFile(f.clone())),
        ));
        let text = ui::vbox(2);
        text.set_hexpand(true);
        text.append(&ui::ellipsized(&a.title, &["heading"]));
        let mut meta: Vec<String> = Vec::new();
        if let Some(l) = s.label().or(a.label.as_deref()) {
            meta.push(l.to_owned());
        }
        if let Some(c) = s.tag("releasecountry").or(a.country.as_deref()) {
            meta.push(c.to_owned());
        }
        if let Some(d) = s.date() {
            meta.push(d.to_owned());
        }
        if let Some(od) = s.original_date().filter(|od| Some(*od) != s.date()) {
            meta.push(format!("(orig. {})", od.get(..4).unwrap_or(od)));
        }
        text.append(&ui::ellipsized(
            &meta.join(" · "),
            &["dim-label", "caption"],
        ));
        let path = gtk::Label::builder()
            .label(&a.folder)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::Start)
            .css_classes(["dim-label", "monospace", "caption"])
            .build();
        text.append(&path);
        row.append(&text);
        row.append(&ui::format_badge(&a.format()));
        row.append(
            &gtk::Label::builder()
                .label(s.duration.map(ui::mmss).unwrap_or_default())
                .css_classes(["dim-label", "numeric"])
                .build(),
        );
        list.append(&row);
        folders.push(a.folder.clone());
    }
    let c = ctx.clone();
    list.connect_row_activated(move |_, row| {
        if let Some(f) = folders.get(row.index() as usize) {
            c.act(Action::Open(Route::Album(f.clone())));
        }
    });
    list
}
