//! Small widget helpers shared by pages.

use std::rc::Rc;

use obiplayer_core::library::Album;
use relm4::adw;
use relm4::gtk::{self, prelude::*};

use crate::ctx::{Action, Ctx, Route};
use crate::tr;

pub fn mmss(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

pub fn label(text: &str, classes: &[&str]) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .css_classes(classes.iter().map(|s| s.to_string()).collect::<Vec<_>>())
        .build()
}

pub fn ellipsized(text: &str, classes: &[&str]) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(classes.iter().map(|s| s.to_string()).collect::<Vec<_>>())
        .build()
}

pub fn vbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}

pub fn hbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}

pub fn icon_button(icon: &str, tip: &str, f: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tr(tip))
        .valign(gtk::Align::Center)
        .css_classes(["flat", "circular"])
        .build();
    b.connect_clicked(move |_| f());
    b
}

/// A flat button that looks like text and acts like a link.
pub fn link(text: &str, classes: &[&str], f: impl Fn() + 'static) -> gtk::Button {
    let l = ellipsized(text, classes);
    let b = gtk::Button::builder()
        .child(&l)
        .halign(gtk::Align::Start)
        .css_classes(["flat", "link-like"])
        .build();
    b.set_cursor_from_name(Some("pointer"));
    b.connect_clicked(move |_| f());
    b
}

/// CSS class for a format badge colour.
pub fn format_class(format: &str) -> &'static str {
    let f = format.to_uppercase();
    if ["DSF", "DFF", "DSD"].iter().any(|x| f.contains(x)) {
        "fmt-dsd"
    } else if f.contains("MQA") {
        "fmt-mqa"
    } else if ["FLAC", "WAV", "AIFF", "ALAC"]
        .iter()
        .any(|x| f.contains(x))
    {
        "fmt-lossless"
    } else if ["MP3", "AAC", "OGG", "OPUS"].iter().any(|x| f.contains(x)) {
        "fmt-lossy"
    } else {
        "fmt-other"
    }
}

pub fn badge(text: &str, class: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .valign(gtk::Align::Center)
        .css_classes(["badge", class])
        .build()
}

pub fn format_badge(format: &str) -> gtk::Label {
    badge(format, format_class(format))
}

/// Section heading.
pub fn heading(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .margin_top(12)
        .css_classes(["title-4"])
        .build()
}

/// Two-column "label: value" grid.
pub struct InfoGrid {
    pub grid: gtk::Grid,
    row: i32,
}

impl InfoGrid {
    pub fn new() -> InfoGrid {
        InfoGrid {
            grid: gtk::Grid::builder()
                .row_spacing(4)
                .column_spacing(16)
                .build(),
            row: 0,
        }
    }

    pub fn widget(&mut self, key: &str, value: &impl IsA<gtk::Widget>) {
        let k = gtk::Label::builder()
            .label(tr(key))
            .xalign(1.0)
            .valign(gtk::Align::Start)
            .width_chars(12)
            .css_classes(["dim-label"])
            .build();
        self.grid.attach(&k, 0, self.row, 1, 1);
        self.grid.attach(value, 1, self.row, 1, 1);
        self.row += 1;
    }

    pub fn text(&mut self, key: &str, value: &str) {
        self.text_impl(key, value, false);
    }

    /// Like `text`, but the value can be selected and copied.
    pub fn copyable(&mut self, key: &str, value: &str) {
        self.text_impl(key, value, true);
    }

    fn text_impl(&mut self, key: &str, value: &str, copyable: bool) {
        if value.is_empty() {
            return;
        }
        let v = label(value, &[]);
        if copyable {
            v.set_selectable(true);
            // Selectable labels grab focus when the page opens and show
            // their text selected; keep them mouse-only.
            v.set_focusable(false);
        }
        // Without a minimum width a wrapping label measures as one word per
        // line, inflating the grid's minimum height.
        v.set_width_chars(24);
        v.set_max_width_chars(80);
        self.widget(key, &v);
    }

    pub fn is_empty(&self) -> bool {
        self.row == 0
    }
}

/// Clickable pills with owned callbacks.
pub fn chip_box(items: Vec<(String, Box<dyn Fn()>)>) -> gtk::FlowBox {
    let fb = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(30)
        .column_spacing(6)
        .row_spacing(6)
        .build();
    for (text, f) in items {
        let b = gtk::Button::builder()
            .label(text.as_str())
            .css_classes(["pill", "chip"])
            .build();
        b.set_cursor_from_name(Some("pointer"));
        b.connect_clicked(move |_| f());
        fb.append(&b);
    }
    fb
}

/// Album cover that fills in asynchronously.
pub fn cover(ctx: &Rc<Ctx>, album: &Album, size: i32) -> gtk::Overlay {
    let placeholder = gtk::Image::builder()
        .icon_name("folder-music-symbolic")
        .pixel_size((size / 3).max(16))
        .css_classes(["dim-label"])
        .build();
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .width_request(size)
        .height_request(size)
        .build();
    let frame = gtk::Overlay::builder()
        .width_request(size)
        .height_request(size)
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Start)
        .overflow(gtk::Overflow::Hidden)
        .css_classes([if size < 64 {
            "cover-small"
        } else {
            "album-frame"
        }])
        .build();
    frame.set_child(Some(&placeholder));
    frame.add_overlay(&picture);
    let (pic, ph) = (picture.clone(), placeholder.clone());
    ctx.covers.request(album, move |t| {
        pic.set_paintable(t);
        ph.set_visible(t.is_none());
    });
    frame
}

/// A wrapped grid of album cells (for artist/producer pages, ≤ a few hundred).
pub fn album_flow(ctx: &Rc<Ctx>, albums: &[u32]) -> gtk::FlowBox {
    let fb = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(2)
        .max_children_per_line(12)
        .column_spacing(8)
        .row_spacing(8)
        .activate_on_single_click(true)
        .build();
    let lib = ctx.lib();
    let mut folders = Vec::new();
    for &i in albums {
        let a = lib.album(i);
        let cell = vbox(4);
        cell.set_width_request(160);
        cell.append(&cover(ctx, a, 160));
        let t = gtk::Label::builder()
            .label(&a.title)
            .lines(2)
            .wrap(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(1)
            .justify(gtk::Justification::Center)
            .css_classes(["caption-heading"])
            .build();
        cell.append(&t);
        let sub = [a.artist.as_str(), a.year().unwrap_or_default()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        let s = gtk::Label::builder()
            .label(sub)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(1)
            .css_classes(["caption", "dim-label"])
            .build();
        cell.append(&s);
        fb.append(&cell);
        folders.push(a.folder.clone());
    }
    let c = ctx.clone();
    fb.connect_child_activated(move |_, child| {
        if let Some(f) = folders.get(child.index() as usize) {
            c.act(Action::Open(Route::Album(f.clone())));
        }
    });
    fb
}

/// Five clickable stars; clicking the current rating clears it.
pub fn stars(ctx: &Rc<Ctx>, file: &str) -> gtk::Box {
    let rating = ctx.rating(file);
    let b = hbox(0);
    b.add_css_class("stars");
    for n in 1..=5u8 {
        let on = n <= rating;
        let btn = gtk::Button::builder()
            .icon_name(if on {
                "starred-symbolic"
            } else {
                "non-starred-symbolic"
            })
            .css_classes(["flat", "star", if on { "star-on" } else { "star-off" }])
            .tooltip_text(format!("{n}/5"))
            .build();
        let (c, f) = (ctx.clone(), file.to_owned());
        btn.connect_clicked(move |_| {
            let new = if n == rating { 0 } else { n };
            c.act(Action::SetRating(f.clone(), new));
        });
        b.append(&btn);
    }
    b
}

/// A page with its own header bar and a scrolled, width-clamped body.
pub fn page(title: &str, tag: &str, body: &impl IsA<gtk::Widget>) -> adw::NavigationPage {
    let clamp = adw::Clamp::builder()
        .maximum_size(1200)
        .tightening_threshold(900)
        .child(body)
        .build();
    // Pages rebuild rows when data arrives (DR, ratings); don't let the
    // focus moving to a new widget scroll the page.
    let viewport = gtk::Viewport::builder()
        .child(&clamp)
        .scroll_to_focus(false)
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .child(&viewport)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&adw::HeaderBar::new());
    tv.set_content(Some(&scroll));
    adw::NavigationPage::builder()
        .title(title)
        .tag(tag)
        .child(&tv)
        .build()
}

pub fn page_body() -> gtk::Box {
    let b = vbox(12);
    b.set_margin_top(12);
    b.set_margin_bottom(24);
    b.set_margin_start(18);
    b.set_margin_end(18);
    b
}

/// Remove all children of a box.
pub fn clear(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
}

/// "DRn" pill in DrPlayer's colour bands.
pub fn dr_badge(dr: i32) -> gtk::Label {
    let l = badge(&format!("DR{dr}"), crate::player_bar::dr_class(dr));
    l.set_width_chars(4);
    l.set_tooltip_text(Some(&tr("Dynamic range (DR14)")));
    l
}

/// Horizontal bar filled to DR/20.
pub fn dr_bar(dr: i32, width: i32) -> gtk::LevelBar {
    let b = gtk::LevelBar::builder()
        .min_value(0.0)
        .max_value(20.0)
        .value((dr as f64).clamp(0.0, 20.0))
        .width_request(width)
        .valign(gtk::Align::Center)
        .css_classes(["dr-bar", crate::player_bar::dr_class(dr)])
        .build();
    // Drop the default low/high/full offsets: colour comes from our class.
    for o in ["low", "high", "full"] {
        b.remove_offset_value(Some(o));
    }
    b
}
