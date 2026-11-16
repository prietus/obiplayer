//! Listening statistics (Stats browse mode).

use std::collections::HashMap;
use std::rc::Rc;

use obiplayer_core::history::History;
use obiplayer_core::stats::{self, Period, Stats, StatsInput};
use relm4::gtk::{self, cairo, prelude::*};

use crate::ctx::{Action, Ctx, Route};
use crate::tr;
use crate::ui;

pub struct StatsView {
    pub root: gtk::Box,
    body: gtk::Box,
    period: std::cell::Cell<Period>,
}

fn hm(secs: f64) -> String {
    let m = (secs / 60.0).round() as u64;
    if m >= 60 {
        format!("{}h {}m", m / 60, m % 60)
    } else {
        format!("{m}m")
    }
}

fn card(title: &str) -> gtk::Box {
    let c = ui::vbox(8);
    c.add_css_class("card");
    c.add_css_class("stats-card");
    c.set_hexpand(true);
    c.append(&ui::label(title, &["heading"]));
    c
}

/// Bars proportional to the first value; `label` builds the left part.
fn bar_list(rows: Vec<(gtk::Widget, f64, String)>) -> gtk::Box {
    let b = ui::vbox(6);
    let max = rows.iter().map(|r| r.1).fold(0.0, f64::max).max(1e-9);
    for (left, v, right) in rows {
        let r = ui::vbox(2);
        let top = ui::hbox(8);
        left.set_hexpand(true);
        top.append(&left);
        top.append(&ui::label(&right, &["dim-label", "caption", "numeric"]));
        r.append(&top);
        r.append(
            &gtk::LevelBar::builder()
                .min_value(0.0)
                .max_value(1.0)
                .value(v / max)
                .css_classes(["stats-bar"])
                .build(),
        );
        b.append(&r);
    }
    b
}

fn utc_offset() -> i32 {
    relm4::gtk::glib::DateTime::now_local()
        .map(|d| d.utc_offset().as_seconds() as i32)
        .unwrap_or(0)
}

impl StatsView {
    pub fn new(ctx: &Rc<Ctx>) -> Rc<StatsView> {
        let periods = gtk::Box::builder()
            .css_classes(["linked"])
            .halign(gtk::Align::Center)
            .margin_top(12)
            .build();
        let body = ui::vbox(16);
        body.set_margin_start(18);
        body.set_margin_end(18);
        body.set_margin_bottom(24);
        let col = ui::vbox(12);
        col.append(&periods);
        col.append(&body);
        let root = ui::vbox(0);
        root.append(
            &gtk::ScrolledWindow::builder()
                .child(
                    &relm4::adw::Clamp::builder()
                        .maximum_size(1200)
                        .child(&col)
                        .build(),
                )
                .hscrollbar_policy(gtk::PolicyType::Never)
                .vexpand(true)
                .build(),
        );
        let v = Rc::new(StatsView {
            root,
            body,
            period: std::cell::Cell::new(Period::AllTime),
        });
        let mut first: Option<gtk::ToggleButton> = None;
        for p in [Period::Week, Period::Month, Period::Year, Period::AllTime] {
            let b = gtk::ToggleButton::builder()
                .label(tr(p.label()))
                .active(p == Period::AllTime)
                .build();
            if let Some(f) = &first {
                b.set_group(Some(f));
            } else {
                first = Some(b.clone());
            }
            let (me, c) = (v.clone(), ctx.clone());
            b.connect_toggled(move |b| {
                if b.is_active() {
                    me.period.set(p);
                    me.reload(&c);
                }
            });
            periods.append(&b);
        }
        v
    }

    /// Fetch play counts from MPD stickers and the local history, then draw.
    pub fn reload(self: &Rc<Self>, ctx: &Rc<Ctx>) {
        ui::clear(&self.body);
        self.body.append(
            &gtk::Spinner::builder()
                .spinning(true)
                .halign(gtk::Align::Center)
                .build(),
        );
        let (me, c, client) = (self.clone(), ctx.clone(), ctx.client.clone());
        let period = self.period.get();
        relm4::spawn_local(async move {
            let (counts, last) = relm4::spawn(async move {
                let counts: HashMap<String, u32> = client
                    .sticker_find("", "play_count")
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|(f, v)| v.parse().ok().map(|n| (f, n)))
                    .collect();
                let last: HashMap<String, i64> = client
                    .sticker_find("", "last_played")
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|(f, v)| v.parse().ok().map(|n| (f, n)))
                    .collect();
                (counts, last)
            })
            .await
            .unwrap_or_default();
            let history = History::new(History::default_path()).load();
            let lib = c.lib();
            let extra = c.extra_genres.borrow().clone();
            let now = relm4::gtk::glib::DateTime::now_utc()
                .map(|d| d.to_unix())
                .unwrap_or(0);
            let s = stats::compute(
                &StatsInput {
                    lib: &lib,
                    history: &history,
                    play_counts: &counts,
                    last_played: &last,
                    extra_genres: &extra,
                    now,
                    utc_offset_secs: utc_offset(),
                },
                period,
            );
            me.render(&c, &s, period);
        });
    }

    fn render(&self, ctx: &Rc<Ctx>, s: &Stats, period: Period) {
        ui::clear(&self.body);
        if s.plays == 0 && s.top_tracks.is_empty() {
            self.body.append(
                &relm4::adw::StatusPage::builder()
                    .icon_name("utilities-system-monitor-symbolic")
                    .title(tr("No play data yet"))
                    .description(tr("Stats will appear here as you listen to music"))
                    .build(),
            );
            return;
        }
        let lib = ctx.lib();

        // Hero
        let hero = ui::vbox(4);
        hero.add_css_class("stats-hero");
        hero.set_halign(gtk::Align::Center);
        hero.append(&ui::label(&hm(s.listening_secs), &["title-1"]));
        hero.append(&ui::label(
            &if period == Period::AllTime {
                tr("Total listening time")
            } else {
                format!("{} {}", tr(period.label()), tr("listening time"))
            },
            &["dim-label"],
        ));
        let chips = ui::hbox(12);
        chips.set_halign(gtk::Align::Center);
        for (n, what) in [
            (s.plays as usize, "plays"),
            (s.tracks, "tracks"),
            (s.artists, "artists"),
            (s.albums, "albums"),
        ] {
            chips.append(&ui::label(&format!("{n} {}", tr(what)), &["caption"]));
        }
        hero.append(&chips);
        self.body.append(&hero);

        // Row 1: genres donut · top artists · top albums
        let row1 = ui::hbox(12);
        let g = card(&tr("Genres"));
        g.append(&donut(&s.genres));
        g.append(&bar_list(
            s.genres
                .iter()
                .map(|(n, v)| (ui::ellipsized(n, &[]).upcast(), *v, hm(*v)))
                .collect(),
        ));
        row1.append(&g);
        let a = card(&tr("Top artists"));
        a.append(&bar_list(
            s.top_artists
                .iter()
                .map(|(n, v)| {
                    let r = ui::hbox(8);
                    r.append(&ui::avatar(ctx, n, 28));
                    let (c, n2) = (ctx.clone(), n.clone());
                    r.append(&ui::link(n, &[], move || {
                        c.act(Action::Open(Route::Artist(n2.clone())))
                    }));
                    (r.upcast(), *v, hm(*v))
                })
                .collect(),
        ));
        row1.append(&a);
        let al = card(&tr("Top albums"));
        al.append(&bar_list(
            s.top_albums
                .iter()
                .map(|(i, v)| {
                    let album = lib.album(*i);
                    let r = ui::hbox(8);
                    r.append(&ui::cover(ctx, album, 28));
                    let (c, f) = (ctx.clone(), album.folder.clone());
                    r.append(&ui::link(&album.title, &[], move || {
                        c.act(Action::Open(Route::Album(f.clone())))
                    }));
                    (r.upcast(), *v, hm(*v))
                })
                .collect(),
        ));
        row1.append(&al);
        self.body.append(&row1);

        // Row 2: labels · weekday · hours
        let row2 = ui::hbox(12);
        let l = card(&tr("Top labels"));
        if s.top_labels.is_empty() {
            l.append(&ui::label(&tr("No label data"), &["dim-label"]));
        } else {
            l.append(&bar_list(
                s.top_labels
                    .iter()
                    .map(|(n, v)| (ui::ellipsized(n, &[]).upcast(), *v, hm(*v)))
                    .collect(),
            ));
        }
        row2.append(&l);
        let w = card(&tr("Day of week"));
        w.append(&columns(
            &s.weekday_hours,
            &["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"],
            |v| format!("{:.0}h", v),
        ));
        w.append(&ui::label(
            &format!(
                "{} {:.1}h/{}",
                tr("Avg"),
                s.weekday_hours.iter().sum::<f64>() / 7.0,
                tr("day")
            ),
            &["dim-label", "caption"],
        ));
        row2.append(&w);
        let h = card(&tr("Listening hours"));
        let hours: Vec<f64> = s.hour_plays.iter().map(|&n| n as f64).collect();
        let labels: Vec<String> = (0..24)
            .map(|i| {
                if i % 6 == 0 {
                    format!("{i}h")
                } else {
                    String::new()
                }
            })
            .collect();
        h.append(&columns(
            &hours,
            &labels.iter().map(String::as_str).collect::<Vec<_>>(),
            |_| String::new(),
        ));
        if let Some((peak, _)) = s.hour_plays.iter().enumerate().max_by_key(|(_, n)| **n) {
            h.append(&ui::label(
                &format!("{} {peak}:00", tr("Peak listening:")),
                &["dim-label", "caption"],
            ));
        }
        row2.append(&h);
        self.body.append(&row2);

        // Top tracks · recently played (all-time, from MPD stickers)
        let row3 = ui::hbox(12);
        let t = card(&tr("Top tracks"));
        let max = s.top_tracks.first().map(|x| x.1).unwrap_or(1).max(1);
        for (rank, (file, n)) in s.top_tracks.iter().enumerate() {
            t.append(&track_line(
                ctx,
                rank + 1,
                file,
                &format!("{n}×"),
                *n as f64 / max as f64,
            ));
        }
        row3.append(&t);
        let r = card(&tr("Recently played"));
        let now = relm4::gtk::glib::DateTime::now_utc()
            .map(|d| d.to_unix())
            .unwrap_or(0);
        for (i, (file, ts)) in s.recent.iter().enumerate() {
            r.append(&track_line(ctx, i + 1, file, &relative(now - ts), 0.0));
        }
        row3.append(&r);
        self.body.append(&row3);
    }
}

fn relative(secs: i64) -> String {
    match secs {
        ..60 => tr("now"),
        60..3600 => format!("{} min", secs / 60),
        3600..86400 => format!("{} h", secs / 3600),
        _ => format!("{} d", secs / 86400),
    }
}

fn track_line(ctx: &Rc<Ctx>, rank: usize, file: &str, right: &str, frac: f64) -> gtk::Box {
    let lib = ctx.lib();
    let (title, sub) = match lib.find_file(file) {
        Some(r) => {
            let s = lib.song(r);
            let a = lib.album(r.album);
            (
                s.display_title().to_owned(),
                format!("{} — {}", s.artist().unwrap_or(&a.artist), a.title),
            )
        }
        None => (
            file.rsplit('/').next().unwrap_or(file).to_owned(),
            String::new(),
        ),
    };
    let row = ui::hbox(8);
    row.append(&ui::label(&rank.to_string(), &["dim-label", "numeric"]));
    let (c, f) = (ctx.clone(), file.to_owned());
    row.append(&ui::icon_button(
        "media-playback-start-symbolic",
        "Play",
        move || c.act(Action::PlayFile(f.clone())),
    ));
    let t = ui::vbox(0);
    t.set_hexpand(true);
    t.append(&ui::ellipsized(&title, &[]));
    if !sub.is_empty() {
        t.append(&ui::ellipsized(&sub, &["dim-label", "caption"]));
    }
    if frac > 0.0 {
        t.append(
            &gtk::LevelBar::builder()
                .value(frac)
                .css_classes(["stats-bar"])
                .build(),
        );
    }
    row.append(&t);
    row.append(&ui::label(right, &["dim-label", "caption", "numeric"]));
    row
}

/// Vertical columns (day of week, hours).
fn columns(values: &[f64], labels: &[&str], top: impl Fn(f64) -> String) -> gtk::Box {
    let max = values.iter().copied().fold(0.0, f64::max).max(1e-9);
    let b = ui::hbox(4);
    b.set_homogeneous(true);
    for (i, &v) in values.iter().enumerate() {
        let col = ui::vbox(2);
        col.set_valign(gtk::Align::End);
        let t = top(v);
        if !t.is_empty() {
            col.append(&ui::label(&t, &["caption", "dim-label", "numeric"]));
        }
        let bar = gtk::Box::builder()
            .height_request(((v / max) * 80.0).max(2.0) as i32)
            .css_classes(["stats-column"])
            .valign(gtk::Align::End)
            .build();
        col.append(&bar);
        col.append(&ui::label(
            labels.get(i).copied().unwrap_or(""),
            &["caption", "dim-label"],
        ));
        b.append(&col);
    }
    b
}

/// Donut chart of the genre split.
fn donut(parts: &[(String, f64)]) -> gtk::DrawingArea {
    let data: Vec<f64> = parts.iter().map(|p| p.1).collect();
    let n = parts.len();
    let area = gtk::DrawingArea::builder()
        .content_height(140)
        .content_width(140)
        .halign(gtk::Align::Center)
        .css_classes(["visualizer"])
        .build();
    area.set_draw_func(move |a, cr, w, h| {
        let total: f64 = data.iter().sum();
        if total <= 0.0 {
            return;
        }
        let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
        let r = (w.min(h) as f64) / 2.0 - 2.0;
        let accent = crate::ui::accent_rgba(a);
        let mut ang = -std::f64::consts::FRAC_PI_2;
        for (i, v) in data.iter().enumerate() {
            let sweep = v / total * std::f64::consts::TAU;
            let alpha = 1.0 - i as f64 * 0.13;
            cr.set_source_rgba(
                accent.red() as f64,
                accent.green() as f64,
                accent.blue() as f64,
                alpha.max(0.25),
            );
            cr.move_to(cx, cy);
            cr.arc(cx, cy, r, ang, ang + sweep);
            cr.close_path();
            let _ = cr.fill();
            ang += sweep;
        }
        cr.set_operator(cairo::Operator::Clear);
        cr.arc(cx, cy, r * 0.6, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
        cr.set_operator(cairo::Operator::Over);
        let fg = a.parent().map(|p| p.color()).unwrap_or(accent);
        cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.9);
        cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
        cr.set_font_size(20.0);
        let text = n.to_string();
        if let Ok(ext) = cr.text_extents(&text) {
            cr.move_to(
                cx - ext.width() / 2.0 - ext.x_bearing(),
                cy + ext.height() / 2.0,
            );
            let _ = cr.show_text(&text);
        }
    });
    area
}
