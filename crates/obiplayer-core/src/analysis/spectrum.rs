//! Spectrum bands for the visualizer (Hann window + real FFT, log bands),
//! plus the attack/decay smoothing the views apply per frame.

use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex};

pub const FFT_SIZE: usize = 2048;

pub struct Spectrum {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    input: Vec<f32>,
    output: Vec<realfft::num_complex::Complex<f32>>,
}

impl Default for Spectrum {
    fn default() -> Self {
        Spectrum::new()
    }
}

impl Spectrum {
    pub fn new() -> Spectrum {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE);
        let window = (0..FFT_SIZE)
            .map(|i| {
                0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos()
            })
            .collect();
        let input = fft.make_input_vec();
        let output = fft.make_output_vec();
        Spectrum {
            fft,
            window,
            input,
            output,
        }
    }

    /// `bands` log-spaced magnitudes (roughly 0..1) from the latest samples.
    pub fn bands(&mut self, samples: &[f32], bands: usize) -> Vec<f32> {
        if samples.len() < FFT_SIZE || bands == 0 {
            return vec![0.0; bands];
        }
        let s = &samples[samples.len() - FFT_SIZE..];
        for (i, x) in self.input.iter_mut().enumerate() {
            *x = s[i] * self.window[i];
        }
        if self.fft.process(&mut self.input, &mut self.output).is_err() {
            return vec![0.0; bands];
        }
        let n = FFT_SIZE as f32;
        let mags: Vec<f32> = self.output.iter().map(|c| c.norm() * 2.0 / n).collect();
        let nyquist = mags.len() as f32;
        (0..bands)
            .map(|b| {
                let lo = nyquist.powf(b as f32 / bands as f32) as usize;
                let hi = (nyquist.powf((b + 1) as f32 / bands as f32) as usize)
                    .max(lo + 1)
                    .min(mags.len());
                let lo = lo.min(hi.saturating_sub(1));
                let slice = &mags[lo..hi];
                let avg = slice.iter().sum::<f32>() / slice.len().max(1) as f32;
                (avg.sqrt() * 4.0).min(1.0)
            })
            .collect()
    }
}

/// Per-frame smoothing: fast attack, slower decay, falling peak markers.
#[derive(Debug, Clone, Default)]
pub struct Smoother {
    pub values: Vec<f32>,
    pub peaks: Vec<f32>,
}

impl Smoother {
    const ATTACK: f32 = 0.8;
    const DECAY: f32 = 0.35;
    const PEAK_FALL: f32 = 0.012;

    pub fn update(&mut self, target: &[f32]) {
        if self.values.len() != target.len() {
            self.values = vec![0.0; target.len()];
            self.peaks = vec![0.0; target.len()];
        }
        for (i, &t) in target.iter().enumerate() {
            let v = &mut self.values[i];
            let k = if t > *v { Self::ATTACK } else { Self::DECAY };
            *v += (t - *v) * k;
            self.peaks[i] = (self.peaks[i] - Self::PEAK_FALL).max(*v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_lands_in_one_band() {
        let rate = 44100.0;
        let freq = 1000.0;
        let s: Vec<f32> = (0..FFT_SIZE)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate).sin())
            .collect();
        let b = Spectrum::new().bands(&s, 32);
        let (imax, _) = b
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap();
        // 1 kHz is bin ~46 of 1024 → log band ≈ 32·ln(46)/ln(1025) ≈ 17.
        assert!((16..=18).contains(&imax), "band {imax}");
        assert!(
            Spectrum::new()
                .bands(&[0.0; 10], 8)
                .iter()
                .all(|&v| v == 0.0)
        );
    }

    #[test]
    fn smoothing_attacks_fast_decays_slow() {
        let mut s = Smoother::default();
        s.update(&[1.0]);
        assert!((s.values[0] - 0.8).abs() < 1e-6);
        s.update(&[0.0]);
        assert!((s.values[0] - 0.52).abs() < 1e-6);
        assert!(s.peaks[0] > s.values[0]);
    }
}
