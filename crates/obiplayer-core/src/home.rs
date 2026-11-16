//! Home shelves (the section building in DrPlayer's `HomeView`).

use std::collections::{HashMap, HashSet};

use crate::catalog::Library;
use crate::history::Play;
use crate::library::album_key;
use crate::radio::Rng;
use crate::text::fold;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShelfKind {
    RecentlyPlayed,
    /// Display name of the genre.
    Genre(String),
    NotPlayed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shelf {
    pub kind: ShelfKind,
    /// Album indices into `Library::albums`.
    pub albums: Vec<u32>,
}

const MAX_RECENT: usize = 20;
const MAX_GENRES: usize = 8;
const MAX_PER_SHELF: usize = 12;
const MIN_GENRE_ALBUMS: usize = 2;

/// Album genres plus enriched ones, trimmed, non-empty, unique by fold.
fn genres_of<'a>(
    lib: &'a Library,
    i: u32,
    extra: &'a HashMap<String, Vec<String>>,
) -> Vec<&'a str> {
    let a = lib.album(i);
    let mut out: Vec<&str> = Vec::new();
    for g in a
        .genres
        .iter()
        .chain(extra.get(&a.folder).into_iter().flatten())
    {
        let g = g.trim();
        if !g.is_empty() && !out.iter().any(|x| fold(x) == fold(g)) {
            out.push(g);
        }
    }
    out
}

/// Build the Home shelves: recently played albums, top genres by listening
/// time and a random sample of albums never played. Empty shelves are left
/// out.
pub fn shelves(
    lib: &Library,
    history: &[Play],
    extra_genres: &HashMap<String, Vec<String>>,
    rng_seed: u64,
) -> Vec<Shelf> {
    let mut out = Vec::new();

    // Recently played: newest first (the log is append-only), one entry per
    // album folder.
    let mut seen: HashSet<&str> = HashSet::new();
    let mut recent: Vec<u32> = Vec::new();
    for p in history.iter().rev() {
        let folder = album_key(&p.file);
        if seen.insert(folder)
            && let Some(i) = lib.album_index(folder)
        {
            recent.push(i);
            if recent.len() >= MAX_RECENT {
                break;
            }
        }
    }
    if !recent.is_empty() {
        out.push(Shelf {
            kind: ShelfKind::RecentlyPlayed,
            albums: recent,
        });
    }

    // Plays per folder and listening time per genre (keyed by fold, the
    // first spelling seen is displayed).
    let mut folder_plays: HashMap<&str, u32> = HashMap::new();
    let mut genre_time: HashMap<String, (String, f64)> = HashMap::new();
    let mut album_genres: HashMap<u32, Vec<&str>> = HashMap::new();
    for p in history {
        let folder = album_key(&p.file);
        *folder_plays.entry(folder).or_default() += 1;
        if let Some(i) = lib.album_index(folder) {
            let gs = album_genres
                .entry(i)
                .or_insert_with(|| genres_of(lib, i, extra_genres));
            for g in gs.iter() {
                genre_time
                    .entry(fold(g))
                    .or_insert_with(|| ((*g).to_owned(), 0.0))
                    .1 += p.duration;
            }
        }
    }
    let mut top: Vec<(String, String, f64)> = genre_time
        .into_iter()
        .map(|(k, (name, t))| (k, name, t))
        .collect();
    top.sort_by(|a, b| b.2.total_cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    top.truncate(MAX_GENRES);

    let plays_of = |i: u32| {
        folder_plays
            .get(lib.album(i).folder.as_str())
            .copied()
            .unwrap_or(0)
    };
    for (key, name, _) in top {
        let mut matching: Vec<u32> = (0..lib.albums.len() as u32)
            .filter(|&i| {
                genres_of(lib, i, extra_genres)
                    .iter()
                    .any(|g| fold(g) == key)
            })
            .collect();
        // Stable: equally played albums keep library order.
        matching.sort_by_key(|&i| std::cmp::Reverse(plays_of(i)));
        matching.truncate(MAX_PER_SHELF);
        if matching.len() >= MIN_GENRE_ALBUMS {
            out.push(Shelf {
                kind: ShelfKind::Genre(name),
                albums: matching,
            });
        }
    }

    // Not played yet: random sample of albums whose folder is not in history.
    let mut unplayed: Vec<u32> = (0..lib.albums.len() as u32)
        .filter(|&i| !folder_plays.contains_key(lib.album(i).folder.as_str()))
        .collect();
    Rng::new(rng_seed).shuffle(&mut unplayed);
    unplayed.truncate(MAX_PER_SHELF);
    if !unplayed.is_empty() {
        out.push(Shelf {
            kind: ShelfKind::NotPlayed,
            albums: unplayed,
        });
    }
    out
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

    fn play(ts: i64, file: &str, dur: f64) -> Play {
        Play {
            ts,
            file: file.into(),
            artist: String::new(),
            album: String::new(),
            title: String::new(),
            duration: dur,
        }
    }

    /// Albums "A00".."A{n-1}" in folder "F{i}", genre given per album.
    fn albums(genres: &[&str]) -> Library {
        let mut s = String::new();
        for (i, g) in genres.iter().enumerate() {
            s.push_str(&format!("file: F{i:02}/01.flac\nAlbum: A{i:02}\n"));
            if !g.is_empty() {
                s.push_str(&format!("Genre: {g}\n"));
            }
            s.push_str(&format!(
                "file: F{i:02}/CD1.cue/track0001\nAlbum: A{i:02}\n"
            ));
        }
        lib(&s)
    }

    fn kinds(sh: &[Shelf]) -> Vec<&ShelfKind> {
        sh.iter().map(|s| &s.kind).collect()
    }

    #[test]
    fn recent_dedupes_by_folder_and_caps() {
        let l = albums(&["Rock"; 25]);
        let mut h: Vec<Play> = (0..25)
            .map(|i| play(i, &format!("F{i:02}/01.flac"), 1.0))
            .collect();
        // A CUE track of F03 played last counts for folder F03.
        h.push(play(99, "F03/CD1.cue/track0001", 1.0));
        h.push(play(100, "Gone/01.flac", 1.0));
        let sh = shelves(&l, &h, &HashMap::new(), 1);
        assert_eq!(sh[0].kind, ShelfKind::RecentlyPlayed);
        let r = &sh[0].albums;
        assert_eq!(r.len(), 20);
        assert_eq!(l.album(r[0]).folder, "F03");
        assert_eq!(l.album(r[1]).folder, "F24");
        assert!(!r[1..].iter().any(|&i| l.album(i).folder == "F03"));
        // Every album was played: no "Not Played Yet" shelf.
        assert!(!kinds(&sh).contains(&&ShelfKind::NotPlayed));
    }

    #[test]
    fn genre_shelves() {
        // F00/F01 Rock, F02 Jazz (only one album), F03 untagged but
        // enriched to "rock", F04 Blues never played.
        let l = albums(&["Rock", "Rock", "Jazz", "", "Blues", "Blues"]);
        let extra: HashMap<String, Vec<String>> =
            [("F03".to_owned(), vec!["rock".to_owned()])].into();
        let h = vec![
            play(1, "F02/01.flac", 1000.0),
            play(2, "F01/01.flac", 100.0),
            play(3, "F01/01.flac", 100.0),
            play(4, "F00/01.flac", 100.0),
        ];
        let sh = shelves(&l, &h, &extra, 7);
        assert_eq!(
            kinds(&sh),
            [
                &ShelfKind::RecentlyPlayed,
                &ShelfKind::Genre("Rock".into()),
                &ShelfKind::NotPlayed
            ]
        );
        // Jazz has more time but only one album: no shelf. Rock is sorted
        // by plays (F01 twice, F00 once, F03 never).
        let rock: Vec<&str> = sh[1]
            .albums
            .iter()
            .map(|&i| l.album(i).folder.as_str())
            .collect();
        assert_eq!(rock, ["F01", "F00", "F03"]);
        let mut unplayed: Vec<&str> = sh[2]
            .albums
            .iter()
            .map(|&i| l.album(i).folder.as_str())
            .collect();
        unplayed.sort();
        assert_eq!(unplayed, ["F03", "F04", "F05"]);
    }

    #[test]
    fn top_eight_genres_and_unplayed_sample() {
        let names = ["G1", "G2", "G3", "G4", "G5", "G6", "G7", "G8", "G9"];
        let mut gs: Vec<&str> = names.iter().flat_map(|g| [*g, *g]).collect();
        gs.extend(["X"; 20]);
        let l = albums(&gs);
        // Genre Gk gets k minutes; G1 is the 9th and is dropped.
        let h: Vec<Play> = (0..9)
            .map(|k| {
                play(
                    k as i64,
                    &format!("F{:02}/01.flac", 2 * k),
                    60.0 * (k + 1) as f64,
                )
            })
            .collect();
        let sh = shelves(&l, &h, &HashMap::new(), 3);
        let genres: Vec<&ShelfKind> = sh
            .iter()
            .map(|s| &s.kind)
            .filter(|k| matches!(k, ShelfKind::Genre(_)))
            .collect();
        assert_eq!(genres.len(), 8);
        assert_eq!(genres[0], &ShelfKind::Genre("G9".into()));
        assert!(!genres.contains(&&ShelfKind::Genre("G1".into())));
        let np = sh.last().unwrap();
        assert_eq!(np.kind, ShelfKind::NotPlayed);
        assert_eq!(np.albums.len(), 12);
        assert_eq!(
            np.albums,
            shelves(&l, &h, &HashMap::new(), 3).last().unwrap().albums
        );
        assert_ne!(
            np.albums,
            shelves(&l, &h, &HashMap::new(), 4).last().unwrap().albums
        );
    }
}
