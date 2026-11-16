//! Library model: songs from MPD grouped into albums.
//!
//! An album is a directory: every song whose parent directory is the same
//! belongs to the same album (CUE sheets — `Album/disc.cue/track0001` — are
//! attributed to the directory holding the sheet). This matches how editions
//! are stored on disk: two pressings of the same record live in two folders
//! and are two albums.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::mpd::Song;
use crate::text;

#[derive(Debug, Clone, PartialEq)]
pub struct Album {
    /// Directory relative to MPD's music directory; unique, used as id.
    pub folder: String,
    pub title: String,
    pub artist: String,
    pub date: Option<String>,
    pub original_date: Option<String>,
    pub label: Option<String>,
    pub catalog_number: Option<String>,
    pub country: Option<String>,
    pub musicbrainz_album_id: Option<String>,
    pub genres: Vec<String>,
    pub composers: Vec<String>,
    pub tracks: Vec<Song>,
}

impl Album {
    /// Total length in seconds.
    pub fn total_duration(&self) -> f64 {
        self.tracks.iter().filter_map(|t| t.duration).sum()
    }

    /// `"1h 12min"` or `"43 min"`.
    pub fn formatted_duration(&self) -> String {
        let mins = (self.total_duration() / 60.0).round() as u64;
        if mins >= 60 {
            format!("{}h {}min", mins / 60, mins % 60)
        } else {
            format!("{mins} min")
        }
    }

    /// Upper-case file extensions, e.g. `"DSF"` or `"FLAC / MP3"`.
    pub fn format(&self) -> String {
        self.tracks
            .iter()
            .filter_map(Song::extension)
            .map(|e| e.to_ascii_uppercase())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(" / ")
    }

    /// Year from the date tag.
    pub fn year(&self) -> Option<&str> {
        self.date.as_deref().and_then(|d| d.get(..4))
    }

    /// First track's file, used to ask MPD for cover art.
    pub fn first_file(&self) -> Option<&str> {
        self.tracks.first().map(|t| t.file.as_str())
    }
}

/// The album folder a song belongs to.
pub fn album_key(file: &str) -> &str {
    let dir = parent(file);
    if dir.to_ascii_lowercase().ends_with(".cue") {
        parent(dir)
    } else {
        dir
    }
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

/// Split a raw genre tag ("Rock / Hard Rock; Blues") into clean names.
pub fn split_genres(raw: &str) -> Vec<String> {
    let mut parts = vec![raw.to_owned()];
    for sep in [" / ", ", ", "/", ";"] {
        parts = parts
            .iter()
            .flat_map(|p| p.split(sep).map(str::to_owned))
            .collect();
    }
    parts
        .into_iter()
        .map(|p| {
            p.trim_matches(|c: char| c.is_whitespace() || c == '*')
                .to_owned()
        })
        .filter(|p| !p.is_empty())
        .map(|p| text::capitalize_words(&p))
        .collect()
}

fn push_unique_ci(list: &mut Vec<String>, value: &str) {
    let f = text::fold(value);
    if !list.iter().any(|v| text::fold(v) == f) {
        list.push(value.to_owned());
    }
}

/// Group songs into albums, sorted by title (case/accent-insensitive).
pub fn group_albums(songs: Vec<Song>) -> Vec<Arc<Album>> {
    let mut order: Vec<String> = Vec::new();
    let mut map: std::collections::HashMap<String, Album> = std::collections::HashMap::new();

    for song in songs {
        let key = album_key(&song.file).to_owned();
        let album = map.entry(key.clone()).or_insert_with(|| {
            order.push(key.clone());
            let own = |v: Option<&str>| v.map(str::to_owned);
            Album {
                title: song
                    .album()
                    .map(str::to_owned)
                    .unwrap_or_else(|| key.rsplit('/').next().unwrap_or(&key).to_owned()),
                artist: song
                    .album_artist()
                    .or(song.artist())
                    .unwrap_or_default()
                    .to_owned(),
                date: own(song.date()),
                original_date: own(song.original_date()),
                label: own(song.label()),
                catalog_number: own(song.tag("catalognumber")),
                country: own(song.tag("releasecountry")),
                musicbrainz_album_id: own(song.musicbrainz_album_id()),
                folder: key.clone(),
                genres: Vec::new(),
                composers: Vec::new(),
                tracks: Vec::new(),
            }
        });
        for g in song.genres().iter().flat_map(|g| split_genres(g)) {
            push_unique_ci(&mut album.genres, &g);
        }
        if let Some(c) = song.composer() {
            push_unique_ci(&mut album.composers, c);
        }
        album.tracks.push(song);
    }

    let mut albums: Vec<Arc<Album>> = order
        .into_iter()
        .filter_map(|k| map.remove(&k))
        .map(Arc::new)
        .collect();
    albums.sort_by(|a, b| text::compare(&a.title, &b.title));
    albums
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mpd::Entry;

    fn songs(text: &str) -> Vec<Song> {
        let pairs: Vec<_> = text
            .lines()
            .filter_map(|l| l.split_once(": "))
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        Entry::songs(&pairs)
    }

    #[test]
    fn cue_tracks_belong_to_the_sheet_folder() {
        assert_eq!(album_key("Rock/Album/CD1.cue/track0001"), "Rock/Album");
        assert_eq!(album_key("Rock/Album/01.flac"), "Rock/Album");
        assert_eq!(album_key("loose.flac"), "");
    }

    #[test]
    fn genre_splitting() {
        assert_eq!(
            split_genres("rock / hard rock; blues, *jazz*"),
            ["Rock", "Hard Rock", "Blues", "Jazz"]
        );
        assert_eq!(split_genres("UK garage"), ["UK Garage"]);
        assert!(split_genres(" ; ").is_empty());
    }

    #[test]
    fn grouping() {
        let albums = group_albums(songs(
            "file: B/Zeta/01.dsf\nAlbum: Zeta\nArtist: X\nTime: 600\n\
             file: A/Alpha (SHM-CD)/01.flac\nAlbum: Alpha\nAlbumArtist: Band\nArtist: Singer\n\
             Date: 1984-05-01\nGenre: Rock\nComposer: Smith\nLABEL: Epic\nCATALOGNUMBER: ESCA-1\nTime: 3000\n\
             file: A/Alpha (SHM-CD)/02.flac\nAlbum: Alpha\nGenre: rock/Blues\nComposer: Jones\nTime: 1500\n\
             file: A/Alpha (SHM-CD)/03.mp3\nTime: 100\n\
             file: C/NoTags/01.flac\n",
        ));
        assert_eq!(albums.len(), 3);
        let a = &albums[0];
        assert_eq!(a.title, "Alpha");
        assert_eq!(a.artist, "Band");
        assert_eq!(a.year(), Some("1984"));
        assert_eq!(a.label.as_deref(), Some("Epic"));
        assert_eq!(a.catalog_number.as_deref(), Some("ESCA-1"));
        assert_eq!(a.genres, ["Rock", "Blues"]);
        assert_eq!(a.composers, ["Smith", "Jones"]);
        assert_eq!(a.tracks.len(), 3);
        assert_eq!(a.format(), "FLAC / MP3");
        assert_eq!(a.formatted_duration(), "1h 17min");
        assert_eq!(albums[1].title, "NoTags");
        assert_eq!(albums[1].artist, "");
        assert_eq!(albums[2].format(), "DSF");
        assert_eq!(albums[2].formatted_duration(), "10 min");
    }
}
