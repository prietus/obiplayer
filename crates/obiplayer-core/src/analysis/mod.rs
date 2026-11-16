//! Audio analysis on local files: DR14, waveform peaks and spectrum.
//!
//! Decoding is delegated to `ffmpeg`, read from its stdout as a stream, so
//! long hi-res files (DSD decodes to 352.8 kHz) never sit fully in memory.

pub mod dr14;
pub mod spectrum;
pub mod waveform;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use crate::probe::find_binary;

/// Sample rate of the first audio stream, via ffprobe.
pub fn sample_rate(path: &Path) -> Option<u32> {
    let out = Command::new(find_binary("ffprobe")?)
        .args([
            "-v",
            "quiet",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=sample_rate",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .ok()
        .filter(|&r: &u32| r > 0)
}

/// Spawn ffmpeg decoding `path` to mono raw samples on stdout.
/// `format` is an ffmpeg sample format (`f32le`, `u8`), `rate` the output rate.
fn ffmpeg_mono(path: &Path, format: &str, codec: &str, rate: u32) -> Option<Child> {
    Command::new(find_binary("ffmpeg")?)
        .args(["-v", "quiet", "-nostdin", "-i"])
        .arg(path)
        .args([
            "-ac",
            "1",
            "-ar",
            &rate.to_string(),
            "-f",
            format,
            "-acodec",
            codec,
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

/// Stream mono f32 samples from ffmpeg in chunks. Returns `false` if ffmpeg
/// is missing or exits with an error.
pub fn for_each_f32(path: &Path, rate: u32, mut f: impl FnMut(&[f32])) -> bool {
    let Some(mut child) = ffmpeg_mono(path, "f32le", "pcm_f32le", rate) else {
        return false;
    };
    let Some(mut out) = child.stdout.take() else {
        return false;
    };
    let mut buf = vec![0u8; 1 << 16];
    let mut carry: Vec<u8> = Vec::with_capacity(4);
    let mut samples: Vec<f32> = Vec::with_capacity(buf.len() / 4 + 1);
    loop {
        let n = match out.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => {
                let _ = child.kill();
                return false;
            }
        };
        carry.extend_from_slice(&buf[..n]);
        let whole = carry.len() / 4 * 4;
        samples.clear();
        samples.extend(
            carry[..whole]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b)),
        );
        carry.drain(..whole);
        f(&samples);
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

/// Decode a whole file to mono u8 at a low rate (for waveforms).
fn decode_u8(path: &Path, rate: u32) -> Option<Vec<u8>> {
    let child = ffmpeg_mono(path, "u8", "pcm_u8", rate)?;
    let out = child.wait_with_output().ok()?;
    out.status.success().then_some(out.stdout)
}

/// Decode a whole file to mono f32 (for the file-based visualizer).
pub fn decode_f32(path: &Path, rate: u32) -> Option<Vec<f32>> {
    let mut all = Vec::new();
    for_each_f32(path, rate, |s| all.extend_from_slice(s)).then_some(all)
}

/// Resolve symlinks (NFS libraries are often mapped through them).
pub(crate) fn resolve(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok().filter(|p| p.is_file())
}
