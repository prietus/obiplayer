//! Album page: online information column (release summary, Wikipedia,
//! collapsible MusicBrainz / Discogs / credits / members details) and the
//! certified-pressing badge.

use std::collections::BTreeMap;
use std::rc::Rc;

use obiplayer_core::library::Album;
use obiplayer_core::meta::discogs::PressingInfo;
use obiplayer_core::meta::release::ReleaseInfo;
use obiplayer_core::meta::wikipedia::Summary;
use relm4::gtk::{self, prelude::*};

use crate::ctx::Ctx;
use crate::tr;
use crate::ui::{self, InfoGrid};

fn link_button(label: &str, url: String) -> gtk::Button {
    ui::uri_button(label, url)
}

fn source(text: &str) -> gtk::Label {
    ui::label(
        &format!("{} {text}", tr("Source:")),
        &["dim-label", "caption", "source-note"],
    )
}

fn wiki_section(title: &str, w: &Summary) -> gtk::Box {
    let b = ui::vbox(4);
    b.append(&ui::label(title, &["heading"]));
    let text = ui::label(&w.extract, &[]);
    text.set_lines(6);
    text.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.set_width_chars(30);
    b.append(&text);
    let row = ui::hbox(6);
    row.append(&link_button(
        &tr("Read more on Wikipedia"),
        w.page_url.clone(),
    ));
    b.append(&row);
    b
}

fn expander(title: &str, content: &impl IsA<gtk::Widget>) -> gtk::Expander {
    gtk::Expander::builder()
        .label(title)
        .child(content)
        .css_classes(["details-expander"])
        .build()
}

/// Fill the info column for `album` from `info`.
pub fn fill(column: &gtk::Box, album: &Album, info: &ReleaseInfo) {
    ui::clear(column);
    let mb = info.mb.as_ref().filter(|_| info.mb_matches_edition);
    let dg = info.dg.as_ref();

    // -- Summary badge ---------------------------------------------------------
    if mb.is_some() || dg.is_some() {
        let card = ui::vbox(4);
        card.add_css_class("summary-card");
        let line1: Vec<String> = [info.label(), info.country()]
            .into_iter()
            .flatten()
            .chain(
                mb.map(|m| m.date.clone())
                    .or_else(|| dg.and_then(|d| d.year.map(|y| y.to_string()))),
            )
            .filter(|s| !s.is_empty())
            .collect();
        card.append(&ui::label(&line1.join(" · "), &["heading"]));
        let mut line2: Vec<String> = Vec::new();
        if let Some(c) = info.catalog() {
            line2.push(format!("{} {c}", tr("Cat.")));
        }
        if let Some(m) = mb.filter(|m| !m.status.is_empty()) {
            line2.push(m.status.clone());
        }
        if let Some(f) = dg.and_then(|d| d.formats.first()) {
            line2.push(f.clone());
        }
        if !line2.is_empty() {
            card.append(&ui::label(&line2.join(" · "), &["dim-label", "caption"]));
        }
        let links = ui::hbox(4);
        if let Some(m) = mb {
            links.append(&link_button(
                "MusicBrainz",
                format!("https://musicbrainz.org/release/{}", m.id),
            ));
        }
        if let Some(d) = dg {
            links.append(&link_button("Discogs", d.url.clone()));
        }
        if let Some(w) = &info.album_wiki {
            links.append(&link_button("Wikipedia", w.page_url.clone()));
        }
        card.append(&links);
        column.append(&card);
    }

    // -- Wikipedia ----------------------------------------------------------------
    if let Some(w) = &info.album_wiki {
        column.append(&wiki_section(&tr("About the album"), w));
    }
    if let Some(w) = &info.artist_wiki {
        column.append(&wiki_section(
            &format!("{} {}", tr("About"), album.artist),
            w,
        ));
    }

    // -- Details ----------------------------------------------------------------------
    let details = ui::vbox(2);
    if let Some(m) = mb {
        let mut g = InfoGrid::new();
        g.text("Label", &m.label);
        g.copyable("Catalog", &m.catalog_number);
        g.text("Date", &m.date);
        g.text("Country", &m.country);
        g.copyable("Barcode", &m.barcode);
        g.text("Status", &m.status);
        g.text("Genres", &m.genres.join(", "));
        let b = ui::vbox(6);
        b.append(&g.grid);
        b.append(&source("MusicBrainz"));
        details.append(&expander(&tr("Release (MusicBrainz)"), &b));
    }
    if let Some(d) = dg {
        let mut g = InfoGrid::new();
        g.text("Label", &d.label);
        g.copyable("Catalog", &d.catalog_number);
        g.text("Year", &d.year.map(|y| y.to_string()).unwrap_or_default());
        g.text("Country", &d.country);
        g.text("Format", &d.formats.join("; "));
        g.text("Styles", &d.styles.join(", "));
        g.text("Mastered by", &d.mastering_engineer);
        g.text("Cut by", &d.cut_by);
        g.text("Pressed at", &d.pressed_at);
        g.copyable("Matrix", &d.matrix_runout);
        if let Some(p) = &d.lowest_price {
            g.text("Min. price", &format!("${p}"));
        }
        let b = ui::vbox(6);
        b.append(&g.grid);
        if !d.notes.is_empty() {
            let n = ui::label(&d.notes, &["caption"]);
            n.set_lines(4);
            n.set_ellipsize(gtk::pango::EllipsizeMode::End);
            n.set_width_chars(30);
            b.append(&n);
        }
        let row = ui::hbox(6);
        row.append(&link_button(&tr("View on Discogs"), d.url.clone()));
        b.append(&row);
        b.append(&source("Discogs"));
        details.append(&expander(&tr("Physical edition (Discogs)"), &b));
    }
    if let Some(m) = mb.filter(|m| !m.credits.is_empty()) {
        let mut by_role: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for c in &m.credits {
            let names = by_role
                .entry(obiplayer_core::text::capitalize_words(&c.role))
                .or_default();
            if !names.contains(&c.name) {
                names.push(c.name.clone());
            }
        }
        let mut g = InfoGrid::new();
        for (role, names) in &by_role {
            g.text(role, &names.join(", "));
        }
        let b = ui::vbox(6);
        b.append(&g.grid);
        b.append(&source("MusicBrainz"));
        details.append(&expander(&tr("Credits"), &b));
    }
    if let Some(a) = info.artist.as_ref().filter(|a| !a.members.is_empty()) {
        let b = ui::vbox(4);
        let mut head = vec![a.area.clone()];
        if !a.begin.is_empty() {
            head.push(format!("{} {}", tr("since"), a.begin));
        }
        b.append(&ui::label(
            &head
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · "),
            &["dim-label", "caption"],
        ));
        let mut g = InfoGrid::new();
        for m in &a.members {
            let mut v = m.role.clone();
            if !m.period.is_empty() {
                if !v.is_empty() {
                    v.push_str(" · ");
                }
                v.push_str(&m.period);
            }
            g.text(&m.name, if v.is_empty() { "—" } else { &v });
        }
        b.append(&g.grid);
        b.append(&source("MusicBrainz"));
        details.append(&expander(&tr("Members"), &b));
    }
    if details.first_child().is_some() {
        column.append(&ui::label(
            &tr("DETAILS"),
            &["dim-label", "caption-heading", "details-header"],
        ));
        column.append(&details);
    }

    if column.first_child().is_none() {
        column.append(&ui::label(&tr("No additional information"), &["dim-label"]));
    }
}

pub fn loading(column: &gtk::Box) {
    ui::clear(column);
    let row = ui::hbox(8);
    row.append(&gtk::Spinner::builder().spinning(true).build());
    row.append(&ui::label(&tr("Loading…"), &["dim-label"]));
    column.append(&row);
}

/// `.drmobile-pressing.json` sidecar written by DrPlayer/DrMobile.
pub fn read_pressing(ctx: &Rc<Ctx>, album: &Album) -> Option<PressingInfo> {
    obiplayer_core::meta::producers::read_pressing(&ctx.local_path(&album.folder)?)
}

pub fn pressing_badge(p: &PressingInfo) -> gtk::Box {
    let b = ui::vbox(2);
    b.add_css_class("pressing-card");
    let head = ui::hbox(6);
    head.append(&gtk::Image::from_icon_name("emblem-ok-symbolic"));
    head.append(&ui::label(&tr("Certified pressing"), &["heading"]));
    b.append(&head);
    let l1: Vec<&str> = [p.label.as_str(), p.country.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    b.append(&ui::label(&l1.join(" · "), &[]));
    let l2: Vec<&str> = [p.catalog_number.as_str(), p.year.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    b.append(&ui::label(&l2.join(" · "), &["monospace"]));
    if !p.formats.is_empty() {
        b.append(&ui::label(&p.formats.join(", "), &["dim-label", "caption"]));
    }
    let mut g = InfoGrid::new();
    g.text("Mastered by", &p.mastering_engineer);
    g.text("Cut by", &p.cut_by);
    g.text("Pressed at", &p.pressed_at);
    g.text("Recorded at", &p.recorded_at);
    g.text("Mixed at", &p.mixed_at);
    g.copyable("Matrix", &p.matrix_runout);
    if !g.is_empty() {
        b.append(&g.grid);
    }
    b
}
