//! Album grid cells.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use obiplayer_core::library::Album;
use relm4::gtk::{self, prelude::*};
use relm4::typed_view::grid::RelmGridItem;

use crate::covers::CoverLoader;

pub const CELL: i32 = 180;

#[derive(Clone)]
pub struct AlbumItem {
    pub album: Arc<Album>,
    pub playing: bool,
    pub covers: Rc<CoverLoader>,
}

pub struct AlbumWidgets {
    picture: gtk::Picture,
    placeholder: gtk::Image,
    badge: gtk::Image,
    title: gtk::Label,
    artist: gtk::Label,
    /// Folder currently bound, so late cover results don't land in a
    /// recycled cell.
    bound: Rc<RefCell<String>>,
}

impl RelmGridItem for AlbumItem {
    type Root = gtk::Box;
    type Widgets = AlbumWidgets;

    fn setup(_item: &gtk::ListItem) -> (gtk::Box, AlbumWidgets) {
        let picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Cover)
            .width_request(CELL)
            .height_request(CELL)
            .css_classes(["album-cover"])
            .build();
        let placeholder = gtk::Image::builder()
            .icon_name("folder-music-symbolic")
            .pixel_size(64)
            .css_classes(["dim-label"])
            .build();
        let badge = gtk::Image::builder()
            .icon_name("audio-speakers-symbolic")
            .halign(gtk::Align::End)
            .valign(gtk::Align::End)
            .margin_end(8)
            .margin_bottom(8)
            .css_classes(["playing-badge"])
            .visible(false)
            .build();
        let frame = gtk::Overlay::builder()
            .width_request(CELL)
            .height_request(CELL)
            .css_classes(["album-frame"])
            .overflow(gtk::Overflow::Hidden)
            .build();
        frame.set_child(Some(&placeholder));
        frame.add_overlay(&picture);
        frame.add_overlay(&badge);

        let title = gtk::Label::builder()
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .lines(2)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .justify(gtk::Justification::Center)
            .max_width_chars(1)
            .hexpand(true)
            .css_classes(["heading", "caption-heading"])
            .build();
        let artist = gtk::Label::builder()
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(1)
            .hexpand(true)
            .css_classes(["dim-label", "caption"])
            .build();

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(8)
            .margin_end(8)
            .width_request(CELL)
            .build();
        root.append(&frame);
        root.append(&title);
        root.append(&artist);

        let widgets = AlbumWidgets {
            picture,
            placeholder,
            badge,
            title,
            artist,
            bound: Rc::default(),
        };
        (root, widgets)
    }

    fn bind(&mut self, w: &mut AlbumWidgets, root: &mut gtk::Box) {
        let a = &self.album;
        w.title.set_label(&a.title);
        w.artist.set_label(&a.artist);
        w.artist.set_visible(!a.artist.is_empty());
        w.badge.set_visible(self.playing);
        root.set_tooltip_text(Some(&format!("{} — {}", a.artist, a.title)));
        *w.bound.borrow_mut() = a.folder.clone();

        match self.covers.cached(&a.folder) {
            Some(t) => show(&w.picture, &w.placeholder, t.as_ref()),
            None => {
                show(&w.picture, &w.placeholder, None);
                let (pic, ph, bound, folder) = (
                    w.picture.clone(),
                    w.placeholder.clone(),
                    w.bound.clone(),
                    a.folder.clone(),
                );
                self.covers.request(a, move |t| {
                    if *bound.borrow() == folder {
                        show(&pic, &ph, t);
                    }
                });
            }
        }
    }

    fn unbind(&mut self, w: &mut AlbumWidgets, _root: &mut gtk::Box) {
        w.bound.borrow_mut().clear();
        w.picture.set_paintable(None::<&gtk::gdk::Paintable>);
    }
}

fn show(picture: &gtk::Picture, placeholder: &gtk::Image, t: Option<&gtk::gdk::Texture>) {
    picture.set_paintable(t);
    placeholder.set_visible(t.is_none());
}
