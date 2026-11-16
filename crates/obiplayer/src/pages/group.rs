//! Artist, label and composer detail pages.

use std::collections::BTreeSet;
use std::rc::Rc;

use obiplayer_core::catalog::Group;
use obiplayer_core::text;
use relm4::adw;
use relm4::gtk::{self, prelude::*};

use super::Page;
use crate::ctx::{Action, AlbumFilter, Ctx, Route};
use crate::tr;
use crate::ui::{self, InfoGrid};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Artist,
    Label,
    Composer,
}

pub struct GroupPage {
    nav: adw::NavigationPage,
    route: Route,
}

fn find(ctx: &Ctx, kind: Kind, name: &str) -> Option<Group> {
    let lib = ctx.lib();
    let groups = match kind {
        Kind::Artist => lib.artists(),
        Kind::Label => lib.labels(),
        Kind::Composer => lib.composers(),
    };
    let key = text::fold(name);
    groups.into_iter().find(|g| text::fold(&g.name) == key)
}

fn hours(secs: f64) -> String {
    let m = (secs / 60.0).round() as u64;
    if m >= 60 {
        format!("{}h {}min", m / 60, m % 60)
    } else {
        format!("{m} min")
    }
}

impl GroupPage {
    pub fn new(ctx: &Rc<Ctx>, kind: Kind, name: &str) -> Option<GroupPage> {
        let g = find(ctx, kind, name)?;
        let lib = ctx.lib();
        let body = ui::page_body();

        let icon = match kind {
            Kind::Artist => "avatar-default-symbolic",
            Kind::Label => "building-symbolic",
            Kind::Composer => "music-note-symbolic",
        };
        let header = ui::hbox(18);
        header.append(
            &gtk::Image::builder()
                .icon_name(icon)
                .pixel_size(64)
                .css_classes(["dim-label", "group-avatar"])
                .build(),
        );
        let title = ui::vbox(4);
        title.set_valign(gtk::Align::Center);
        let name = ui::label(&g.name, &["title-1"]);
        name.set_width_chars(16);
        title.append(&name);
        title.append(&ui::label(
            &format!(
                "{} {} · {} {}",
                g.albums.len(),
                tr("albums"),
                g.tracks.len(),
                tr("tracks")
            ),
            &["dim-label"],
        ));
        header.append(&title);
        body.append(&header);

        // -- In your library ----------------------------------------------------------
        let albums: Vec<_> = g.albums.iter().map(|&i| lib.album(i).clone()).collect();
        let duration: f64 = g.tracks.iter().filter_map(|&r| lib.song(r).duration).sum();
        let years: Vec<&str> = albums.iter().filter_map(|a| a.year()).collect();
        let formats: BTreeSet<String> = albums
            .iter()
            .flat_map(|a| {
                a.format()
                    .split(" / ")
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .filter(|f| !f.is_empty())
            .collect();

        let mut info = InfoGrid::new();
        info.text("Albums", &g.albums.len().to_string());
        info.text("Tracks", &g.tracks.len().to_string());
        info.text("Duration", &hours(duration));
        if let (Some(min), Some(max)) = (years.iter().min(), years.iter().max()) {
            info.text(
                if kind == Kind::Label {
                    "Years"
                } else {
                    "Period"
                },
                &if min == max {
                    min.to_string()
                } else {
                    format!("{min} – {max}")
                },
            );
        }
        if !formats.is_empty() {
            let v = ui::label(
                &formats.into_iter().collect::<Vec<_>>().join(" · "),
                &["monospace"],
            );
            info.widget("Formats", &v);
        }
        if kind != Kind::Artist {
            let mut names: Vec<String> = Vec::new();
            for r in &g.tracks {
                let a = lib.album(r.album);
                let n = lib.song(*r).artist().unwrap_or(&a.artist).to_owned();
                if !n.is_empty() && !names.iter().any(|x| text::fold(x) == text::fold(&n)) {
                    names.push(n);
                }
            }
            names.sort_by(|a, b| text::compare(a, b));
            let limit = if kind == Kind::Label { 10 } else { 5 };
            let more = names.len() > limit;
            names.truncate(limit);
            let mut s = names.join(", ");
            if more {
                s.push_str(", …");
            }
            info.text(
                if kind == Kind::Composer {
                    "Performers"
                } else {
                    "Artists"
                },
                &s,
            );
        }
        body.append(&ui::heading(&tr("In your library")));
        body.append(&info.grid);

        if !g.genres.is_empty() {
            let items = g
                .genres
                .iter()
                .take(20)
                .map(|x| {
                    let (c, x2) = (ctx.clone(), x.clone());
                    (
                        x.to_uppercase(),
                        Box::new(move || c.act(Action::Filter(AlbumFilter::Genre(x2.clone()))))
                            as Box<dyn Fn()>,
                    )
                })
                .collect();
            body.append(&ui::chip_box(items));
        }

        // -- Albums / compositions ---------------------------------------------------
        match kind {
            Kind::Composer => {
                body.append(&ui::heading(&format!(
                    "{} ({} {})",
                    tr("Compositions"),
                    g.tracks.len(),
                    tr("tracks")
                )));
                body.append(&compositions(ctx, &g));
            }
            _ => {
                let mut idx = g.albums.clone();
                if kind == Kind::Label {
                    idx.sort_by(|a, b| lib.album(*a).date.cmp(&lib.album(*b).date));
                }
                body.append(&ui::heading(&format!(
                    "{} ({})",
                    tr(if kind == Kind::Artist {
                        "Discography"
                    } else {
                        "Albums"
                    }),
                    idx.len()
                )));
                body.append(&ui::album_flow(ctx, &idx));
            }
        }

        let route = match kind {
            Kind::Artist => Route::Artist(g.name.clone()),
            Kind::Label => Route::Label(g.name.clone()),
            Kind::Composer => Route::Composer(g.name.clone()),
        };
        Some(GroupPage {
            nav: ui::page(&g.name, &super::tag(&route), &body),
            route,
        })
    }
}

impl Page for GroupPage {
    fn nav(&self) -> &adw::NavigationPage {
        &self.nav
    }
    fn route(&self) -> &Route {
        &self.route
    }
}

fn compositions(ctx: &Rc<Ctx>, g: &Group) -> gtk::ListBox {
    let lib = ctx.lib();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let mut files = Vec::new();
    for &r in &g.tracks {
        let a = lib.album(r.album);
        let s = lib.song(r);
        let row = ui::hbox(10);
        row.set_margin_top(4);
        row.set_margin_bottom(4);
        row.set_margin_start(6);
        row.set_margin_end(8);
        let (c, f) = (ctx.clone(), s.file.clone());
        row.append(&ui::icon_button(
            "media-playback-start-symbolic",
            "Play",
            move || c.act(Action::PlayFile(f.clone())),
        ));
        let text = ui::vbox(0);
        text.set_hexpand(true);
        text.append(&ui::ellipsized(s.display_title(), &[]));
        text.append(&ui::ellipsized(
            &format!("{} · {}", s.artist().unwrap_or(&a.artist), a.title),
            &["dim-label", "caption"],
        ));
        row.append(&text);
        row.append(
            &gtk::Label::builder()
                .label(s.duration.map(ui::mmss).unwrap_or_default())
                .css_classes(["dim-label", "numeric"])
                .build(),
        );
        list.append(&row);
        files.push(s.file.clone());
    }
    let c = ctx.clone();
    list.connect_row_activated(move |_, row| {
        if let Some(f) = files.get(row.index() as usize) {
            c.act(Action::Open(Route::Track(f.clone())));
        }
    });
    list
}
