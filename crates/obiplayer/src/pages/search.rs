//! Library search.

use std::rc::Rc;

use relm4::adw::{self, prelude::*};
use relm4::gtk::{self, glib};

use super::Page;
use crate::ctx::{Action, Ctx, Route};
use crate::tr;
use crate::ui;

pub struct SearchPage {
    nav: adw::NavigationPage,
    route: Route,
    entry: gtk::SearchEntry,
}

impl SearchPage {
    pub fn new(ctx: &Rc<Ctx>, query: &str) -> SearchPage {
        let entry = gtk::SearchEntry::builder()
            .placeholder_text(tr("Search artists, albums, tracks…"))
            .hexpand(true)
            .build();
        let results = ui::vbox(12);
        results.set_margin_top(12);
        results.set_margin_bottom(24);
        results.set_margin_start(18);
        results.set_margin_end(18);

        {
            let (c, r) = (ctx.clone(), results.clone());
            entry.connect_search_changed(move |e| render(&c, &r, e.text().as_str()));
        }

        let clamp = adw::Clamp::builder()
            .maximum_size(1000)
            .child(&results)
            .build();
        let scroll = gtk::ScrolledWindow::builder()
            .child(&clamp)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let header = adw::HeaderBar::builder()
            .title_widget(
                &adw::Clamp::builder()
                    .maximum_size(500)
                    .child(&entry)
                    .build(),
            )
            .build();
        let tv = adw::ToolbarView::new();
        tv.add_top_bar(&header);
        tv.set_content(Some(&scroll));
        let route = Route::Search(String::new());
        let nav = adw::NavigationPage::builder()
            .title(tr("Search"))
            .tag(super::tag(&route))
            .child(&tv)
            .build();

        let page = SearchPage { nav, route, entry };
        page.apply_query(query);
        render(ctx, &results, query);
        {
            let e = page.entry.clone();
            page.nav.connect_shown(move |_| {
                e.grab_focus();
            });
        }
        page
    }

    fn apply_query(&self, q: &str) {
        self.entry.set_text(q);
        self.entry.set_position(-1);
        let e = self.entry.clone();
        glib::idle_add_local_once(move || {
            e.grab_focus();
        });
    }
}

impl Page for SearchPage {
    fn nav(&self) -> &adw::NavigationPage {
        &self.nav
    }
    fn route(&self) -> &Route {
        &self.route
    }
    fn set_query(&self, q: &str) {
        self.apply_query(q);
    }
}

fn status(icon: &str, title: &str) -> adw::StatusPage {
    adw::StatusPage::builder()
        .icon_name(icon)
        .title(title)
        .vexpand(true)
        .build()
}

fn list() -> gtk::ListBox {
    gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build()
}

fn row() -> gtk::Box {
    let r = ui::hbox(10);
    r.set_margin_top(4);
    r.set_margin_bottom(4);
    r.set_margin_start(8);
    r.set_margin_end(8);
    r
}

fn render(ctx: &Rc<Ctx>, out: &gtk::Box, query: &str) {
    ui::clear(out);
    if query.trim().chars().count() < 2 {
        out.append(&status("system-search-symbolic", &tr("Type to search")));
        return;
    }
    let lib = ctx.lib();
    let res = lib.search(query);
    if res.is_empty() {
        out.append(&status(
            "edit-find-symbolic",
            &format!("{} “{}”", tr("No results for"), query.trim()),
        ));
        return;
    }

    if !res.artists.is_empty() {
        out.append(&ui::heading(&format!(
            "{} ({})",
            tr("Artists"),
            res.total_artists
        )));
        let l = list();
        let mut names = Vec::new();
        for (name, albums, tracks) in &res.artists {
            let r = row();
            r.append(
                &gtk::Image::builder()
                    .icon_name("avatar-default-symbolic")
                    .css_classes(["dim-label"])
                    .build(),
            );
            let t = ui::vbox(0);
            t.set_hexpand(true);
            t.append(&ui::ellipsized(name, &[]));
            t.append(&ui::ellipsized(
                &format!("{albums} {} · {tracks} {}", tr("albums"), tr("tracks")),
                &["dim-label", "caption"],
            ));
            r.append(&t);
            l.append(&r);
            names.push(name.clone());
        }
        let c = ctx.clone();
        l.connect_row_activated(move |_, row| {
            if let Some(n) = names.get(row.index() as usize) {
                c.act(Action::Open(Route::Artist(n.clone())));
            }
        });
        out.append(&l);
    }

    if !res.albums.is_empty() {
        out.append(&ui::heading(&format!(
            "{} ({})",
            tr("Albums"),
            res.total_albums
        )));
        let l = list();
        let mut folders = Vec::new();
        for &i in &res.albums {
            let a = lib.album(i);
            let r = row();
            r.append(&ui::cover(ctx, a, 40));
            let t = ui::vbox(0);
            t.set_hexpand(true);
            t.set_valign(gtk::Align::Center);
            t.append(&ui::ellipsized(&a.title, &[]));
            t.append(&ui::ellipsized(&a.artist, &["dim-label", "caption"]));
            r.append(&t);
            r.append(&ui::format_badge(&a.format()));
            l.append(&r);
            folders.push(a.folder.clone());
        }
        let c = ctx.clone();
        l.connect_row_activated(move |_, row| {
            if let Some(f) = folders.get(row.index() as usize) {
                c.act(Action::Open(Route::Album(f.clone())));
            }
        });
        out.append(&l);
    }

    if !res.tracks.is_empty() {
        out.append(&ui::heading(&format!(
            "{} ({})",
            tr("Tracks"),
            res.total_tracks
        )));
        let l = list();
        let mut files = Vec::new();
        for &tr_ref in &res.tracks {
            let a = lib.album(tr_ref.album);
            let s = lib.song(tr_ref);
            let r = row();
            let (c, f) = (ctx.clone(), s.file.clone());
            r.append(&ui::icon_button(
                "media-playback-start-symbolic",
                "Play",
                move || c.act(Action::PlayFile(f.clone())),
            ));
            let t = ui::vbox(0);
            t.set_hexpand(true);
            t.append(&ui::ellipsized(s.display_title(), &[]));
            t.append(&ui::ellipsized(
                &format!("{} · {}", s.artist().unwrap_or(&a.artist), a.title),
                &["dim-label", "caption"],
            ));
            r.append(&t);
            let (c, f) = (ctx.clone(), s.file.clone());
            r.append(&ui::icon_button(
                "list-add-symbolic",
                "Add to queue",
                move || c.act(Action::EnqueueFile(f.clone())),
            ));
            r.append(
                &gtk::Label::builder()
                    .label(s.duration.map(ui::mmss).unwrap_or_default())
                    .css_classes(["dim-label", "numeric"])
                    .build(),
            );
            l.append(&r);
            files.push(s.file.clone());
        }
        let c = ctx.clone();
        l.connect_row_activated(move |_, row| {
            if let Some(f) = files.get(row.index() as usize) {
                c.act(Action::Open(Route::Track(f.clone())));
            }
        });
        out.append(&l);
    }
}
