//! Colour themes (accent overrides, DrPlayer's palette) and light/dark mode.

use std::cell::{Cell, RefCell};

use obiplayer_core::config::UiConfig;
use relm4::adw;
use relm4::gtk::{self, gdk};

/// Name and accent colour; "System" follows the desktop accent.
pub const THEMES: &[(&str, Option<&str>)] = &[
    ("System", None),
    ("Classic", Some("#598cbf")),
    ("Warm", Some("#bf8c4d")),
    ("Mono", Some("#b3b3b3")),
    ("Vinyl", Some("#4d8c66")),
];

pub const SCHEMES: &[&str] = &["system", "light", "dark"];

thread_local! {
    static PROVIDER: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
    static ACCENT: Cell<Option<gdk::RGBA>> = const { Cell::new(None) };
}

/// Accent forced by the theme, if any (drawing code uses it).
pub fn accent_override() -> Option<gdk::RGBA> {
    ACCENT.with(Cell::get)
}

pub fn apply(ui: &UiConfig) {
    let sm = adw::StyleManager::default();
    sm.set_color_scheme(match ui.color_scheme.as_str() {
        "light" => adw::ColorScheme::ForceLight,
        "dark" => adw::ColorScheme::ForceDark,
        _ => adw::ColorScheme::Default,
    });

    let color = THEMES
        .iter()
        .find(|(n, _)| *n == ui.theme)
        .and_then(|(_, c)| *c);
    ACCENT.with(|a| a.set(color.and_then(|c| gdk::RGBA::parse(c).ok())));
    let css = match color {
        Some(c) => format!(
            "@define-color accent_bg_color {c};\n@define-color accent_color {c};\n\
             :root {{ --accent-bg-color: {c}; --accent-color: {c}; }}\n"
        ),
        None => String::new(),
    };
    PROVIDER.with(|p| {
        let mut p = p.borrow_mut();
        let provider = p.get_or_insert_with(|| {
            let prov = gtk::CssProvider::new();
            if let Some(d) = gdk::Display::default() {
                gtk::style_context_add_provider_for_display(
                    &d,
                    &prov,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
                );
            }
            prov
        });
        provider.load_from_string(&css);
    });
}
