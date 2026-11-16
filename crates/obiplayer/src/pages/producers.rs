//! Producers browse mode and producer page (credits from cached MusicBrainz
//! releases).

use std::cell::RefCell;
use std::rc::Rc;

use obiplayer_core::meta::producers::{self, Producer};
use obiplayer_core::text::fold;
use relm4::adw;
use relm4::gtk::{self, prelude::*};

use super::Page;
use crate::ctx::{Action, Ctx, Route};
use crate::tr;
use crate::ui;

pub struct ProducersView {
    pub root: gtk::Box,
    list: gtk::ListBox,
    chips: gtk::Box,
    count: gtk::Label,
    data: Rc<RefCell<Vec<Producer>>>,
    query: Rc<RefCell<String>>,
    role: Rc<RefCell<Option<String>>>,
}

impl ProducersView {
    pub fn new(ctx: &Rc<Ctx>) -> Rc<ProducersView> {
        let entry = gtk::SearchEntry::builder()
            .placeholder_text(tr("Search producers…"))
            .hexpand(true)
            .build();
        let count = gtk::Label::builder()
            .css_classes(["dim-label", "caption"])
            .build();
        let bar = ui::hbox(12);
        bar.set_margin_top(8);
        bar.set_margin_start(12);
        bar.set_margin_end(12);
        bar.append(&entry);
        bar.append(&count);
        let chips = ui::hbox(6);
        chips.set_margin_start(12);
        chips.set_margin_bottom(6);
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["navigation-sidebar"])
            .build();
        let root = ui::vbox(6);
        root.append(&bar);
        root.append(
            &gtk::ScrolledWindow::builder()
                .child(&chips)
                .vscrollbar_policy(gtk::PolicyType::Never)
                .build(),
        );
        root.append(
            &gtk::ScrolledWindow::builder()
                .child(&list)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .vexpand(true)
                .build(),
        );
        let v = Rc::new(ProducersView {
            root,
            list,
            chips,
            count,
            data: Rc::default(),
            query: Rc::default(),
            role: Rc::default(),
        });
        {
            let v2 = v.clone();
            let c = ctx.clone();
            entry.connect_search_changed(move |e| {
                *v2.query.borrow_mut() = fold(e.text().trim());
                v2.render(&c);
            });
        }
        {
            let (c, v2) = (ctx.clone(), v.clone());
            v.list.connect_row_activated(move |_, row| {
                if let Some(name) = row.widget_name().strip_prefix("p:") {
                    c.act(Action::Open(Route::Producer(name.to_owned())));
                }
                let _ = &v2;
            });
        }
        v
    }

    /// Recompute from the metadata cache (off the main thread).
    pub fn reload(self: &Rc<Self>, ctx: &Rc<Ctx>) {
        ui::clear(&self.chips);
        self.count.set_label(&tr("Scanning…"));
        let (s, lib) = (ctx.services.clone(), ctx.lib());
        // Library is not Send; hand the worker a cheap clone of the albums.
        let albums = lib.albums.clone();
        let (me, c) = (self.clone(), ctx.clone());
        relm4::spawn_local(async move {
            let found = relm4::spawn_blocking(move || {
                producers::collect(&s, &obiplayer_core::catalog::Library::new(albums))
            })
            .await
            .unwrap_or_default();
            *me.data.borrow_mut() = found;
            me.build_chips(&c);
            me.render(&c);
        });
    }

    fn build_chips(self: &Rc<Self>, ctx: &Rc<Ctx>) {
        ui::clear(&self.chips);
        let mut roles: Vec<String> = Vec::new();
        for p in self.data.borrow().iter() {
            for r in p.base_roles() {
                if !roles.contains(&r) {
                    roles.push(r);
                }
            }
        }
        roles.sort();
        let all = gtk::ToggleButton::builder()
            .label(tr("All"))
            .active(true)
            .css_classes(["pill", "chip"])
            .build();
        self.chips.append(&all);
        {
            let (me, c) = (self.clone(), ctx.clone());
            all.connect_toggled(move |b| {
                if b.is_active() {
                    *me.role.borrow_mut() = None;
                    me.render(&c);
                }
            });
        }
        for r in roles {
            let b = gtk::ToggleButton::builder()
                .label(obiplayer_core::text::capitalize_words(&r))
                .group(&all)
                .css_classes(["pill", "chip"])
                .build();
            let (me, c) = (self.clone(), ctx.clone());
            b.connect_toggled(move |b| {
                if b.is_active() {
                    *me.role.borrow_mut() = Some(r.clone());
                    me.render(&c);
                }
            });
            self.chips.append(&b);
        }
    }

    fn render(&self, ctx: &Rc<Ctx>) {
        self.list.remove_all();
        let q = self.query.borrow().clone();
        let role = self.role.borrow().clone();
        let mut n = 0;
        for p in self.data.borrow().iter() {
            if !q.is_empty() && !fold(&p.name).contains(&q) {
                continue;
            }
            let roles = p.base_roles();
            if let Some(r) = &role
                && !roles.iter().any(|x| x.contains(r.as_str()))
            {
                continue;
            }
            n += 1;
            let row = ui::hbox(12);
            row.set_margin_top(6);
            row.set_margin_bottom(6);
            row.set_margin_start(6);
            row.append(&ui::avatar(ctx, &p.name, 36));
            let t = ui::vbox(0);
            t.set_hexpand(true);
            t.append(&ui::ellipsized(&p.name, &[]));
            t.append(&ui::ellipsized(
                &format!("{} {} · {}", p.albums.len(), tr("albums"), roles.join(", ")),
                &["dim-label", "caption"],
            ));
            row.append(&t);
            row.append(&gtk::Image::from_icon_name("go-next-symbolic"));
            let lr = gtk::ListBoxRow::builder()
                .child(&row)
                .name(format!("p:{}", p.name))
                .build();
            self.list.append(&lr);
        }
        if self.data.borrow().is_empty() {
            self.count.set_label(&tr("No producer data yet"));
            let hint = ui::label(
                &tr("Producer credits come from MusicBrainz. Open albums to fetch their credits."),
                &["dim-label"],
            );
            hint.set_margin_start(12);
            self.list.append(&hint);
        } else {
            self.count.set_label(&format!("{n} {}", tr("producers")));
        }
    }
}

pub struct ProducerPage {
    nav: adw::NavigationPage,
    route: Route,
}

impl ProducerPage {
    pub fn new(ctx: &Rc<Ctx>, name: &str) -> ProducerPage {
        let body = ui::page_body();
        let header = ui::hbox(18);
        header.append(&ui::avatar(ctx, name, 120));
        let title = ui::vbox(4);
        title.set_valign(gtk::Align::Center);
        let n = ui::label(name, &["title-1"]);
        n.set_width_chars(16);
        title.append(&n);
        let roles_l = ui::label("", &["dim-label"]);
        let count_l = ui::label("", &["dim-label", "caption"]);
        title.append(&roles_l);
        title.append(&count_l);
        header.append(&title);
        body.append(&header);
        let bio = ui::vbox(4);
        body.append(&bio);
        let albums = ui::vbox(8);
        body.append(&albums);

        // Credits (cache-only, fast) then Wikipedia (network).
        {
            let (c, name2) = (ctx.clone(), name.to_owned());
            let (roles_l, count_l, albums, bio) = (roles_l, count_l, albums, bio);
            relm4::spawn_local(async move {
                let (s, list) = (c.services.clone(), c.lib().albums.clone());
                let key = fold(&name2);
                let p = relm4::spawn_blocking(move || {
                    producers::collect(&s, &obiplayer_core::catalog::Library::new(list))
                        .into_iter()
                        .find(|p| fold(&p.name) == key)
                })
                .await
                .ok()
                .flatten();
                if let Some(p) = p {
                    roles_l.set_label(
                        &p.base_roles()
                            .iter()
                            .map(|r| obiplayer_core::text::capitalize_words(r))
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                    count_l.set_label(&format!("{} {}", p.albums.len(), tr("albums in library")));
                    albums.append(&ui::heading(&tr("Albums")));
                    let idx: Vec<u32> = p.albums.iter().map(|(i, _)| *i).collect();
                    albums.append(&ui::album_flow(&c, &idx));
                }
                let s = c.services.clone();
                let n2 = name2.clone();
                let wiki = relm4::spawn(async move { s.wiki.search_artist(&n2).await })
                    .await
                    .ok()
                    .flatten();
                if let Some(w) = wiki {
                    let t = ui::label(&w.extract, &[]);
                    t.set_lines(6);
                    t.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    t.set_width_chars(30);
                    bio.append(&t);
                    bio.append(&ui::uri_button(&tr("Wikipedia"), w.page_url.clone()));
                }
            });
        }
        let route = Route::Producer(name.to_owned());
        ProducerPage {
            nav: ui::page(name, &super::tag(&route), &body),
            route,
        }
    }
}

impl Page for ProducerPage {
    fn nav(&self) -> &adw::NavigationPage {
        &self.nav
    }
    fn route(&self) -> &Route {
        &self.route
    }
}
