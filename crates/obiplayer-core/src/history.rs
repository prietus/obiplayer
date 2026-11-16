//! Append-only local log of play events (port of DrPlayer's `PlayHistory`).
//!
//! One line per play: `ts\tfile\tartist\talbum\ttitle\tduration`, with the
//! timestamp in Unix seconds and the duration in whole seconds. Unlike
//! DrPlayer, text fields escape `\\`, `\t`, `\n` and `\r`, so a tab in a title
//! no longer corrupts the line. Lines written by DrPlayer read back unchanged
//! unless they contain a literal backslash followed by `t`, `n`, `r` or `\`.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::paths;

/// One play event.
#[derive(Debug, Clone, PartialEq)]
pub struct Play {
    /// Unix seconds.
    pub ts: i64,
    pub file: String,
    pub artist: String,
    pub album: String,
    pub title: String,
    /// Seconds.
    pub duration: f64,
}

/// The play history file.
#[derive(Debug, Clone)]
pub struct History {
    path: PathBuf,
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

fn unescape(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('\\') => out.push('\\'),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            // Unknown escape or trailing backslash: keep it literally.
            _ => {
                out.push('\\');
                continue;
            }
        }
        chars.next();
    }
    out
}

/// Serialise one play as a TSV line (with trailing newline).
pub fn format_line(p: &Play) -> String {
    let dur = if p.duration.is_finite() {
        p.duration.trunc() as i64
    } else {
        0
    };
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\n",
        p.ts,
        escape(&p.file),
        escape(&p.artist),
        escape(&p.album),
        escape(&p.title),
        dur
    )
}

/// Parse one TSV line; `None` for malformed lines (fewer than six columns or
/// a non-integer timestamp).
pub fn parse_line(line: &str) -> Option<Play> {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() < 6 {
        return None;
    }
    let ts = parts[0].parse::<i64>().ok()?;
    Some(Play {
        ts,
        file: unescape(parts[1]),
        artist: unescape(parts[2]),
        album: unescape(parts[3]),
        title: unescape(parts[4]),
        duration: parts[5]
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|d| d.is_finite())
            .unwrap_or(0.0),
    })
}

/// Parse a whole history file's contents.
pub fn parse(content: &str) -> Vec<Play> {
    content
        .split('\n')
        .filter(|l| !l.is_empty())
        .filter_map(parse_line)
        .collect()
}

impl History {
    /// `$XDG_DATA_HOME/obiplayer/play_history.tsv`.
    pub fn default_path() -> PathBuf {
        paths::data_dir().join("play_history.tsv")
    }

    pub fn new(path: impl Into<PathBuf>) -> History {
        History { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one play, creating the directory and file if needed. The line
    /// is written with a single `write` so concurrent appends don't interleave.
    pub fn append(&self, play: &Play) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        f.write_all(format_line(play).as_bytes())
    }

    /// Every play in file order (oldest first); empty if the file is missing.
    pub fn load(&self) -> Vec<Play> {
        match std::fs::read(&self.path) {
            Ok(bytes) => parse(&String::from_utf8_lossy(&bytes)),
            Err(_) => Vec::new(),
        }
    }

    /// Plays with `ts >= since`.
    pub fn since(&self, since: i64) -> Vec<Play> {
        let mut v = self.load();
        v.retain(|p| p.ts >= since);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(ts: i64, title: &str) -> Play {
        Play {
            ts,
            file: "A/B/01.flac".into(),
            artist: "Art\\ist".into(),
            album: "Al\tbum".into(),
            title: title.into(),
            duration: 241.9,
        }
    }

    #[test]
    fn escaping_round_trip() {
        let p = play(100, "Ti\ttle\nwith \\t literal\r");
        let line = format_line(&p);
        assert_eq!(line.matches('\t').count(), 5);
        assert_eq!(line.matches('\n').count(), 1);
        let back = parse_line(line.trim_end_matches('\n')).unwrap();
        assert_eq!(back.title, p.title);
        assert_eq!(back.album, p.album);
        assert_eq!(back.artist, p.artist);
        // Whole seconds, like DrPlayer.
        assert_eq!(back.duration, 241.0);
    }

    #[test]
    fn reads_drplayer_lines_and_skips_malformed() {
        let content = "1700000000\tA/01.flac\tQueen\tOpera\tSong\t300\n\
                       garbage\n\
                       x\tA\tB\tC\tD\t1\n\
                       1700000001\tA/02.flac\tQueen\n\
                       \n\
                       1700000002\tA/03.flac\t\t\t\tbad\textra\n";
        let v = parse(content);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].artist, "Queen");
        assert_eq!(v[0].duration, 300.0);
        assert_eq!(v[1].ts, 1700000002);
        assert_eq!(v[1].duration, 0.0);
        // Unknown escapes are kept literally.
        assert_eq!(unescape("a\\qb\\"), "a\\qb\\");
    }

    #[test]
    fn append_load_since() {
        let dir = std::env::temp_dir().join(format!(
            "obiplayer-history-test-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let h = History::new(dir.join("nested/play_history.tsv"));
        assert!(h.load().is_empty());
        h.append(&play(10, "one")).unwrap();
        h.append(&play(20, "two\tt")).unwrap();
        let all = h.load();
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].title, "two\tt");
        let recent = h.since(15);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].ts, 20);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
