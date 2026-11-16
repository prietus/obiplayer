//! Listening statistics (the computations behind DrPlayer's `StatsView`).
//!
//! "All Time" uses the MPD `play_count` stickers (plays × track duration);
//! the other periods use the local play history filtered by date. Day of
//! week and hour of day always come from the (filtered) history.

use std::collections::{HashMap, HashSet};

use crate::catalog::Library;
use crate::history::Play;
use crate::library::album_key;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Period {
    Week,
    Month,
    Year,
    #[default]
    AllTime,
}

const DAY: i64 = 86_400;

/// Days since 1970-01-01 for a proleptic Gregorian date (H. Hinnant).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (
        if m <= 2 {
            yoe + era * 400 + 1
        } else {
            yoe + era * 400
        },
        m,
        d,
    )
}

fn days_in_month(y: i64, m: u32) -> u32 {
    let next = if m == 12 {
        days_from_civil(y + 1, 1, 1)
    } else {
        days_from_civil(y, m + 1, 1)
    };
    (next - days_from_civil(y, m, 1)) as u32
}

/// `ts` moved back by `months` calendar months, clamping the day of month
/// (Mar 31 − 1 month = Feb 28/29), keeping the time of day.
fn minus_months(ts: i64, months: i64) -> i64 {
    let days = ts.div_euclid(DAY);
    let secs = ts.rem_euclid(DAY);
    let (y, m, d) = civil_from_days(days);
    let total = y * 12 + i64::from(m) - 1 - months;
    let (ny, nm) = (total.div_euclid(12), (total.rem_euclid(12) + 1) as u32);
    let nd = d.min(days_in_month(ny, nm));
    days_from_civil(ny, nm, nd) * DAY + secs
}

impl Period {
    pub fn all() -> &'static [Period] {
        &[Period::Week, Period::Month, Period::Year, Period::AllTime]
    }

    /// Start of the period (Unix seconds): 7 days, one calendar month or one
    /// calendar year before `now`; `None` for all time.
    pub fn cutoff(self, now: i64) -> Option<i64> {
        match self {
            Period::Week => Some(now - 7 * DAY),
            Period::Month => Some(minus_months(now, 1)),
            Period::Year => Some(minus_months(now, 12)),
            Period::AllTime => None,
        }
    }

    /// DrPlayer's label ("Week", "Month", "Year", "All Time").
    pub fn label(self) -> &'static str {
        match self {
            Period::Week => "Week",
            Period::Month => "Month",
            Period::Year => "Year",
            Period::AllTime => "All Time",
        }
    }
}

pub struct StatsInput<'a> {
    pub lib: &'a Library,
    pub history: &'a [Play],
    /// `play_count` stickers (file → count).
    pub play_counts: &'a HashMap<String, u32>,
    /// `last_played` stickers (file → Unix seconds).
    pub last_played: &'a HashMap<String, i64>,
    /// Enriched genres by album folder.
    pub extra_genres: &'a HashMap<String, Vec<String>>,
    pub now: i64,
    /// Local time offset from UTC, for day-of-week and hour-of-day.
    pub utc_offset_secs: i32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    pub listening_secs: f64,
    pub plays: u32,
    pub tracks: usize,
    pub artists: usize,
    pub albums: usize,
    /// Top 6 genres by listening seconds.
    pub genres: Vec<(String, f64)>,
    /// Number of distinct genres (the donut's centre figure).
    pub genre_count: usize,
    /// Top 5 by listening seconds.
    pub top_artists: Vec<(String, f64)>,
    /// Top 5 (album index, listening seconds).
    pub top_albums: Vec<(u32, f64)>,
    /// Top 5 by listening seconds.
    pub top_labels: Vec<(String, f64)>,
    /// Listening hours per weekday, Monday first.
    pub weekday_hours: [f64; 7],
    /// Plays per local hour of day.
    pub hour_plays: [u32; 24],
    /// Top 10 (file, play count), always all-time.
    pub top_tracks: Vec<(String, u32)>,
    /// 8 most recent (file, last played), always all-time.
    pub recent: Vec<(String, i64)>,
}

/// Per-file data from the library.
struct TrackInfo<'a> {
    duration: f64,
    /// Track genre tag, else the album's first genre ("" if none).
    genre: &'a str,
    /// Track label, else the album's ("" if none).
    label: &'a str,
    artist: &'a str,
    album: u32,
}

fn track_info<'a>(input: &StatsInput<'a>, file: &str) -> Option<TrackInfo<'a>> {
    let lib = input.lib;
    let r = lib.find_file(file)?;
    let album = lib.album(r.album);
    let song = lib.song(r);
    let album_genre = album
        .genres
        .first()
        .or_else(|| input.extra_genres.get(&album.folder)?.first())
        .map(String::as_str)
        .unwrap_or("");
    Some(TrackInfo {
        duration: song.duration.unwrap_or(0.0),
        genre: song
            .genres()
            .first()
            .map(|g| g.trim())
            .filter(|g| !g.is_empty())
            .unwrap_or(album_genre),
        label: song.label().or(album.label.as_deref()).unwrap_or(""),
        artist: song.artist().unwrap_or(&album.artist),
        album: r.album,
    })
}

/// Sorted by value descending (ties by key), truncated to `n`.
fn top<K: Ord + Clone>(map: HashMap<K, f64>, n: usize) -> Vec<(K, f64)> {
    let mut v: Vec<(K, f64)> = map.into_iter().collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.truncate(n);
    v
}

/// Album identity: the library album when known, else "artist|album".
#[derive(Hash, PartialEq, Eq)]
enum AlbumId {
    Lib(u32),
    Tags(String),
}

/// Accumulates one period's aggregations.
#[derive(Default)]
struct Acc {
    listening: f64,
    plays: u32,
    tracks: HashSet<String>,
    artists: HashSet<String>,
    albums: HashSet<AlbumId>,
    genres: HashMap<String, f64>,
    artist_time: HashMap<String, f64>,
    album_time: HashMap<u32, f64>,
    label_time: HashMap<String, f64>,
}

impl Acc {
    fn add_genre_label(&mut self, info: Option<&TrackInfo>, secs: f64) {
        let genre = info.map_or("Unknown", |i| i.genre);
        if !genre.is_empty() {
            *self.genres.entry(genre.to_owned()).or_default() += secs;
        }
        if let Some(label) = info.map(|i| i.label).filter(|l| !l.is_empty()) {
            *self.label_time.entry(label.to_owned()).or_default() += secs;
        }
        if let Some(i) = info {
            *self.album_time.entry(i.album).or_default() += secs;
        }
    }
}

fn local_day_hour(ts: i64, offset: i32) -> (usize, usize) {
    let t = ts + i64::from(offset);
    let days = t.div_euclid(DAY);
    // 1970-01-01 was a Thursday (index 3 with Monday = 0).
    let weekday = (days + 3).rem_euclid(7) as usize;
    let hour = (t.rem_euclid(DAY) / 3600) as usize;
    (weekday, hour)
}

pub fn compute(input: &StatsInput, period: Period) -> Stats {
    let cutoff = period.cutoff(input.now);
    let filtered: Vec<&Play> = input
        .history
        .iter()
        .filter(|p| cutoff.is_none_or(|c| p.ts >= c))
        .collect();

    let mut acc = Acc::default();
    if period == Period::AllTime {
        for (file, &count) in input.play_counts {
            if count == 0 {
                continue;
            }
            let info = track_info(input, file);
            let secs = info.as_ref().map_or(0.0, |i| i.duration) * f64::from(count);
            acc.listening += secs;
            acc.plays += count;
            acc.tracks.insert(file.clone());
            // DrPlayer counts distinct raw artists here ("" included) but
            // shows "Unknown" in the ranking.
            let raw_artist = info.as_ref().map_or("", |i| i.artist);
            acc.artists.insert(raw_artist.to_owned());
            let name = if raw_artist.is_empty() {
                "Unknown"
            } else {
                raw_artist
            };
            *acc.artist_time.entry(name.to_owned()).or_default() += secs;
            acc.albums.insert(match &info {
                Some(i) => AlbumId::Lib(i.album),
                None => AlbumId::Tags("|".into()),
            });
            acc.add_genre_label(info.as_ref(), secs);
        }
    } else {
        for p in &filtered {
            let info = track_info(input, &p.file);
            acc.listening += p.duration;
            acc.plays += 1;
            acc.tracks.insert(p.file.clone());
            let name = if p.artist.is_empty() {
                "Unknown"
            } else {
                &p.artist
            };
            acc.artists.insert(name.to_owned());
            *acc.artist_time.entry(name.to_owned()).or_default() += p.duration;
            let album = input
                .lib
                .find_file(&p.file)
                .map(|r| r.album)
                .or_else(|| input.lib.album_index(album_key(&p.file)));
            acc.albums.insert(match album {
                Some(i) => AlbumId::Lib(i),
                None => AlbumId::Tags(format!("{}|{}", p.artist, p.album)),
            });
            acc.add_genre_label(info.as_ref(), p.duration);
            // A file no longer in the library still credits its folder's album.
            if info.is_none()
                && let Some(i) = album
            {
                *acc.album_time.entry(i).or_default() += p.duration;
            }
        }
    }

    let mut weekday_hours = [0.0; 7];
    let mut hour_plays = [0u32; 24];
    for p in &filtered {
        let (d, h) = local_day_hour(p.ts, input.utc_offset_secs);
        weekday_hours[d] += p.duration / 3600.0;
        hour_plays[h] += 1;
    }

    let mut counted: Vec<(&String, u32)> = input
        .play_counts
        .iter()
        .filter(|(_, c)| **c > 0)
        .map(|(f, c)| (f, *c))
        .collect();
    counted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let top_tracks = counted
        .iter()
        .take(10)
        .map(|(f, c)| ((*f).clone(), *c))
        .collect();
    let mut recent: Vec<(String, i64)> = counted
        .iter()
        .filter_map(|(f, _)| input.last_played.get(*f).map(|t| ((*f).clone(), *t)))
        .collect();
    recent.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    recent.truncate(8);

    let genre_count = acc.genres.len();
    Stats {
        listening_secs: acc.listening,
        plays: acc.plays,
        tracks: acc.tracks.len(),
        artists: acc.artists.len(),
        albums: acc.albums.len(),
        genres: top(acc.genres, 6),
        genre_count,
        top_artists: top(acc.artist_time, 5),
        top_albums: top(acc.album_time, 5),
        top_labels: top(acc.label_time, 5),
        weekday_hours,
        hour_plays,
        top_tracks,
        recent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::group_albums;
    use crate::mpd::Entry;

    fn lib(text: &str) -> Library {
        let pairs: Vec<_> = text
            .lines()
            .filter_map(|l| l.split_once(": "))
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        Library::new(group_albums(Entry::songs(&pairs)))
    }

    // Albums by title: Alpha (0), Beta (1).
    const DATA: &str = "\
file: R/Alpha/01.flac\nAlbum: Alpha\nAlbumArtist: Queen\nGenre: Rock\nLabel: EMI\nTime: 300\n\
file: R/Alpha/02.flac\nAlbum: Alpha\nAlbumArtist: Queen\nTime: 200\n\
file: J/Beta/01.flac\nAlbum: Beta\nAlbumArtist: Miles\nTime: 600\n";

    // 2024-01-15 12:00:00 UTC, a Monday.
    const NOW: i64 = 1_705_320_000;

    fn play(ts: i64, file: &str, artist: &str, album: &str, dur: f64) -> Play {
        Play {
            ts,
            file: file.into(),
            artist: artist.into(),
            album: album.into(),
            title: String::new(),
            duration: dur,
        }
    }

    #[test]
    fn calendar() {
        assert_eq!(civil_from_days(days_from_civil(2024, 2, 29)), (2024, 2, 29));
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        // 2024-03-31 10:00 − 1 month = 2024-02-29 10:00.
        let t = days_from_civil(2024, 3, 31) * DAY + 36_000;
        assert_eq!(
            minus_months(t, 1),
            days_from_civil(2024, 2, 29) * DAY + 36_000
        );
        assert_eq!(
            Period::Year.cutoff(t),
            Some(days_from_civil(2023, 3, 31) * DAY + 36_000)
        );
        assert_eq!(Period::Month.cutoff(NOW), Some(NOW - 31 * DAY));
        assert_eq!(Period::Week.cutoff(NOW), Some(NOW - 7 * DAY));
        assert_eq!(Period::AllTime.cutoff(NOW), None);
        assert_eq!(Period::AllTime.label(), "All Time");
    }

    #[test]
    fn weekday_and_hour_mapping() {
        // Thursday 1970-01-01 00:30 UTC.
        assert_eq!(local_day_hour(1800, 0), (3, 0));
        // UTC+2 → 02:30 Thursday; UTC−1 → 23:30 Wednesday.
        assert_eq!(local_day_hour(1800, 7200), (3, 2));
        assert_eq!(local_day_hour(1800, -3600), (2, 23));
        assert_eq!(local_day_hour(NOW, 0), (0, 12));
        assert_eq!(local_day_hour(NOW - DAY, 0), (6, 12));
    }

    #[test]
    fn periods_use_history() {
        let l = lib(DATA);
        let history = vec![
            play(NOW - 40 * DAY, "J/Beta/01.flac", "Miles", "Beta", 600.0),
            play(NOW - 3 * DAY, "R/Alpha/01.flac", "Queen", "Alpha", 300.0),
            play(NOW - DAY, "R/Alpha/02.flac", "", "Alpha", 200.0),
            play(NOW - 3600, "Gone/X/01.flac", "Ghost", "X", 100.0),
        ];
        let empty = HashMap::new();
        let input = StatsInput {
            lib: &l,
            history: &history,
            play_counts: &empty,
            last_played: &HashMap::new(),
            extra_genres: &HashMap::new(),
            now: NOW,
            utc_offset_secs: 0,
        };
        let s = compute(&input, Period::Week);
        assert_eq!(s.plays, 3);
        assert_eq!(s.listening_secs, 600.0);
        assert_eq!(s.tracks, 3);
        assert_eq!(s.artists, 3); // Queen, Unknown, Ghost
        assert_eq!(s.albums, 2); // Alpha, "Ghost|X"
        // Alpha/02 has no genre tag → album's first genre "Rock"; the
        // missing file → "Unknown".
        assert_eq!(
            s.genres,
            [("Rock".to_owned(), 500.0), ("Unknown".to_owned(), 100.0)]
        );
        assert_eq!(s.top_labels, [("EMI".to_owned(), 500.0)]);
        assert_eq!(s.top_albums, [(0, 500.0)]);
        assert_eq!(s.top_artists[0], ("Queen".to_owned(), 300.0));
        // NOW − 3 days = Friday, NOW − 1 day = Sunday, NOW − 1h = Monday.
        assert_eq!(s.weekday_hours[4], 300.0 / 3600.0);
        assert_eq!(s.weekday_hours[6], 200.0 / 3600.0);
        assert_eq!(s.weekday_hours[0], 100.0 / 3600.0);
        assert_eq!(s.hour_plays[11], 1);
        assert_eq!(s.hour_plays[12], 2);

        let m = compute(&input, Period::Year);
        assert_eq!(m.plays, 4);
        assert_eq!(m.top_albums[0], (1, 600.0));
    }

    #[test]
    fn all_time_uses_stickers() {
        let l = lib(DATA);
        let history = vec![play(NOW - 3600, "R/Alpha/01.flac", "Queen", "Alpha", 300.0)];
        let counts: HashMap<String, u32> = [
            ("R/Alpha/01.flac".to_owned(), 3),
            ("J/Beta/01.flac".to_owned(), 1),
            ("Gone/01.flac".to_owned(), 2),
            ("R/Alpha/02.flac".to_owned(), 0),
        ]
        .into();
        let last: HashMap<String, i64> = [
            ("R/Alpha/01.flac".to_owned(), 50),
            ("J/Beta/01.flac".to_owned(), 70),
            ("R/Alpha/02.flac".to_owned(), 99),
        ]
        .into();
        let extra: HashMap<String, Vec<String>> =
            [("J/Beta".to_owned(), vec!["Jazz".to_owned()])].into();
        let input = StatsInput {
            lib: &l,
            history: &history,
            play_counts: &counts,
            last_played: &last,
            extra_genres: &extra,
            now: NOW,
            utc_offset_secs: 0,
        };
        let s = compute(&input, Period::AllTime);
        assert_eq!(s.plays, 6);
        assert_eq!(s.listening_secs, 900.0 + 600.0);
        assert_eq!(s.tracks, 3);
        assert_eq!(s.artists, 3); // Queen, Miles, ""
        assert_eq!(s.albums, 3);
        assert_eq!(
            s.genres,
            [
                ("Rock".to_owned(), 900.0),
                ("Jazz".to_owned(), 600.0),
                ("Unknown".to_owned(), 0.0)
            ]
        );
        assert_eq!(s.genre_count, 3);
        assert_eq!(s.top_artists[2], ("Unknown".to_owned(), 0.0));
        assert_eq!(s.top_tracks[0], ("R/Alpha/01.flac".to_owned(), 3));
        assert_eq!(s.top_tracks.len(), 3);
        // Zero-count files are excluded from "recent" too.
        assert_eq!(
            s.recent,
            [
                ("J/Beta/01.flac".to_owned(), 70),
                ("R/Alpha/01.flac".to_owned(), 50)
            ]
        );
        // Hour/day charts still come from the history.
        assert_eq!(s.hour_plays[11], 1);
    }
}
