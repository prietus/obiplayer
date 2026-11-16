//! "Compare editions": every edition of an album in the library, ranked by
//! quality (DR + format), with each pressing's release details, plus the
//! Loudness War DB entries for the album.

use std::rc::Rc;
use std::sync::Arc;

use obiplayer_core::library::Album;
use obiplayer_core::meta::loudnesswar;
use obiplayer_core::meta::release::ReleaseInfo;
use obiplayer_core::meta::titles;
use obiplayer_core::quality::{self, Badge};
use relm4::adw;
use relm4::gtk::{self, prelude::*};

use super::Page;
use crate::ctx::{Action, Ctx, Route};
use crate::tr;
use crate::ui::{self, InfoGrid};

pub struct EditionsPage {
    nav: adw::NavigationPage,
    route: Route,
    ctx: Rc<Ctx>,
    editions: Vec<Arc<Album>>,
    current: String,
    cards: gtk::Box,
    header_dr: gtk::Box,
}

impl EditionsPage {
    pub fn new(ctx: &Rc<Ctx>, folder: &str) -> Option<EditionsPage> {
        let lib = ctx.lib();
        let album = lib.album_by_folder(folder)?.clone();
        let mut editions = vec![album.clone()];
        editions.extend(
            lib.other_editions(&album)
                .into_iter()
                .map(|i| lib.album(i).clone()),
        );

        let body = ui::page_body();
        body.append(&ui::label(&tr("Compare editions"), &["title-1"]));
        let sub = ui::hbox(8);
        sub.append(&ui::label(
            &format!("{} — {}", album.artist, album.title),
            &["title-4", "dim-label"],
        ));
        let header_dr = ui::hbox(6);
        sub.append(&header_dr);
        body.append(&sub);

        body.append(&ui::heading(&format!(
            "{} ({})",
            tr("My versions"),
            editions.len()
        )));
        let cards = ui::vbox(10);
        body.append(&cards);

        body.append(&ui::heading("Loudness War DB"));
        let lw = ui::vbox(4);
        body.append(&lw);
        fill_loudness_war(ctx, &album, &lw);

        // Analyse every edition's tracks (the app queues them).
        ctx.act(Action::AnalyzeDr(
            editions
                .iter()
                .flat_map(|a| a.tracks.iter().map(|t| t.file.clone()))
                .collect(),
        ));

        let route = Route::Editions(folder.to_owned());
        let page = EditionsPage {
            nav: ui::page(&tr("Compare editions"), &super::tag(&route), &body),
            route,
            ctx: ctx.clone(),
            editions,
            current: folder.to_owned(),
            cards,
            header_dr,
        };
        page.refresh();

        // Release details, one edition at a time (MusicBrainz rate limit).
        {
            let (c, eds) = (ctx.clone(), page.editions.clone());
            let cards = page.cards.clone();
            let current = page.current.clone();
            let header = page.header_dr.clone();
            relm4::spawn_local(async move {
                for a in &eds {
                    let _ = c.release_info(a).await;
                    render(&c, &eds, &current, &cards, &header);
                }
            });
        }
        Some(page)
    }
}

impl Page for EditionsPage {
    fn nav(&self) -> &adw::NavigationPage {
        &self.nav
    }
    fn route(&self) -> &Route {
        &self.route
    }
    fn refresh(&self) {
        render(
            &self.ctx,
            &self.editions,
            &self.current,
            &self.cards,
            &self.header_dr,
        );
    }
}

fn render(ctx: &Rc<Ctx>, eds: &[Arc<Album>], current: &str, cards: &gtk::Box, header: &gtk::Box) {
    ui::clear(header);
    if let Some(d) = eds.first().and_then(|a| ctx.album_dr(a)) {
        header.append(&ui::dr_badge(d));
    }

    let items: Vec<(Option<i32>, String)> =
        eds.iter().map(|a| (ctx.album_dr(a), a.format())).collect();
    let ranked = quality::rank(
        &items
            .iter()
            .map(|(d, f)| (*d, f.as_str()))
            .collect::<Vec<_>>(),
    );
    ui::clear(cards);
    for (i, score) in ranked {
        let a = &eds[i];
        let best = score.badge == Badge::Best && eds.len() > 1;
        let card = ui::vbox(6);
        card.add_css_class("edition-card");
        if best {
            card.add_css_class("row-best");
        } else if score.badge == Badge::Warning {
            card.add_css_class("row-warning");
        }

        // Row 1: play · title · badges · score
        let r1 = ui::hbox(8);
        let (c, f) = (ctx.clone(), a.first_file().unwrap_or_default().to_owned());
        r1.append(&ui::icon_button(
            "media-playback-start-symbolic",
            "Play",
            move || c.act(Action::PlayFile(f.clone())),
        ));
        let (c, fo) = (ctx.clone(), a.folder.clone());
        let t = ui::link(&a.title, if best { &["heading"] } else { &[] }, move || {
            c.act(Action::Open(Route::Album(fo.clone())))
        });
        t.set_hexpand(true);
        r1.append(&t);
        if best {
            r1.append(&ui::badge(&tr("BEST"), "badge-best"));
        }
        if a.folder == current {
            r1.append(&ui::badge(&tr("CURRENT"), "badge-current"));
        }
        r1.append(&ui::ellipsized(
            &format!("{}pts", score.total),
            &["dim-label", "caption", "numeric"],
        ));
        card.append(&r1);

        // Row 2: format · DR · tracks · duration
        let r2 = ui::hbox(10);
        r2.append(&ui::format_badge(&a.format()));
        match ctx.album_dr(a) {
            Some(d) => {
                r2.append(&ui::dr_badge(d));
                r2.append(&ui::dr_bar(d, 60));
            }
            None => r2.append(&ui::label("DR …", &["dim-label", "caption"])),
        }
        r2.append(&ui::label(
            &format!("{} {}", a.tracks.len(), tr("tracks")),
            &["dim-label", "caption"],
        ));
        r2.append(&ui::label(
            &a.formatted_duration(),
            &["dim-label", "caption"],
        ));
        card.append(&r2);

        // Row 3: release details (when looked up)
        if let Some(info) = ctx.releases.borrow().get(&a.folder).cloned() {
            card.append(&details(ctx, a, &info).grid);
        }

        // Row 4: folder
        let path = gtk::Label::builder()
            .label(&a.folder)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::Start)
            .css_classes(["dim-label", "monospace", "caption"])
            .build();
        card.append(&path);
        cards.append(&card);
    }
}

fn details(ctx: &Rc<Ctx>, a: &Album, info: &ReleaseInfo) -> InfoGrid {
    let mut g = InfoGrid::new();
    let mb = info.mb.as_ref().filter(|_| info.mb_matches_edition);
    if let Some(l) = info.label() {
        let (c, l2) = (ctx.clone(), l.clone());
        g.widget(
            "Label",
            &ui::link(&l, &[], move || {
                c.act(Action::Open(Route::Label(l2.clone())))
            }),
        );
    }
    if let Some(cat) = info.catalog() {
        g.copyable("Catalog", &cat);
    }
    if let Some(m) = mb {
        g.copyable("Barcode", &m.barcode);
    }
    if let Some(c) = info.country() {
        g.text("Country", &c);
    }
    let date = a
        .date
        .clone()
        .or_else(|| mb.map(|m| m.date.clone()))
        .unwrap_or_default();
    g.text("Date", &date);
    if let Some(od) = a
        .original_date
        .as_deref()
        .filter(|d| Some(*d) != a.date.as_deref())
    {
        g.text("Original", od);
    }
    if let Some(d) = &info.dg {
        g.text("Source", &d.formats.join("; "));
    }
    if let Some(m) = mb {
        g.text("Producer", &m.producers.join(", "));
        g.text("Engineer", &m.engineers.join(", "));
        g.text("Mastering", &m.mastering_engineers.join(", "));
    }
    g
}

fn fill_loudness_war(ctx: &Rc<Ctx>, album: &Album, out: &gtk::Box) {
    let row = ui::hbox(8);
    row.append(&gtk::Spinner::builder().spinning(true).build());
    row.append(&ui::label(&tr("Querying Loudness War DB…"), &["dim-label"]));
    out.append(&row);
    let (s, artist, title) = (
        ctx.services.clone(),
        album.artist.clone(),
        titles::clean_title_for_search(&album.title),
    );
    let my_dr = ctx.album_dr(album);
    let out = out.clone();
    let url = loudnesswar::search_url(&artist, &title);
    relm4::spawn_local(async move {
        let entries = relm4::spawn(async move {
            let found = s.loudness.search(&artist, &title).await;
            let mut detailed = Vec::with_capacity(found.len());
            for e in found {
                detailed.push(s.loudness.fetch_detail(e).await);
            }
            detailed
        })
        .await
        .unwrap_or_default();
        ui::clear(&out);
        if entries.is_empty() {
            out.append(&ui::label(
                &tr("No data found in Loudness War DB for this album"),
                &["dim-label"],
            ));
        }
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        for e in &entries {
            let r = ui::hbox(10);
            r.set_margin_top(6);
            r.set_margin_bottom(6);
            r.set_margin_start(8);
            r.set_margin_end(8);
            let t = ui::vbox(2);
            t.set_hexpand(true);
            if e.id.is_empty() {
                t.append(&ui::ellipsized(&e.album, &["heading"]));
            } else {
                t.append(&ui::uri_button(&e.album, loudnesswar::album_url(&e.id)));
            }
            let meta: Vec<&str> = [
                e.source.as_str(),
                e.year.as_str(),
                e.label.as_str(),
                e.catalog.as_str(),
                e.country.as_str(),
            ]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect();
            t.append(&ui::ellipsized(
                &meta.join(" · "),
                &["dim-label", "caption"],
            ));
            r.append(&t);
            r.append(&ui::badge(
                &e.codec,
                if e.codec.to_lowercase().contains("lossless") {
                    "fmt-lossless"
                } else {
                    "fmt-lossy"
                },
            ));
            r.append(&ui::dr_badge(e.dr));
            r.append(&ui::label(
                &format!("({}–{})", e.dr_min, e.dr_max),
                &["dim-label", "caption", "numeric"],
            ));
            if let Some(m) = my_dr {
                if e.dr == m {
                    r.add_css_class("lw-row-same");
                } else if e.dr > m {
                    r.add_css_class("lw-row-better");
                }
            }
            list.append(&r);
        }
        if !entries.is_empty() {
            out.append(&list);
        }
        let foot = ui::hbox(8);
        let src = ui::label(
            "dr.loudness-war.info",
            &["dim-label", "caption", "source-note"],
        );
        src.set_hexpand(true);
        foot.append(&src);
        foot.append(&ui::uri_button(&tr("Open in browser"), url));
        out.append(&foot);
    });
}
