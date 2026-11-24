//! User configuration, stored as TOML in `$XDG_CONFIG_HOME/obiplayer/config.toml`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::mpd::ConnectConfig;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub mpd: MpdConfig,
    pub library: LibraryConfig,
    pub apis: ApiConfig,
    pub ui: UiConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// "System" (desktop accent), "Classic", "Warm", "Mono", "Vinyl".
    pub theme: String,
    /// "system", "light", "dark".
    pub color_scheme: String,
    /// "system" (follow the locale), "en", "es".
    pub language: String,
    /// Closing the window keeps the app running in the system tray (only
    /// while a tray host is showing the icon).
    pub close_to_tray: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        UiConfig {
            theme: "System".into(),
            color_scheme: "system".into(),
            language: "system".into(),
            close_to_tray: true,
        }
    }
}

/// Keys for online metadata services. Kept only in the user's config file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiConfig {
    pub lastfm_api_key: String,
    pub discogs_key: String,
    pub discogs_secret: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MpdConfig {
    pub host: String,
    pub port: u16,
    /// Empty = no password.
    pub password: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LibraryConfig {
    /// Local path of MPD's music directory (a mount, for remote servers).
    /// Used for local cover art, DR analysis and waveforms. When unset,
    /// cover art is fetched through MPD instead.
    pub music_dir: Option<PathBuf>,
}

impl Default for MpdConfig {
    fn default() -> Self {
        MpdConfig {
            host: "localhost".into(),
            port: 6600,
            password: String::new(),
        }
    }
}

const TEMPLATE_HEADER: &str = "\
# ObiPlayer configuration.
# MPD_HOST ([password@]host) and MPD_PORT environment variables override [mpd].
";

impl Config {
    pub fn path() -> PathBuf {
        crate::paths::config_dir().join("config.toml")
    }

    /// Load from the default path; a missing file yields defaults (and is
    /// created so the user has something to edit).
    pub fn load() -> Config {
        let path = Self::path();
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!(path = %path.display(), error = %e, "invalid config, using defaults");
                Config::default()
            }),
            Err(_) => {
                let cfg = Config::default();
                if let Err(e) = cfg.save_to(&path) {
                    tracing::debug!(error = %e, "could not write default config");
                }
                cfg
            }
        }
    }

    /// True until a configuration has been saved (first run).
    pub fn exists() -> bool {
        Self::path().is_file()
    }

    /// Like `load`, but doesn't create the file (the setup wizard does).
    pub fn load_or_default() -> Config {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&Self::path())
    }

    fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let body = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, format!("{TEMPLATE_HEADER}\n{body}"))?;
        // The file can hold API keys and the MPD password.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    /// Connection settings, with `MPD_HOST`/`MPD_PORT` taking precedence.
    pub fn connect_config(&self) -> ConnectConfig {
        let mut c = ConnectConfig::new(self.mpd.host.clone(), self.mpd.port)
            .with_password(self.mpd.password.clone());
        let env = ConnectConfig::from_env();
        if std::env::var_os("MPD_HOST").is_some() {
            c.host = env.host;
            c.password = env.password;
        }
        if std::env::var_os("MPD_PORT").is_some() {
            c.port = env.port;
        }
        c.timeout = Duration::from_secs(60);
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_file_fills_defaults() {
        let c: Config = toml::from_str("[mpd]\nhost = \"nas\"\n").unwrap();
        assert_eq!(c.mpd.host, "nas");
        assert_eq!(c.mpd.port, 6600);
        assert_eq!(c.library.music_dir, None);
    }

    #[test]
    fn roundtrip() {
        let mut c = Config::default();
        c.library.music_dir = Some("/mnt/music".into());
        let text = toml::to_string_pretty(&c).unwrap();
        assert_eq!(toml::from_str::<Config>(&text).unwrap(), c);
    }
}
