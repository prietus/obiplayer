//! Play queue rows.

use obiplayer_core::mpd::Song;
use relm4::gtk::{self, glib, prelude::*};
use relm4::typed_view::list::RelmListItem;

#[derive(Clone)]
pub struct QueueItem {
    pub song: Song,
    pub current: bool,
    /// Called with the queue position when the row's remove button is pressed.
    pub on_remove: relm4::Sender<u32>,
}

pub struct QueueWidgets {
    marker: gtk::Stack,
    pos: gtk::Label,
    title: gtk::Label,
    artist: gtk::Label,
    remove: gtk::Button,
    handler: Option<glib::SignalHandlerId>,
}

impl RelmListItem for QueueItem {
    type Root = gtk::Box;
    type Widgets = QueueWidgets;

    fn setup(_item: &gtk::ListItem) -> (gtk::Box, QueueWidgets) {
        let pos = gtk::Label::builder()
            .css_classes(["dim-label", "numeric", "caption"])
            .xalign(1.0)
            .build();
        let marker = gtk::Stack::builder().width_request(28).build();
        marker.add_named(&pos, Some("pos"));
        marker.add_named(
            &gtk::Image::builder()
                .icon_name("audio-speakers-symbolic")
                .css_classes(["accent"])
                .build(),
            Some("current"),
        );

        let title = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        let artist = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["dim-label", "caption"])
            .build();
        let text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        text.append(&title);
        text.append(&artist);

        let remove = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text("Remove from queue")
            .css_classes(["flat", "circular"])
            .build();

        let root = gtk::Box::builder()
            .spacing(10)
            .margin_top(4)
            .margin_bottom(4)
            .margin_start(8)
            .margin_end(4)
            .build();
        root.append(&marker);
        root.append(&text);
        root.append(&remove);

        (
            root,
            QueueWidgets {
                marker,
                pos,
                title,
                artist,
                remove,
                handler: None,
            },
        )
    }

    fn bind(&mut self, w: &mut QueueWidgets, _root: &mut gtk::Box) {
        let pos = self.song.pos.unwrap_or(0);
        w.pos.set_label(&(pos + 1).to_string());
        w.marker
            .set_visible_child_name(if self.current { "current" } else { "pos" });
        w.title.set_label(self.song.display_title());
        if self.current {
            w.title.add_css_class("heading");
        } else {
            w.title.remove_css_class("heading");
        }
        let artist = self.song.artist().unwrap_or_default();
        w.artist.set_label(artist);
        w.artist.set_visible(!artist.is_empty());

        let tx = self.on_remove.clone();
        w.handler = Some(w.remove.connect_clicked(move |_| tx.emit(pos)));
    }

    fn unbind(&mut self, w: &mut QueueWidgets, _root: &mut gtk::Box) {
        if let Some(h) = w.handler.take() {
            w.remove.disconnect(h);
        }
    }
}
