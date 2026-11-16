use std::collections::BTreeMap;
use std::fmt;

use super::protocol::Pair;

// ---------------------------------------------------------------------------
// Audio format
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcmBits {
    Int(u8),
    Float,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    Pcm {
        rate: u32,
        bits: PcmBits,
    },
    /// DSD with its multiplier of 44.1 kHz (64 = DSD64).
    Dsd {
        multiplier: u32,
    },
}

/// An MPD audio format such as `44100:24:2`, `96000:f:2` or `dsd64:2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormat {
    pub sample: SampleFormat,
    pub channels: u8,
}

impl AudioFormat {
    pub fn parse(s: &str) -> Option<AudioFormat> {
        let mut parts = s.split(':');
        let first = parts.next()?;
        if let Some(mult) = first.strip_prefix("dsd") {
            let channels = parts.next()?.parse().ok()?;
            return Some(AudioFormat {
                sample: SampleFormat::Dsd {
                    multiplier: mult.parse().ok()?,
                },
                channels,
            });
        }
        let rate = first.parse().ok()?;
        let bits = match parts.next()? {
            "f" => PcmBits::Float,
            b => PcmBits::Int(b.parse().ok()?),
        };
        let channels = parts.next()?.parse().ok()?;
        Some(AudioFormat {
            sample: SampleFormat::Pcm { rate, bits },
            channels,
        })
    }

    pub fn is_dsd(&self) -> bool {
        matches!(self.sample, SampleFormat::Dsd { .. })
    }

    /// PCM sample rate, or the native DSD bit rate (DSD64 = 2 822 400 Hz).
    pub fn sample_rate(&self) -> u32 {
        match self.sample {
            SampleFormat::Pcm { rate, .. } => rate,
            SampleFormat::Dsd { multiplier } => 44_100 * multiplier,
        }
    }

    /// The PCM carrier rate a DAC sees when DSD is sent as DoP
    /// (DSD64 → 176.4 kHz, DSD128 → 352.8 kHz). PCM returns its own rate.
    pub fn dop_carrier_rate(&self) -> u32 {
        match self.sample {
            SampleFormat::Pcm { rate, .. } => rate,
            SampleFormat::Dsd { multiplier } => 44_100 * multiplier / 16,
        }
    }
}

impl fmt::Display for AudioFormat {
    /// Compact human form: `44.1kHz/24bit stereo`, `DSD64 stereo`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.sample {
            SampleFormat::Dsd { multiplier } => write!(f, "DSD{multiplier}")?,
            SampleFormat::Pcm { rate, bits } => {
                if rate % 1000 == 0 {
                    write!(f, "{}kHz", rate / 1000)?;
                } else {
                    write!(f, "{:.1}kHz", rate as f64 / 1000.0)?;
                }
                match bits {
                    PcmBits::Int(b) => write!(f, "/{b}bit")?,
                    PcmBits::Float => write!(f, "/float")?,
                }
            }
        }
        match self.channels {
            1 => write!(f, " mono"),
            2 => write!(f, " stereo"),
            n => write!(f, " {n}ch"),
        }
    }
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayState {
    #[default]
    Stop,
    Play,
    Pause,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Single {
    #[default]
    Off,
    On,
    Oneshot,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub state: PlayState,
    /// `None` when MPD has no mixer (bit-perfect setups report `-1`/absent).
    pub volume: Option<u8>,
    pub repeat: bool,
    pub random: bool,
    pub single: Single,
    pub consume: bool,
    pub playlist_version: u32,
    pub playlist_length: u32,
    pub song_pos: Option<u32>,
    pub song_id: Option<u32>,
    pub next_song_pos: Option<u32>,
    pub elapsed: Option<f64>,
    pub duration: Option<f64>,
    /// kbit/s.
    pub bitrate: Option<u32>,
    pub audio: Option<AudioFormat>,
    /// Raw `audio:` value, kept even when it cannot be parsed.
    pub audio_raw: Option<String>,
    pub updating_db: Option<u32>,
    pub error: Option<String>,
}

fn flag(v: &str) -> bool {
    v == "1"
}

impl Status {
    pub fn from_pairs(pairs: &[Pair]) -> Status {
        let mut s = Status::default();
        let mut legacy_time: Option<(f64, f64)> = None;
        for (k, v) in pairs {
            match k.as_str() {
                "state" => {
                    s.state = match v.as_str() {
                        "play" => PlayState::Play,
                        "pause" => PlayState::Pause,
                        _ => PlayState::Stop,
                    }
                }
                "volume" => s.volume = v.parse::<i32>().ok().and_then(|n| u8::try_from(n).ok()),
                "repeat" => s.repeat = flag(v),
                "random" => s.random = flag(v),
                "single" => {
                    s.single = match v.as_str() {
                        "1" => Single::On,
                        "oneshot" => Single::Oneshot,
                        _ => Single::Off,
                    }
                }
                "consume" => s.consume = v != "0",
                "playlist" => s.playlist_version = v.parse().unwrap_or(0),
                "playlistlength" => s.playlist_length = v.parse().unwrap_or(0),
                "song" => s.song_pos = v.parse().ok(),
                "songid" => s.song_id = v.parse().ok(),
                "nextsong" => s.next_song_pos = v.parse().ok(),
                "elapsed" => s.elapsed = v.parse().ok(),
                "duration" => s.duration = v.parse().ok(),
                "time" => {
                    legacy_time = v
                        .split_once(':')
                        .and_then(|(e, t)| Some((e.parse().ok()?, t.parse().ok()?)))
                }
                "bitrate" => s.bitrate = v.parse().ok().filter(|&b| b > 0),
                "audio" => {
                    s.audio = AudioFormat::parse(v);
                    s.audio_raw = Some(v.clone());
                }
                "updating_db" => s.updating_db = v.parse().ok(),
                "error" => s.error = Some(v.clone()),
                _ => {}
            }
        }
        if let Some((e, t)) = legacy_time {
            s.elapsed.get_or_insert(e);
            s.duration.get_or_insert(t);
        }
        s
    }

    pub fn is_playing(&self) -> bool {
        self.state == PlayState::Play
    }
}

// ---------------------------------------------------------------------------
// Songs and directory entries
// ---------------------------------------------------------------------------

/// A song as reported by `currentsong`, `playlistinfo`, `listallinfo`, etc.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Song {
    /// URI relative to MPD's music directory.
    pub file: String,
    pub duration: Option<f64>,
    /// Queue position (only for queue items).
    pub pos: Option<u32>,
    /// Queue id (only for queue items).
    pub id: Option<u32>,
    pub last_modified: Option<String>,
    /// Audio format of the file (`Format:`), when MPD knows it.
    pub format: Option<AudioFormat>,
    /// Tags keyed by lower-case name; repeated tags keep every value in order.
    pub tags: BTreeMap<String, Vec<String>>,
}

impl Song {
    fn new(file: String) -> Song {
        Song {
            file,
            ..Default::default()
        }
    }

    fn apply(&mut self, k: &str, v: &str) {
        match k {
            "duration" => self.duration = v.parse().ok(),
            "Time" => {
                if self.duration.is_none() {
                    self.duration = v.parse().ok();
                }
            }
            "Pos" => self.pos = v.parse().ok(),
            "Id" => self.id = v.parse().ok(),
            "Last-Modified" => self.last_modified = Some(v.to_owned()),
            "Format" => self.format = AudioFormat::parse(v),
            "Range" | "Prio" | "Added" => {}
            _ => self
                .tags
                .entry(k.to_ascii_lowercase())
                .or_default()
                .push(v.to_owned()),
        }
    }

    /// First value of a tag (case-insensitive name).
    pub fn tag(&self, name: &str) -> Option<&str> {
        self.tags
            .get(&name.to_ascii_lowercase())
            .and_then(|v| v.first())
            .map(String::as_str)
            .filter(|s| !s.is_empty())
    }

    /// All values of a tag.
    pub fn tag_values(&self, name: &str) -> &[String] {
        self.tags
            .get(&name.to_ascii_lowercase())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn title(&self) -> Option<&str> {
        self.tag("title")
    }
    pub fn artist(&self) -> Option<&str> {
        self.tag("artist")
    }
    pub fn album(&self) -> Option<&str> {
        self.tag("album")
    }
    pub fn album_artist(&self) -> Option<&str> {
        self.tag("albumartist")
    }
    pub fn date(&self) -> Option<&str> {
        self.tag("date")
    }
    pub fn original_date(&self) -> Option<&str> {
        self.tag("originaldate")
    }
    pub fn track(&self) -> Option<&str> {
        self.tag("track")
    }
    pub fn disc(&self) -> Option<&str> {
        self.tag("disc")
    }
    pub fn composer(&self) -> Option<&str> {
        self.tag("composer")
    }
    pub fn label(&self) -> Option<&str> {
        self.tag("label")
    }
    pub fn genres(&self) -> &[String] {
        self.tag_values("genre")
    }
    pub fn musicbrainz_album_id(&self) -> Option<&str> {
        self.tag("musicbrainz_albumid")
    }
    pub fn musicbrainz_track_id(&self) -> Option<&str> {
        self.tag("musicbrainz_trackid")
    }

    /// Title, falling back to the file name.
    pub fn display_title(&self) -> &str {
        self.title()
            .unwrap_or_else(|| self.file.rsplit('/').next().unwrap_or(&self.file))
    }

    /// Lower-case file extension (`flac`, `dsf`, …).
    pub fn extension(&self) -> Option<String> {
        let name = self.file.rsplit('/').next()?;
        let (_, ext) = name.rsplit_once('.')?;
        Some(ext.to_ascii_lowercase())
    }

    /// Parent directory of the file within the music directory.
    pub fn directory(&self) -> &str {
        self.file.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
    }
}

/// An entry of `lsinfo`/`listallinfo` output.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Song(Song),
    Directory {
        path: String,
        last_modified: Option<String>,
    },
    Playlist {
        path: String,
        last_modified: Option<String>,
    },
}

impl Entry {
    /// Split a flat response into entries. Each `file:`, `directory:` or
    /// `playlist:` key starts a new entry, so metadata never leaks between
    /// a directory and the song that follows it.
    pub fn parse_all(pairs: &[Pair]) -> Vec<Entry> {
        let mut out: Vec<Entry> = Vec::new();
        for (k, v) in pairs {
            match k.as_str() {
                "file" => out.push(Entry::Song(Song::new(v.clone()))),
                "directory" => out.push(Entry::Directory {
                    path: v.clone(),
                    last_modified: None,
                }),
                "playlist" => out.push(Entry::Playlist {
                    path: v.clone(),
                    last_modified: None,
                }),
                _ => match out.last_mut() {
                    Some(Entry::Song(s)) => s.apply(k, v),
                    Some(
                        Entry::Directory { last_modified, .. }
                        | Entry::Playlist { last_modified, .. },
                    ) if k == "Last-Modified" => *last_modified = Some(v.clone()),
                    _ => {}
                },
            }
        }
        out
    }

    pub fn songs(pairs: &[Pair]) -> Vec<Song> {
        Entry::parse_all(pairs)
            .into_iter()
            .filter_map(|e| match e {
                Entry::Song(s) => Some(s),
                _ => None,
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Stats and outputs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stats {
    pub artists: u32,
    pub albums: u32,
    pub songs: u32,
    pub uptime: u64,
    pub playtime: u64,
    pub db_playtime: u64,
    /// Unix time of the last database update.
    pub db_update: Option<u64>,
}

impl Stats {
    pub fn from_pairs(pairs: &[Pair]) -> Stats {
        let mut s = Stats::default();
        for (k, v) in pairs {
            match k.as_str() {
                "artists" => s.artists = v.parse().unwrap_or(0),
                "albums" => s.albums = v.parse().unwrap_or(0),
                "songs" => s.songs = v.parse().unwrap_or(0),
                "uptime" => s.uptime = v.parse().unwrap_or(0),
                "playtime" => s.playtime = v.parse().unwrap_or(0),
                "db_playtime" => s.db_playtime = v.parse().unwrap_or(0),
                "db_update" => s.db_update = v.parse().ok(),
                _ => {}
            }
        }
        s
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    pub id: u32,
    pub name: String,
    pub plugin: String,
    pub enabled: bool,
    /// Runtime attributes, e.g. `dop`, `allowed_formats`.
    pub attributes: BTreeMap<String, String>,
}

impl Output {
    pub fn parse_all(pairs: &[Pair]) -> Vec<Output> {
        let mut out: Vec<Output> = Vec::new();
        for (k, v) in pairs {
            if k == "outputid" {
                out.push(Output {
                    id: v.parse().unwrap_or(0),
                    ..Default::default()
                });
                continue;
            }
            let Some(o) = out.last_mut() else { continue };
            match k.as_str() {
                "outputname" => o.name = v.clone(),
                "plugin" => o.plugin = v.clone(),
                "outputenabled" => o.enabled = flag(v),
                "attribute" => {
                    if let Some((ak, av)) = v.split_once('=') {
                        o.attributes.insert(ak.to_owned(), av.to_owned());
                    }
                }
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(text: &str) -> Vec<Pair> {
        text.lines()
            .filter_map(|l| l.split_once(": "))
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect()
    }

    #[test]
    fn audio_format_pcm() {
        let f = AudioFormat::parse("44100:24:2").unwrap();
        assert_eq!(
            f.sample,
            SampleFormat::Pcm {
                rate: 44100,
                bits: PcmBits::Int(24)
            }
        );
        assert_eq!(f.to_string(), "44.1kHz/24bit stereo");
        assert_eq!(
            AudioFormat::parse("96000:f:2").unwrap().to_string(),
            "96kHz/float stereo"
        );
        assert_eq!(
            AudioFormat::parse("48000:16:6").unwrap().to_string(),
            "48kHz/16bit 6ch"
        );
    }

    #[test]
    fn audio_format_dsd() {
        let f = AudioFormat::parse("dsd64:2").unwrap();
        assert!(f.is_dsd());
        assert_eq!(f.sample_rate(), 2_822_400);
        assert_eq!(f.dop_carrier_rate(), 176_400);
        assert_eq!(
            AudioFormat::parse("dsd128:2").unwrap().dop_carrier_rate(),
            352_800
        );
        assert_eq!(
            AudioFormat::parse("dsd256:2").unwrap().dop_carrier_rate(),
            705_600
        );
        assert_eq!(f.to_string(), "DSD64 stereo");
    }

    #[test]
    fn audio_format_garbage() {
        assert!(AudioFormat::parse("").is_none());
        assert!(AudioFormat::parse("44100").is_none());
        assert!(AudioFormat::parse("dsdx:2").is_none());
    }

    #[test]
    fn status_parses_bitperfect_server() {
        let s = Status::from_pairs(&pairs(
            "volume: -1\nrepeat: 1\nrandom: 0\nsingle: oneshot\nconsume: 0\n\
             playlist: 42\nplaylistlength: 12\nstate: play\nsong: 3\nsongid: 7\n\
             elapsed: 61.250\nduration: 312.4\nbitrate: 2822\naudio: dsd64:2\n",
        ));
        assert_eq!(s.state, PlayState::Play);
        assert_eq!(s.volume, None);
        assert!(s.repeat && !s.random);
        assert_eq!(s.single, Single::Oneshot);
        assert_eq!(s.playlist_version, 42);
        assert_eq!(s.song_pos, Some(3));
        assert_eq!(s.elapsed, Some(61.25));
        assert_eq!(s.bitrate, Some(2822));
        assert!(s.audio.unwrap().is_dsd());
        assert_eq!(s.audio_raw.as_deref(), Some("dsd64:2"));
    }

    #[test]
    fn status_legacy_time_and_zero_bitrate() {
        let s = Status::from_pairs(&pairs("state: pause\ntime: 10:200\nbitrate: 0\n"));
        assert_eq!(s.elapsed, Some(10.0));
        assert_eq!(s.duration, Some(200.0));
        assert_eq!(s.bitrate, None);
    }

    #[test]
    fn entries_do_not_leak_between_records() {
        let p = pairs(
            "directory: Rock\nLast-Modified: 2024-01-01T00:00:00Z\n\
             file: Rock/a.flac\nTitle: A\nArtist: X\nGenre: Rock\nGenre: Blues\nTime: 100\nduration: 100.5\n\
             directory: Rock/Sub\n\
             file: Rock/Sub/b.dsf\nTitle: B\nFormat: dsd64:2\nMUSICBRAINZ_ALBUMID: abc\n\
             playlist: mix.m3u\n",
        );
        let e = Entry::parse_all(&p);
        assert_eq!(e.len(), 5);
        assert!(
            matches!(&e[0], Entry::Directory { path, last_modified: Some(_) } if path == "Rock")
        );
        let Entry::Song(a) = &e[1] else { panic!() };
        assert_eq!(a.title(), Some("A"));
        assert_eq!(a.genres(), ["Rock", "Blues"]);
        assert_eq!(a.duration, Some(100.5));
        assert_eq!(a.directory(), "Rock");
        assert_eq!(a.extension().as_deref(), Some("flac"));
        let Entry::Song(b) = &e[3] else { panic!() };
        assert_eq!(b.artist(), None);
        assert_eq!(b.musicbrainz_album_id(), Some("abc"));
        assert!(b.format.unwrap().is_dsd());
        assert_eq!(Entry::songs(&p).len(), 2);
    }

    #[test]
    fn song_display_title_falls_back_to_filename() {
        let s = Song::new("A/B/03 - Track.flac".into());
        assert_eq!(s.display_title(), "03 - Track.flac");
    }

    #[test]
    fn outputs_with_attributes() {
        let o = Output::parse_all(&pairs(
            "outputid: 0\noutputname: DAC\nplugin: alsa\noutputenabled: 1\n\
             attribute: dop=1\nattribute: allowed_formats=\n\
             outputid: 1\noutputname: Visualizer\nplugin: fifo\noutputenabled: 0\n",
        ));
        assert_eq!(o.len(), 2);
        assert_eq!(o[0].plugin, "alsa");
        assert!(o[0].enabled);
        assert_eq!(o[0].attributes.get("dop").map(String::as_str), Some("1"));
        assert_eq!(
            o[0].attributes.get("allowed_formats").map(String::as_str),
            Some("")
        );
        assert!(!o[1].enabled);
    }

    #[test]
    fn stats() {
        let s = Stats::from_pairs(&pairs("songs: 12000\nalbums: 900\ndb_update: 1700000000\n"));
        assert_eq!(s.songs, 12000);
        assert_eq!(s.db_update, Some(1_700_000_000));
    }
}
