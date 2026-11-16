//! Everything about the playing track from ONE decode: DR14, waveform peaks
//! and (optionally) 44.1 kHz samples for the visualizers. DSD decodes are
//! expensive; DrPlayer decoded the same file three times.

use std::path::Path;

use super::dr14::{self, Dr, Meter};
use super::waveform::{self, BARS};
use super::{for_each_f32, resolve, stream_info};

pub const VIZ_RATE: u32 = 44_100;

#[derive(Debug, Default)]
pub struct TrackAnalysis {
    pub dr: Option<Dr>,
    pub peaks: Option<Vec<f32>>,
    pub samples: Option<Vec<f32>>,
}

/// Analyse `path` (MPD URI `key`). Cached DR/waveform are reused; the file
/// is decoded only if something is missing. Blocking.
pub fn analyze(key: &str, path: &Path, want_samples: bool) -> TrackAnalysis {
    let mut out = TrackAnalysis {
        dr: dr14::cached(key),
        peaks: waveform::cached(key),
        samples: None,
    };
    if out.dr.is_some() && out.peaks.is_some() && !want_samples {
        return out;
    }
    let Some(resolved) = resolve(path) else {
        return out;
    };
    let Some((rate, duration)) = stream_info(&resolved) else {
        return out;
    };

    let mut meter = Meter::new(rate);
    // Waveform: max |x| per bar. Bar width from the expected length; if the
    // duration is unknown, gather 10 ms buckets and reduce at the end.
    let total = duration
        .map(|d| (d * rate as f64) as usize)
        .filter(|&n| n > 0);
    let per_bar = total.map(|t| (t / BARS).max(1));
    let bucket = (rate as usize / 100).max(1);
    let mut bars = vec![0f32; BARS];
    let mut buckets: Vec<f32> = Vec::new();
    // Visualizer: nearest-sample resampling to 44.1 kHz (no filter needed
    // for drawing).
    let step = rate as f64 / VIZ_RATE as f64;
    let mut viz: Vec<f32> = Vec::new();
    let mut next_viz = 0f64;
    let mut n = 0usize;

    let ok = for_each_f32(&resolved, rate, |chunk| {
        meter.push(chunk);
        for &x in chunk {
            let a = x.abs();
            match per_bar {
                Some(pb) => {
                    let i = (n / pb).min(BARS - 1);
                    bars[i] = bars[i].max(a);
                }
                None => {
                    let i = n / bucket;
                    if i >= buckets.len() {
                        buckets.push(0.0);
                    }
                    buckets[i] = buckets[i].max(a);
                }
            }
            if want_samples && n as f64 >= next_viz {
                viz.push(x);
                next_viz += step;
            }
            n += 1;
        }
    });
    if !ok {
        return out;
    }

    if out.dr.is_none()
        && let Some(d) = meter.finish()
    {
        dr14::store(key, &d);
        out.dr = Some(d);
    }
    if out.peaks.is_none() {
        if per_bar.is_none() && !buckets.is_empty() {
            let per = (buckets.len() / BARS).max(1);
            bars = (0..BARS)
                .map(|i| {
                    buckets
                        .get(i * per..((i + 1) * per).min(buckets.len()))
                        .unwrap_or(&[])
                        .iter()
                        .copied()
                        .fold(0.0, f32::max)
                })
                .collect();
        }
        let max = bars.iter().copied().fold(0.0, f32::max);
        if max > 0.0 {
            for b in &mut bars {
                *b /= max;
            }
            waveform::store(key, &bars);
            out.peaks = Some(bars);
        }
    }
    if want_samples {
        out.samples = Some(viz);
    }
    out
}
