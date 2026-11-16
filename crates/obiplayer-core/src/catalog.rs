//! Indexed view over the library: lookups, versions of a track, other
//! editions of an album, browse aggregations and search.

use std::collections::HashMap;
use std::sync::Arc;

use crate::library::Album;
use crate::mpd::Song;
use crate::text::{self, fold};

/// Position of a track inside the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TrackRef {
    pub album: u32,
    pub track: u32,
}

/// An artist, label or composer with the albums/tracks that carry it.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub name: String,
    pub albums: Vec<u32>,
    pub tracks: Vec<TrackRef>,
    pub genres: Vec<String>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SearchResults {
    pub artists: Vec<(String, usize, usize)>,
    pub albums: Vec<u32>,
    pub tracks: Vec<TrackRef>,
    pub total_artists: usize,
    pub total_albums: usize,
    pub total_tracks: usize,
}

impl SearchResults {
    pub fn is_empty(&self) -> bool {
        self.total_artists + self.total_albums + self.total_tracks == 0
    }
}

#[derive(Debug, Default)]
pub struct Library {
    pub albums: Vec<Arc<Album>>,
    by_folder: HashMap<String, u32>,
    by_file: HashMap<String, TrackRef>,
    versions: HashMap<String, Vec<TrackRef>>,
}

/// Strip trailing `(…)` / `[…]` groups and a ` - remaster…` suffix.
fn strip_edition_noise(s: &str) -> &str {
    let mut t = s.trim_end();
    loop {
        let before = t;
        for (open, close) in [('(', ')'), ('[', ']')] {
            if t.ends_with(close)
                && let Some(i) = t.rfind(open)
            {
                t = t[..i].trim_end();
            }
        }
        if t == before {
            break;
        }
    }
    t
}

fn strip_remaster(s: &str) -> &str {
    let lower = s.to_lowercase();
    // Byte offsets stay valid: lower-casing ASCII "- remaster" keeps lengths
    // for the part we search, and we only cut at the match start.
    if let Some(i) = lower
        .find(" - remaster")
        .or_else(|| lower.find("- remaster"))
        && s.is_char_boundary(i)
    {
        return s[..i].trim_end();
    }
    s
}

/// Key under which different recordings/releases of the same song match.
pub fn version_key(title: &str, artist: &str) -> String {
    let t = fold(title);
    let t = strip_remaster(strip_edition_noise(&t));
    let t = strip_edition_noise(t);
    format!("{}|{}", fold(artist), t)
}

/// Key under which different editions of the same album match.
pub fn edition_key(album: &Album) -> String {
    format!(
        "{}|{}",
        fold(&album.artist),
        strip_edition_noise(&fold(&album.title))
    )
}

fn track_artist<'a>(album: &'a Album, song: &'a Song) -> &'a str {
    song.artist().unwrap_or(&album.artist)
}

impl Library {
    pub fn new(albums: Vec<Arc<Album>>) -> Library {
        let mut lib = Library {
            albums,
            ..Default::default()
        };
        for (ai, a) in lib.albums.iter().enumerate() {
            lib.by_folder.insert(a.folder.clone(), ai as u32);
            for (ti, t) in a.tracks.iter().enumerate() {
                let r = TrackRef {
                    album: ai as u32,
                    track: ti as u32,
                };
                lib.by_file.insert(t.file.clone(), r);
                let key = version_key(t.display_title(), track_artist(a, t));
                lib.versions.entry(key).or_default().push(r);
            }
        }
        lib
    }

    pub fn track_count(&self) -> usize {
        self.by_file.len()
    }

    pub fn album_index(&self, folder: &str) -> Option<u32> {
        self.by_folder.get(folder).copied()
    }

    pub fn album_by_folder(&self, folder: &str) -> Option<&Arc<Album>> {
        self.album_index(folder).map(|i| &self.albums[i as usize])
    }

    pub fn album(&self, i: u32) -> &Arc<Album> {
        &self.albums[i as usize]
    }

    pub fn song(&self, r: TrackRef) -> &Song {
        &self.albums[r.album as usize].tracks[r.track as usize]
    }

    pub fn find_file(&self, file: &str) -> Option<TrackRef> {
        self.by_file.get(file).copied()
    }

    /// All versions of the track at `r`, itself included.
    pub fn versions(&self, r: TrackRef) -> &[TrackRef] {
        let a = self.album(r.album);
        let s = self.song(r);
        self.versions
            .get(&version_key(s.display_title(), track_artist(a, s)))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Number of *other* versions of a track.
    pub fn version_count(&self, r: TrackRef) -> usize {
        self.versions(r).len().saturating_sub(1)
    }

    /// Other editions of an album (same artist and title, different folder),
    /// sorted by format.
    pub fn other_editions(&self, album: &Album) -> Vec<u32> {
        let key = edition_key(album);
        let mut v: Vec<u32> = self
            .albums
            .iter()
            .enumerate()
            .filter(|(_, a)| a.folder != album.folder && edition_key(a) == key)
            .map(|(i, _)| i as u32)
            .collect();
        v.sort_by_key(|&i| self.album(i).format());
        v
    }

    fn group_albums_by(&self, key: impl Fn(&Album) -> Option<String>) -> Vec<Group> {
        let mut map: HashMap<String, Group> = HashMap::new();
        for (i, a) in self.albums.iter().enumerate() {
            let Some(name) = key(a) else { continue };
            let g = map.entry(fold(&name)).or_insert_with(|| Group {
                name: name.clone(),
                albums: Vec::new(),
                tracks: Vec::new(),
                genres: Vec::new(),
            });
            g.albums.push(i as u32);
            g.tracks.extend((0..a.tracks.len()).map(|t| TrackRef {
                album: i as u32,
                track: t as u32,
            }));
            for genre in &a.genres {
                if !g.genres.iter().any(|x| fold(x) == fold(genre)) {
                    g.genres.push(genre.clone());
                }
            }
        }
        let mut v: Vec<Group> = map.into_values().collect();
        v.sort_by(|a, b| text::compare(&a.name, &b.name));
        v
    }

    /// Album artists (album artist, falling back to artist).
    pub fn artists(&self) -> Vec<Group> {
        self.group_albums_by(|a| (!a.artist.is_empty()).then(|| a.artist.clone()))
    }

    /// Record labels; albums without one are grouped under "Unknown".
    pub fn labels(&self) -> Vec<Group> {
        self.group_albums_by(|a| {
            Some(
                a.label
                    .clone()
                    .filter(|l| !l.is_empty())
                    .unwrap_or_else(|| "Unknown".into()),
            )
        })
    }

    /// Composers, from track tags.
    pub fn composers(&self) -> Vec<Group> {
        let mut map: HashMap<String, Group> = HashMap::new();
        for (ai, a) in self.albums.iter().enumerate() {
            for (ti, t) in a.tracks.iter().enumerate() {
                let Some(c) = t.composer() else { continue };
                let g = map.entry(fold(c)).or_insert_with(|| Group {
                    name: c.to_owned(),
                    albums: Vec::new(),
                    tracks: Vec::new(),
                    genres: Vec::new(),
                });
                if g.albums.last() != Some(&(ai as u32)) {
                    g.albums.push(ai as u32);
                }
                g.tracks.push(TrackRef {
                    album: ai as u32,
                    track: ti as u32,
                });
                for genre in t
                    .genres()
                    .iter()
                    .flat_map(|x| crate::library::split_genres(x))
                {
                    if !g.genres.iter().any(|x| fold(x) == fold(&genre)) {
                        g.genres.push(genre);
                    }
                }
            }
        }
        let mut v: Vec<Group> = map.into_values().collect();
        v.sort_by(|a, b| text::compare(&a.name, &b.name));
        v
    }

    /// Local search over artists, albums and tracks (case/accent-insensitive).
    /// Queries shorter than two characters return nothing.
    pub fn search(&self, query: &str) -> SearchResults {
        let q = fold(query.trim());
        if q.chars().count() < 2 {
            return SearchResults::default();
        }
        let hit = |s: &str| fold(s).contains(&q);

        let mut albums = Vec::new();
        let mut tracks = Vec::new();
        // folded name -> (display, albums, tracks)
        let mut artists: HashMap<String, (String, usize, usize)> = HashMap::new();

        for (ai, a) in self.albums.iter().enumerate() {
            if hit(&a.title) || hit(&a.artist) || a.genres.iter().any(|g| hit(g)) {
                albums.push(ai as u32);
            }
            if !a.artist.is_empty() && hit(&a.artist) {
                artists
                    .entry(fold(&a.artist))
                    .or_insert_with(|| (a.artist.clone(), 0, 0))
                    .1 += 1;
            }
            for (ti, t) in a.tracks.iter().enumerate() {
                let artist = track_artist(a, t);
                if hit(t.display_title()) || hit(artist) {
                    tracks.push(TrackRef {
                        album: ai as u32,
                        track: ti as u32,
                    });
                }
                if !artist.is_empty() && hit(artist) {
                    artists
                        .entry(fold(artist))
                        .or_insert_with(|| (artist.to_owned(), 0, 0))
                        .2 += 1;
                }
            }
        }

        let mut artists: Vec<_> = artists.into_values().collect();
        artists.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| text::compare(&a.0, &b.0)));

        SearchResults {
            total_artists: artists.len(),
            total_albums: albums.len(),
            total_tracks: tracks.len(),
            artists: artists.into_iter().take(10).collect(),
            albums: albums.into_iter().take(20).collect(),
            tracks: tracks.into_iter().take(30).collect(),
        }
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

    const DATA: &str = "\
file: Queen/Opera (SHM-CD)/01.flac\nAlbum: A Night at the Opera (SHM-CD)\nAlbumArtist: Queen\nTitle: Bohemian Rhapsody\nComposer: Mercury\nLabel: EMI\nGenre: Rock\n\
file: Queen/Opera (SHM-CD)/02.flac\nAlbum: A Night at the Opera (SHM-CD)\nAlbumArtist: Queen\nTitle: Love of My Life\nComposer: Mercury\nLabel: EMI\n\
file: Queen/Opera SACD/01.dsf\nAlbum: A Night at the Opera\nAlbumArtist: Queen\nTitle: Bohemian Rhapsody - Remastered 2011\nLabel: Universal\n\
file: Queen/Greatest/01.flac\nAlbum: Greatest Hits\nAlbumArtist: Queen\nTitle: Bohemian Rhapsody (Live)\n\
file: Other/Song/01.mp3\nAlbum: Covers\nAlbumArtist: Various\nArtist: Barón Rojo\nTitle: Bohemian Rhapsody\n";

    #[test]
    fn version_keys() {
        assert_eq!(
            version_key("Bohemian Rhapsody - Remastered 2011", "Queen"),
            version_key("Bohemian Rhapsody (Live) [Bonus]", "QUEEN")
        );
        assert_ne!(version_key("Song", "A"), version_key("Song", "B"));
    }

    #[test]
    fn versions_and_editions() {
        let l = lib(DATA);
        let r = l.find_file("Queen/Opera (SHM-CD)/01.flac").unwrap();
        // Same artist only: the Barón Rojo cover is not a version.
        assert_eq!(l.version_count(r), 2);
        let opera = l.album_by_folder("Queen/Opera (SHM-CD)").unwrap();
        let eds = l.other_editions(opera);
        assert_eq!(eds.len(), 1);
        assert_eq!(l.album(eds[0]).folder, "Queen/Opera SACD");
    }

    #[test]
    fn aggregations() {
        let l = lib(DATA);
        let artists = l.artists();
        assert_eq!(
            artists.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(),
            ["Queen", "Various"]
        );
        assert_eq!(artists[0].albums.len(), 3);
        let labels = l.labels();
        assert!(
            labels
                .iter()
                .any(|g| g.name == "Unknown" && g.albums.len() == 2)
        );
        let comp = l.composers();
        assert_eq!(comp.len(), 1);
        assert_eq!(comp[0].tracks.len(), 2);
        assert_eq!(comp[0].albums.len(), 1);
        assert_eq!(comp[0].genres, ["Rock"]);
    }

    #[test]
    fn search() {
        let l = lib(DATA);
        assert!(l.search("q").is_empty());
        let r = l.search("baron");
        assert_eq!(r.total_tracks, 1);
        assert_eq!(r.artists[0].0, "Barón Rojo");
        let r = l.search("opera");
        assert_eq!(r.total_albums, 2);
        let r = l.search("queen");
        assert_eq!(r.artists[0], ("Queen".into(), 3, 4));
    }
}
