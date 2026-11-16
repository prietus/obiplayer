//! Album detail.

use std::rc::Rc;
use std::sync::Arc;

use obiplayer_core::catalog::TrackRef;
use obiplayer_core::covers::all_artwork;
use obiplayer_core::library::Album;
use obiplayer_core::quality::{self, Badge};
use relm4::adw;
use relm4::gtk::{self, prelude::*};

use super::Page;
use super::albuminfo;
use crate::covers::load_file_texture;
use crate::ctx::{Action, AlbumFilter, Ctx, Route};
use crate::tr;
use crate::ui::{self, InfoGrid};
use obiplayer_core::meta::release::ReleaseInfo;

pub struct AlbumPage {
    nav: adw::NavigationPage,
    route: Route,
    ctx: Rc<Ctx>,
    album: Arc<Album>,
    album_index: u32,
    tracks: gtk::ListBox,
    dr_box: gtk::Box,
}

/// Credit names as links to the producer page.
fn people_links(ctx: &Rc<Ctx>, names: &[String]) -> gtk::FlowBox {
    let b = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(8)
        .row_spacing(0)
        .column_spacing(4)
        .build();
    for n in names {
        let (c, n2) = (ctx.clone(), n.clone());
        let l = ui::link(n, &[], move || {
            c.act(Action::Open(Route::Producer(n2.clone())))
        });
        if let Some(lab) = l.child().and_downcast::<gtk::Label>() {
            lab.set_ellipsize(gtk::pango::EllipsizeMode::None);
        }
        b.append(&l);
    }
    b
}

impl AlbumPage {
    pub fn new(ctx: &Rc<Ctx>, folder: &str) -> Option<AlbumPage> {
        let lib = ctx.lib();
        let album_index = lib.album_index(folder)?;
        let album = lib.album(album_index).clone();
        let body = ui::page_body();

        // -- Header: text + actions | cover ---------------------------------------
        let left = ui::vbox(6);
        left.set_hexpand(true);
        if let Some(date) = album.date.clone() {
            let c = ctx.clone();
            let d = date.clone();
            left.append(&ui::link(&date, &["dim-label"], move || {
                c.act(Action::Open(Route::Search(d.clone())))
            }));
        }
        let title = ui::label(&album.title, &["title-1"]);
        title.set_width_chars(16);
        left.append(&title);
        if !album.artist.is_empty() {
            let c = ctx.clone();
            let a = album.artist.clone();
            left.append(&ui::link(&album.artist, &["title-3"], move || {
                c.act(Action::Open(Route::Artist(a.clone())))
            }));
        }

        let actions = ui::hbox(6);
        actions.set_margin_top(6);
        actions.set_margin_bottom(6);
        let play = gtk::Button::builder()
            .label(tr("Play"))
            .css_classes(["pill", "suggested-action"])
            .build();
        {
            let (c, f) = (ctx.clone(), album.folder.clone());
            play.connect_clicked(move |_| c.act(Action::PlayAlbum(f.clone())));
        }
        let enqueue = gtk::Button::builder()
            .label(tr("Add to queue"))
            .css_classes(["pill"])
            .build();
        {
            let (c, f) = (ctx.clone(), album.folder.clone());
            enqueue.connect_clicked(move |_| c.act(Action::EnqueueAlbum(f.clone())));
        }
        actions.append(&play);
        actions.append(&enqueue);
        {
            let radio = gtk::Button::builder()
                .icon_name("network-wireless-symbolic")
                .tooltip_text(tr("Radio: play this album, then similar music"))
                .css_classes(["circular"])
                .build();
            let (c, f) = (ctx.clone(), album.folder.clone());
            radio.connect_clicked(move |_| c.act(Action::StartRadioFromAlbum(f.clone())));
            actions.append(&radio);
        }
        if !lib.other_editions(&album).is_empty() {
            let cmp = gtk::Button::builder()
                .label(tr("Compare editions"))
                .css_classes(["pill"])
                .build();
            let (c, f) = (ctx.clone(), album.folder.clone());
            cmp.connect_clicked(move |_| c.act(Action::Open(Route::Editions(f.clone()))));
            actions.append(&cmp);
        }
        actions.append(&more_menu(ctx, &album));
        left.append(&actions);
        let dr_box = ui::hbox(8);
        let grid_box = ui::vbox(8);
        grid_box.append(&info_grid(ctx, &album, &dr_box, None).grid);
        left.append(&grid_box);
        if let Some(p) = albuminfo::read_pressing(ctx, &album) {
            left.append(&albuminfo::pressing_badge(&p));
        }

        // Online information column (fills in asynchronously).
        let info_col = ui::vbox(12);
        info_col.set_hexpand(true);
        info_col.set_width_request(260);
        albuminfo::loading(&info_col);
        {
            let (c, a, col, gb, drb) = (
                ctx.clone(),
                album.clone(),
                info_col.clone(),
                grid_box.clone(),
                dr_box.clone(),
            );
            relm4::spawn_local(async move {
                let info = c.release_info(&a).await;
                albuminfo::fill(&col, &a, &info);
                // Rebuild the data grid with online fields (dr_box moves over).
                if let Some(g) = gb.first_child() {
                    if drb.parent().is_some() {
                        drb.unparent();
                    }
                    gb.remove(&g);
                }
                gb.prepend(&info_grid(&c, &a, &drb, Some(&info)).grid);
            });
        }

        // Data and online info share the width equally; the cover is fixed.
        let columns = ui::hbox(24);
        columns.set_homogeneous(true);
        columns.set_hexpand(true);
        columns.append(&left);
        columns.append(&info_col);
        let header = ui::hbox(24);
        header.append(&columns);
        header.append(&artwork(ctx, &album));
        body.append(&header);

        // -- Genres ----------------------------------------------------------------
        let genres = ctx.genres(&album);
        if !genres.is_empty() {
            let items = genres
                .iter()
                .map(|g| {
                    let (c, g2) = (ctx.clone(), g.clone());
                    (
                        g.to_uppercase(),
                        Box::new(move || c.act(Action::Filter(AlbumFilter::Genre(g2.clone()))))
                            as Box<dyn Fn()>,
                    )
                })
                .collect();
            body.append(&ui::chip_box(items));
        }

        // -- Tracks ------------------------------------------------------------------
        body.append(&ui::heading(&tr("Tracks")));
        let tracks = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        {
            let c = ctx.clone();
            let files: Vec<String> = album.tracks.iter().map(|t| t.file.clone()).collect();
            tracks.connect_row_activated(move |_, row| {
                if let Some(f) = files.get(row.index() as usize) {
                    c.act(Action::Open(Route::Track(f.clone())));
                }
            });
        }
        body.append(&tracks);

        // -- Other editions ----------------------------------------------------------
        let editions = lib.other_editions(&album);
        if !editions.is_empty() {
            body.append(&ui::heading(&tr("Other editions")));
            body.append(&editions_list(ctx, &editions));
        }

        let page = AlbumPage {
            nav: ui::page(
                &album.title,
                &super::tag(&Route::Album(album.folder.clone())),
                &body,
            ),
            route: Route::Album(album.folder.clone()),
            ctx: ctx.clone(),
            album,
            album_index,
            tracks,
            dr_box,
        };
        page.refresh();
        Some(page)
    }
}

impl Page for AlbumPage {
    fn nav(&self) -> &adw::NavigationPage {
        &self.nav
    }

    fn route(&self) -> &Route {
        &self.route
    }

    fn refresh(&self) {
        ui::clear(&self.dr_box);
        match self.ctx.album_dr(&self.album) {
            Some(d) => {
                self.dr_box.append(&ui::dr_badge(d));
                self.dr_box.append(&ui::dr_bar(d, 60));
            }
            None if self.ctx.music_dir.is_some() => {
                // On demand: only the playing track is analysed automatically.
                let b = gtk::Button::builder()
                    .label(tr("Analyse"))
                    .css_classes(["flat", "caption"])
                    .tooltip_text(tr("Compute DR14 for every track of this album"))
                    .build();
                let (c, files) = (
                    self.ctx.clone(),
                    self.album
                        .tracks
                        .iter()
                        .map(|t| t.file.clone())
                        .collect::<Vec<_>>(),
                );
                b.connect_clicked(move |b| {
                    c.act(Action::AnalyzeDr(files.clone()));
                    b.set_label(&tr("Analysing…"));
                    b.set_sensitive(false);
                });
                self.dr_box.append(&b);
            }
            None => self.dr_box.append(&ui::label(
                &tr("Needs local files (music_dir)"),
                &["dim-label", "caption"],
            )),
        }
        self.tracks.remove_all();
        for i in 0..self.album.tracks.len() {
            let r = TrackRef {
                album: self.album_index,
                track: i as u32,
            };
            self.tracks
                .append(&track_row(&self.ctx, r, &self.album.artist));
        }
    }
}

fn info_grid(ctx: &Rc<Ctx>, a: &Album, dr_box: &gtk::Box, info: Option<&ReleaseInfo>) -> InfoGrid {
    let mut g = InfoGrid::new();
    g.text("Duration", &a.formatted_duration());
    g.text("Tracks", &a.tracks.len().to_string());
    g.widget("Dynamic Range", dr_box);
    let fmt = a.format();
    if !fmt.is_empty() {
        let (c, f) = (ctx.clone(), fmt.clone());
        g.widget(
            "Format",
            &ui::link(&fmt, &["monospace"], move || {
                c.act(Action::Filter(AlbumFilter::Format(f.clone())))
            }),
        );
    }
    let label = info
        .and_then(|i| i.label())
        .or_else(|| a.label.clone())
        .filter(|l| !l.is_empty());
    if let Some(l) = label {
        let c = ctx.clone();
        let l2 = l.clone();
        g.widget(
            "Label",
            &ui::link(&l, &[], move || {
                c.act(Action::Open(Route::Label(l2.clone())))
            }),
        );
    }
    if let Some(cat) = info
        .and_then(|i| i.catalog())
        .or_else(|| a.catalog_number.clone())
    {
        let v = ui::label(&cat, &["monospace"]);
        v.set_selectable(true);
        v.set_focusable(false);
        g.widget("Catalog", &v);
    }
    if let Some(cc) = info
        .and_then(|i| i.country())
        .or_else(|| a.country.clone())
        .filter(|c| !c.is_empty())
    {
        let c = ctx.clone();
        let cc2 = cc.clone();
        g.widget(
            "Country",
            &ui::link(&cc, &[], move || {
                c.act(Action::Filter(AlbumFilter::Country(cc2.clone())))
            }),
        );
    }
    if let Some(od) = a
        .original_date
        .as_deref()
        .filter(|d| Some(*d) != a.date.as_deref())
    {
        g.text("Original", od);
    }
    // Credits: MusicBrainz (only for this exact edition), else file tags.
    let mb = info.and_then(|i| i.mb.as_ref().filter(|_| i.mb_matches_edition));
    let first = a.tracks.first();
    let tag = |t: &str, fallback: Option<&str>| {
        first
            .and_then(|s| s.tag(t))
            .map(str::to_owned)
            .or_else(|| fallback.filter(|f| !f.is_empty()).map(str::to_owned))
    };
    let tags = info.map(|i| &i.tags);
    for (key, names, tag_name, from_file) in [
        (
            "Producer",
            mb.map(|m| &m.producers),
            "producer",
            tags.map(|t| t.producer.as_str()),
        ),
        (
            "Engineer",
            mb.map(|m| &m.engineers),
            "engineer",
            tags.map(|t| t.engineer.as_str()),
        ),
        ("Mastering", mb.map(|m| &m.mastering_engineers), "", None),
    ] {
        match names.filter(|n| !n.is_empty()) {
            Some(n) => g.widget(key, &people_links(ctx, n)),
            None => {
                if let Some(v) = (!tag_name.is_empty())
                    .then(|| tag(tag_name, from_file))
                    .flatten()
                {
                    g.text(key, &v);
                }
            }
        }
    }
    let dg_mastering = info
        .and_then(|i| i.dg.as_ref())
        .map(|d| d.mastering_engineer.clone())
        .filter(|m| !m.is_empty());
    if mb.is_none_or(|m| m.mastering_engineers.is_empty())
        && let Some(m) = dg_mastering
    {
        g.text("Mastering", &m);
    }
    if mb.is_none_or(|m| m.engineers.is_empty())
        && let Some(v) = tag("mixer", tags.map(|t| t.mixer.as_str()))
    {
        g.text("Mixer", &v);
    }
    g
}

fn more_menu(ctx: &Rc<Ctx>, a: &Album) -> gtk::MenuButton {
    let pop = gtk::Popover::new();
    let col = ui::vbox(0);
    let item = |label: &str, action: Action| {
        let b = gtk::Button::builder()
            .label(tr(label))
            .css_classes(["flat"])
            .build();
        if let Some(l) = b.child().and_downcast::<gtk::Label>() {
            l.set_xalign(0.0);
        }
        let (c, p) = (ctx.clone(), pop.clone());
        b.connect_clicked(move |_| {
            p.popdown();
            c.act(action.clone());
        });
        b
    };
    col.append(&item("Refresh tags", Action::UpdateDb(a.folder.clone())));
    col.append(&item(
        "Retry cover art",
        Action::RetryCover(a.folder.clone()),
    ));
    if ctx.music_dir.is_some() {
        col.append(&item("Open folder", Action::OpenFolder(a.folder.clone())));
    }
    pop.set_child(Some(&col));
    gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text(tr("More"))
        .popover(&pop)
        .css_classes(["circular"])
        .build()
}

/// Cover, or a carousel of every local image when there are several.
fn artwork(ctx: &Rc<Ctx>, a: &Album) -> gtk::Widget {
    let size = 240;
    let Some(dir) = ctx.local_path(&a.folder) else {
        return ui::cover(ctx, a, size).upcast();
    };
    let carousel = adw::Carousel::builder()
        .width_request(size)
        .height_request(size)
        .allow_scroll_wheel(false)
        .build();
    let dots = adw::CarouselIndicatorDots::builder()
        .carousel(&carousel)
        .build();
    let col = ui::vbox(4);
    col.set_valign(gtk::Align::Start);
    col.append(&ui::cover(ctx, a, size));
    let (col2, car, dots2) = (col.clone(), carousel.clone(), dots.clone());
    relm4::spawn_local(async move {
        let images = relm4::gtk::gio::spawn_blocking(move || all_artwork(&dir))
            .await
            .unwrap_or_default();
        if images.len() < 2 {
            return;
        }
        // Replace the single cover by the carousel.
        while let Some(ch) = col2.first_child() {
            col2.remove(&ch);
        }
        col2.append(&car);
        col2.append(&dots2);
        for path in images {
            let pic = gtk::Picture::builder()
                .content_fit(gtk::ContentFit::Contain)
                .width_request(size)
                .height_request(size)
                .tooltip_text(
                    path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                )
                .build();
            car.append(&pic);
            let p = pic.clone();
            relm4::spawn_local(async move {
                if let Some(t) = load_file_texture(path, 600).await {
                    p.set_paintable(Some(&t));
                }
            });
        }
    });
    col.upcast()
}

fn editions_list(ctx: &Rc<Ctx>, editions: &[u32]) -> gtk::ListBox {
    let lib = ctx.lib();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let mut folders = Vec::new();
    for &i in editions {
        let a = lib.album(i);
        let row = ui::hbox(12);
        row.set_margin_top(6);
        row.set_margin_bottom(6);
        row.set_margin_start(8);
        row.set_margin_end(8);
        row.append(&ui::cover(ctx, a, 40));
        let text = ui::vbox(2);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        text.append(&ui::ellipsized(&a.title, &["heading"]));
        let sub = [
            a.date.clone().unwrap_or_default(),
            a.label.clone().unwrap_or_default(),
            format!("{} {}", a.tracks.len(), tr("tracks")),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
        text.append(&ui::ellipsized(&sub, &["dim-label", "caption"]));
        row.append(&text);
        row.append(&ui::format_badge(&a.format()));
        let (c, f) = (ctx.clone(), a.folder.clone());
        row.append(&ui::icon_button(
            "media-playback-start-symbolic",
            "Play",
            move || c.act(Action::PlayAlbum(f.clone())),
        ));
        let (c, f) = (ctx.clone(), a.folder.clone());
        row.append(&ui::icon_button(
            "list-add-symbolic",
            "Add to queue",
            move || c.act(Action::EnqueueAlbum(f.clone())),
        ));
        list.append(&row);
        folders.push(a.folder.clone());
    }
    let c = ctx.clone();
    list.connect_row_activated(move |_, row| {
        if let Some(f) = folders.get(row.index() as usize) {
            c.act(Action::Open(Route::Album(f.clone())));
        }
    });
    list
}

/// One track in an album's list.
pub fn track_row(ctx: &Rc<Ctx>, r: TrackRef, album_artist: &str) -> gtk::Box {
    let lib = ctx.lib();
    let song = lib.song(r);
    let current = ctx.is_current(&song.file);

    let row = ui::hbox(8);
    row.set_margin_top(4);
    row.set_margin_bottom(4);
    row.set_margin_start(6);
    row.set_margin_end(6);
    if current {
        row.add_css_class("current-track");
    }

    let (c, f) = (ctx.clone(), song.file.clone());
    let play = ui::icon_button(
        if current {
            "audio-speakers-symbolic"
        } else {
            "media-playback-start-symbolic"
        },
        "Play",
        move || c.act(Action::PlayFile(f.clone())),
    );
    if current {
        play.add_css_class("accent");
    }
    row.append(&play);

    let number = song
        .track()
        .map(|t| t.split('/').next().unwrap_or(t).to_owned())
        .unwrap_or_else(|| (r.track + 1).to_string());
    row.append(
        &gtk::Label::builder()
            .label(number)
            .width_chars(3)
            .xalign(1.0)
            .css_classes(["dim-label", "numeric"])
            .build(),
    );

    let text = ui::vbox(0);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    text.append(&ui::ellipsized(
        song.display_title(),
        if current { &["heading"] } else { &[] },
    ));
    if let Some(a) = song.artist().filter(|a| *a != album_artist) {
        text.append(&ui::ellipsized(a, &["dim-label", "caption"]));
    }
    row.append(&text);

    let versions = lib.version_count(r);
    if versions > 0 {
        let pop = gtk::Popover::new();
        let (c, p) = (ctx.clone(), pop.clone());
        pop.connect_show(move |_| p.set_child(Some(&versions_popover(&c, r, &p))));
        let mb = gtk::MenuButton::builder()
            .popover(&pop)
            .valign(gtk::Align::Center)
            .tooltip_text(tr("Compare versions"))
            .css_classes(["flat", "versions-button"])
            .build();
        let inner = ui::hbox(4);
        inner.append(&gtk::Image::from_icon_name("edit-copy-symbolic"));
        inner.append(&gtk::Label::new(Some(&versions.to_string())));
        mb.set_child(Some(&inner));
        row.append(&mb);
    }

    let (c, f) = (ctx.clone(), song.file.clone());
    row.append(&ui::icon_button(
        "list-add-symbolic",
        "Add to queue",
        move || c.act(Action::EnqueueFile(f.clone())),
    ));
    let album_title = lib.album(r.album).title.clone();
    match ctx.dr(&song.file) {
        Some(d) => row.append(&ui::dr_compare_button(ctx, album_artist, &album_title, d)),
        None => row.append(
            &gtk::Label::builder()
                .width_chars(4)
                .css_classes(["badge"])
                .build(),
        ),
    }
    row.append(&ui::stars(ctx, &song.file));
    row.append(
        &gtk::Label::builder()
            .label(song.duration.map(ui::mmss).unwrap_or_default())
            .width_chars(5)
            .xalign(1.0)
            .css_classes(["dim-label", "numeric"])
            .build(),
    );
    row
}

/// "Compare versions": every version of a track, best first.
fn versions_popover(ctx: &Rc<Ctx>, r: TrackRef, pop: &gtk::Popover) -> gtk::Widget {
    let lib = ctx.lib();
    let refs = lib.versions(r).to_vec();
    let items: Vec<(Option<i32>, String)> = refs
        .iter()
        .map(|v| (ctx.dr(&lib.song(*v).file), lib.album(v.album).format()))
        .collect();
    let ranked = quality::rank(
        &items
            .iter()
            .map(|(d, f)| (*d, f.as_str()))
            .collect::<Vec<_>>(),
    );
    let preferred = ctx.preferred.borrow();

    let col = ui::vbox(6);
    col.set_margin_top(6);
    col.set_margin_bottom(6);
    col.set_margin_start(6);
    col.set_margin_end(6);
    col.append(&ui::label(&tr("Compare versions"), &["heading"]));
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    for (i, score) in ranked {
        let v = refs[i];
        let a = lib.album(v.album);
        let s = lib.song(v);
        let row = ui::hbox(8);
        row.set_margin_top(4);
        row.set_margin_bottom(4);
        row.set_margin_start(6);
        row.set_margin_end(6);
        match score.badge {
            Badge::Best => row.add_css_class("row-best"),
            Badge::Warning => row.add_css_class("row-warning"),
            Badge::Good => {}
        }
        let (c, f) = (ctx.clone(), s.file.clone());
        row.append(&ui::icon_button(
            "media-playback-start-symbolic",
            "Play",
            move || c.act(Action::PlayFile(f.clone())),
        ));
        let (c, fo, p) = (ctx.clone(), a.folder.clone(), pop.clone());
        let title = ui::link(
            &a.title,
            if score.badge == Badge::Best {
                &["heading"]
            } else {
                &[]
            },
            move || {
                p.popdown();
                c.act(Action::Open(Route::Album(fo.clone())))
            },
        );
        title.set_hexpand(true);
        row.append(&title);
        if score.badge == Badge::Best {
            row.append(&ui::badge(&tr("BEST"), "badge-best"));
        }
        if preferred.contains(&s.file) {
            row.append(&ui::badge(&tr("PREFERRED"), "badge-preferred"));
        }
        if v == r {
            row.append(&ui::badge(&tr("CURRENT"), "badge-current"));
        }
        row.append(&ui::format_badge(&a.format()));
        if let Some(d) = &a.date {
            row.append(&ui::ellipsized(d, &["dim-label", "caption"]));
        }
        match ctx.dr(&s.file) {
            Some(d) => row.append(&ui::dr_badge(d)),
            None => row.append(&ui::ellipsized("DR?", &["dim-label", "caption"])),
        }
        row.append(&ui::ellipsized(
            &format!("{}pts", score.total),
            &["dim-label", "caption", "numeric"],
        ));
        let (c, f, p) = (ctx.clone(), s.file.clone(), pop.clone());
        row.append(&ui::icon_button(
            "emblem-ok-symbolic",
            "Set as preferred version",
            move || {
                p.popdown();
                c.act(Action::SetPreferred(f.clone()))
            },
        ));
        list.append(&row);
    }
    col.append(&list);
    col.upcast()
}
