//! Preferences: connection, library folder, API keys, server/maintenance,
//! music sources and audio devices (local MPD), MPD outputs, appearance.
//!
//! Changes are saved immediately. Connection and API changes need a restart
//! (the clients are created at startup); a banner offers it.

use std::cell::RefCell;
use std::rc::Rc;

use obiplayer_core::config::Config;
use obiplayer_core::meta::{Cache, Keys, Services};
use obiplayer_core::mpd::{Client, ConnectConfig};
use obiplayer_core::{paths, signal, system};
use relm4::adw::{self, prelude::*};
use relm4::gtk::{self, gio, glib};

use crate::ctx::Ctx;
use crate::tr;
use crate::ui;

struct State {
    config: RefCell<Config>,
    restart: adw::Banner,
}

impl State {
    fn save(&self, needs_restart: bool) {
        if let Err(e) = self.config.borrow().save() {
            tracing::warn!(error = %e, "could not save config");
        }
        if needs_restart {
            self.restart.set_revealed(true);
        }
    }
}

fn row_button(label: &str) -> gtk::Button {
    gtk::Button::builder()
        .label(label)
        .valign(gtk::Align::Center)
        .build()
}

fn status_label() -> gtk::Label {
    gtk::Label::builder()
        .css_classes(["caption", "dim-label"])
        .wrap(true)
        .build()
}

pub fn show(parent: &impl IsA<gtk::Widget>, ctx: &Rc<Ctx>) {
    let restart = adw::Banner::builder()
        .title(tr("Restart ObiPlayer to apply connection and API changes"))
        .button_label(tr("Restart now"))
        .build();
    restart.connect_button_clicked(|_| crate::request_restart());
    let st = Rc::new(State {
        config: RefCell::new(Config::load_or_default()),
        restart: restart.clone(),
    });

    let dlg = adw::PreferencesDialog::builder()
        .title(tr("Preferences"))
        .search_enabled(false)
        .build();
    dlg.add(&general_page(ctx, &st));
    dlg.add(&server_page(ctx, &st));
    dlg.add(&audio_page(ctx));
    dlg.add(&appearance_page(&st));
    dlg.present(Some(parent));
}

// -----------------------------------------------------------------------------
// General: connection, library, APIs
// -----------------------------------------------------------------------------

fn general_page(ctx: &Rc<Ctx>, st: &Rc<State>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(tr("General"))
        .icon_name("preferences-system-symbolic")
        .build();
    let cfg = st.config.borrow().clone();
    // Restart notice on top (PreferencesPage::set_banner needs libadwaita 1.7).
    let notice = adw::PreferencesGroup::new();
    notice.add(&st.restart);
    page.add(&notice);

    // -- MPD connection ------------------------------------------------------------
    let conn = adw::PreferencesGroup::builder()
        .title(tr("MPD connection"))
        .description(tr(
            "MPD_HOST / MPD_PORT environment variables take precedence",
        ))
        .build();
    let host = adw::EntryRow::builder()
        .title(tr("Host"))
        .text(&cfg.mpd.host)
        .build();
    let port = adw::SpinRow::with_range(1.0, 65535.0, 1.0);
    port.set_title(&tr("Port"));
    port.set_value(cfg.mpd.port as f64);
    let password = adw::PasswordEntryRow::builder()
        .title(tr("Password (optional)"))
        .text(&cfg.mpd.password)
        .build();
    {
        let s = st.clone();
        host.connect_changed(move |e| {
            s.config.borrow_mut().mpd.host = e.text().trim().to_owned();
            s.save(true);
        });
        let s = st.clone();
        port.connect_value_notify(move |r| {
            s.config.borrow_mut().mpd.port = r.value() as u16;
            s.save(true);
        });
        let s = st.clone();
        password.connect_changed(move |e| {
            s.config.borrow_mut().mpd.password = e.text().to_string();
            s.save(true);
        });
    }
    let test = adw::ActionRow::builder()
        .title(tr("Test connection"))
        .build();
    let result = status_label();
    let btn = row_button(&tr("Test"));
    test.add_suffix(&result);
    test.add_suffix(&btn);
    {
        let s = st.clone();
        btn.connect_clicked(move |b| {
            let c = s.config.borrow();
            let cc = ConnectConfig::new(c.mpd.host.clone(), c.mpd.port)
                .with_password(c.mpd.password.clone());
            let (r, b) = (result.clone(), b.clone());
            b.set_sensitive(false);
            r.set_label(&tr("Connecting…"));
            relm4::spawn_local(async move {
                let res = relm4::spawn(async move {
                    match Client::connect(cc).await {
                        Ok(c) => c.status().await.map(|_| ()).map_err(|e| e.to_string()),
                        Err(e) => Err(e.to_string()),
                    }
                })
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
                b.set_sensitive(true);
                match res {
                    Ok(()) => r.set_label(&format!("✓ {}", tr("Connected"))),
                    Err(e) => r.set_label(&format!("✗ {e}")),
                }
            });
        });
    }
    conn.add(&host);
    conn.add(&port);
    conn.add(&password);
    conn.add(&test);
    page.add(&conn);

    // -- Music library ----------------------------------------------------------------
    let lib = adw::PreferencesGroup::builder()
        .title(tr("Music library on this computer"))
        .description(tr(
            "Local path of MPD's music directory (a mount for a remote server). Used for local cover art, artwork galleries, DR analysis, waveforms, visualizers and technical file details.",
        ))
        .build();
    let dir_row = adw::ActionRow::builder()
        .title(tr("Music folder"))
        .subtitle(
            cfg.library
                .music_dir
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| tr("Not set")),
        )
        .build();
    let choose = row_button(&tr("Choose…"));
    let detect = row_button(&tr("Detect"));
    dir_row.add_suffix(&detect);
    dir_row.add_suffix(&choose);
    {
        let (s, row) = (st.clone(), dir_row.clone());
        choose.connect_clicked(move |b| {
            let fd = gtk::FileDialog::builder()
                .title(tr("MPD music folder"))
                .modal(true)
                .build();
            let (s, row) = (s.clone(), row.clone());
            fd.select_folder(
                b.root().and_downcast_ref::<gtk::Window>(),
                gio::Cancellable::NONE,
                move |r| {
                    if let Ok(f) = r
                        && let Some(p) = f.path()
                    {
                        row.set_subtitle(&p.display().to_string());
                        s.config.borrow_mut().library.music_dir = Some(p);
                        s.save(true);
                    }
                },
            );
        });
        let (s, row, c) = (st.clone(), dir_row.clone(), ctx.clone());
        detect.connect_clicked(move |b| {
            let samples = sample_files(&c);
            let (s, row, b) = (s.clone(), row.clone(), b.clone());
            b.set_sensitive(false);
            row.set_subtitle(&tr("Searching network mounts…"));
            relm4::spawn_local(async move {
                let found = relm4::spawn_blocking(move || system::find_music_dir(&samples))
                    .await
                    .ok()
                    .flatten();
                b.set_sensitive(true);
                match found {
                    Some(p) => {
                        row.set_subtitle(&p.display().to_string());
                        s.config.borrow_mut().library.music_dir = Some(p);
                        s.save(true);
                    }
                    None => row.set_subtitle(&tr(
                        "Not found — mount the server's music share and try again, or choose it",
                    )),
                }
            });
        });
    }
    lib.add(&dir_row);
    page.add(&lib);

    // -- APIs -------------------------------------------------------------------------
    let apis = adw::PreferencesGroup::builder()
        .title(tr("External APIs"))
        .description(tr(
            "Optional. Last.fm adds artist bios, tags and similar artists (last.fm/api/account/create); Discogs adds physical edition details (discogs.com/settings/developers). Keys are stored in your configuration file only.",
        ))
        .build();
    let lfm = adw::PasswordEntryRow::builder()
        .title(tr("Last.fm API key"))
        .text(&cfg.apis.lastfm_api_key)
        .build();
    let dk = adw::PasswordEntryRow::builder()
        .title(tr("Discogs consumer key"))
        .text(&cfg.apis.discogs_key)
        .build();
    let ds = adw::PasswordEntryRow::builder()
        .title(tr("Discogs consumer secret"))
        .text(&cfg.apis.discogs_secret)
        .build();
    for (row, which) in [(&lfm, 0), (&dk, 1), (&ds, 2)] {
        let s = st.clone();
        row.connect_changed(move |e| {
            let v = e.text().trim().to_owned();
            {
                let mut c = s.config.borrow_mut();
                match which {
                    0 => c.apis.lastfm_api_key = v,
                    1 => c.apis.discogs_key = v,
                    _ => c.apis.discogs_secret = v,
                }
            }
            s.save(true);
        });
    }
    let test = adw::ActionRow::builder().title(tr("Test keys")).build();
    let res = status_label();
    let btn = row_button(&tr("Test"));
    test.add_suffix(&res);
    test.add_suffix(&btn);
    {
        let s = st.clone();
        btn.connect_clicked(move |b| {
            let a = s.config.borrow().apis.clone();
            let (res, b) = (res.clone(), b.clone());
            b.set_sensitive(false);
            res.set_label(&tr("Testing…"));
            relm4::spawn_local(async move {
                let msg = relm4::spawn(async move {
                    let svc = Services::new(&Keys::from_config(&a));
                    let lfm = if a.lastfm_api_key.trim().is_empty() {
                        "Last.fm: —".to_owned()
                    } else {
                        match svc.lastfm.validate_key(a.lastfm_api_key.trim()).await {
                            Ok(()) => "Last.fm ✓".into(),
                            Err(e) => format!("Last.fm ✗ {e}"),
                        }
                    };
                    let dg = if a.discogs_key.trim().is_empty() {
                        "Discogs: —".to_owned()
                    } else {
                        match svc
                            .discogs
                            .validate_credentials(a.discogs_key.trim(), a.discogs_secret.trim())
                            .await
                        {
                            Ok(()) => "Discogs ✓".into(),
                            Err(e) => format!("Discogs ✗ {e}"),
                        }
                    };
                    format!("{lfm} · {dg}")
                })
                .await
                .unwrap_or_default();
                b.set_sensitive(true);
                res.set_label(&msg);
            });
        });
    }
    apis.add(&lfm);
    apis.add(&dk);
    apis.add(&ds);
    apis.add(&test);
    page.add(&apis);
    page
}

/// A few files from different top-level folders, to recognise the library.
fn sample_files(ctx: &Rc<Ctx>) -> Vec<String> {
    let lib = ctx.lib();
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for a in &lib.albums {
        let top = a.folder.split('/').next().unwrap_or_default().to_owned();
        if seen.contains(&top) {
            continue;
        }
        if let Some(f) = a.first_file() {
            seen.push(top);
            out.push(f.to_owned());
        }
        if out.len() >= 3 {
            break;
        }
    }
    out
}

// -----------------------------------------------------------------------------
// Server: mpd.conf health, sources, maintenance
// -----------------------------------------------------------------------------

fn server_page(ctx: &Rc<Ctx>, st: &Rc<State>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(tr("Server"))
        .icon_name("network-server-symbolic")
        .build();
    let remote = ctx.client.config().is_remote();

    if remote {
        let g = adw::PreferencesGroup::builder()
            .title(format!("{} {}", tr("MPD is running on"), ctx.client.config().host))
            .description(tr(
                "Music sources and mpd.conf are managed on the server. Playback, queue, outputs and database updates work over the network.",
            ))
            .build();
        page.add(&g);
    } else {
        // mpd.conf health
        let g = adw::PreferencesGroup::builder().title("mpd.conf").build();
        match system::detect_mpd_conf() {
            Some(conf) => {
                g.set_description(Some(&conf.path.display().to_string()));
                for h in system::health(&conf) {
                    let row = adw::ActionRow::builder()
                        .title(h.key)
                        .subtitle(&h.detail)
                        .build();
                    row.add_prefix(&gtk::Image::from_icon_name(if h.ok {
                        "emblem-ok-symbolic"
                    } else {
                        "dialog-warning-symbolic"
                    }));
                    g.add(&row);
                }
                page.add(&g);
                page.add(&sources_group(ctx, &conf));
            }
            None => {
                g.set_description(Some(&tr(
                    "No mpd.conf with a music_directory found. Use the setup assistant to generate one.",
                )));
                page.add(&g);
            }
        }
    }

    // Maintenance
    let m = adw::PreferencesGroup::builder()
        .title(tr("Maintenance"))
        .build();
    let upd = adw::ActionRow::builder()
        .title(tr("Update MPD database"))
        .subtitle(tr("Rescan the music folder for new or changed files"))
        .build();
    let b = row_button(&tr("Update"));
    {
        let c = ctx.clone();
        b.connect_clicked(move |b| {
            let client = c.client.clone();
            b.set_sensitive(false);
            let b = b.clone();
            relm4::spawn_local(async move {
                let _ = relm4::spawn(async move { client.update(None).await }).await;
                b.set_label(&tr("Started"));
            });
        });
    }
    upd.add_suffix(&b);
    m.add(&upd);
    let purge = adw::ActionRow::builder()
        .title(tr("Purge metadata caches"))
        .subtitle(tr(
            "Clears cached genres, covers, thumbnails, avatars and online lookups; they are fetched again with the current keys.",
        ))
        .build();
    let b = row_button(&tr("Purge"));
    b.add_css_class("destructive-action");
    b.connect_clicked(|b| {
        Cache::default().purge();
        for d in ["genres", "covers", "thumbs", "artist-avatars"] {
            let _ = std::fs::remove_dir_all(paths::cache_dir().join(d));
        }
        b.set_label(&tr("Done"));
        b.set_sensitive(false);
    });
    purge.add_suffix(&b);
    m.add(&purge);
    page.add(&m);
    let _ = st;
    page
}

/// Rebuilds the sources list after a change.
type Refill = Box<dyn Fn()>;

fn sources_group(ctx: &Rc<Ctx>, conf: &system::MpdConf) -> adw::PreferencesGroup {
    let g = adw::PreferencesGroup::builder()
        .title(tr("Music sources"))
        .description(tr(
            "Folders linked (as symlinks) inside MPD's music_directory. Removing a source only deletes the link, never your files.",
        ))
        .build();
    let Some(dir) = conf.music_directory.clone() else {
        return g;
    };
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let fill: Rc<RefCell<Option<Refill>>> = Rc::default();
    {
        let (l, d, c, f2) = (list.clone(), dir.clone(), ctx.clone(), fill.clone());
        *fill.borrow_mut() = Some(Box::new(move || {
            l.remove_all();
            for s in system::list_sources(&d) {
                let row = adw::ActionRow::builder()
                    .title(&s.name)
                    .subtitle(
                        s.target
                            .as_ref()
                            .map(|t| t.display().to_string())
                            .unwrap_or_else(|| tr("folder")),
                    )
                    .build();
                if s.target.is_some() {
                    let (d2, n, c2, f3) = (d.clone(), s.name.clone(), c.clone(), f2.clone());
                    let rm = gtk::Button::builder()
                        .icon_name("list-remove-symbolic")
                        .tooltip_text(tr("Remove source (only deletes the symlink)"))
                        .valign(gtk::Align::Center)
                        .css_classes(["flat"])
                        .build();
                    rm.connect_clicked(move |_| {
                        let _ = system::remove_source(&d2, &n);
                        update_db(&c2);
                        if let Some(f) = f3.borrow().as_ref() {
                            f();
                        }
                    });
                    row.add_suffix(&rm);
                }
                l.append(&row);
            }
        }));
    }
    if let Some(f) = fill.borrow().as_ref() {
        f();
    }
    g.add(&list);
    let add = gtk::Button::builder()
        .label(tr("Add source…"))
        .halign(gtk::Align::Start)
        .margin_top(6)
        .build();
    {
        let (d, c, f) = (dir, ctx.clone(), fill.clone());
        add.connect_clicked(move |b| {
            let fd = gtk::FileDialog::builder()
                .title(tr("Folder with music"))
                .modal(true)
                .build();
            let (d, c, f) = (d.clone(), c.clone(), f.clone());
            fd.select_folder(
                b.root().and_downcast_ref::<gtk::Window>(),
                gio::Cancellable::NONE,
                move |r| {
                    if let Ok(file) = r
                        && let Some(p) = file.path()
                    {
                        if let Err(e) = system::add_source(&d, &p) {
                            tracing::warn!(error = %e, "add source failed");
                        }
                        update_db(&c);
                        if let Some(f) = f.borrow().as_ref() {
                            f();
                        }
                    }
                },
            );
        });
    }
    g.add(&add);
    g
}

fn update_db(ctx: &Rc<Ctx>) {
    let client = ctx.client.clone();
    relm4::spawn(async move {
        let _ = client.update(None).await;
    });
}

// -----------------------------------------------------------------------------
// Audio: MPD outputs (+ local sound cards)
// -----------------------------------------------------------------------------

fn audio_page(ctx: &Rc<Ctx>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(tr("Audio"))
        .icon_name("audio-speakers-symbolic")
        .build();

    let outs = adw::PreferencesGroup::builder()
        .title(tr("MPD outputs"))
        .description(tr(
            "Outputs are defined in mpd.conf; here you can enable or disable them.",
        ))
        .build();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    outs.add(&list);
    page.add(&outs);
    load_outputs(ctx, &list);

    if !ctx.client.config().is_remote() {
        let cards = system::sound_cards();
        let g = adw::PreferencesGroup::builder()
            .title(tr("Sound cards"))
            .description(tr(
                "Bit-perfect playback: an ALSA output on the card's hw: device, without mixer or resampling. Restart MPD after editing mpd.conf.",
            ))
            .build();
        let conf = system::detect_mpd_conf();
        for c in cards {
            let max = c.rates.last().copied().unwrap_or(0);
            let mut sub = vec![if c.usb {
                "USB".to_owned()
            } else {
                tr("internal")
            }];
            if max > 0 {
                sub.push(system::quality_tier(max).to_owned());
                sub.push(format!("{} kHz max", max / 1000));
            }
            sub.push(format!("hw:CARD={}", c.id));
            let row = adw::ActionRow::builder()
                .title(&c.name)
                .subtitle(sub.join(" · "))
                .build();
            let configured = conf.as_ref().is_some_and(|cf| {
                cf.outputs
                    .iter()
                    .any(|o| o.device.contains(&c.id) || o.name == c.name)
            });
            if configured {
                row.add_suffix(&ui::label(
                    &tr("Configured in mpd.conf"),
                    &["dim-label", "caption"],
                ));
            } else if let Some(cf) = conf.clone() {
                let b = row_button(&tr("Add to mpd.conf"));
                let (name, id) = (c.name.clone(), c.id.clone());
                b.connect_clicked(move |b| {
                    let block = system::alsa_output_block(&name, &id);
                    let res = std::fs::OpenOptions::new()
                        .append(true)
                        .open(&cf.path)
                        .and_then(|mut f| std::io::Write::write_all(&mut f, block.as_bytes()));
                    b.set_label(&match res {
                        Ok(()) => tr("Added — restart MPD"),
                        Err(e) => format!("✗ {e}"),
                    });
                    b.set_sensitive(false);
                });
                row.add_suffix(&b);
            }
            g.add(&row);
        }
        page.add(&g);
    }
    page
}

fn load_outputs(ctx: &Rc<Ctx>, list: &gtk::ListBox) {
    let (client, l, c) = (ctx.client.clone(), list.clone(), ctx.clone());
    relm4::spawn_local(async move {
        let cl = client.clone();
        let (outs, status) = relm4::spawn(async move {
            (
                cl.outputs().await.unwrap_or_default(),
                cl.status().await.unwrap_or_default(),
            )
        })
        .await
        .unwrap_or_default();
        l.remove_all();
        let main = signal::main_output(&outs).map(|o| o.id);
        let path = signal::evaluate(&status, &outs, "off");
        for o in outs {
            let mut sub = vec![o.plugin.to_uppercase()];
            sub.extend(
                o.attributes
                    .iter()
                    .filter(|(_, v)| !v.is_empty())
                    .map(|(k, v)| format!("{k}: {v}")),
            );
            let row = adw::SwitchRow::builder()
                .title(&o.name)
                .subtitle(sub.join(" · "))
                .active(o.enabled)
                .build();
            if Some(o.id) == main && path.is_bitperfect() {
                row.add_prefix(&ui::badge(&tr("Bit-perfect"), "badge-best"));
            }
            let (client, id, c2, l2) = (client.clone(), o.id, c.clone(), l.clone());
            row.connect_active_notify(move |_| {
                let client = client.clone();
                let (c3, l3) = (c2.clone(), l2.clone());
                relm4::spawn_local(async move {
                    let _ = relm4::spawn(async move { client.toggle_output(id).await }).await;
                    glib::timeout_future(std::time::Duration::from_millis(300)).await;
                    load_outputs(&c3, &l3);
                });
            });
            l.append(&row);
        }
    });
}

// -----------------------------------------------------------------------------
// Appearance
// -----------------------------------------------------------------------------

fn appearance_page(st: &Rc<State>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(tr("Appearance"))
        .icon_name("applications-graphics-symbolic")
        .build();
    let g = adw::PreferencesGroup::new();
    let cfg = st.config.borrow().ui.clone();

    let schemes = gtk::StringList::new(&[&tr("Follow system"), &tr("Light"), &tr("Dark")]);
    let scheme = adw::ComboRow::builder()
        .title(tr("Style"))
        .model(&schemes)
        .selected(
            crate::theme::SCHEMES
                .iter()
                .position(|s| *s == cfg.color_scheme)
                .unwrap_or(0) as u32,
        )
        .build();
    let names: Vec<String> = crate::theme::THEMES
        .iter()
        .map(|(n, _)| {
            if *n == "System" {
                tr("System accent")
            } else {
                tr(n)
            }
        })
        .collect();
    let themes = gtk::StringList::new(&names.iter().map(String::as_str).collect::<Vec<_>>());
    let theme = adw::ComboRow::builder()
        .title(tr("Accent colour"))
        .model(&themes)
        .selected(
            crate::theme::THEMES
                .iter()
                .position(|(n, _)| *n == cfg.theme)
                .unwrap_or(0) as u32,
        )
        .build();
    {
        let s = st.clone();
        scheme.connect_selected_notify(move |r| {
            s.config.borrow_mut().ui.color_scheme =
                crate::theme::SCHEMES[r.selected() as usize].to_owned();
            crate::theme::apply(&s.config.borrow().ui);
            s.save(false);
        });
        let s = st.clone();
        theme.connect_selected_notify(move |r| {
            s.config.borrow_mut().ui.theme =
                crate::theme::THEMES[r.selected() as usize].0.to_owned();
            crate::theme::apply(&s.config.borrow().ui);
            s.save(false);
        });
    }
    let langs = gtk::StringList::new(&[&tr("Follow system"), "English", "Español"]);
    let codes = ["system", "en", "es"];
    let lang = adw::ComboRow::builder()
        .title(tr("Language"))
        .subtitle(tr("Applies after restarting"))
        .model(&langs)
        .selected(codes.iter().position(|c| *c == cfg.language).unwrap_or(0) as u32)
        .build();
    {
        let s = st.clone();
        lang.connect_selected_notify(move |r| {
            s.config.borrow_mut().ui.language = codes[r.selected() as usize].to_owned();
            s.save(true);
        });
    }
    g.add(&scheme);
    g.add(&theme);
    g.add(&lang);
    page.add(&g);
    page
}
