//! Live sanity check for MusicBrainz, Wikipedia, LRCLIB and Loudness War.
//! Uses a throwaway cache directory so every lookup hits the network.

use std::sync::Arc;

use obiplayer_core::meta::loudnesswar::LoudnessWar;
use obiplayer_core::meta::lyrics::{LyricsService, parse_lrc};
use obiplayer_core::meta::musicbrainz::{MusicBrainz, ReleaseHints};
use obiplayer_core::meta::wikipedia::Wikipedia;
use obiplayer_core::meta::{Cache, Http};

#[tokio::main]
async fn main() {
    let dir = std::env::temp_dir().join("obi-meta-probe-a");
    let cache = Arc::new(Cache::new(&dir));
    cache.purge();
    let http = Arc::new(Http::new());

    let mb = MusicBrainz::new(http.clone(), cache.clone());
    let hints = ReleaseHints {
        format: Some("CD".into()),
        ..Default::default()
    };
    match mb
        .search_release("Queen", "A Night at the Opera", &hints)
        .await
    {
        Some(r) => println!(
            "MB release: {} | {} | {} {} | {} / {} | formats {:?} | producers {:?} | engineers {:?} | mastering {:?} | wiki {:?} | genres {:?} | {} credits",
            r.id,
            r.title,
            r.date,
            r.country,
            r.label,
            r.catalog_number,
            r.media_formats,
            r.producers,
            r.engineers,
            r.mastering_engineers,
            r.wikipedia_slug,
            r.genres,
            r.credits.len()
        ),
        None => println!("MB release: none"),
    }
    println!(
        "MB cached_release: {:?}",
        mb.cached_release("Queen", "A Night at the Opera")
            .map(|r| r.id)
    );
    if let Some(id) = mb.search_artist("Queen").await {
        match mb.fetch_artist(&id).await {
            Some(a) => println!(
                "MB artist: {} {} {} {}–{} {} wiki {:?} members {:?}",
                a.id,
                a.name,
                a.kind,
                a.begin,
                a.end,
                a.area,
                a.wikipedia_slug,
                a.members
                    .iter()
                    .map(|m| format!("{} [{}] {}", m.name, m.role, m.period))
                    .collect::<Vec<_>>()
            ),
            None => println!("MB artist: fetch failed"),
        }
    }

    if std::env::args().any(|a| a == "mb") {
        return;
    }
    let wiki = Wikipedia::new(http.clone(), cache.clone());
    match wiki.fetch_summary("Queen (band)").await {
        Some(s) => println!(
            "Wiki: {} [{}] {} | {:.80}…",
            s.title, s.lang, s.page_url, s.extract
        ),
        None => println!("Wiki: none"),
    }
    match wiki.search_artist("AC/DC").await {
        Some(s) => println!("Wiki search AC/DC: {} {}", s.title, s.page_url),
        None => println!("Wiki search AC/DC: none"),
    }

    let lyr = LyricsService::new(http.clone());
    match lyr
        .fetch("Queen", "Bohemian Rhapsody", "A Night at the Opera", 355.0)
        .await
    {
        Some(l) => {
            let lines = l.synced.as_deref().map(parse_lrc).unwrap_or_default();
            println!(
                "Lyrics: plain {} chars, {} synced lines, first {:?}, instrumental {}",
                l.plain.as_deref().map_or(0, str::len),
                lines.len(),
                lines.first(),
                l.instrumental
            );
        }
        None => println!("Lyrics: none"),
    }

    let lw = LoudnessWar::new(http);
    let rows = lw
        .search("Queen", "A Night at the Opera (2011 Remaster)")
        .await;
    println!("LW: {} rows", rows.len());
    if let Some(first) = rows.into_iter().nth(1) {
        let d = lw.fetch_detail(first).await;
        println!("LW detail: {:?}", d);
    }
    let rows = lw.search("AC-DC", "Back in Black").await;
    println!(
        "LW AC-DC retry: {} rows, first {:?}",
        rows.len(),
        rows.first().map(|r| &r.artist)
    );
    cache.purge();
}
