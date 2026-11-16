//! Smart playlists: rule model, JSON persistence and evaluation (port of
//! DrPlayer's `SmartPlaylist`).
//!
//! The JSON is compatible with DrPlayer's `JSONEncoder` output for
//! `[SmartPlaylist]` (the value stored under the `smartPlaylists` user
//! default): `{"id","name","match","rules":[{"id","field","op","value","value2"}],"limit"?}`
//! with the Swift enum raw values as strings.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::catalog::{Library, TrackRef};
use crate::library::Album;
use crate::mpd::Song;
use crate::paths;
use crate::radio::Rng;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Field {
    #[default]
    Genre,
    Artist,
    Label,
    Format,
    Year,
    #[serde(rename = "DR")]
    Dr,
    #[serde(rename = "Play Count")]
    PlayCount,
    Rating,
    Composer,
    Country,
}

impl Field {
    pub fn all() -> &'static [Field] {
        use Field::*;
        &[
            Genre, Artist, Label, Format, Year, Dr, PlayCount, Rating, Composer, Country,
        ]
    }

    pub fn is_numeric(self) -> bool {
        matches!(
            self,
            Field::Year | Field::Dr | Field::PlayCount | Field::Rating
        )
    }

    /// DrPlayer has no boolean fields yet (`isBoolean` is always false).
    pub fn is_boolean(self) -> bool {
        false
    }

    /// The Swift raw value, also used as UI label.
    pub fn label(self) -> &'static str {
        match self {
            Field::Genre => "Genre",
            Field::Artist => "Artist",
            Field::Label => "Label",
            Field::Format => "Format",
            Field::Year => "Year",
            Field::Dr => "DR",
            Field::PlayCount => "Play Count",
            Field::Rating => "Rating",
            Field::Composer => "Composer",
            Field::Country => "Country",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Op {
    #[serde(rename = "is")]
    Equals,
    #[serde(rename = "is not")]
    NotEquals,
    #[default]
    #[serde(rename = "contains")]
    Contains,
    #[serde(rename = "doesn't contain")]
    NotContains,
    #[serde(rename = ">")]
    GreaterThan,
    #[serde(rename = "<")]
    LessThan,
    #[serde(rename = "between")]
    Between,
    #[serde(rename = "yes")]
    IsTrue,
    #[serde(rename = "no")]
    IsFalse,
}

impl Op {
    /// Operators offered for a field, in DrPlayer's order.
    pub fn available(field: Field) -> &'static [Op] {
        use Op::*;
        if field.is_boolean() {
            &[IsTrue, IsFalse]
        } else if field.is_numeric() {
            &[Equals, GreaterThan, LessThan, Between]
        } else {
            &[Contains, NotContains, Equals, NotEquals]
        }
    }

    /// The Swift raw value, also used as UI label.
    pub fn label(self) -> &'static str {
        match self {
            Op::Equals => "is",
            Op::NotEquals => "is not",
            Op::Contains => "contains",
            Op::NotContains => "doesn't contain",
            Op::GreaterThan => ">",
            Op::LessThan => "<",
            Op::Between => "between",
            Op::IsTrue => "yes",
            Op::IsFalse => "no",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum MatchMode {
    #[default]
    #[serde(rename = "all")]
    All,
    #[serde(rename = "any")]
    Any,
}

/// Random upper-case UUID v4 string, like Swift's `UUID().uuidString`.
pub fn new_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut rng = Rng::new(
        nanos
            ^ (u64::from(std::process::id()) << 32)
            ^ COUNTER
                .fetch_add(1, Ordering::Relaxed)
                .wrapping_mul(0x9E37_79B9_7F4A_7C15),
    );
    let mut b = [0u8; 16];
    b[..8].copy_from_slice(&rng.next_u64().to_le_bytes());
    b[8..].copy_from_slice(&rng.next_u64().to_le_bytes());
    b[6] = (b[6] & 0x0F) | 0x40;
    b[8] = (b[8] & 0x3F) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02X}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rule {
    pub id: String,
    pub field: Field,
    pub op: Op,
    pub value: String,
    /// Upper bound for `between`.
    pub value2: String,
}

impl Default for Rule {
    /// Genre contains "".
    fn default() -> Rule {
        Rule {
            id: new_id(),
            field: Field::Genre,
            op: Op::Contains,
            value: String::new(),
            value2: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SmartPlaylist {
    pub id: String,
    pub name: String,
    #[serde(rename = "match")]
    pub match_mode: MatchMode,
    pub rules: Vec<Rule>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

impl Default for SmartPlaylist {
    fn default() -> SmartPlaylist {
        SmartPlaylist::new_default()
    }
}

/// Per-file facts that don't live in the tags.
pub struct Facts<'a> {
    pub play_counts: &'a HashMap<String, u32>,
    /// 0–5 stars.
    pub ratings: &'a HashMap<String, u8>,
    pub dr: &'a HashMap<String, i32>,
    /// Enriched genres by album folder.
    pub extra_genres: &'a HashMap<String, Vec<String>>,
}

/// Swift `Int(String)`: no trimming; failures count as 0.
fn int(s: &str) -> i64 {
    s.parse().unwrap_or(0)
}

impl SmartPlaylist {
    /// "New Playlist" matching all of one rule, Genre contains "".
    pub fn new_default() -> SmartPlaylist {
        SmartPlaylist {
            id: new_id(),
            name: "New Playlist".into(),
            match_mode: MatchMode::All,
            rules: vec![Rule::default()],
            limit: None,
        }
    }

    /// Matching tracks in library order, truncated to `limit`.
    pub fn evaluate(&self, lib: &Library, facts: &Facts) -> Vec<TrackRef> {
        let mut out = Vec::new();
        for (ai, album) in lib.albums.iter().enumerate() {
            for (ti, song) in album.tracks.iter().enumerate() {
                let mut results = self.rules.iter().map(|r| eval_rule(r, song, album, facts));
                let ok = match self.match_mode {
                    MatchMode::All => results.all(|b| b),
                    MatchMode::Any => results.any(|b| b),
                };
                if ok {
                    out.push(TrackRef {
                        album: ai as u32,
                        track: ti as u32,
                    });
                }
            }
        }
        if let Some(limit) = self.limit {
            out.truncate(limit);
        }
        out
    }
}

fn eval_rule(rule: &Rule, song: &Song, album: &Album, facts: &Facts) -> bool {
    let file = song.file.as_str();
    match rule.field {
        Field::Genre => {
            // Track genre split on "/" (as DrPlayer), plus album and
            // enriched genres.
            let mut values: Vec<&str> = song
                .genres()
                .iter()
                .flat_map(|g| g.split('/'))
                // Swift's `split` omits empty pieces (before trimming).
                .filter(|g| !g.is_empty())
                .map(str::trim)
                .collect();
            values.extend(album.genres.iter().map(String::as_str));
            if let Some(extra) = facts.extra_genres.get(&album.folder) {
                values.extend(extra.iter().map(String::as_str));
            }
            match_string(rule, &values)
        }
        Field::Artist => match_string(
            rule,
            &[
                song.artist().unwrap_or(""),
                song.album_artist().unwrap_or(&album.artist),
            ],
        ),
        Field::Label => match_string(
            rule,
            &[
                song.label().unwrap_or(""),
                album.label.as_deref().unwrap_or(""),
            ],
        ),
        Field::Format => {
            let ext = song.extension().unwrap_or_default().to_ascii_uppercase();
            // "DSD" means DSF or DFF; any operator other than "is" negates.
            if rule.value.to_uppercase() == "DSD" {
                let is_dsd = ext == "DSF" || ext == "DFF";
                return if rule.op == Op::Equals {
                    is_dsd
                } else {
                    !is_dsd
                };
            }
            match_string(rule, &[&ext])
        }
        Field::Year => {
            let head: String = song.date().unwrap_or("").chars().take(4).collect();
            match_numeric(rule, int(&head))
        }
        Field::Dr => match_numeric(rule, facts.dr.get(file).copied().unwrap_or(0).into()),
        Field::PlayCount => match_numeric(
            rule,
            facts.play_counts.get(file).copied().unwrap_or(0).into(),
        ),
        Field::Rating => match_numeric(rule, facts.ratings.get(file).copied().unwrap_or(0).into()),
        Field::Composer => match_string(rule, &[song.composer().unwrap_or("")]),
        Field::Country => match_string(
            rule,
            &[song
                .tag("releasecountry")
                .or(album.country.as_deref())
                .unwrap_or("")],
        ),
    }
}

fn match_string(rule: &Rule, values: &[&str]) -> bool {
    let q = rule.value.to_lowercase();
    let contains = || values.iter().any(|v| v.to_lowercase().contains(&q));
    let equals = || values.iter().any(|v| v.to_lowercase() == q);
    match rule.op {
        Op::Contains => contains(),
        Op::NotContains => !contains(),
        Op::Equals => equals(),
        Op::NotEquals => !equals(),
        _ => false,
    }
}

fn match_numeric(rule: &Rule, value: i64) -> bool {
    let v1 = int(&rule.value);
    match rule.op {
        Op::Equals => value == v1,
        Op::GreaterThan => value > v1,
        Op::LessThan => value < v1,
        Op::Between => value >= v1 && value <= int(&rule.value2),
        _ => false,
    }
}

/// `$XDG_DATA_HOME/obiplayer/smart_playlists.json`.
pub fn default_path() -> PathBuf {
    paths::data_dir().join("smart_playlists.json")
}

/// Load playlists; empty on a missing or unreadable file.
pub fn load_all(path: &Path) -> Vec<SmartPlaylist> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Save playlists atomically (temp file + rename), creating the directory.
pub fn save_all(path: &Path, playlists: &[SmartPlaylist]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_vec_pretty(playlists).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
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

    // Albums sort by title: "Alpha" (2 tracks), "Beta" (DSD), "Gamma".
    const DATA: &str = "\
file: R/Alpha/01.flac\nAlbum: Alpha\nAlbumArtist: Queen\nArtist: Freddie\nGenre: Rock/Hard Rock\nDate: 1975-11-21\nLabel: EMI\nComposer: Mercury\n\
file: R/Alpha/02.mp3\nAlbum: Alpha\nAlbumArtist: Queen\nGenre: Pop\nDate: 1975\nRELEASECOUNTRY: GB\n\
file: J/Beta/01.dsf\nAlbum: Beta\nAlbumArtist: Miles\nGenre: Jazz\nDate: 1959\nLabel: Columbia\n\
file: J/Gamma/01.dff\nAlbum: Gamma\nAlbumArtist: Coltrane\nDate: 1965\n";

    struct Maps {
        plays: HashMap<String, u32>,
        ratings: HashMap<String, u8>,
        dr: HashMap<String, i32>,
        extra: HashMap<String, Vec<String>>,
    }

    fn maps() -> Maps {
        Maps {
            plays: [("R/Alpha/01.flac".to_owned(), 5)].into(),
            ratings: [("J/Beta/01.dsf".to_owned(), 4)].into(),
            dr: [
                ("R/Alpha/01.flac".to_owned(), 12),
                ("J/Beta/01.dsf".to_owned(), 8),
            ]
            .into(),
            extra: [("J/Gamma".to_owned(), vec!["Free Jazz".to_owned()])].into(),
        }
    }

    fn files(l: &Library, pl: &SmartPlaylist, m: &Maps) -> Vec<String> {
        let f = Facts {
            play_counts: &m.plays,
            ratings: &m.ratings,
            dr: &m.dr,
            extra_genres: &m.extra,
        };
        pl.evaluate(l, &f)
            .into_iter()
            .map(|r| l.song(r).file.clone())
            .collect()
    }

    fn one(field: Field, op: Op, value: &str, value2: &str) -> SmartPlaylist {
        SmartPlaylist {
            rules: vec![Rule {
                field,
                op,
                value: value.into(),
                value2: value2.into(),
                ..Rule::default()
            }],
            ..SmartPlaylist::new_default()
        }
    }

    fn check(field: Field, op: Op, value: &str, value2: &str, expect: &[&str]) {
        let l = lib(DATA);
        let got = files(&l, &one(field, op, value, value2), &maps());
        assert_eq!(got, expect, "{field:?} {op:?} {value:?} {value2:?}");
    }

    const A1: &str = "R/Alpha/01.flac";
    const A2: &str = "R/Alpha/02.mp3";
    const B: &str = "J/Beta/01.dsf";
    const G: &str = "J/Gamma/01.dff";

    #[test]
    fn string_operators() {
        // Album genres apply to every track of the album.
        check(Field::Genre, Op::Contains, "rock", "", &[A1, A2]);
        check(Field::Genre, Op::Equals, "HARD ROCK", "", &[A1, A2]);
        check(Field::Genre, Op::Equals, "jazz", "", &[B]);
        check(Field::Genre, Op::Contains, "free", "", &[G]);
        check(Field::Genre, Op::NotContains, "jazz", "", &[A1, A2]);
        check(Field::Genre, Op::NotEquals, "pop", "", &[B, G]);
        // Empty query: contains "" matches everything, even empty values.
        check(Field::Composer, Op::Contains, "", "", &[A1, A2, B, G]);
        check(Field::Artist, Op::Equals, "freddie", "", &[A1]);
        check(Field::Artist, Op::Equals, "queen", "", &[A1, A2]);
        check(Field::Label, Op::Contains, "emi", "", &[A1, A2]);
        // Track tag, falling back to the album (first track) country.
        check(Field::Country, Op::Equals, "gb", "", &[A2]);
    }

    #[test]
    fn format_and_dsd() {
        check(Field::Format, Op::Equals, "flac", "", &[A1]);
        check(Field::Format, Op::NotEquals, "flac", "", &[A2, B, G]);
        check(Field::Format, Op::Equals, "dsd", "", &[B, G]);
        check(Field::Format, Op::NotEquals, "DSD", "", &[A1, A2]);
        // Any operator other than "is" negates the DSD test.
        check(Field::Format, Op::Contains, "DSD", "", &[A1, A2]);
        check(Field::Format, Op::Contains, "DS", "", &[B]);
    }

    #[test]
    fn numeric_operators() {
        check(Field::Year, Op::Equals, "1975", "", &[A1, A2]);
        check(Field::Year, Op::GreaterThan, "1960", "", &[A1, A2, G]);
        check(Field::Year, Op::LessThan, "1960", "", &[B]);
        check(Field::Year, Op::Between, "1959", "1965", &[B, G]);
        check(Field::Dr, Op::GreaterThan, "10", "", &[A1]);
        check(Field::Dr, Op::Equals, "0", "", &[A2, G]);
        check(Field::PlayCount, Op::Between, "1", "5", &[A1]);
        check(Field::Rating, Op::GreaterThan, "3", "", &[B]);
        // Unparsable value counts as 0.
        check(Field::Rating, Op::Equals, "x", "", &[A1, A2, G]);
        // String operators never match numeric fields.
        check(Field::Year, Op::Contains, "19", "", &[]);
    }

    #[test]
    fn match_mode_and_limit() {
        let l = lib(DATA);
        let m = maps();
        let mut pl = one(Field::Genre, Op::Contains, "jazz", "");
        pl.rules.push(Rule {
            field: Field::Year,
            op: Op::Equals,
            value: "1975".into(),
            ..Rule::default()
        });
        assert!(files(&l, &pl, &m).is_empty());
        pl.match_mode = MatchMode::Any;
        assert_eq!(files(&l, &pl, &m), [A1, A2, B, G]);
        pl.limit = Some(2);
        assert_eq!(files(&l, &pl, &m), [A1, A2]);
        pl.rules.clear();
        assert!(files(&l, &pl, &m).is_empty());
        pl.match_mode = MatchMode::All;
        assert_eq!(files(&l, &pl, &m).len(), 2);
    }

    #[test]
    fn swift_json_compat() {
        let json = r#"[{"id":"9F1C2B3A-1111-4222-8333-444455556666","name":"Hi-Res Jazz",
            "match":"any","limit":50,
            "rules":[{"id":"0B4E7C3D-AAAA-4BBB-8CCC-DDDDEEEEFFFF","field":"Play Count","op":"between","value":"1","value2":"9"},
                     {"id":"1B4E7C3D-AAAA-4BBB-8CCC-DDDDEEEEFFFF","field":"Genre","op":"doesn't contain","value":"pop","value2":""},
                     {"id":"2B4E7C3D-AAAA-4BBB-8CCC-DDDDEEEEFFFF","field":"DR","op":">","value":"10","value2":""}]},
            {"id":"X","name":"No limit","match":"all","rules":[]}]"#;
        let v: Vec<SmartPlaylist> = serde_json::from_str(json).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].match_mode, MatchMode::Any);
        assert_eq!(v[0].limit, Some(50));
        assert_eq!(v[0].rules[0].field, Field::PlayCount);
        assert_eq!(v[0].rules[0].op, Op::Between);
        assert_eq!(v[0].rules[1].op, Op::NotContains);
        assert_eq!(v[0].rules[2].field, Field::Dr);
        assert_eq!(v[1].limit, None);

        let out = serde_json::to_string(&v).unwrap();
        assert!(out.contains(r#""field":"Play Count""#));
        assert!(out.contains(r#""op":"doesn't contain""#));
        assert!(out.contains(r#""match":"any""#));
        // nil limits are omitted, like Swift's encodeIfPresent.
        assert_eq!(out.matches("\"limit\":").count(), 1);
        let back: Vec<SmartPlaylist> = serde_json::from_str(&out).unwrap();
        assert_eq!(back, v);

        // Partial JSON still loads.
        let p: SmartPlaylist =
            serde_json::from_str(r#"{"name":"P","rules":[{"field":"Year"}]}"#).unwrap();
        assert_eq!(p.match_mode, MatchMode::All);
        assert_eq!(p.rules[0].op, Op::Contains);
        assert!(!p.id.is_empty());
    }

    #[test]
    fn model_helpers_and_persistence() {
        assert_eq!(Field::all().len(), 10);
        assert_eq!(Field::PlayCount.label(), "Play Count");
        assert_eq!(Op::NotContains.label(), "doesn't contain");
        assert_eq!(
            Op::available(Field::Year),
            [Op::Equals, Op::GreaterThan, Op::LessThan, Op::Between]
        );
        assert_eq!(Op::available(Field::Label)[0], Op::Contains);
        let p = SmartPlaylist::new_default();
        assert_eq!(p.name, "New Playlist");
        assert_eq!(p.rules.len(), 1);
        assert_eq!(
            (p.rules[0].field, p.rules[0].op),
            (Field::Genre, Op::Contains)
        );
        let id = new_id();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert_ne!(id, new_id());

        let dir = std::env::temp_dir().join(format!("obiplayer-smart-test-{}", std::process::id()));
        let path = dir.join("smart_playlists.json");
        assert!(load_all(&path).is_empty());
        save_all(&path, std::slice::from_ref(&p)).unwrap();
        assert_eq!(load_all(&path), vec![p]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
