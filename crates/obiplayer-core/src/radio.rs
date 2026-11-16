//! Metadata-similarity radio (port of DrPlayer's `RadioEngine`).
//!
//! A seed (album or the tail of the queue) gives genres, artist, decade and
//! composer; every library track with a real connection to it is scored, and
//! a diversity pass caps tracks per artist and per album.

use std::collections::{HashMap, HashSet};

use crate::catalog::Library;
use crate::library::Album;
use crate::mpd::Song;

/// Small deterministic PRNG (SplitMix64). Enough for shuffling shelves and
/// radio tiers; seeded explicitly so results are reproducible in tests.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n` (`n > 0`).
    pub fn below(&mut self, n: usize) -> usize {
        // Multiply-shift: negligible bias for the sizes used here.
        ((self.next_u64() as u128 * n as u128) >> 64) as usize
    }

    /// Fisher–Yates shuffle.
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.below(i + 1);
            v.swap(i, j);
        }
    }
}

/// What the radio looks for.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Seed {
    /// Lower-cased, trimmed, stop tags removed, unique.
    pub genres: Vec<String>,
    pub artist: String,
    pub decade: Option<i32>,
    pub composer: Option<String>,
    pub label: Option<String>,
}

/// Tags too ambiguous or noisy to drive genre similarity. "classic" alone
/// matches both classic rock and classical after Last.fm enrichment; decade
/// tags are weak signals already covered by `decade`.
const STOP_TAGS: &[&str] = &[
    "classic",
    "seen live",
    "favorite",
    "favorites",
    "favourite",
    "favourites",
    "awesome",
    "amazing",
    "good",
    "great",
    "best",
    "love",
    "loved",
    "albums i own",
    "owned",
    "own",
    "music",
    "song",
    "songs",
    "00s",
    "10s",
    "20s",
    "30s",
    "40s",
    "50s",
    "60s",
    "70s",
    "80s",
    "90s",
    "1900s",
    "1910s",
    "1920s",
    "1930s",
    "1940s",
    "1950s",
    "1960s",
    "1970s",
    "1980s",
    "1990s",
    "2000s",
    "2010s",
    "2020s",
];

/// Lower-case, trim, drop empties and stop tags, dedupe (keeps first order).
pub fn normalized_genres<'a>(raw: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for g in raw {
        let lower = g.trim().to_lowercase();
        if lower.is_empty() || STOP_TAGS.contains(&lower.as_str()) || out.contains(&lower) {
            continue;
        }
        out.push(lower);
    }
    out
}

/// `(year / 10) * 10` when the date starts with a year after 1900.
pub fn decade(date: &str) -> Option<i32> {
    if date.chars().count() < 4 {
        return None;
    }
    let head: String = date.chars().take(4).collect();
    let year: i32 = head.parse().ok()?;
    (year > 1900).then_some((year / 10) * 10)
}

/// Raw genre tag values split on `/` like DrPlayer's `Track.genre`.
fn raw_track_genres(song: &Song) -> impl Iterator<Item = &str> {
    song.genres()
        .iter()
        .flat_map(|g| g.split('/'))
        .filter(|g| !g.is_empty())
}

/// Album genres plus enriched ones for its folder.
fn album_genres<'a>(
    album: &'a Album,
    extra: &'a HashMap<String, Vec<String>>,
) -> impl Iterator<Item = &'a str> {
    album
        .genres
        .iter()
        .chain(extra.get(&album.folder).into_iter().flatten())
        .map(String::as_str)
}

/// Seed from an album (genres, album artist, decade, first composer, label).
pub fn seed_from_album(album: &Album) -> Seed {
    seed_from_album_with(album, &HashMap::new())
}

/// Like [`seed_from_album`], also using enriched genres (`folder → genres`).
pub fn seed_from_album_with(album: &Album, extra_genres: &HashMap<String, Vec<String>>) -> Seed {
    Seed {
        genres: normalized_genres(album_genres(album, extra_genres)),
        artist: album.artist.clone(),
        decade: album.date.as_deref().and_then(decade),
        composer: album.composers.first().cloned(),
        label: album.label.clone().filter(|l| !l.is_empty()),
    }
}

/// Most frequent value; ties go to the value seen first.
fn most_frequent(values: Vec<String>) -> Option<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for v in values {
        match counts.iter_mut().find(|(k, _)| *k == v) {
            Some((_, n)) => *n += 1,
            None => counts.push((v, 1)),
        }
    }
    let max = counts.iter().map(|(_, n)| *n).max()?;
    counts.into_iter().find(|(_, n)| *n == max).map(|(k, _)| k)
}

/// Seed from the last five songs (e.g. of the queue): union of genres, most
/// frequent artist, decade and composer; no label.
pub fn seed_from_songs(songs: &[&Song]) -> Seed {
    let recent = &songs[songs.len().saturating_sub(5)..];
    let genres = normalized_genres(recent.iter().flat_map(|s| raw_track_genres(s)));
    let artist = most_frequent(
        recent
            .iter()
            .filter_map(|s| s.artist())
            .map(str::to_owned)
            .collect(),
    )
    .unwrap_or_default();
    let decade = most_frequent(
        recent
            .iter()
            .filter_map(|s| s.date().and_then(decade))
            .map(|d| d.to_string())
            .collect(),
    )
    .and_then(|d| d.parse().ok());
    let composer = most_frequent(
        recent
            .iter()
            .filter_map(|s| s.composer())
            .map(str::to_owned)
            .collect(),
    );
    Seed {
        genres,
        artist,
        decade,
        composer,
        label: None,
    }
}

struct Scored<'a> {
    file: &'a str,
    score: i32,
    artist: String,
    album: usize,
}

/// Generate up to `count` files similar to `seed`.
///
/// `similar` maps lower-cased artist names to a Last.fm match score (0–1);
/// when non-empty it dominates and genre becomes a tie-breaker. `ratings`
/// (file → 0–5) add 3 points per star. Files in `exclude` are skipped.
pub fn generate(
    lib: &Library,
    seed: &Seed,
    similar: &HashMap<String, f64>,
    ratings: &HashMap<String, u8>,
    exclude: &HashSet<String>,
    count: usize,
    rng_seed: u64,
) -> Vec<String> {
    generate_with(
        lib,
        seed,
        similar,
        ratings,
        &HashMap::new(),
        exclude,
        count,
        rng_seed,
    )
}

/// Like [`generate`], also using enriched album genres (`folder → genres`).
#[allow(clippy::too_many_arguments)]
pub fn generate_with(
    lib: &Library,
    seed: &Seed,
    similar: &HashMap<String, f64>,
    ratings: &HashMap<String, u8>,
    extra_genres: &HashMap<String, Vec<String>>,
    exclude: &HashSet<String>,
    count: usize,
    rng_seed: u64,
) -> Vec<String> {
    let seed_artist = seed.artist.to_lowercase();
    let seed_composer = seed
        .composer
        .as_deref()
        .filter(|c| !c.is_empty())
        .map(str::to_lowercase);
    let seed_genres: HashSet<&str> = seed.genres.iter().map(String::as_str).collect();
    let genre_max = if similar.is_empty() { 50.0 } else { 25.0 };

    let mut scored: Vec<Scored> = Vec::new();
    for (ai, album) in lib.albums.iter().enumerate() {
        let genres = normalized_genres(album_genres(album, extra_genres));
        let album_genres: HashSet<&str> = genres.iter().map(String::as_str).collect();
        let album_decade = album.date.as_deref().and_then(decade);
        let track_artist = album.artist.to_lowercase();
        let similar_match = similar.get(&track_artist).copied().unwrap_or(0.0);

        // Jaccard: an album with 8 broad tags shouldn't outscore one with two
        // precise tags when both overlap on "rock".
        let union = seed_genres.union(&album_genres).count();
        let overlap = seed_genres.intersection(&album_genres).count();
        let jaccard = if union > 0 {
            overlap as f64 / union as f64
        } else {
            0.0
        };
        let same_artist = !seed_artist.is_empty() && track_artist == seed_artist;

        for track in &album.tracks {
            if exclude.contains(&track.file) {
                continue;
            }
            let composer_match = seed_composer
                .as_deref()
                .is_some_and(|c| track.composer().unwrap_or("").to_lowercase() == c);

            // Affinity gate: rating + decade alone must not let unrelated
            // highly-rated tracks flood the queue.
            if !(similar_match > 0.0 || jaccard > 0.0 || composer_match || same_artist) {
                continue;
            }

            let mut score = 0i32;
            if similar_match > 0.0 {
                score += (similar_match * 20.0) as i32;
            }
            score += (jaccard * genre_max) as i32;
            if let (Some(sd), Some(ad)) = (seed.decade, album_decade) {
                if sd == ad {
                    score += 3;
                }
                if (sd - ad).abs() == 10 {
                    score += 1;
                }
            }
            if composer_match {
                score += 8;
            }
            let rating = ratings.get(&track.file).copied().unwrap_or(0);
            if rating > 0 {
                score += i32::from(rating) * 3;
            }
            if same_artist {
                score += 2;
            }
            if score > 0 {
                scored.push(Scored {
                    file: &track.file,
                    score,
                    artist: track_artist.clone(),
                    album: ai,
                });
            }
        }
    }

    let ordered = diversify_order(scored, rng_seed);

    let max_per_artist = (count / 10).max(2);
    let max_per_album = 2;
    let mut artist_counts: HashMap<&str, usize> = HashMap::new();
    let mut album_counts: HashMap<usize, usize> = HashMap::new();
    let mut taken: HashSet<&str> = HashSet::new();
    let mut result: Vec<String> = Vec::new();

    for item in &ordered {
        if result.len() >= count {
            break;
        }
        let ac = artist_counts.entry(&item.artist).or_default();
        if *ac >= max_per_artist {
            continue;
        }
        let bc = album_counts.entry(item.album).or_default();
        if *bc >= max_per_album {
            continue;
        }
        *ac += 1;
        *bc += 1;
        taken.insert(item.file);
        result.push(item.file.to_owned());
    }

    // Restrictive genres: relax the album cap, keep the artist cap.
    if result.len() < count {
        for item in &ordered {
            if result.len() >= count {
                break;
            }
            if taken.contains(item.file) {
                continue;
            }
            let ac = artist_counts.entry(&item.artist).or_default();
            if *ac >= max_per_artist {
                continue;
            }
            *ac += 1;
            taken.insert(item.file);
            result.push(item.file.to_owned());
        }
    }
    result
}

/// Highest score first, random order among tracks within 3 points of each
/// tier's top score.
///
/// DrPlayer shuffled and then sorted with "equal if within 3 points", which is
/// not a strict weak ordering (its result is unspecified, and Rust's sort may
/// panic on it). Here tiers are anchored at the best remaining score: each
/// tier holds every track scoring ≥ head − 3, shuffled.
fn diversify_order(mut scored: Vec<Scored>, rng_seed: u64) -> Vec<Scored> {
    scored.sort_by_key(|s| std::cmp::Reverse(s.score));
    let mut rng = Rng::new(rng_seed);
    let mut start = 0;
    while start < scored.len() {
        let floor = scored[start].score - 3;
        let end = scored[start..]
            .iter()
            .position(|s| s.score < floor)
            .map_or(scored.len(), |p| start + p);
        rng.shuffle(&mut scored[start..end]);
        start = end;
    }
    scored
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

    /// `artists × albums × tracks` rock albums from the 70s, plus a 5★
    /// classical album that has no affinity with a rock seed.
    fn big_lib(artists: usize, albums: usize, tracks: usize) -> Library {
        let mut s = String::new();
        for a in 0..artists {
            for b in 0..albums {
                for t in 0..tracks {
                    s.push_str(&format!(
                        "file: A{a}/B{b}/{t:02}.flac\nAlbum: Album {a}-{b}\nAlbumArtist: Artist {a}\n\
                         Genre: Rock\nDate: 1975\n"
                    ));
                }
            }
        }
        s.push_str("file: C/Bach/01.flac\nAlbum: Bach\nAlbumArtist: Gould\nGenre: Classical\nDate: 1981\nComposer: Bach\n");
        lib(&s)
    }

    #[test]
    fn helpers() {
        assert_eq!(decade("1975-03-01"), Some(1970));
        assert_eq!(decade("1900"), None);
        assert_eq!(decade("75"), None);
        assert_eq!(
            normalized_genres([" Rock ", "classic", "70s", "rock", "", "Blues"]),
            ["rock", "blues"]
        );
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        assert_eq!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn seeds() {
        let l = lib(
            "file: X/A/01.flac\nAlbum: A\nAlbumArtist: Band\nGenre: Rock/Seen Live\nDate: 1983\nComposer: Smith\nLabel: EMI\n",
        );
        let s = seed_from_album(&l.albums[0]);
        assert_eq!(s.genres, ["rock"]);
        assert_eq!(s.artist, "Band");
        assert_eq!(s.decade, Some(1980));
        assert_eq!(s.composer.as_deref(), Some("Smith"));
        assert_eq!(s.label.as_deref(), Some("EMI"));

        let q = lib("file: Q/1.flac\nArtist: Old\nGenre: Jazz\nDate: 1960\n\
                     file: Q/2.flac\nArtist: B\nGenre: Rock/Blues\nDate: 1971\nComposer: C1\n\
                     file: Q/3.flac\nArtist: A\nGenre: rock\nDate: 1972\n\
                     file: Q/4.flac\nArtist: A\nDate: 1985\nComposer: C2\n\
                     file: Q/5.flac\nArtist: B\nDate: 1979\nComposer: C2\n\
                     file: Q/6.flac\nArtist: A\nGenre: 70s\n");
        let songs: Vec<&Song> = q.albums[0].tracks.iter().collect();
        let s = seed_from_songs(&songs);
        // Track 1 (Jazz, 1960) is outside the last five.
        assert_eq!(s.genres, ["rock", "blues"]);
        assert_eq!(s.artist, "A");
        assert_eq!(s.decade, Some(1970));
        assert_eq!(s.composer.as_deref(), Some("C2"));
        assert_eq!(s.label, None);
    }

    #[test]
    fn affinity_gate_and_scoring() {
        let l = big_lib(1, 1, 2);
        let seed = Seed {
            genres: vec!["rock".into()],
            decade: Some(1970),
            ..Default::default()
        };
        let ratings: HashMap<String, u8> = [("C/Bach/01.flac".to_owned(), 5)].into();
        let out = generate(&l, &seed, &HashMap::new(), &ratings, &HashSet::new(), 10, 1);
        // The 5★ classical track has no affinity and is never picked.
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|f| f.starts_with("A0/")));

        // Composer affinity lets it in.
        let seed = Seed {
            composer: Some("bach".into()),
            ..Default::default()
        };
        let out = generate(&l, &seed, &HashMap::new(), &ratings, &HashSet::new(), 10, 1);
        assert_eq!(out, ["C/Bach/01.flac"]);

        // Similar-artist signal, and exclusions.
        let similar: HashMap<String, f64> = [("artist 0".to_owned(), 0.9)].into();
        let excl: HashSet<String> = ["A0/B0/00.flac".to_owned()].into();
        let out = generate(
            &l,
            &Seed::default(),
            &similar,
            &HashMap::new(),
            &excl,
            10,
            1,
        );
        assert_eq!(out, ["A0/B0/01.flac"]);
    }

    #[test]
    fn diversity_caps() {
        let seed = Seed {
            genres: vec!["rock".into()],
            ..Default::default()
        };
        let none = HashMap::new();
        // 5 artists × 3 albums × 4 tracks; count 20 → 2 per artist (first
        // pass caps albums at 2, the artist cap already binds).
        let l = big_lib(5, 3, 4);
        let out = generate(&l, &seed, &HashMap::new(), &none, &HashSet::new(), 20, 42);
        assert_eq!(out.len(), 10);
        let mut per_artist: HashMap<&str, usize> = HashMap::new();
        for f in &out {
            *per_artist.entry(&f[..2]).or_default() += 1;
        }
        assert!(per_artist.values().all(|&n| n == 2));

        // One artist, one album of 10 tracks, count 20 → artist cap is
        // max(2, 2) = 2; album cap 2.
        let l = big_lib(1, 1, 10);
        let out = generate(&l, &seed, &HashMap::new(), &none, &HashSet::new(), 20, 42);
        assert_eq!(out.len(), 2);

        // count 40 → artist cap 4 while album cap is 2: the relaxed second
        // pass fills the rest from the same album.
        let out = generate(&l, &seed, &HashMap::new(), &none, &HashSet::new(), 40, 42);
        assert_eq!(out.len(), 4);
        let unique: HashSet<_> = out.iter().collect();
        assert_eq!(unique.len(), 4);
    }

    #[test]
    fn deterministic_and_score_ordered() {
        let seed = Seed {
            genres: vec!["rock".into()],
            ..Default::default()
        };
        let l = big_lib(20, 2, 3);
        // Rate one artist's tracks highly: they must come first.
        let ratings: HashMap<String, u8> = (0..3)
            .map(|t| (format!("A7/B1/{t:02}.flac"), 5u8))
            .collect();
        let run = |s| generate(&l, &seed, &HashMap::new(), &ratings, &HashSet::new(), 30, s);
        let a = run(9);
        assert_eq!(a, run(9));
        assert_eq!(a.len(), 30);
        assert!(a[0].starts_with("A7/B1/") && a[1].starts_with("A7/B1/"));
        // Different seeds shuffle ties differently.
        assert_ne!(a, run(10));
    }
}
