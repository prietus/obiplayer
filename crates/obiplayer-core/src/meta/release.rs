//! Find the online release data for an album *edition* (MusicBrainz +
//! Discogs) plus album/artist Wikipedia summaries — one cascade shared by the
//! album page, its info column and the editions comparison (DrPlayer ran the
//! same lookups three times).
//!
//! Order follows DrPlayer: MB by id → catalog+label → catalog → title with
//! format/country hints → title+label → title → raw title; then Discogs by
//! catalog → MB catalog → MB barcode → file barcode → title; finally MB is
//! retried with Discogs' catalog number. File tags always win over online
//! data, and MB data is only shown when it matches the file's label/catalog.

use std::path::PathBuf;

use super::discogs::{DgRelease, PreferHints};
use super::musicbrainz::{MbArtist, MbRelease, ReleaseHints};
use super::services::Services;
use super::titles::{self, Hints};
use super::wikipedia::Summary;

/// What we know about the album locally.
#[derive(Debug, Clone, Default)]
pub struct AlbumQuery {
    pub folder: String,
    pub title: String,
    pub artist: String,
    /// "DSF", "FLAC / MP3"…
    pub format: String,
    pub label: String,
    pub catalog: String,
    pub country: String,
    pub barcode: String,
    pub musicbrainz_id: Option<String>,
    /// First track on disk, to read tags MPD doesn't expose (via ffprobe).
    pub first_track_path: Option<PathBuf>,
}

/// Tags read from the file (ffprobe), filling what MPD lacked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileTags {
    pub label: String,
    pub catalog: String,
    pub country: String,
    pub barcode: String,
    pub producer: String,
    pub engineer: String,
    pub mixer: String,
}

#[derive(Debug, Clone, Default)]
pub struct ReleaseInfo {
    pub tags: FileTags,
    pub mb: Option<MbRelease>,
    pub dg: Option<DgRelease>,
    /// MB release agrees with the file's label and catalog number.
    pub mb_matches_edition: bool,
    pub artist: Option<MbArtist>,
    pub album_wiki: Option<Summary>,
    pub artist_wiki: Option<Summary>,
}

impl ReleaseInfo {
    fn mb_edition(&self) -> Option<&MbRelease> {
        self.mb.as_ref().filter(|_| self.mb_matches_edition)
    }

    /// Label: file tag, then an online source that also has a catalog number.
    pub fn label(&self) -> Option<String> {
        let some = |s: &str| (!s.trim().is_empty()).then(|| s.to_owned());
        some(&self.tags.label)
            .or_else(|| {
                self.mb_edition()
                    .filter(|m| !m.catalog_number.is_empty())
                    .and_then(|m| some(&m.label))
            })
            .or_else(|| {
                self.dg
                    .as_ref()
                    .filter(|d| !d.catalog_number.is_empty())
                    .and_then(|d| some(&d.label))
            })
            .or_else(|| self.mb_edition().and_then(|m| some(&m.label)))
            .or_else(|| self.dg.as_ref().and_then(|d| some(&d.label)))
    }

    pub fn catalog(&self) -> Option<String> {
        let some = |s: &str| (!s.trim().is_empty()).then(|| s.to_owned());
        some(&self.tags.catalog)
            .or_else(|| self.mb_edition().and_then(|m| some(&m.catalog_number)))
            .or_else(|| self.dg.as_ref().and_then(|d| some(&d.catalog_number)))
    }

    pub fn country(&self) -> Option<String> {
        let some = |s: &str| (!s.trim().is_empty()).then(|| s.to_owned());
        some(&self.tags.country)
            .or_else(|| self.mb_edition().and_then(|m| some(&m.country)))
            .or_else(|| self.dg.as_ref().and_then(|d| some(&d.country)))
    }
}

fn read_tags(q: &AlbumQuery) -> FileTags {
    let mut t = FileTags {
        label: q.label.clone(),
        catalog: q.catalog.clone(),
        country: q.country.clone(),
        barcode: q.barcode.clone(),
        ..Default::default()
    };
    let Some(path) = &q.first_track_path else {
        return t;
    };
    let Some(p) = crate::probe::probe(path) else {
        return t;
    };
    let get = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| {
                p.tags
                    .iter()
                    .find(|(tk, v)| tk.eq_ignore_ascii_case(k) && !v.trim().is_empty())
                    .map(|(_, v)| v.trim().to_owned())
            })
            .unwrap_or_default()
    };
    if t.label.is_empty() {
        t.label = get(&["LABEL", "PUBLISHER", "ORGANIZATION"]);
    }
    if t.catalog.is_empty() {
        t.catalog = get(&["CATALOGNUMBER", "CATALOG"]);
    }
    if t.country.is_empty() {
        t.country = get(&["RELEASECOUNTRY"]);
    }
    if t.barcode.is_empty() {
        t.barcode = get(&["BARCODE", "UPC", "EAN"]);
    }
    t.producer = get(&["PRODUCER"]);
    t.engineer = get(&["ENGINEER"]);
    t.mixer = get(&["MIXER", "MIXEDBY", "MIXED BY"]);
    t
}

/// Cache key mapping an album folder to the MB release found for it, so
/// cache-only readers (Producers view) find it regardless of search hints.
pub fn folder_key(folder: &str) -> String {
    format!("mb_release_for_folder_{folder}")
}

/// Run the whole cascade. Slow (rate-limited network); call off the UI thread.
pub async fn lookup(s: &Services, q: &AlbumQuery) -> ReleaseInfo {
    let q2 = q.clone();
    let tags = tokio::task::spawn_blocking(move || read_tags(&q2))
        .await
        .unwrap_or_default();

    let clean = titles::clean_title_for_search(&q.title);
    let mut catno = titles::extract_catalog(&q.title);
    if catno.is_none() && !tags.catalog.is_empty() {
        catno = Some(tags.catalog.clone());
    }
    let Hints { format, country } = titles::detect_hints(&q.format, &q.title);
    let tag_label = titles::clean_label(&tags.label);
    let label_opt = (!tag_label.is_empty()).then_some(tag_label.as_str());
    let artist = q.artist.as_str();

    // -- MusicBrainz release ----------------------------------------------------
    let mut mb: Option<MbRelease> = None;
    if let Some(id) = q.musicbrainz_id.as_deref().filter(|i| !i.is_empty()) {
        mb = s.mb.fetch_release(id).await;
    }
    if mb.is_none()
        && let (Some(c), Some(l)) = (catno.as_deref(), label_opt)
    {
        mb = s.mb.search_by_catalog(artist, c, Some(l)).await;
    }
    if mb.is_none()
        && let Some(c) = catno.as_deref()
    {
        mb = s.mb.search_by_catalog(artist, c, None).await;
    }
    if mb.is_none() && (format.is_some() || country.is_some()) {
        let h = ReleaseHints {
            format: format.clone(),
            country: country.clone(),
            label: label_opt.map(str::to_owned),
        };
        mb = s.mb.search_release(artist, &clean, &h).await;
    }
    if mb.is_none() && label_opt.is_some() {
        let h = ReleaseHints {
            label: label_opt.map(str::to_owned),
            ..Default::default()
        };
        mb = s.mb.search_release(artist, &clean, &h).await;
    }
    if mb.is_none() {
        mb =
            s.mb.search_release(artist, &clean, &ReleaseHints::default())
                .await;
    }
    if mb.is_none() && clean != q.title {
        mb =
            s.mb.search_release(artist, &q.title, &ReleaseHints::default())
                .await;
    }

    // -- Discogs ----------------------------------------------------------------
    let mb_country = mb
        .as_ref()
        .map(|m| m.country.clone())
        .filter(|c| !c.is_empty());
    let prefer = PreferHints {
        country: mb_country.clone(),
        label: label_opt.map(str::to_owned),
        format: format.clone(),
    };
    let mut dg: Option<DgRelease> = None;
    if s.discogs.is_configured() {
        if let Some(c) = catno.as_deref() {
            dg = s.discogs.search_by_catalog(c, &prefer).await;
        }
        if dg.is_none()
            && let Some(c) = mb
                .as_ref()
                .map(|m| m.catalog_number.clone())
                .filter(|c| !c.is_empty())
        {
            dg = s.discogs.search_by_catalog(&c, &prefer).await;
        }
        if dg.is_none()
            && let Some(b) = mb
                .as_ref()
                .map(|m| m.barcode.clone())
                .filter(|b| !b.is_empty())
        {
            dg = s.discogs.search_by_barcode(&b, mb_country.as_deref()).await;
        }
        if dg.is_none() && !tags.barcode.is_empty() {
            dg = s
                .discogs
                .search_by_barcode(&tags.barcode, mb_country.as_deref())
                .await;
        }
        if dg.is_none() {
            dg = s.discogs.search(artist, &clean).await;
        }
    }

    // Discogs found a more specific catalog number: retry MB with it.
    if let Some(d) = &dg
        && !d.catalog_number.is_empty()
        && mb.as_ref().map(|m| m.catalog_number.as_str()) != Some(d.catalog_number.as_str())
        && let Some(better) =
            s.mb.search_by_catalog(artist, &d.catalog_number, None)
                .await
    {
        mb = Some(better);
    }

    let file_catalog = titles::extract_catalog(&q.title)
        .or_else(|| (!tags.catalog.is_empty()).then(|| tags.catalog.clone()));
    let mb_matches_edition = mb.as_ref().is_none_or(|m| {
        titles::label_matches(&tags.label, &m.label)
            && titles::catalog_matches(file_catalog.as_deref(), &m.catalog_number)
    });
    if let Some(m) = &mb {
        s.cache.set(&folder_key(&q.folder), &m.id);
    }

    // -- Artist + Wikipedia -------------------------------------------------------
    let artist_mb = match s.mb.search_artist(artist).await {
        Some(id) => s.mb.fetch_artist(&id).await,
        None => None,
    };
    let mut album_wiki = None;
    if let Some(slug) = mb.as_ref().and_then(|m| m.wikipedia_slug.clone()) {
        album_wiki = s.wiki.fetch_summary(&slug).await;
    }
    for query in [
        format!("{clean} {artist} album"),
        format!("{clean} ({artist} album)"),
    ] {
        if album_wiki.is_none() {
            album_wiki = s.wiki.search(&query, "en").await;
        }
    }
    if album_wiki.is_none()
        && let Some(w) = s.wiki.search(&clean, "en").await
        && crate::text::fold(&w.extract).contains(&crate::text::fold(artist))
    {
        album_wiki = Some(w);
    }
    let mut artist_wiki = None;
    if let Some(slug) = artist_mb.as_ref().and_then(|a| a.wikipedia_slug.clone()) {
        artist_wiki = s.wiki.fetch_summary(&slug).await;
    }
    if artist_wiki.is_none() {
        artist_wiki = s.wiki.search_artist(artist).await;
    }
    // Don't show the same article twice.
    if let (Some(a), Some(b)) = (&album_wiki, &artist_wiki)
        && a.page_url == b.page_url
    {
        artist_wiki = None;
    }

    ReleaseInfo {
        tags,
        mb,
        dg,
        mb_matches_edition,
        artist: artist_mb,
        album_wiki,
        artist_wiki,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mb(label: &str, catno: &str, country: &str) -> MbRelease {
        MbRelease {
            label: label.into(),
            catalog_number: catno.into(),
            country: country.into(),
            ..Default::default()
        }
    }

    #[test]
    fn file_tags_win_and_mismatched_mb_is_hidden() {
        let info = ReleaseInfo {
            tags: FileTags {
                label: "Analogue Productions".into(),
                ..Default::default()
            },
            mb: Some(mb("Fantasy", "F-123", "US")),
            mb_matches_edition: false,
            ..Default::default()
        };
        assert_eq!(info.label().as_deref(), Some("Analogue Productions"));
        // MB is for another pressing: its catalog/country must not leak in.
        assert_eq!(info.catalog(), None);
        assert_eq!(info.country(), None);
    }

    #[test]
    fn matching_mb_fills_gaps() {
        let info = ReleaseInfo {
            mb: Some(mb("Island", "UICY-75221", "JP")),
            mb_matches_edition: true,
            ..Default::default()
        };
        assert_eq!(info.label().as_deref(), Some("Island"));
        assert_eq!(info.catalog().as_deref(), Some("UICY-75221"));
        assert_eq!(info.country().as_deref(), Some("JP"));
    }
}
