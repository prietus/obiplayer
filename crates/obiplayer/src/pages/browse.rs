//! Filterable lists of artists, labels and composers (top-level browse modes).

use std::cell::RefCell;
use std::rc::Rc;

use obiplayer_core::catalog::Group;
use obiplayer_core::text::fold;
use relm4::gtk::{self, prelude::*};
use relm4::typed_view::list::{RelmListItem, TypedListView};

use super::group::Kind;
use crate::ctx::{Action, Ctx, Route};
use crate::tr;
use crate::ui;

#[derive(Clone)]
pub struct GroupItem {
    name: String,
    subtitle: String,
    icon: &'static str,
    key: String,
}

pub struct GroupWidgets {
    icon: gtk::Image,
    name: gtk::Label,
    subtitle: gtk::Label,
}

impl RelmListItem for GroupItem {
    type Root = gtk::Box;
    type Widgets = GroupWidgets;

    fn setup(_: &gtk::ListItem) -> (gtk::Box, GroupWidgets) {
        let root = ui::hbox(12);
        root.set_margin_top(6);
        root.set_margin_bottom(6);
        root.set_margin_start(6);
        let icon = gtk::Image::builder()
            .pixel_size(24)
            .css_classes(["dim-label"])
            .build();
        let text = ui::vbox(0);
        text.set_hexpand(true);
        let name = ui::ellipsized("", &[]);
        let subtitle = ui::ellipsized("", &["dim-label", "caption"]);
        text.append(&name);
        text.append(&subtitle);
        root.append(&icon);
        root.append(&text);
        root.append(&gtk::Image::from_icon_name("go-next-symbolic"));
        (
            root,
            GroupWidgets {
                icon,
                name,
                subtitle,
            },
        )
    }

    fn bind(&mut self, w: &mut GroupWidgets, _: &mut gtk::Box) {
        w.icon.set_icon_name(Some(self.icon));
        w.name.set_label(&self.name);
        w.subtitle.set_label(&self.subtitle);
    }
}

pub struct Browse {
    pub root: gtk::Box,
    kind: Kind,
    list: Rc<RefCell<TypedListView<GroupItem, gtk::NoSelection>>>,
    count: gtk::Label,
}

impl Browse {
    pub fn new(ctx: &Rc<Ctx>, kind: Kind) -> Browse {
        let mut list: TypedListView<GroupItem, gtk::NoSelection> = TypedListView::new();
        list.view.set_single_click_activate(true);
        list.view.add_css_class("navigation-sidebar");
        let query: Rc<RefCell<String>> = Rc::default();
        {
            let q = query.clone();
            list.add_filter(move |item| {
                let q = q.borrow();
                q.is_empty() || item.key.contains(q.as_str())
            });
        }
        let list = Rc::new(RefCell::new(list));
        {
            let (l, c) = (list.clone(), ctx.clone());
            list.borrow().view.connect_activate(move |_, pos| {
                let Some(item) = l.borrow().get_visible(pos) else {
                    return;
                };
                let name = item.borrow().name.clone();
                c.act(Action::Open(match kind {
                    Kind::Artist => Route::Artist(name),
                    Kind::Label => Route::Label(name),
                    Kind::Composer => Route::Composer(name),
                }));
            });
        }

        let placeholder = match kind {
            Kind::Artist => "Search artists…",
            Kind::Label => "Search labels…",
            Kind::Composer => "Search composers…",
        };
        let entry = gtk::SearchEntry::builder()
            .placeholder_text(tr(placeholder))
            .hexpand(true)
            .build();
        let count = gtk::Label::builder()
            .css_classes(["dim-label", "caption"])
            .build();
        count.set_hexpand(false);
        {
            let (l, q, cnt) = (list.clone(), query.clone(), count.clone());
            entry.connect_search_changed(move |e| {
                *q.borrow_mut() = fold(e.text().trim());
                let l = l.borrow();
                l.notify_filter_changed(0);
                cnt.set_label(&count_text(kind, visible_len(&l)));
            });
        }
        let bar = ui::hbox(12);
        bar.set_margin_top(8);
        bar.set_margin_bottom(8);
        bar.set_margin_start(12);
        bar.set_margin_end(12);
        bar.append(&entry);
        bar.append(&count);

        let root = ui::vbox(0);
        root.append(&bar);
        root.append(
            &gtk::ScrolledWindow::builder()
                .child(&list.borrow().view)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .vexpand(true)
                .build(),
        );
        Browse {
            root,
            kind,
            list,
            count,
        }
    }

    /// Rebuild from the current library.
    pub fn reload(&self, ctx: &Ctx) {
        let lib = ctx.lib();
        let groups: Vec<Group> = match self.kind {
            Kind::Artist => lib.artists(),
            Kind::Label => lib.labels(),
            Kind::Composer => lib.composers(),
        };
        let icon = match self.kind {
            Kind::Artist => "avatar-default-symbolic",
            Kind::Label => "building-symbolic",
            Kind::Composer => "music-note-symbolic",
        };
        let mut l = self.list.borrow_mut();
        l.clear();
        l.extend_from_iter(groups.into_iter().map(|g| {
            let subtitle = match self.kind {
                Kind::Composer => {
                    let mut s = format!("{} {}", g.tracks.len(), tr("tracks"));
                    if !g.genres.is_empty() {
                        s.push_str(" · ");
                        s.push_str(
                            &g.genres
                                .iter()
                                .take(3)
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", "),
                        );
                    }
                    s
                }
                _ => format!(
                    "{} {} · {} {}",
                    g.albums.len(),
                    tr("albums"),
                    g.tracks.len(),
                    tr("tracks")
                ),
            };
            GroupItem {
                key: fold(&g.name),
                name: g.name,
                subtitle,
                icon,
            }
        }));
        self.count
            .set_label(&count_text(self.kind, visible_len(&l)));
    }
}

fn visible_len(l: &TypedListView<GroupItem, gtk::NoSelection>) -> u32 {
    l.view.model().map(|m| m.n_items()).unwrap_or(0)
}

fn count_text(kind: Kind, n: u32) -> String {
    let what = match kind {
        Kind::Artist => "artists",
        Kind::Label => "labels",
        Kind::Composer => "composers",
    };
    format!("{n} {}", tr(what))
}
