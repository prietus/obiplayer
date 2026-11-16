//! Analyse local files: `cargo run -p obiplayer-core --example analyze -- <file>...`
//! Uses throwaway cache keys so results are always computed.

use std::path::Path;
use std::time::Instant;

use obiplayer_core::analysis::{dr14, track};

fn main() {
    for (i, arg) in std::env::args().skip(1).enumerate() {
        let p = Path::new(&arg);
        let t = Instant::now();
        let old = dr14::analyze(&format!("probe-old-{i}-{}", std::process::id()), p);
        let t_old = t.elapsed();
        let t = Instant::now();
        let a = track::analyze(&format!("probe-new-{i}-{}", std::process::id()), p, true);
        println!(
            "{arg}\n  separate DR: {:?} ({:.1}s)\n  single pass: DR {:?}, {} bars, {} viz samples ({:.1}s)",
            old.map(|d| d.dr),
            t_old.as_secs_f32(),
            a.dr.map(|d| d.dr),
            a.peaks.map(|p| p.len()).unwrap_or(0),
            a.samples.map(|s| s.len()).unwrap_or(0),
            t.elapsed().as_secs_f32()
        );
    }
}
