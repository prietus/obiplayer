//! Local system integration for setup and settings: MPD configuration file,
//! dependencies, ALSA cards, music sources (symlinks) and network mounts.
//!
//! Linux equivalents of what DrPlayer did with Homebrew paths and CoreAudio.

use std::path::{Path, PathBuf};

use crate::probe::find_binary;

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn expand(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None if p == "~" => home(),
        None => PathBuf::from(p),
    }
}

// -----------------------------------------------------------------------------
// Dependencies
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub name: &'static str,
    pub path: Option<PathBuf>,
}

/// mpd (only needed for a local server), ffmpeg and ffprobe (analysis).
pub fn dependencies() -> Vec<Dependency> {
    ["mpd", "ffmpeg", "ffprobe"]
        .into_iter()
        .map(|n| Dependency {
            name: n,
            path: find_binary(n),
        })
        .collect()
}

/// Install hint for the running distribution.
pub fn install_hint(missing: &[&str]) -> String {
    // ffprobe ships with ffmpeg everywhere.
    let mut pkgs: Vec<&str> = missing
        .iter()
        .map(|m| if *m == "ffprobe" { "ffmpeg" } else { m })
        .collect();
    pkgs.dedup();
    let list = pkgs.join(" ");
    let os = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    let id = |k: &str| os.lines().any(|l| l.starts_with("ID") && l.contains(k));
    if id("arch") {
        format!("sudo pacman -S {list}")
    } else if id("fedora") {
        format!("sudo dnf install {list}")
    } else if id("debian") || id("ubuntu") {
        format!("sudo apt install {list}")
    } else if cfg!(target_os = "macos") {
        format!("brew install {list}")
    } else {
        format!("install: {list}")
    }
}

// -----------------------------------------------------------------------------
// mpd.conf
// -----------------------------------------------------------------------------

/// Search order for a user/system MPD configuration.
pub fn mpd_conf_candidates() -> Vec<PathBuf> {
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"));
    vec![
        xdg.join("mpd/mpd.conf"),
        home().join(".mpdconf"),
        home().join(".mpd/mpd.conf"),
        PathBuf::from("/etc/mpd.conf"),
        PathBuf::from("/usr/local/etc/mpd.conf"),
        PathBuf::from("/opt/homebrew/etc/mpd.conf"),
    ]
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioOutputConf {
    pub kind: String,
    pub name: String,
    pub device: String,
    pub mixer_type: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MpdConf {
    pub path: PathBuf,
    pub music_directory: Option<PathBuf>,
    pub bind_to_address: Option<String>,
    pub port: Option<u16>,
    pub sticker_file: Option<String>,
    pub follow_outside_symlinks: Option<String>,
    pub follow_inside_symlinks: Option<String>,
    pub replaygain: Option<String>,
    pub auto_update: Option<String>,
    pub db_file: Option<String>,
    pub outputs: Vec<AudioOutputConf>,
}

/// `key "value"` → (key, value). Exact key match (DrPlayer matched prefixes,
/// so `port` also matched longer keys).
fn kv(line: &str) -> Option<(&str, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (k, rest) = line.split_once(char::is_whitespace)?;
    let v = rest.trim();
    let v = v
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(v);
    Some((k, v.to_owned()))
}

pub fn parse_mpd_conf(path: &Path, text: &str) -> MpdConf {
    let mut c = MpdConf {
        path: path.to_owned(),
        ..Default::default()
    };
    let mut block: Option<AudioOutputConf> = None;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("audio_output") && t.ends_with('{') {
            block = Some(AudioOutputConf::default());
            continue;
        }
        if t == "}" {
            if let Some(b) = block.take()
                && !b.name.is_empty()
            {
                c.outputs.push(b);
            }
            continue;
        }
        let Some((k, v)) = kv(t) else { continue };
        if let Some(b) = block.as_mut() {
            match k {
                "type" => b.kind = v,
                "name" => b.name = v,
                "device" => b.device = v,
                "mixer_type" => b.mixer_type = v,
                _ => {}
            }
            continue;
        }
        match k {
            "music_directory" => c.music_directory = Some(expand(&v)),
            "bind_to_address" => c.bind_to_address = Some(v),
            "port" => c.port = v.parse().ok(),
            "sticker_file" => c.sticker_file = Some(v),
            "follow_outside_symlinks" => c.follow_outside_symlinks = Some(v),
            "follow_inside_symlinks" => c.follow_inside_symlinks = Some(v),
            "replaygain" => c.replaygain = Some(v),
            "auto_update" => c.auto_update = Some(v),
            "db_file" => c.db_file = Some(v),
            _ => {}
        }
    }
    c
}

/// First readable configuration that sets `music_directory`.
pub fn detect_mpd_conf() -> Option<MpdConf> {
    mpd_conf_candidates().into_iter().find_map(|p| {
        let text = std::fs::read_to_string(&p).ok()?;
        let c = parse_mpd_conf(&p, &text);
        c.music_directory.is_some().then_some(c)
    })
}

/// One line of the mpd.conf health check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health {
    pub key: &'static str,
    pub ok: bool,
    pub detail: String,
}

pub fn health(c: &MpdConf) -> Vec<Health> {
    let yes = |v: &Option<String>| v.as_deref() == Some("yes");
    vec![
        Health {
            key: "sticker_file",
            ok: c.sticker_file.is_some(),
            detail: c.sticker_file.clone().unwrap_or_else(|| {
                "missing — ratings, play counts and preferred versions won't persist".into()
            }),
        },
        Health {
            key: "follow_outside_symlinks",
            ok: yes(&c.follow_outside_symlinks),
            detail: if yes(&c.follow_outside_symlinks) {
                "yes".into()
            } else {
                "no — music sources via symlinks won't work".into()
            },
        },
        Health {
            key: "follow_inside_symlinks",
            ok: yes(&c.follow_inside_symlinks),
            detail: if yes(&c.follow_inside_symlinks) {
                "yes".into()
            } else {
                "no — nested symlinks won't resolve".into()
            },
        },
        Health {
            key: "replaygain",
            ok: c.replaygain.as_deref().is_none_or(|v| v == "off"),
            detail: c
                .replaygain
                .clone()
                .unwrap_or_else(|| "not set (bit-perfect)".into()),
        },
        Health {
            key: "auto_update",
            ok: yes(&c.auto_update),
            detail: if yes(&c.auto_update) {
                "yes".into()
            } else {
                "no — run `mpc update` after adding music".into()
            },
        },
        Health {
            key: "db_file",
            ok: c.db_file.is_some(),
            detail: c.db_file.clone().unwrap_or_else(|| "not set".into()),
        },
    ]
}

/// An ALSA output block for a sound card, bit-perfect settings.
pub fn alsa_output_block(name: &str, card_id: &str) -> String {
    format!(
        "\naudio_output {{\n    type            \"alsa\"\n    name            \"{name}\"\n    device          \"hw:CARD={card_id},DEV=0\"\n    mixer_type      \"none\"\n    auto_resample   \"no\"\n    auto_format     \"no\"\n    dop             \"yes\"\n}}\n"
    )
}

/// A basic bit-perfect configuration under `~/.config/mpd`.
pub fn default_mpd_conf(
    music_dir: &Path,
    host: &str,
    port: u16,
    card: Option<(&str, &str)>,
) -> String {
    let dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("mpd");
    let d = dir.display();
    let mut s = format!(
        "music_directory         \"{}\"\nfollow_outside_symlinks \"yes\"\nfollow_inside_symlinks  \"yes\"\nplaylist_directory      \"{d}/playlists\"\ndb_file                 \"{d}/database\"\nstate_file              \"{d}/state\"\nsticker_file            \"{d}/sticker.sql\"\nlog_file                \"{d}/log\"\n\nbind_to_address         \"{host}\"\nport                    \"{port}\"\n\nauto_update             \"yes\"\nreplaygain              \"off\"\n",
        music_dir.display()
    );
    match card {
        Some((name, id)) => s.push_str(&alsa_output_block(name, id)),
        None => s.push_str(
            "\naudio_output {\n    type            \"pipewire\"\n    name            \"Default output\"\n}\n",
        ),
    }
    s.push_str(
        "\naudio_output {\n    type            \"fifo\"\n    name            \"Visualizer\"\n    path            \"/tmp/mpd.fifo\"\n    format          \"44100:16:2\"\n}\n",
    );
    s
}

/// Where a generated configuration is written.
pub fn user_mpd_conf_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("mpd/mpd.conf")
}

/// Write a generated configuration (creating its directories and a
/// `.mpdignore` that skips CUE sheets to avoid duplicate virtual tracks).
pub fn install_mpd_conf(text: &str, music_dir: &Path) -> std::io::Result<PathBuf> {
    let path = user_mpd_conf_path();
    let dir = path.parent().map(Path::to_owned).unwrap_or_default();
    std::fs::create_dir_all(dir.join("playlists"))?;
    std::fs::create_dir_all(music_dir)?;
    std::fs::write(&path, text)?;
    let ignore = music_dir.join(".mpdignore");
    if !ignore.exists() {
        let _ = std::fs::write(ignore, "*.cue\n");
    }
    Ok(path)
}

// -----------------------------------------------------------------------------
// ALSA cards
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub index: u32,
    /// Short id used in `hw:CARD=<id>`.
    pub id: String,
    pub name: String,
    pub usb: bool,
    /// Sample rates seen in the USB stream descriptor (empty if unknown).
    pub rates: Vec<u32>,
}

/// Parse `/proc/asound/cards`:
/// ` 1 [D90III         ]: USB-Audio - Topping D90 III SABRE`
pub fn parse_cards(text: &str) -> Vec<Card> {
    text.lines()
        .filter_map(|l| {
            let (head, name) = l.split_once("]: ")?;
            let (idx, id) = head.split_once('[')?;
            let index = idx.trim().parse().ok()?;
            let (driver, long) = name.split_once(" - ").unwrap_or((name, name));
            Some(Card {
                index,
                id: id.trim().to_owned(),
                name: long.trim().to_owned(),
                usb: driver.contains("USB"),
                rates: Vec::new(),
            })
        })
        .collect()
}

/// Rates listed in `/proc/asound/cardN/stream0` (USB audio).
pub fn parse_stream_rates(text: &str) -> Vec<u32> {
    let mut v: Vec<u32> = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("Rates:"))
        .flat_map(|r| r.split(',').filter_map(|x| x.trim().parse::<u32>().ok()))
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

pub fn sound_cards() -> Vec<Card> {
    let mut cards = parse_cards(&std::fs::read_to_string("/proc/asound/cards").unwrap_or_default());
    for c in &mut cards {
        if let Ok(s) = std::fs::read_to_string(format!("/proc/asound/card{}/stream0", c.index)) {
            c.rates = parse_stream_rates(&s);
        }
    }
    cards
}

/// DrPlayer's quality tier from the highest rate.
pub fn quality_tier(max_rate: u32) -> &'static str {
    match max_rate {
        705_600.. => "DSD512",
        384_000.. => "DSD / DXD",
        96_000.. => "Hi-Res",
        48_000.. => "CD+",
        _ => "CD",
    }
}

// -----------------------------------------------------------------------------
// Music sources (local MPD): symlinks inside music_directory
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub name: String,
    /// Symlink target, or `None` for a real directory.
    pub target: Option<PathBuf>,
}

pub fn list_sources(music_dir: &Path) -> Vec<Source> {
    let Ok(rd) = std::fs::read_dir(music_dir) else {
        return Vec::new();
    };
    let mut v: Vec<Source> = rd
        .filter_map(Result::ok)
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| {
            let p = e.path();
            let meta = std::fs::symlink_metadata(&p).ok()?;
            if meta.file_type().is_symlink() {
                Some(Source {
                    name: e.file_name().to_string_lossy().into_owned(),
                    target: std::fs::read_link(&p).ok(),
                })
            } else if meta.is_dir() {
                Some(Source {
                    name: e.file_name().to_string_lossy().into_owned(),
                    target: None,
                })
            } else {
                None
            }
        })
        .collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

pub fn add_source(music_dir: &Path, target: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(music_dir)?;
    let name = target
        .file_name()
        .ok_or_else(|| std::io::Error::other("folder has no name"))?;
    let link = music_dir.join(name);
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, &link)?;
    Ok(link)
}

/// Remove a source. Only symlinks are removed; real directories are never
/// touched.
pub fn remove_source(music_dir: &Path, name: &str) -> std::io::Result<()> {
    let p = music_dir.join(name);
    if std::fs::symlink_metadata(&p)?.file_type().is_symlink() {
        std::fs::remove_file(p)
    } else {
        Err(std::io::Error::other("not a symlink; refusing to delete"))
    }
}

// -----------------------------------------------------------------------------
// Finding a remote server's library on this computer
// -----------------------------------------------------------------------------

const NET_FS: &[&str] = &[
    "nfs",
    "nfs4",
    "cifs",
    "smb3",
    "smbfs",
    "fuse.sshfs",
    "davfs",
    "fuse.davfs2",
    "autofs",
    "fuse.rclone",
    "afpfs",
    "webdav",
];

/// Mount points of network filesystems from `/proc/self/mounts` text.
pub fn parse_mounts(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let (_dev, mnt, fs) = (f.next()?, f.next()?, f.next()?);
            NET_FS
                .contains(&fs)
                .then(|| PathBuf::from(mnt.replace("\\040", " ")))
        })
        .collect()
}

/// Roots worth searching: network mounts plus removable-media folders.
pub fn candidate_roots() -> Vec<PathBuf> {
    let mut roots = parse_mounts(&std::fs::read_to_string("/proc/self/mounts").unwrap_or_default());
    if cfg!(target_os = "macos")
        && let Ok(out) = std::process::Command::new("mount").output()
    {
        // "server:/export on /path (nfs, …)"
        for l in String::from_utf8_lossy(&out.stdout).lines() {
            if let Some((_, rest)) = l.split_once(" on ")
                && let Some((path, kind)) = rest.split_once(" (")
                && NET_FS.iter().any(|k| kind.starts_with(k))
            {
                roots.push(PathBuf::from(path));
            }
        }
    }
    let user = std::env::var("USER").unwrap_or_default();
    for base in [
        PathBuf::from("/media").join(&user),
        PathBuf::from("/run/media").join(&user),
        PathBuf::from("/mnt"),
        PathBuf::from("/Volumes"),
    ] {
        if let Ok(rd) = std::fs::read_dir(&base) {
            roots.extend(
                rd.filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.is_dir()),
            );
        }
    }
    roots.sort();
    roots.dedup();
    roots
}

/// Find a directory under the candidate roots (and up to two levels below)
/// where all `samples` (MPD-relative file paths) exist. That directory is
/// the server's music_directory as seen from this computer.
pub fn find_music_dir(samples: &[String]) -> Option<PathBuf> {
    if samples.is_empty() {
        return None;
    }
    let hits = |dir: &Path| samples.iter().all(|s| dir.join(s).exists());
    let mut level: Vec<PathBuf> = candidate_roots();
    for _ in 0..3 {
        if let Some(d) = level.iter().find(|d| hits(d)) {
            return Some(d.clone());
        }
        let mut next = Vec::new();
        for d in &level {
            if let Ok(rd) = std::fs::read_dir(d) {
                next.extend(
                    rd.filter_map(Result::ok)
                        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                        .map(|e| e.path())
                        .filter(|p| p.is_dir())
                        .take(40),
                );
            }
        }
        level = next;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mpd_conf_parsing() {
        let c = parse_mpd_conf(
            Path::new("/x"),
            r#"
# comment
music_directory "/mnt/music"
port "6601"
portable_thing "no"
sticker_file "~/.mpd/sticker.sql"
replaygain "album"
audio_output {
    type "alsa"
    name "DAC"
    device "hw:CARD=D90III,DEV=0"
    mixer_type "none"
}
audio_output {
    type "fifo"
}
"#,
        );
        assert_eq!(c.music_directory, Some(PathBuf::from("/mnt/music")));
        assert_eq!(c.port, Some(6601));
        assert!(c.sticker_file.unwrap().ends_with(".mpd/sticker.sql"));
        assert_eq!(c.outputs.len(), 1);
        assert_eq!(c.outputs[0].mixer_type, "none");
    }

    #[test]
    fn health_rows() {
        let c = parse_mpd_conf(
            Path::new("/x"),
            "replaygain \"track\"\nauto_update \"yes\"\n",
        );
        let h = health(&c);
        assert!(!h.iter().find(|r| r.key == "replaygain").unwrap().ok);
        assert!(h.iter().find(|r| r.key == "auto_update").unwrap().ok);
        assert!(!h.iter().find(|r| r.key == "sticker_file").unwrap().ok);
    }

    #[test]
    fn cards_and_rates() {
        let cards = parse_cards(
            " 0 [vc4hdmi0       ]: vc4-hdmi - vc4-hdmi-0\n                      vc4-hdmi-0\n 1 [D90III         ]: USB-Audio - Topping D90 III SABRE\n                      Topping D90 III at usb-xhci, high speed\n",
        );
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[1].id, "D90III");
        assert!(cards[1].usb);
        assert_eq!(cards[1].name, "Topping D90 III SABRE");
        let r =
            parse_stream_rates("  Rates: 44100, 48000, 96000\n  Rates: 192000, 384000, 768000\n");
        assert_eq!(r.last(), Some(&768000));
        assert_eq!(quality_tier(768_000), "DSD512");
        assert_eq!(quality_tier(44_100), "CD");
    }

    #[test]
    fn mounts() {
        let m = parse_mounts(
            "proc /proc proc rw 0 0\n192.0.2.10:/mnt/music /mnt/music nfs rw 0 0\n//nas/a /mnt/my\\040share cifs rw 0 0\n",
        );
        assert_eq!(
            m,
            [PathBuf::from("/mnt/music"), PathBuf::from("/mnt/my share")]
        );
    }

    #[test]
    fn generated_conf_is_bitperfect() {
        let t = default_mpd_conf(
            Path::new("/music"),
            "localhost",
            6600,
            Some(("DAC", "D90III")),
        );
        let c = parse_mpd_conf(Path::new("/x"), &t);
        assert_eq!(c.outputs[0].device, "hw:CARD=D90III,DEV=0");
        assert!(health(&c).iter().all(|h| h.ok), "{:?}", health(&c));
    }

    #[test]
    fn sources_only_remove_symlinks() {
        let dir = std::env::temp_dir().join(format!("obi-src-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let target = dir.join("target/Rock");
        std::fs::create_dir_all(&target).unwrap();
        let music = dir.join("music");
        std::fs::create_dir_all(music.join("real")).unwrap();
        add_source(&music, &target).unwrap();
        let s = list_sources(&music);
        assert_eq!(s.len(), 2);
        assert!(remove_source(&music, "real").is_err());
        remove_source(&music, "Rock").unwrap();
        assert_eq!(list_sources(&music).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
