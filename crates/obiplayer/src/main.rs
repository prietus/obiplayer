//! ObiPlayer — GTK front-end.

mod albums;
mod app;
mod covers;
mod ctx;
mod lyrics_ui;
mod nowplaying;
mod pages;
mod player_bar;
mod queue;
mod signal_ui;
mod tasks_ui;
mod ui;
mod viz;

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
.dr-green { color: #33d17a; } .badge.dr-green { background: alpha(#33d17a, 0.18); }
.dr-yellow { color: #f6d32d; } .badge.dr-yellow { background: alpha(#f6d32d, 0.18); }
.dr-orange { color: #ff7800; } .badge.dr-orange { background: alpha(#ff7800, 0.18); }
.dr-red { color: #e01b24; } .badge.dr-red { background: alpha(#e01b24, 0.18); }
levelbar.dr-bar trough { min-height: 6px; border-radius: 3px; }
levelbar.dr-bar block { min-height: 6px; border-radius: 3px; }
levelbar.dr-bar.dr-green block.filled { background: #33d17a; }
levelbar.dr-bar.dr-yellow block.filled { background: #f6d32d; }
levelbar.dr-bar.dr-orange block.filled { background: #ff7800; }
levelbar.dr-bar.dr-red block.filled { background: #e01b24; }
.waveform, .visualizer { color: @accent_color; }
.now-playing .waveform, .now-playing .visualizer { color: white; }
.signal-dot { border-radius: 999px; min-width: 10px; min-height: 10px; }
.signal-idle { background: alpha(currentColor, 0.35); }
.signal-perfect { background: #33d17a; box-shadow: 0 0 6px alpha(#33d17a, 0.8); }
.signal-processed { background: #9141ac; box-shadow: 0 0 6px alpha(#9141ac, 0.8); }
.signal-ok { color: #33d17a; }
.signal-warn { color: #e5a50a; }
.signal-connector { background: alpha(@accent_color, 0.5); }
.now-playing { background: black; color: white; }
.np-shade { background: alpha(black, 0.55); }
.np-background { transition: filter 800ms; }
.np-blur { filter: blur(30px); }
.np-dim { color: alpha(white, 0.6); }
.np-play { min-width: 56px; min-height: 56px; }
.np-art { border-radius: 12px; }
.lyric-line { transition: all 300ms; }
.lyric-past { opacity: 0.35; }
.lyric-future { opacity: 0.65; }
.lyric-current { opacity: 1; font-weight: bold; }
.lyric-large { font-size: 1.4em; }
.lyric-large.lyric-current { font-size: 1.9em; }
.lyrics-synced { color: #33d17a; }
.lyrics-unsynced { color: #ff7800; }
.avatar-frame { border-radius: 999px; background: alpha(currentColor, 0.08); }
.summary-card, .pressing-card { padding: 10px 12px; border-radius: 10px; background: alpha(currentColor, 0.05); }
.pressing-card { background: alpha(#33d17a, 0.10); }
.link-pill { padding: 2px 8px; min-height: 0; }
.details-header { margin-top: 6px; letter-spacing: 1px; }
.source-note { font-style: italic; }
.lw-better { color: #33d17a; }
.lw-worse { color: #e66100; }
.lw-same { color: @accent_color; font-weight: bold; }
.dr-button { padding: 0 2px; min-height: 0; }
.task-active { color: #ff7800; }
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
