//! First-run setup assistant.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use obiplayer_core::config::Config;
use obiplayer_core::mpd::{Client, ConnectConfig, Entry};
use obiplayer_core::system;
use relm4::adw::{self, prelude::*};
use relm4::gtk::{self, gio};

use crate::tr;

struct Wiz {
    config: RefCell<Config>,
    local: RefCell<bool>,
}

fn button(label: &str) -> gtk::Button {
    gtk::Button::builder()
        .label(label)
        .valign(gtk::Align::Center)
        .build()
}

/// Up to three files from different top-level folders of the server, found
/// by walking `lsinfo` (cheap, unlike listallinfo).
async fn server_samples(cc: ConnectConfig) -> Vec<String> {
    let Ok(c) = Client::connect(cc).await else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let Ok(top) = c.lsinfo("").await else {
        return out;
    };
    for e in top.into_iter().take(6) {
        let Entry::Directory { path, .. } = e else {
            continue;
        };
        let mut dir = path;
        for _ in 0..6 {
            let Ok(items) = c.lsinfo(&dir).await else {
                break;
            };
            if let Some(Entry::Song(s)) = items.iter().find(|i| matches!(i, Entry::Song(_))) {
                out.push(s.file.clone());
                break;
            }
            match items.into_iter().find_map(|i| match i {
                Entry::Directory { path, .. } => Some(path),
                _ => None,
            }) {
                Some(d) => dir = d,
                None => break,
            }
        }
        if out.len() >= 3 {
            break;
        }
    }
    out
}

pub fn show(parent: &impl IsA<gtk::Widget>) {
    let detected = system::detect_mpd_conf();
    let mut cfg = Config::load_or_default();
    if let Some(c) = &detected {
        if let Some(h) = &c.bind_to_address {
            cfg.mpd.host = h.clone();
        }
        if let Some(p) = c.port {
            cfg.mpd.port = p;
        }
        cfg.library.music_dir = c.music_directory.clone();
    }
    let w = Rc::new(Wiz {
        local: RefCell::new(!ConnectConfig::new(cfg.mpd.host.clone(), cfg.mpd.port).is_remote()),
        config: RefCell::new(cfg),
    });

    let page = adw::PreferencesPage::new();

    // 1. Where
    let where_g = adw::PreferencesGroup::builder()
        .title(tr("1. Where does MPD run?"))
        .build();
    let where_row = adw::ComboRow::builder()
        .title(tr("MPD server"))
        .model(&gtk::StringList::new(&[
            &tr("On this computer"),
            &tr("On another machine"),
        ]))
        .selected(if *w.local.borrow() { 0 } else { 1 })
        .build();
    where_g.add(&where_row);
    page.add(&where_g);

    // 2. Prerequisites
    let deps_g = adw::PreferencesGroup::builder()
        .title(tr("2. Prerequisites"))
        .build();
    let deps = system::dependencies();
    let missing: Vec<&str> = deps
        .iter()
        .filter(|d| d.path.is_none())
        .map(|d| d.name)
        .collect();
    for d in &deps {
        let row = adw::ActionRow::builder()
            .title(d.name)
            .subtitle(
                d.path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| tr("not found")),
            )
            .build();
        row.add_prefix(&gtk::Image::from_icon_name(if d.path.is_some() {
            "emblem-ok-symbolic"
        } else {
            "dialog-warning-symbolic"
        }));
        deps_g.add(&row);
    }
    if !missing.is_empty() {
        deps_g.set_description(Some(&format!(
            "{}\n{}",
            tr("Install the missing programs (mpd is only needed for a local server):"),
            system::install_hint(&missing)
        )));
    }
    page.add(&deps_g);

    // 3. Connection
    let conn_g = adw::PreferencesGroup::builder()
        .title(tr("3. Connection"))
        .build();
    let host = adw::EntryRow::builder()
        .title(tr("Host"))
        .text(&w.config.borrow().mpd.host)
        .build();
    let port = adw::SpinRow::with_range(1.0, 65535.0, 1.0);
    port.set_title(&tr("Port"));
    port.set_value(w.config.borrow().mpd.port as f64);
    let test_row = adw::ActionRow::builder()
        .title(tr("Test connection"))
        .build();
    let test_res = gtk::Label::builder()
        .css_classes(["caption", "dim-label"])
        .build();
    let test_btn = button(&tr("Test"));
    test_row.add_suffix(&test_res);
    test_row.add_suffix(&test_btn);
    {
        let w2 = w.clone();
        host.connect_changed(move |e| w2.config.borrow_mut().mpd.host = e.text().trim().to_owned());
        let w2 = w.clone();
        port.connect_value_notify(move |r| w2.config.borrow_mut().mpd.port = r.value() as u16);
        let w2 = w.clone();
        test_btn.connect_clicked(move |b| {
            let c = w2.config.borrow();
            let cc = ConnectConfig::new(c.mpd.host.clone(), c.mpd.port);
            let (r, b) = (test_res.clone(), b.clone());
            b.set_sensitive(false);
            relm4::spawn_local(async move {
                let ok = relm4::spawn(async move {
                    Client::connect(cc)
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                })
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
                b.set_sensitive(true);
                r.set_label(&match ok {
                    Ok(()) => format!("✓ {}", tr("Connected")),
                    Err(e) => format!("✗ {e}"),
                });
            });
        });
    }
    conn_g.add(&host);
    conn_g.add(&port);
    conn_g.add(&test_row);
    page.add(&conn_g);

    // 4a. Local: mpd.conf
    let conf_g = adw::PreferencesGroup::builder()
        .title(tr("4. MPD configuration"))
        .build();
    match &detected {
        Some(c) => {
            let row = adw::ActionRow::builder()
                .title(tr("mpd.conf detected"))
                .subtitle(c.path.display().to_string())
                .build();
            row.add_prefix(&gtk::Image::from_icon_name("emblem-ok-symbolic"));
            conf_g.add(&row);
        }
        None => {
            conf_g.set_description(Some(&tr(
                "No mpd.conf found. Generate a basic bit-perfect one in ~/.config/mpd (ALSA output on your card, no mixer, no resampling, plus a FIFO for the visualizer).",
            )));
            let cards = system::sound_cards();
            let names: Vec<String> = std::iter::once(tr("Default (PipeWire)"))
                .chain(cards.iter().map(|c| c.name.clone()))
                .collect();
            let card_row = adw::ComboRow::builder()
                .title(tr("Output"))
                .model(&gtk::StringList::new(
                    &names.iter().map(String::as_str).collect::<Vec<_>>(),
                ))
                .selected(
                    cards
                        .iter()
                        .position(|c| c.usb)
                        .map(|i| i as u32 + 1)
                        .unwrap_or(0),
                )
                .build();
            let gen_row = adw::ActionRow::builder()
                .title(tr("Generate mpd.conf"))
                .subtitle(tr("Uses the music folder chosen below"))
                .build();
            let gen_btn = button(&tr("Generate"));
            gen_row.add_suffix(&gen_btn);
            {
                let (w2, cr, gr) = (w.clone(), card_row.clone(), gen_row.clone());
                gen_btn.connect_clicked(move |b| {
                    let c = w2.config.borrow();
                    let Some(dir) = c.library.music_dir.clone() else {
                        gr.set_subtitle(&tr("Choose the music folder first"));
                        return;
                    };
                    let card = (cr.selected() > 0)
                        .then(|| cards.get(cr.selected() as usize - 1))
                        .flatten();
                    let text = system::default_mpd_conf(
                        &dir,
                        &c.mpd.host,
                        c.mpd.port,
                        card.map(|k| (k.name.as_str(), k.id.as_str())),
                    );
                    match system::install_mpd_conf(&text, &dir) {
                        Ok(p) => {
                            gr.set_subtitle(&format!(
                                "{} {} — {}",
                                tr("Created at"),
                                p.display(),
                                tr("start MPD with: systemctl --user enable --now mpd")
                            ));
                            b.set_sensitive(false);
                        }
                        Err(e) => gr.set_subtitle(&format!("✗ {e}")),
                    }
                });
            }
            conf_g.add(&card_row);
            conf_g.add(&gen_row);
        }
    }
    page.add(&conf_g);

    // 5. Music folder
    let lib_g = adw::PreferencesGroup::builder()
        .title(tr("5. Music folder on this computer"))
        .description(tr(
            "MPD's music directory as seen from here — for a remote server, where its share is mounted (NFS/SMB). Optional, but needed for local covers, DR analysis, waveforms and visualizers.",
        ))
        .build();
    let dir_row = adw::ActionRow::builder()
        .title(tr("Music folder"))
        .subtitle(
            w.config
                .borrow()
                .library
                .music_dir
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| tr("Not set")),
        )
        .build();
    let choose = button(&tr("Choose…"));
    let detect = button(&tr("Detect"));
    dir_row.add_suffix(&detect);
    dir_row.add_suffix(&choose);
    let set_dir = {
        let (w2, row) = (w.clone(), dir_row.clone());
        Rc::new(move |p: PathBuf| {
            row.set_subtitle(&p.display().to_string());
            w2.config.borrow_mut().library.music_dir = Some(p);
        })
    };
    {
        let sd = set_dir.clone();
        choose.connect_clicked(move |b| {
            let fd = gtk::FileDialog::builder()
                .title(tr("Music folder"))
                .modal(true)
                .build();
            let sd = sd.clone();
            fd.select_folder(
                b.root().and_downcast_ref::<gtk::Window>(),
                gio::Cancellable::NONE,
                move |r| {
                    if let Ok(f) = r
                        && let Some(p) = f.path()
                    {
                        sd(p);
                    }
                },
            );
        });
        let (w2, sd, row) = (w.clone(), set_dir.clone(), dir_row.clone());
        detect.connect_clicked(move |b| {
            let c = w2.config.borrow();
            let cc = ConnectConfig::new(c.mpd.host.clone(), c.mpd.port);
            let (sd, row, b) = (sd.clone(), row.clone(), b.clone());
            b.set_sensitive(false);
            row.set_subtitle(&tr("Searching network mounts…"));
            relm4::spawn_local(async move {
                let samples = relm4::spawn(server_samples(cc)).await.unwrap_or_default();
                let found = relm4::spawn_blocking(move || system::find_music_dir(&samples))
                    .await
                    .ok()
                    .flatten();
                b.set_sensitive(true);
                match found {
                    Some(p) => sd(p),
                    None => row.set_subtitle(&tr(
                        "Not found — mount the server's music share first, or choose it",
                    )),
                }
            });
        });
    }
    lib_g.add(&dir_row);
    page.add(&lib_g);

    {
        let (w2, cg) = (w.clone(), conf_g.clone());
        let update = move |local: bool| {
            *w2.local.borrow_mut() = local;
            cg.set_visible(local);
        };
        let local_now = *w.local.borrow();
        update(local_now);
        let (h, w3) = (host.clone(), w.clone());
        where_row.connect_selected_notify(move |r| {
            let local = r.selected() == 0;
            if local {
                h.set_text("localhost");
            } else if !ConnectConfig::new(w3.config.borrow().mpd.host.clone(), 6600).is_remote() {
                h.set_text("");
            }
            update(local);
        });
    }

    // Header with Start
    let start = gtk::Button::builder()
        .label(tr("Start"))
        .css_classes(["suggested-action"])
        .build();
    let header = adw::HeaderBar::builder()
        .show_end_title_buttons(false)
        .show_start_title_buttons(false)
        .title_widget(&adw::WindowTitle::new(&tr("Set up ObiPlayer"), ""))
        .build();
    header.pack_end(&start);
    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&header);
    tv.set_content(Some(&page));
    let dlg = adw::Dialog::builder()
        .child(&tv)
        .content_width(640)
        .content_height(760)
        .can_close(false)
        .build();
    {
        let w2 = w.clone();
        start.connect_clicked(move |_| {
            if let Err(e) = w2.config.borrow().save() {
                tracing::warn!(error = %e, "could not save config");
            }
            crate::request_restart();
        });
    }
    dlg.present(Some(parent));
}
