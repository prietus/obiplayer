//! Dynamic Range database (<https://dr.loudness-war.info>): scrapes the album
//! list and album detail pages. Not cached (port of `LoudnessWarService.swift`).

use std::sync::{Arc, LazyLock};

use regex::Regex;
use reqwest::Url;

use super::{Http, Service};

const LIST_URL: &str = "https://dr.loudness-war.info/album/list";
const VIEW_URL: &str = "https://dr.loudness-war.info/album/view/";

/// One measured release from the DR database.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LwEntry {
    /// Album view id (empty if the row had no `album/view/<id>` link).
    pub id: String,
    pub artist: String,
    pub album: String,
    pub year: String,
    pub dr: i32,
    pub dr_min: i32,
    pub dr_max: i32,
    /// "Lossless", "Lossy".
    pub codec: String,
    /// "CD", "Vinyl", "Download", …
    pub source: String,
    // Detail fields, filled by `fetch_detail`.
    pub label: String,
    pub catalog: String,
    pub country: String,
}

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).expect("regex"));
    };
}

re!(RE_LEAD_YEAR, r"^\d{4}\s+");
re!(RE_TRAIL_PARENS, r"\s*\([^)]*\)\s*$");
re!(RE_TRAIL_BRACKETS, r"\s*\[[^\]]*\]\s*$");
re!(
    RE_FORMAT,
    r"(?i)\s+(SHM-CD|SHM-SACD|HDCD|MQA-CD|XRCD|K2HD|HQCD|Blu-spec CD|UHQCD).*$"
);
re!(
    RE_EDITION,
    r"(?i)\s+(Legacy|Anniversary|Collector|Limited|Special)\s+(Collection|Edition).*$"
);
re!(RE_DASH_REMASTER, r"(?i)\s*-\s*(remaster|deluxe|bonus).*$");
re!(RE_DASH_DISC, r"(?i)\s*-\s*(CD|Disc)\s.*$");
re!(RE_TRAIL_YEAR, r"\s+\d{4}\s*$");
re!(RE_ROW, r"(?s)<tr>(.*?)</tr>");
re!(RE_CELL, r"(?s)<td[^>]*>(.*?)</td>");
re!(RE_VIEW_ID, r"album/view/(\d+)");
re!(RE_TAG, r"<[^>]+>");

/// Clean an album title for searching: drop quotes, year prefixes, catalog /
/// edition / format suffixes. Steps run in the same order as DrPlayer.
pub fn clean_album_title(title: &str) -> String {
    let mut s = title
        .trim_matches(|c| matches!(c, '"' | '\u{201C}' | '\u{201D}' | '\u{00AB}' | '\u{00BB}'))
        .to_owned();
    s = RE_LEAD_YEAR.replace(&s, "").into_owned();
    while let Some(m) = RE_TRAIL_PARENS.find(&s) {
        s.truncate(m.start());
    }
    while let Some(m) = RE_TRAIL_BRACKETS.find(&s) {
        s.truncate(m.start());
    }
    for re in [
        &*RE_FORMAT,
        &*RE_EDITION,
        &*RE_DASH_REMASTER,
        &*RE_DASH_DISC,
        &*RE_TRAIL_YEAR,
    ] {
        if let Some(m) = re.find(&s) {
            s.truncate(m.start());
        }
    }
    s.trim().to_owned()
}

/// Search page URL (also usable to open in a browser).
pub fn search_url(artist: &str, album: &str) -> String {
    Url::parse_with_params(LIST_URL, &[("artist", artist), ("album", album)])
        .map(String::from)
        .unwrap_or_else(|_| LIST_URL.to_owned())
}

/// Album detail page URL.
pub fn album_url(id: &str) -> String {
    format!("{VIEW_URL}{id}")
}

fn strip_html(s: &str) -> String {
    RE_TAG
        .replace_all(s, "")
        .replace("&#039;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .trim()
        .to_owned()
}

/// Parse the `album/list` results table. Rows with fewer than 8 cells or
/// non-numeric DR values are skipped; each row's id comes from its own
/// `album/view/<id>` link.
pub fn parse_album_list(html: &str) -> Vec<LwEntry> {
    RE_ROW
        .captures_iter(html)
        .filter_map(|row| {
            let row = row.get(1)?.as_str();
            let cells: Vec<String> = RE_CELL
                .captures_iter(row)
                .map(|c| strip_html(&c[1]))
                .collect();
            if cells.len() < 8 {
                return None;
            }
            let dr = cells[3].parse().ok()?;
            let dr_min = cells[4].parse().ok()?;
            let dr_max = cells[5].parse().ok()?;
            let id = RE_VIEW_ID
                .captures(row)
                .map(|c| c[1].to_owned())
                .unwrap_or_default();
            Some(LwEntry {
                id,
                artist: cells[0].clone(),
                album: cells[1].clone(),
                year: cells[2].clone(),
                dr,
                dr_min,
                dr_max,
                codec: cells[6].clone(),
                source: cells[7].clone(),
                ..Default::default()
            })
        })
        .collect()
}

/// Extract a field from the detail page: `<th>Field</th><td>value</td>`,
/// then `<dt>Field</dt><dd>value</dd>`.
pub fn extract_field(html: &str, field: &str) -> String {
    let f = regex::escape(field);
    for pat in [
        format!(r"(?s)<th[^>]*>\s*{f}\s*</th>\s*<td[^>]*>(.*?)</td>"),
        format!(r"(?s)<dt[^>]*>\s*{f}\s*</dt>\s*<dd[^>]*>(.*?)</dd>"),
    ] {
        if let Some(c) = Regex::new(&pat).ok().and_then(|re| re.captures(html)) {
            return strip_html(&c[1]);
        }
    }
    String::new()
}

/// Fill label / catalog / country from a detail page.
pub fn apply_detail(mut entry: LwEntry, html: &str) -> LwEntry {
    entry.label = extract_field(html, "Label");
    entry.catalog = extract_field(html, "Catalog number");
    entry.country = extract_field(html, "Country");
    entry
}

pub struct LoudnessWar {
    http: Arc<Http>,
}

impl LoudnessWar {
    pub fn new(http: Arc<Http>) -> LoudnessWar {
        LoudnessWar { http }
    }

    /// Search DR entries for an album (title cleaned first). With no results
    /// and a `-` in the artist, retries with `/` (AC-DC → AC/DC).
    pub async fn search(&self, artist: &str, album: &str) -> Vec<LwEntry> {
        let cleaned = clean_album_title(album);
        let mut results = self.search_raw(artist, &cleaned).await;
        if results.is_empty() && artist.contains('-') {
            results = self.search_raw(&artist.replace('-', "/"), &cleaned).await;
        }
        results
    }

    async fn search_raw(&self, artist: &str, album: &str) -> Vec<LwEntry> {
        let query = [("artist", artist), ("album", album)];
        match self.http.get_text(Service::Other, LIST_URL, &query).await {
            Some(html) => parse_album_list(&html),
            None => Vec::new(),
        }
    }

    /// Fetch label, catalog number and country from the album page. Returns
    /// the entry unchanged on failure.
    pub async fn fetch_detail(&self, entry: LwEntry) -> LwEntry {
        if entry.id.is_empty() {
            return entry;
        }
        match self
            .http
            .get_text(Service::Other, &album_url(&entry.id), &[])
            .await
        {
            Some(html) => apply_detail(entry, &html),
            None => entry,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_titles() {
        let cases = [
            ("\u{201C}Heroes\u{201D}", "Heroes"),
            ("\"Heroes\"", "Heroes"),
            ("1972 Demons and Wizards", "Demons and Wizards"),
            ("Abbey Road (UICY-40261) (Remastered 2011)", "Abbey Road"),
            ("Rumours [Deluxe Edition]", "Rumours"),
            ("Beat SHM-CD Legacy Collection 1980", "Beat"),
            ("Thriller Special Edition", "Thriller"),
            ("Thriller 25th Anniversary Edition", "Thriller 25th"),
            ("Led Zeppelin IV - Remastered", "Led Zeppelin IV"),
            ("The Wall - Disc Two", "The Wall"),
            ("Beat 1980", "Beat"),
            ("1984", "1984"),
            ("A Night at the Opera", "A Night at the Opera"),
            (
                "Kind of Blue (Legacy Edition) [Disc 1]",
                "Kind of Blue (Legacy Edition)",
            ),
        ];
        for (input, want) in cases {
            assert_eq!(clean_album_title(input), want, "input: {input}");
        }
    }

    const LIST: &str = r#"
<table><thead><tr><th>Artist</th><th>Album</th></tr></thead><tbody>
  <tr>
    <td><a href="/?artist=Queen">Queen</a></td>
    <td><a href="/album/view/234163">A Night At The Opera (30th Anniversary)</a></td>
    <td>2005</td>
    <td>
        <span class="badge-dr badge-dr-09">09</span>
    </td>
    <td><span class="badge-dr">07</span></td>
    <td><span class="badge-dr">12</span></td>
    <td>Lossless</td>
    <td>DVD</td>
  </tr>
  <tr>
    <td>Queen</td><td><a href="/album/view/1">Broken</a></td><td>2000</td>
    <td>-</td><td>1</td><td>2</td><td>Lossy</td><td>CD</td>
  </tr>
  <tr>
    <td><a href="/?artist=Queen">Queen</a></td>
    <td><a href="/album/view/209667">Rock &amp; Roll &#039;75</a></td>
    <td>1986</td><td>11</td><td>10</td><td>13</td><td>Lossless</td><td>CD</td>
  </tr>
</tbody></table>"#;

    #[test]
    fn parses_rows_with_own_ids() {
        let rows = parse_album_list(LIST);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "234163");
        assert_eq!(rows[0].dr, 9);
        assert_eq!((rows[0].dr_min, rows[0].dr_max), (7, 12));
        assert_eq!(rows[0].source, "DVD");
        // The skipped row's id must not shift onto the next row.
        assert_eq!(rows[1].id, "209667");
        assert_eq!(rows[1].album, "Rock & Roll '75");
        assert_eq!(rows[1].codec, "Lossless");
    }

    #[test]
    fn parses_detail_fields() {
        let html = r#"<table>
    <tr>
        <th scope="row">Label</th>
        <td>EMI</td>
    </tr>
    <tr><th scope="row">Catalog number</th><td>CDP 7 46207 2, CZ 106</td></tr>
</table>
<dl><dt>Country</dt> <dd><b>UK</b></dd></dl>"#;
        let e = apply_detail(LwEntry::default(), html);
        assert_eq!(e.label, "EMI");
        assert_eq!(e.catalog, "CDP 7 46207 2, CZ 106");
        assert_eq!(e.country, "UK");
    }

    #[test]
    fn urls() {
        assert_eq!(
            search_url("Guns N' Roses", "Use Your Illusion"),
            "https://dr.loudness-war.info/album/list?artist=Guns+N%27+Roses&album=Use+Your+Illusion"
        );
        assert_eq!(
            album_url("42"),
            "https://dr.loudness-war.info/album/view/42"
        );
    }
}
