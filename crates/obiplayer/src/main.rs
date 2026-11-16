//! ObiPlayer — GTK front-end.
//!
//! Phase-1 smoke test: one window that follows MPD through `idle` and offers
//! basic transport controls. It establishes the threading pattern the real UI
//! will use: Tokio runs network I/O on its own threads, GTK stays on the main
//! thread, and results come back through futures awaited on the GLib loop.

use std::future::Future;
use std::sync::OnceLock;

use adw::prelude::*;
use gtk::glib;
use obiplayer_core::mpd::{self, Client, ConnectConfig, Event, Subsystem};

const APP_ID: &str = "us.priet.ObiPlayer";

type BoxedCmd = std::pin::Pin<Box<dyn Future<Output = mpd::Result<()>> + Send>>;

fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

/// Run `fut` on Tokio and await its result from the GLib main loop.
async fn on_tokio<F, T>(fut: F) -> T
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    runtime().spawn(fut).await.expect("tokio task panicked")
}

fn main() -> glib::ExitCode {
    tracing_subscriber::fmt::init();
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_window);
    app.run()
}

#[derive(Clone)]
struct Ui {
    page: adw::StatusPage,
    status: gtk::Label,
    play: gtk::Button,
}

fn build_window(app: &adw::Application) {
    let cfg = ConnectConfig::from_env();
    let client = Client::new(cfg.clone());

    let page = adw::StatusPage::builder()
        .icon_name("audio-x-generic-symbolic")
        .title("ObiPlayer")
        .vexpand(true)
        .build();

    let prev = gtk::Button::from_icon_name("media-skip-backward-symbolic");
    let play = gtk::Button::from_icon_name("media-playback-start-symbolic");
    let next = gtk::Button::from_icon_name("media-skip-forward-symbolic");
    play.add_css_class("circular");
    play.add_css_class("suggested-action");
    let transport = gtk::Box::builder()
        .spacing(12)
        .halign(gtk::Align::Center)
        .margin_bottom(24)
        .build();
    transport.append(&prev);
    transport.append(&play);
    transport.append(&next);

    let status = gtk::Label::builder()
        .css_classes(["dim-label", "caption"])
        .margin_bottom(12)
        .build();

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&page);
    content.append(&transport);
    content.append(&status);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&content));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("ObiPlayer")
        .default_width(900)
        .default_height(700)
        .width_request(600)
        .height_request(500)
        .content(&toolbar)
        .build();

    let ui = Ui {
        page,
        status,
        play: play.clone(),
    };
    ui.status
        .set_label(&format!("Conectando a {}:{}…", cfg.host, cfg.port));

    let action = |f: fn(Client) -> BoxedCmd| {
        let client = client.clone();
        move |_: &gtk::Button| {
            let client = client.clone();
            glib::spawn_future_local(async move {
                if let Err(e) = on_tokio(f(client)).await {
                    tracing::warn!(error = %e, "MPD command failed");
                }
            });
        }
    };
    prev.connect_clicked(action(|c| Box::pin(async move { c.previous().await })));
    play.connect_clicked(action(|c| Box::pin(async move { c.toggle_pause().await })));
    next.connect_clicked(action(|c| Box::pin(async move { c.next().await })));

    // Server events → UI. The watcher stops when the receiver is dropped.
    let (tx, rx) = async_channel::unbounded();
    runtime().spawn(mpd::watch(cfg, tx));
    glib::spawn_future_local(async move {
        while let Ok(ev) = rx.recv().await {
            match ev {
                Event::Connected { version } => {
                    tracing::info!(%version, "connected to MPD");
                    refresh(&client, &ui).await;
                }
                Event::Changed(subs) => {
                    if subs
                        .iter()
                        .any(|s| matches!(s, Subsystem::Player | Subsystem::Playlist))
                    {
                        refresh(&client, &ui).await;
                    }
                }
                Event::Disconnected { reason } => {
                    ui.status
                        .set_label(&format!("Sin conexión con MPD: {reason}"));
                }
            }
        }
    });

    window.present();
}

async fn refresh(client: &Client, ui: &Ui) {
    let c = client.clone();
    let res = on_tokio(async move {
        let status = c.status().await?;
        let song = c.current_song().await?;
        mpd::Result::Ok((status, song))
    })
    .await;

    let (status, song) = match res {
        Ok(v) => v,
        Err(e) => {
            ui.status.set_label(&format!("Error: {e}"));
            return;
        }
    };

    ui.play.set_icon_name(if status.is_playing() {
        "media-playback-pause-symbolic"
    } else {
        "media-playback-start-symbolic"
    });

    match song {
        Some(s) => {
            ui.page.set_title(s.display_title());
            let mut desc = [s.artist(), s.album()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" — ");
            if let Some(a) = status.audio {
                desc.push_str(&format!("\n{a}"));
            }
            ui.page
                .set_description(Some(&glib::markup_escape_text(&desc)));
        }
        None => {
            ui.page.set_title("Nada en reproducción");
            ui.page.set_description(None);
        }
    }
    let cfg = client.config();
    ui.status
        .set_label(&format!("Conectado a {}:{}", cfg.host, cfg.port));
}
