//! "Tracks" browse mode: every track in a sortable, filterable table.

use std::cell::RefCell;
use std::rc::Rc;

use obiplayer_core::text::fold;
use relm4::gtk::{self, glib, prelude::*};
use relm4::typed_view::OrdFn;
use relm4::typed_view::column::{RelmColumn, TypedColumnView};

use crate::ctx::{Action, Ctx, Route};
use crate::tr;
use crate::ui;

pub struct TrackItem {
    file: String,
    title: String,
    artist: String,
    album: String,
    label: String,
    composer: String,
    duration: f64,
    /// Folded "title artist album composer label" for filtering.
    haystack: String,
    keys: [String; 5],
    ctx: Rc<Ctx>,
}

macro_rules! text_column {
    ($name:ident, $title:literal, $field:ident, $key:literal, $expand:literal) => {
        struct $name;
        impl RelmColumn for $name {
            type Root = gtk::Label;
            type Widgets = ();
            type Item = TrackItem;
            const COLUMN_NAME: &'static str = $title;
            const ENABLE_RESIZE: bool = true;
            const ENABLE_EXPAND: bool = $expand;

            fn header_title() -> String {
                tr($title)
            }
            fn setup(_: &gtk::ListItem) -> (gtk::Label, ()) {
                (
                    gtk::Label::builder()
                        .xalign(0.0)
                        .ellipsize(gtk::pango::EllipsizeMode::End)
                        .width_chars(12)
                        .build(),
                    (),
                )
            }
            fn bind(item: &mut TrackItem, _: &mut (), label: &mut gtk::Label) {
                label.set_label(&item.$field);
            }
            fn sort_fn() -> OrdFn<TrackItem> {
                Some(Box::new(|a, b| a.keys[$key].cmp(&b.keys[$key])))
            }
        }
    };
}

text_column!(TitleColumn, "Title", title, 0, true);
text_column!(ArtistColumn, "Artist", artist, 1, false);
text_column!(AlbumColumn, "Album", album, 2, false);
text_column!(LabelColumn, "Label", label, 3, false);
text_column!(ComposerColumn, "Composer", composer, 4, false);

struct DurationColumn;
impl RelmColumn for DurationColumn {
    type Root = gtk::Label;
    type Widgets = ();
    type Item = TrackItem;
    const COLUMN_NAME: &'static str = "Dur.";

    fn header_title() -> String {
        tr("Dur.")
    }
    fn setup(_: &gtk::ListItem) -> (gtk::Label, ()) {
        (
            gtk::Label::builder()
                .xalign(1.0)
                .css_classes(["numeric", "dim-label"])
                .build(),
            (),
        )
    }
    fn bind(item: &mut TrackItem, _: &mut (), label: &mut gtk::Label) {
        label.set_label(&ui::mmss(item.duration));
    }
    fn sort_fn() -> OrdFn<TrackItem> {
        Some(Box::new(|a, b| a.duration.total_cmp(&b.duration)))
    }
}

struct PlayColumn;
impl RelmColumn for PlayColumn {
    type Root = gtk::Button;
    type Widgets = Option<glib::SignalHandlerId>;
    type Item = TrackItem;
    const COLUMN_NAME: &'static str = "";

    fn setup(_: &gtk::ListItem) -> (gtk::Button, Self::Widgets) {
        (
            gtk::Button::builder()
                .icon_name("media-playback-start-symbolic")
                .tooltip_text(tr("Play"))
                .css_classes(["flat", "circular"])
                .build(),
            None,
        )
    }
    fn bind(item: &mut TrackItem, h: &mut Self::Widgets, b: &mut gtk::Button) {
        let (c, f) = (item.ctx.clone(), item.file.clone());
        *h = Some(b.connect_clicked(move |_| c.act(Action::PlayFile(f.clone()))));
    }
    fn unbind(_: &mut TrackItem, h: &mut Self::Widgets, b: &mut gtk::Button) {
        if let Some(id) = h.take() {
            b.disconnect(id);
        }
    }
}

pub struct TracksView {
    pub root: gtk::Box,
    view: Rc<RefCell<TypedColumnView<TrackItem, gtk::NoSelection>>>,
    count: gtk::Label,
}

impl TracksView {
    pub fn new(ctx: &Rc<Ctx>) -> TracksView {
        let mut view: TypedColumnView<TrackItem, gtk::NoSelection> = TypedColumnView::new();
        view.append_column::<PlayColumn>();
        view.append_column::<TitleColumn>();
        view.append_column::<ArtistColumn>();
        view.append_column::<AlbumColumn>();
        view.append_column::<LabelColumn>();
        view.append_column::<ComposerColumn>();
        view.append_column::<DurationColumn>();
        view.view.set_single_click_activate(false);
        view.view.set_show_column_separators(false);
        view.view.add_css_class("data-table");

        let query: Rc<RefCell<String>> = Rc::default();
        {
            let q = query.clone();
            view.add_filter(move |t| {
                let q = q.borrow();
                q.is_empty() || t.haystack.contains(q.as_str())
            });
        }
        let view = Rc::new(RefCell::new(view));
        {
            let (v, c) = (view.clone(), ctx.clone());
            view.borrow().view.connect_activate(move |_, pos| {
                if let Some(item) = v.borrow().get_visible(pos) {
                    c.act(Action::Open(Route::Track(item.borrow().file.clone())));
                }
            });
        }

        let entry = gtk::SearchEntry::builder()
            .placeholder_text(tr("Search tracks…"))
            .hexpand(true)
            .build();
        let count = gtk::Label::builder()
            .css_classes(["dim-label", "caption"])
            .build();
        {
            let (v, q, cnt) = (view.clone(), query.clone(), count.clone());
            entry.connect_search_changed(move |e| {
                *q.borrow_mut() = fold(e.text().trim());
                let v = v.borrow();
                v.notify_filter_changed(0);
                cnt.set_label(&count_text(&v));
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
                .child(&view.borrow().view)
                .vexpand(true)
                .build(),
        );
        TracksView { root, view, count }
    }

    pub fn reload(&self, ctx: &Rc<Ctx>) {
        let lib = ctx.lib();
        let mut v = self.view.borrow_mut();
        v.clear();
        let items = lib.albums.iter().flat_map(|a| {
            a.tracks.iter().map(move |t| {
                let title = t.display_title().to_owned();
                let artist = t.artist().unwrap_or(&a.artist).to_owned();
                let label = t
                    .label()
                    .or(a.label.as_deref())
                    .unwrap_or_default()
                    .to_owned();
                let composer = t.composer().unwrap_or_default().to_owned();
                let keys = [
                    fold(&title),
                    fold(&artist),
                    fold(&a.title),
                    fold(&label),
                    fold(&composer),
                ];
                TrackItem {
                    haystack: keys.join(" "),
                    keys,
                    file: t.file.clone(),
                    title,
                    artist,
                    album: a.title.clone(),
                    label,
                    composer,
                    duration: t.duration.unwrap_or(0.0),
                    ctx: ctx.clone(),
                }
            })
        });
        v.extend_from_iter(items);
        self.count.set_label(&count_text(&v));
    }
}

fn count_text(v: &TypedColumnView<TrackItem, gtk::NoSelection>) -> String {
    let n = v.view.model().map(|m| m.n_items()).unwrap_or(0);
    format!("{n} {}", tr("tracks"))
}
