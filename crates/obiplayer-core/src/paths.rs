//! XDG base directories (also used as-is on macOS during development).

use std::path::PathBuf;

const APP: &str = "obiplayer";

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(fallback))
        .join(APP)
}

/// `$XDG_CONFIG_HOME/obiplayer` (`~/.config/obiplayer`).
pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config")
}

/// `$XDG_CACHE_HOME/obiplayer` (`~/.cache/obiplayer`).
pub fn cache_dir() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache")
}

/// `$XDG_DATA_HOME/obiplayer` (`~/.local/share/obiplayer`).
pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share")
}
