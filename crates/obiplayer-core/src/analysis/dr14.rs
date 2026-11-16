//! DR14 dynamic range, computed like DrPlayer (and close to the TT DR meter).
//!
//! - Mono downmix (ffmpeg `-ac 1`) at the file's own sample rate.
//! - 3-second blocks; a trailing partial block is ignored.
//! - Per block: RMS = sqrt(2·Σx²/N) and peak = max|x|.
//! - Loudest 20 % of block RMS values (at least one) are averaged linearly.
//! - The second-highest block peak is used (highest if there is one block).
//! - DR = −20·log10(meanRMS / peak2), rounded, clamped to 0..=99.

use std::path::Path;

use super::{for_each_f32, resolve, sample_rate};
use crate::{paths, text};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dr {
    pub dr: i32,
    /// dBFS of the second-highest block peak.
    pub peak_db: f64,
    /// dBFS of the mean top-20 % RMS.
    pub rms_db: f64,
}

/// Incremental DR computation over a sample stream.
#[derive(Debug)]
pub struct Meter {
    block: usize,
    filled: usize,
    sum_sq: f64,
    peak: f32,
    rms: Vec<f64>,
    peaks: Vec<f32>,
}

impl Meter {
    pub fn new(sample_rate: u32) -> Meter {
        Meter {
            block: (sample_rate as usize * 3).max(1),
            filled: 0,
            sum_sq: 0.0,
            peak: 0.0,
            rms: Vec::new(),
            peaks: Vec::new(),
        }
    }

    pub fn push(&mut self, samples: &[f32]) {
        for &x in samples {
            self.sum_sq += (x as f64) * (x as f64);
            self.peak = self.peak.max(x.abs());
            self.filled += 1;
            if self.filled == self.block {
                self.rms
                    .push((2.0 * self.sum_sq / self.block as f64).sqrt());
                self.peaks.push(self.peak);
                self.filled = 0;
                self.sum_sq = 0.0;
                self.peak = 0.0;
            }
        }
    }

    pub fn finish(mut self) -> Option<Dr> {
        let n = self.rms.len();
        if n == 0 {
            return None;
        }
        self.rms.sort_by(f64::total_cmp);
        let top = (n / 5).max(1);
        let mean_rms = self.rms[n - top..].iter().sum::<f64>() / top as f64;

        self.peaks.sort_by(f32::total_cmp);
        let peak = if n >= 2 {
            self.peaks[n - 2]
        } else {
            self.peaks[n - 1]
        } as f64;

        if mean_rms <= 0.0 || peak <= 0.0 {
            return None;
        }
        let dr = (-20.0 * (mean_rms / peak).log10()).round().clamp(0.0, 99.0) as i32;
        Some(Dr {
            dr,
            peak_db: 20.0 * peak.log10(),
            rms_db: 20.0 * mean_rms.log10(),
        })
    }
}

/// Album DR: integer mean of the tracks' positive DR values.
pub fn album_dr(values: impl IntoIterator<Item = i32>) -> Option<i32> {
    let v: Vec<i32> = values.into_iter().filter(|&d| d > 0).collect();
    (!v.is_empty()).then(|| v.iter().sum::<i32>() / v.len() as i32)
}

/// Cache entries are keyed by the MPD URI, so lookups never touch the
/// (possibly network) music directory.
fn cache_path(key: &str) -> std::path::PathBuf {
    paths::cache_dir()
        .join("dr14")
        .join(format!("{}.dr14", text::djb2_hex(key)))
}

fn parse_cache(s: &str) -> Option<Dr> {
    let mut it = s.trim().split(',');
    let dr = it.next()?.parse().ok()?;
    let peak_db = it.next()?.parse().ok()?;
    let rms_db = it.next()?.parse().ok()?;
    it.next().is_none().then_some(Dr {
        dr,
        peak_db,
        rms_db,
    })
}

/// Cached result for an MPD URI (no decoding, local disk only).
pub fn cached(key: &str) -> Option<Dr> {
    parse_cache(&std::fs::read_to_string(cache_path(key)).ok()?)
}

/// Analyse the local file `path` of MPD URI `key` (cached). Blocking.
pub fn analyze(key: &str, path: &Path) -> Option<Dr> {
    if let Some(d) = cached(key) {
        return Some(d);
    }
    let cache = cache_path(key);
    let resolved = resolve(path)?;
    let rate = sample_rate(&resolved)?;
    let mut meter = Meter::new(rate);
    if !for_each_f32(&resolved, rate, |s| meter.push(s)) {
        return None;
    }
    let d = meter.finish()?;
    if let Some(dir) = cache.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&cache, format!("{},{:.1},{:.1}", d.dr, d.peak_db, d.rms_db));
    Some(d)
}

/// Number of cached analyses (for the background-tasks panel).
pub fn cache_count() -> usize {
    std::fs::read_dir(paths::cache_dir().join("dr14"))
        .map(|d| d.count())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, secs: usize, amp: f32) -> Vec<f32> {
        (0..rate as usize * secs)
            .map(|i| amp * (i as f32 * 2.0 * std::f32::consts::PI * 1000.0 / rate as f32).sin())
            .collect()
    }

    #[test]
    fn full_scale_sine_is_dr0() {
        // A sine's RMS·√2 equals its peak, so DR ≈ 0.
        let mut m = Meter::new(8000);
        m.push(&tone(8000, 12, 1.0));
        assert_eq!(m.finish().unwrap().dr, 0);
    }

    #[test]
    fn quiet_body_with_loud_peaks() {
        let rate = 8000;
        let mut s = tone(rate, 30, 0.1);
        // Two short spikes in different blocks: the second-highest peak counts.
        s[rate as usize] = 1.0;
        s[rate as usize * 10] = 0.9;
        let mut m = Meter::new(rate);
        m.push(&s);
        let d = m.finish().unwrap();
        // RMS of the 0.1 sine (×√2) ≈ 0.1 → −20·log10(0.1/0.9) ≈ 19.
        assert_eq!(d.dr, 19);
    }

    #[test]
    fn too_short_or_silent() {
        let mut m = Meter::new(8000);
        m.push(&tone(8000, 2, 0.5));
        assert!(m.finish().is_none());
        let mut m = Meter::new(8000);
        m.push(&vec![0.0; 8000 * 6]);
        assert!(m.finish().is_none());
    }

    #[test]
    fn chunking_does_not_matter() {
        let s = tone(8000, 9, 0.3);
        let mut a = Meter::new(8000);
        a.push(&s);
        let mut b = Meter::new(8000);
        for c in s.chunks(777) {
            b.push(c);
        }
        assert_eq!(a.finish(), b.finish());
    }

    #[test]
    fn album_average() {
        assert_eq!(album_dr([12, 13, 0, 14]), Some(13));
        assert_eq!(album_dr([0]), None);
    }

    #[test]
    fn cache_format() {
        assert_eq!(
            parse_cache("12,-0.3,-14.2"),
            Some(Dr {
                dr: 12,
                peak_db: -0.3,
                rms_db: -14.2
            })
        );
        assert_eq!(parse_cache("12,-0.3"), None);
    }
}
