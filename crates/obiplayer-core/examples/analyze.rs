//! Analyse local files: `cargo run -p obiplayer-core --example analyze -- <file>...`

use std::path::Path;
use std::time::Instant;

use obiplayer_core::analysis::{dr14, waveform};

fn main() {
    for arg in std::env::args().skip(1) {
        let p = Path::new(&arg);
        let t = Instant::now();
        let dr = dr14::analyze(&arg, p);
        let t_dr = t.elapsed();
        let t = Instant::now();
        let wf = waveform::generate(&arg, p);
        println!(
            "{arg}\n  DR: {:?} ({:.1}s)\n  waveform: {} bars ({:.1}s)",
            dr,
            t_dr.as_secs_f32(),
            wf.map(|w| w.len()).unwrap_or(0),
            t.elapsed().as_secs_f32()
        );
    }
}
