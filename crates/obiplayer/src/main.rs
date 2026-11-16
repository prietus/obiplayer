//! ObiPlayer — GTK front-end.

mod albums;
mod app;
mod covers;
mod player_bar;
mod queue;

use obiplayer_core::config::Config;
use relm4::RelmApp;

const APP_ID: &str = "us.priet.ObiPlayer";

/// Translation hook. Strings are written in English and will go through
/// gettext once localisation lands (see docs/PARITY.md).
pub fn tr(s: &str) -> String {
    s.to_owned()
}

const CSS: &str = r#"
.album-frame, .bar-cover { border-radius: 8px; background: alpha(currentColor, 0.08); }
.bar-cover { border-radius: 6px; }
.playing-badge {
  padding: 6px; border-radius: 999px;
  background: alpha(@window_bg_color, 0.85); color: @accent_color;
}
.player-bar { }
.play-button { min-width: 40px; min-height: 40px; }
.link-like { padding: 0; min-height: 0; }
.queue-panel { background: @sidebar_bg_color; border-left: 1px solid @borders; }
"#;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "obiplayer=info,obiplayer_core=info".into()),
        )
        .init();
    // Network I/O, cover decoding queues, etc.
    let _ = relm4::RELM_THREADS.set(2);

    let config = Config::load();
    let app = RelmApp::new(APP_ID);
    relm4::set_global_css(CSS);
    app.run::<app::App>(config);
}
