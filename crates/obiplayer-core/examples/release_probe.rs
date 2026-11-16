//! Live check of the release cascade for one album (uses configured keys;
//! never prints them).
//! cargo run -p obiplayer-core --example release_probe -- "<artist>" "<title>" <FORMAT> "<label>" [first-track-path]

use obiplayer_core::config::Config;
use obiplayer_core::meta::release::{AlbumQuery, lookup};
use obiplayer_core::meta::{Keys, Services};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let s = Services::new(&Keys::from_config(&Config::load().apis));
    let q = AlbumQuery {
        folder: format!("probe/{}", a[1]),
        artist: a[0].clone(),
        title: a[1].clone(),
        format: a.get(2).cloned().unwrap_or_default(),
        label: a.get(3).cloned().unwrap_or_default(),
        first_track_path: a.get(4).map(Into::into),
        ..Default::default()
    };
    let t = std::time::Instant::now();
    let r = lookup(&s, &q).await;
    println!("took {:.1}s", t.elapsed().as_secs_f32());
    println!("file tags: {:?}", r.tags);
    match &r.mb {
        Some(m) => println!(
            "MB: {} | {} | {} | {} | {} | matches edition: {} | producers {:?} | mastering {:?}",
            m.id,
            m.title,
            m.label,
            m.catalog_number,
            m.country,
            r.mb_matches_edition,
            m.producers,
            m.mastering_engineers
        ),
        None => println!("MB: none"),
    }
    match &r.dg {
        Some(d) => println!(
            "Discogs: {} | {} | {} | {} | {:?} | mastering {:?} | cut {:?}",
            d.id, d.label, d.catalog_number, d.country, d.formats, d.mastering_engineer, d.cut_by
        ),
        None => println!("Discogs: none"),
    }
    println!(
        "shown → label {:?} catalog {:?} country {:?}",
        r.label(),
        r.catalog(),
        r.country()
    );
    println!("album wiki: {:?}", r.album_wiki.as_ref().map(|w| &w.title));
    println!(
        "artist: {:?} / wiki: {:?}",
        r.artist.as_ref().map(|x| (&x.name, x.members.len())),
        r.artist_wiki.as_ref().map(|w| &w.title)
    );
}
