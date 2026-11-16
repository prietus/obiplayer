//! Seek-bar waveform: 400 normalised peaks per track.

use std::path::Path;

use super::{decode_u8, resolve};
use crate::{paths, text};

pub const BARS: usize = 400;
/// Decode rate; plenty for an overview, tiny to transfer.
const RATE: u32 = 200;

/// Reduce unsigned 8-bit mono samples to `BARS` peaks in 0..=1.
pub fn peaks_from_u8(data: &[u8]) -> Vec<f32> {
    let chunk = (data.len() / BARS).max(1);
    let mut bars: Vec<f32> = (0..BARS)
        .map(|i| {
            data.get(i * chunk..((i + 1) * chunk).min(data.len()))
                .unwrap_or(&[])
                .iter()
                .map(|&b| (b as f32 - 128.0).abs() / 128.0)
                .fold(0.0, f32::max)
        })
        .collect();
    let max = bars.iter().copied().fold(0.0, f32::max);
    if max > 0.0 {
        for b in &mut bars {
            *b /= max;
        }
    }
    bars
}

fn cache_path(key: &str) -> std::path::PathBuf {
    paths::cache_dir()
        .join("waveforms")
        .join(format!("{}.wf", text::djb2_hex(key)))
}

/// Cached peaks for an MPD URI.
pub fn cached(key: &str) -> Option<Vec<f32>> {
    let b = std::fs::read(cache_path(key)).ok()?;
    (b.len() == BARS).then(|| b.iter().map(|&v| v as f32 / 255.0).collect())
}

/// Save peaks (0..=1) in the cache.
pub fn store(key: &str, peaks: &[f32]) {
    let cache = cache_path(key);
    if let Some(dir) = cache.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let bytes: Vec<u8> = peaks
        .iter()
        .map(|p| (p.clamp(0.0, 1.0) * 255.0) as u8)
        .collect();
    let _ = std::fs::write(&cache, bytes);
}

/// Peaks for MPD URI `key` whose local file is `path` (cached). Blocking.
pub fn generate(key: &str, path: &Path) -> Option<Vec<f32>> {
    let cache = cache_path(key);
    if let Ok(b) = std::fs::read(&cache)
        && b.len() == BARS
    {
        return Some(b.iter().map(|&v| v as f32 / 255.0).collect());
    }
    let resolved = resolve(path)?;
    let peaks = peaks_from_u8(&decode_u8(&resolved, RATE)?);
    if let Some(dir) = cache.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let bytes: Vec<u8> = peaks
        .iter()
        .map(|p| (p.clamp(0.0, 1.0) * 255.0) as u8)
        .collect();
    let _ = std::fs::write(&cache, bytes);
    Some(peaks)
}

pub fn cache_count() -> usize {
    std::fs::read_dir(paths::cache_dir().join("waveforms"))
        .map(|d| d.count())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peaks_are_normalised() {
        let mut data = vec![128u8; 4000];
        data[0] = 255;
        data[2000] = 192;
        let p = peaks_from_u8(&data);
        assert_eq!(p.len(), BARS);
        assert!((p[0] - 1.0).abs() < 1e-6);
        assert!((p[200] - 64.0 / 127.0).abs() < 1e-3);
        assert_eq!(p[100], 0.0);
    }

    #[test]
    fn short_input_pads_with_zero() {
        let p = peaks_from_u8(&[0, 255]);
        assert_eq!(p.len(), BARS);
        assert!(p[2..].iter().all(|&v| v == 0.0));
    }
}
