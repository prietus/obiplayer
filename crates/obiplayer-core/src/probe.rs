//! Technical file metadata via `ffprobe`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Probe {
    pub codec: String,
    pub codec_long: String,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
    pub sample_format: String,
    pub channels: Option<u32>,
    pub channel_layout: String,
    pub duration: Option<f64>,
    pub format_name: String,
    pub size: Option<u64>,
    /// bit/s
    pub bit_rate: Option<u64>,
    /// Container tags, keys as found in the file.
    pub tags: BTreeMap<String, String>,
}

/// Locate an executable in `$PATH` (plus common prefixes).
pub fn find_binary(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain(["/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"].map(PathBuf::from))
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

fn str_field(v: &serde_json::Value, k: &str) -> String {
    v.get(k)
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_owned()
}

fn num<T: std::str::FromStr>(v: &serde_json::Value, k: &str) -> Option<T> {
    match v.get(k)? {
        serde_json::Value::String(s) => s.parse().ok(),
        serde_json::Value::Number(n) => n.to_string().parse().ok(),
        _ => None,
    }
}

/// Parse `ffprobe -print_format json -show_format -show_streams` output.
pub fn parse(json: &str) -> Option<Probe> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let stream = v
        .get("streams")?
        .as_array()?
        .iter()
        .find(|s| s.get("codec_type").and_then(|t| t.as_str()) == Some("audio"))?;
    let format = v.get("format").cloned().unwrap_or_default();

    let bit_depth = num::<u32>(stream, "bits_per_raw_sample")
        .filter(|&b| b > 0)
        .or_else(|| num::<u32>(stream, "bits_per_sample").filter(|&b| b > 0));
    let format_name = Some(str_field(&format, "format_long_name"))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| str_field(&format, "format_name"));
    let tags = format
        .get("tags")
        .and_then(|t| t.as_object())
        .map(|o| {
            o.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
                .collect()
        })
        .unwrap_or_default();

    Some(Probe {
        codec: str_field(stream, "codec_name"),
        codec_long: str_field(stream, "codec_long_name"),
        sample_rate: num(stream, "sample_rate"),
        bit_depth,
        sample_format: str_field(stream, "sample_fmt"),
        channels: num(stream, "channels"),
        channel_layout: str_field(stream, "channel_layout"),
        duration: num(stream, "duration").or_else(|| num(&format, "duration")),
        format_name,
        size: num(&format, "size"),
        bit_rate: num(&format, "bit_rate"),
        tags,
    })
}

/// Run ffprobe on a local file. `None` if ffprobe is missing or fails.
pub fn probe(path: &Path) -> Option<Probe> {
    let ffprobe = find_binary("ffprobe")?;
    let path = std::fs::canonicalize(path).ok()?;
    let out = Command::new(ffprobe)
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(&path)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ffprobe_json() {
        let p = parse(
            r#"{"streams":[{"codec_type":"video","codec_name":"mjpeg"},
                {"codec_type":"audio","codec_name":"flac","codec_long_name":"FLAC (Free Lossless Audio Codec)",
                 "sample_rate":"96000","channels":2,"channel_layout":"stereo","sample_fmt":"s32",
                 "bits_per_raw_sample":"24","duration":"312.5"}],
               "format":{"format_name":"flac","format_long_name":"raw FLAC","size":"123456789",
                 "bit_rate":"3160000","tags":{"ARTIST":"Queen","title":"Bohemian Rhapsody"}}}"#,
        )
        .unwrap();
        assert_eq!(p.codec, "flac");
        assert_eq!(p.sample_rate, Some(96000));
        assert_eq!(p.bit_depth, Some(24));
        assert_eq!(p.channels, Some(2));
        assert_eq!(p.duration, Some(312.5));
        assert_eq!(p.format_name, "raw FLAC");
        assert_eq!(p.size, Some(123_456_789));
        assert_eq!(p.tags.get("ARTIST").map(String::as_str), Some("Queen"));
    }

    #[test]
    fn bit_depth_fallbacks() {
        let p = parse(r#"{"streams":[{"codec_type":"audio","bits_per_raw_sample":"0","bits_per_sample":16}],"format":{}}"#).unwrap();
        assert_eq!(p.bit_depth, Some(16));
        let p = parse(r#"{"streams":[{"codec_type":"audio","bits_per_sample":0}],"format":{}}"#)
            .unwrap();
        assert_eq!(p.bit_depth, None);
    }
}
