//! Throwaway live check for Discogs and Last.fm. Prints only OK/fail and
//! non-secret results; never prints keys.

use std::sync::Arc;

use obiplayer_core::config::Config;
use obiplayer_core::meta::{Cache, Http, Keys, discogs::Discogs, lastfm::LastFm};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let keys = Keys::from_config(&Config::load().apis);
    let http = Arc::new(Http::new());
    let dir = std::env::temp_dir().join("obi-meta-probe-b");
    let cache = Arc::new(Cache::new(&dir));
    cache.purge();

    let dg = Discogs::new(http.clone(), cache.clone(), keys.discogs.clone());
    let lfm = LastFm::new(http.clone(), cache.clone(), keys.lastfm.clone());
    println!("discogs configured: {}", dg.is_configured());
    println!("lastfm configured: {}", lfm.is_configured());

    if let Some((k, s)) = &keys.discogs {
        let r = dg.validate_credentials(k, s).await;
        println!(
            "discogs validate: {}",
            if r.is_ok() {
                "OK".into()
            } else {
                format!("fail ({})", r.unwrap_err())
            }
        );
    }
    if let Some(k) = &keys.lastfm {
        let r = lfm.validate_key(k).await;
        println!(
            "lastfm validate: {}",
            if r.is_ok() {
                "OK".into()
            } else {
                format!("fail ({})", r.unwrap_err())
            }
        );
    }
    let bad = lfm.validate_key("0000000000000000000000000000dead").await;
    println!("lastfm bad key -> {bad:?}");

    match dg.search("Queen", "A Night at the Opera").await {
        Some(r) => println!(
            "discogs search OK: {} {} {:?} {} {} | {:?} | master={:?} cut={:?} pressed={:?} matrix={:?} | {}",
            r.id,
            r.title,
            r.year,
            r.country,
            r.catalog_number,
            r.formats,
            r.mastering_engineer,
            r.cut_by,
            r.pressed_at,
            r.matrix_runout,
            r.url
        ),
        None => println!("discogs search fail"),
    }
    let hints = obiplayer_core::meta::discogs::PreferHints {
        country: Some("Japan".into()),
        ..Default::default()
    };
    match dg.search_by_catalog("UICY-20003", &hints).await {
        Some(r) => println!(
            "discogs catno OK: {} {} {:?} {} {} {:?} | {}",
            r.id,
            r.title,
            r.year,
            r.country,
            r.catalog_number,
            r.formats,
            r.to_pressing_info().format_display()
        ),
        None => println!("discogs catno: none"),
    }
    match lfm.artist("Queen").await {
        Some(a) => println!(
            "lastfm artist OK: {} listeners={} tags={:?} similar={:?} img={} summary_len={}",
            a.name,
            a.listeners,
            a.tags,
            a.similar,
            a.image_url.is_some(),
            a.summary.len()
        ),
        None => println!("lastfm artist fail"),
    }
    println!(
        "lastfm album_tags: {:?}",
        lfm.album_tags("Queen", "A Night at the Opera").await
    );
    let sim = lfm.similar_artists("Queen", 5).await;
    println!("lastfm similar: {sim:?}");
    cache.purge();
}
