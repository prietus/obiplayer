//! Title/label/catalog heuristics used to find the *exact pressing* online.
//!
//! Ported from DrPlayer's AlbumDetailView/AlbumInfoView. The guiding rule:
//! never show data that is not for the edition the user owns, so online
//! results are cross-checked against the file's label and catalog number.

use std::sync::LazyLock;

use regex::{Regex, RegexBuilder};

fn re(p: &str) -> Regex {
    Regex::new(p).expect("regex")
}

fn re_ci(p: &str) -> Regex {
    RegexBuilder::new(p)
        .case_insensitive(true)
        .build()
        .expect("regex")
}

static LEADING_YEAR: LazyLock<Regex> = LazyLock::new(|| re(r"^\d{4}\s+"));
static TRAILING_PARENS: LazyLock<Regex> = LazyLock::new(|| re(r"\s*\([^)]*\)\s*$"));
static TRAILING_BRACKETS: LazyLock<Regex> = LazyLock::new(|| re(r"\s*\[[^\]]*\]\s*$"));
static MEDIA_SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    re_ci(r"\s+(SHM-CD|SHM-SACD|HDCD|MQA-CD|XRCD|K2HD|HQCD|Blu-spec CD|UHQCD).*$")
});
static EDITION_SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    re_ci(r"\s+(Legacy|Anniversary|Collector|Limited|Special)\s+(Collection|Edition).*$")
});
static DASH_REMASTER: LazyLock<Regex> =
    LazyLock::new(|| re_ci(r"\s*-\s*(remaster|deluxe|bonus).*$"));
static DASH_DISC: LazyLock<Regex> = LazyLock::new(|| re_ci(r"\s*-\s*(CD|Disc)\s.*$"));
static TRAILING_YEAR: LazyLock<Regex> = LazyLock::new(|| re(r"\s+\d{4}\s*$"));

const QUOTES: &[char] = &['"', '\u{201C}', '\u{201D}', '\u{00AB}', '\u{00BB}'];

fn strip_repeated(s: &str, r: &Regex) -> String {
    let mut out = s.to_owned();
    while let Some(m) = r.find(&out) {
        out.truncate(m.start());
    }
    out
}

fn strip_once(s: &str, r: &Regex) -> String {
    match r.find(s) {
        Some(m) => {
            let mut o = s.to_owned();
            o.replace_range(m.range(), "");
            o
        }
        None => s.to_owned(),
    }
}

/// Clean an album title for API searches ("1975 A Night at the Opera
/// (SHM-CD) [Remaster]" → "A Night at the Opera").
pub fn clean_title_for_search(title: &str) -> String {
    let mut s = title.trim_matches(QUOTES).to_owned();
    s = strip_once(&s, &LEADING_YEAR);
    // Alternate until stable so "(SHM-CD) [Remaster]" loses both groups
    // (DrPlayer stripped parens then brackets once, leaving "(SHM-CD)").
    loop {
        let before = s.len();
        s = strip_repeated(&s, &TRAILING_PARENS);
        s = strip_repeated(&s, &TRAILING_BRACKETS);
        if s.len() == before {
            break;
        }
    }
    for r in [
        &*MEDIA_SUFFIX,
        &*EDITION_SUFFIX,
        &*DASH_REMASTER,
        &*DASH_DISC,
        &*TRAILING_YEAR,
    ] {
        s = strip_once(&s, r);
    }
    s.trim().to_owned()
}

static CATALOG_PATTERNS: LazyLock<[Regex; 2]> = LazyLock::new(|| {
    [
        re_ci(r"\(([A-Z]{2,}[\s-]?\d[\w\s-]*)\)"),
        re_ci(r"\((\d+[\s-][A-Z][\w\s-]*)\)"),
    ]
});

/// Catalog number written in the title, e.g. "Kind of Blue (SRCS-9701)".
pub fn extract_catalog(title: &str) -> Option<String> {
    for r in CATALOG_PATTERNS.iter() {
        if let Some(c) = r.captures(title).and_then(|c| c.get(1)) {
            let cand = c.as_str();
            let l = cand.to_lowercase();
            if ["remaster", "deluxe", "live", "edition"]
                .iter()
                .any(|w| l.contains(w))
            {
                continue;
            }
            return Some(cand.to_owned());
        }
    }
    None
}

/// Format/country hints for MusicBrainz/Discogs, from the files and title.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hints {
    pub format: Option<String>,
    pub country: Option<String>,
}

pub fn detect_hints(album_format: &str, title: &str) -> Hints {
    let fmt = album_format.to_uppercase();
    let t = title.to_lowercase();
    let h = |f: Option<&str>, c: Option<&str>| Hints {
        format: f.map(str::to_owned),
        country: c.map(str::to_owned),
    };
    if ["DSF", "DFF", "DSD"].iter().any(|x| fmt.contains(x)) {
        return h(Some("sacd"), Some("JP"));
    }
    if t.contains("shm-sacd") || t.contains("shm sacd") {
        return h(Some("sacd"), Some("JP"));
    }
    if ["shm-cd", "shm cd", "uhqcd", "blu-spec", "hqcd"]
        .iter()
        .any(|x| t.contains(x))
    {
        return h(Some("cd"), Some("JP"));
    }
    if t.contains("sacd") {
        return h(Some("sacd"), None);
    }
    if t.contains("xrcd") || t.contains("k2hd") {
        return h(None, Some("JP"));
    }
    if t.contains("vinyl") || t.contains(" lp") {
        return h(Some("Vinyl"), None);
    }
    Hints::default()
}

static LABEL_PREFIX: LazyLock<Regex> = LazyLock::new(|| re(r"^(\(P\)|\(C\)|[℗©])\s*"));

/// Strip (P)/(C)/℗/© prefixes from a label tag.
pub fn clean_label(label: &str) -> String {
    let mut s = label.trim().to_owned();
    // Swift applied the three patterns in sequence; repeat to cover "(P) ©".
    while let Some(m) = LABEL_PREFIX.find(&s) {
        s.replace_range(m.range(), "");
    }
    s.trim().to_owned()
}

fn ci_contains(hay: &str, needle: &str) -> bool {
    let (h, n) = (crate::text::fold(hay), crate::text::fold(needle));
    h.contains(&n)
}

/// Fuzzy label match: either contains the other (case/accent-insensitive).
/// Empty on either side counts as a match (nothing to contradict).
pub fn label_matches(file_label: &str, online_label: &str) -> bool {
    let f = clean_label(file_label);
    if f.is_empty() || online_label.trim().is_empty() {
        return true;
    }
    ci_contains(online_label, &f) || ci_contains(&f, online_label)
}

fn norm_catalog(c: &str) -> String {
    c.chars()
        .filter(|c| *c != ' ' && *c != '-')
        .flat_map(char::to_lowercase)
        .collect()
}

/// Catalog numbers match after removing spaces/dashes (or one contains the
/// other, e.g. box-set numbers). Missing on either side counts as a match.
pub fn catalog_matches(file_catalog: Option<&str>, online_catalog: &str) -> bool {
    let Some(f) = file_catalog.filter(|c| !c.trim().is_empty()) else {
        return true;
    };
    if online_catalog.trim().is_empty() {
        return true;
    }
    let (a, b) = (norm_catalog(f), norm_catalog(online_catalog));
    a == b || a.contains(&b) || b.contains(&a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleaning() {
        assert_eq!(
            clean_title_for_search("1975 A Night at the Opera (SHM-CD) [2011 Remaster]"),
            "A Night at the Opera"
        );
        assert_eq!(clean_title_for_search("“Heroes”"), "Heroes");
        assert_eq!(clean_title_for_search("Rumours SHM-SACD"), "Rumours");
        assert_eq!(clean_title_for_search("The Wall - Disc 2 of 2"), "The Wall");
        assert_eq!(
            clean_title_for_search("Kind of Blue Legacy Edition"),
            "Kind of Blue"
        );
        assert_eq!(
            clean_title_for_search("Abbey Road - Remastered 2009"),
            "Abbey Road"
        );
        assert_eq!(clean_title_for_search("Live 1984"), "Live");
    }

    #[test]
    fn catalog_extraction() {
        assert_eq!(
            extract_catalog("Kind of Blue (SRCS-9701)").as_deref(),
            Some("SRCS-9701")
        );
        assert_eq!(
            extract_catalog("Aja (UIGY 9030)").as_deref(),
            Some("UIGY 9030")
        );
        assert_eq!(
            extract_catalog("Album (8397 AP)").as_deref(),
            Some("8397 AP")
        );
        assert_eq!(extract_catalog("Album (2011 Remaster)"), None);
        assert_eq!(extract_catalog("Album (Live)"), None);
        assert_eq!(extract_catalog("Album"), None);
    }

    #[test]
    fn hints() {
        assert_eq!(detect_hints("DSF", "X").country.as_deref(), Some("JP"));
        assert_eq!(
            detect_hints("FLAC", "X (SHM-CD)").format.as_deref(),
            Some("cd")
        );
        assert_eq!(detect_hints("FLAC", "X SACD").country, None);
        assert_eq!(detect_hints("FLAC", "X (XRCD)").format, None);
        assert_eq!(
            detect_hints("FLAC", "Blue Vinyl").format.as_deref(),
            Some("Vinyl")
        );
        assert_eq!(detect_hints("FLAC", "Plain"), Hints::default());
    }

    #[test]
    fn edition_guards() {
        assert_eq!(clean_label("(P) ℗ Island Records"), "Island Records");
        assert!(label_matches("(P) Island", "Island Records"));
        assert!(label_matches("Universal Music", "universal"));
        assert!(!label_matches("Analogue Productions", "Fantasy"));
        assert!(label_matches("", "Fantasy"));
        assert!(catalog_matches(Some("UICY-75221"), "UICY 75221"));
        assert!(catalog_matches(Some("CAPP 8397"), "CAPP8397SA"));
        assert!(!catalog_matches(Some("UICY-75221"), "EMI 7243"));
        assert!(catalog_matches(None, "anything"));
    }
}
