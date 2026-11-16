//! ObiPlayer — GTK front-end.

mod albums;
mod app;
mod covers;
mod ctx;
mod pages;
mod player_bar;
mod queue;
mod ui;

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
.cover-small { border-radius: 4px; background: alpha(currentColor, 0.08); }
.badge { font-size: 0.75em; font-weight: bold; padding: 1px 7px; border-radius: 999px;
  font-family: monospace; }
.fmt-dsd { background: alpha(@success_color, 0.18); color: @success_color; }
.fmt-lossless { background: alpha(@accent_color, 0.18); color: @accent_color; }
.fmt-mqa { background: alpha(#9141ac, 0.2); color: #c061cb; }
.fmt-lossy { background: alpha(@warning_color, 0.2); color: @warning_color; }
.fmt-other { background: alpha(currentColor, 0.1); }
.badge-best { background: alpha(@success_color, 0.25); color: @success_color; }
.badge-preferred { background: alpha(currentColor, 0.15); }
.badge-current { background: alpha(@accent_color, 0.25); color: @accent_color; }
.row-best { background: alpha(@success_color, 0.06); border-radius: 6px; }
.row-warning { background: alpha(@error_color, 0.05); border-radius: 6px; }
.current-track { background: alpha(@accent_color, 0.08); border-radius: 6px; }
.star { min-width: 20px; min-height: 20px; padding: 2px; }
.star image { -gtk-icon-size: 12px; }
.star-on { color: #e5a50a; }
.star-off { color: alpha(currentColor, 0.3); }
.chip { padding: 2px 12px; min-height: 0; font-size: 0.85em; font-weight: bold; }
.filter-chip { padding: 2px 4px 2px 12px; border-radius: 999px; background: alpha(@accent_color, 0.15); }
.versions-button { color: @accent_color; padding: 2px 8px; min-height: 0; }
.group-avatar { padding: 18px; border-radius: 999px; background: alpha(currentColor, 0.08); }
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
