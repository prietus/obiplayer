//! Signal path: is playback bit-perfect, and if not, why.
//!
//! Stricter than DrPlayer's rule (which only looked at `mixer_type`), using
//! what MPD exposes over the protocol, so it also works for remote servers:
//! - a mixer at less than 100 % alters samples;
//! - replay gain alters samples;
//! - `allowed_formats` on the output forces conversion of other formats;
//! - sound servers (PipeWire/Pulse) may resample or mix;
//! - network/file outputs (httpd, fifo…) are not a DAC path.
//!
//! MPD does not report global resampling (`audio_output_format`) over the
//! protocol; that cannot be detected remotely.

use crate::mpd::{AudioFormat, Output, Status};

/// Outputs that talk to a sound card directly.
const DIRECT: &[&str] = &["alsa", "osx", "oss", "sndio", "wasapi", "solaris"];
/// Outputs that go through a desktop sound server.
const SERVER: &[&str] = &["pipewire", "pulse", "jack", "ao"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DsdTransport {
    Native,
    DoP,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SignalPath {
    pub source: Option<AudioFormat>,
    pub output: Option<Output>,
    pub dsd: Option<DsdTransport>,
    /// Reasons playback is not bit-perfect; empty means bit-perfect.
    pub issues: Vec<String>,
}

impl SignalPath {
    pub fn is_bitperfect(&self) -> bool {
        self.output.is_some() && self.issues.is_empty()
    }
}

/// The output that reaches the listener: the first enabled direct/server
/// output, falling back to the first enabled one.
pub fn main_output(outputs: &[Output]) -> Option<&Output> {
    let enabled = || outputs.iter().filter(|o| o.enabled);
    enabled()
        .find(|o| DIRECT.contains(&o.plugin.as_str()))
        .or_else(|| enabled().find(|o| SERVER.contains(&o.plugin.as_str())))
        .or_else(|| enabled().next())
}

pub fn evaluate(status: &Status, outputs: &[Output], replay_gain_mode: &str) -> SignalPath {
    let output = main_output(outputs).cloned();
    let mut issues = Vec::new();

    match &output {
        None => issues.push("No enabled output".to_owned()),
        Some(o) => {
            let plugin = o.plugin.as_str();
            if SERVER.contains(&plugin) {
                issues.push(format!("{plugin}: sound server may resample or mix"));
            } else if !DIRECT.contains(&plugin) {
                issues.push(format!("{plugin}: not a sound card output"));
            }
            if o.attributes
                .get("allowed_formats")
                .is_some_and(|f| !f.trim().is_empty())
            {
                issues.push("Output restricts formats (conversion)".to_owned());
            }
        }
    }
    if let Some(v) = status.volume
        && v < 100
    {
        issues.push(format!("Volume at {v}%"));
    }
    if replay_gain_mode != "off" {
        issues.push(format!("Replay gain: {replay_gain_mode}"));
    }

    let dsd = status.audio.filter(AudioFormat::is_dsd).map(|_| {
        match output
            .as_ref()
            .and_then(|o| o.attributes.get("dop"))
            .map(String::as_str)
        {
            Some("1") | Some("yes") | Some("true") => DsdTransport::DoP,
            _ => DsdTransport::Native,
        }
    });

    SignalPath {
        source: status.audio,
        output,
        dsd,
        issues,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn out(id: u32, plugin: &str, enabled: bool, attrs: &[(&str, &str)]) -> Output {
        Output {
            id,
            name: format!("{plugin}{id}"),
            plugin: plugin.into(),
            enabled,
            attributes: attrs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    fn status(audio: &str, volume: Option<u8>) -> Status {
        Status {
            audio: AudioFormat::parse(audio),
            volume,
            ..Default::default()
        }
    }

    #[test]
    fn dac_without_mixer_is_bitperfect() {
        // The user's real server: DAC over ALSA plus an httpd stream.
        let outs = [
            out(0, "alsa", true, &[("allowed_formats", ""), ("dop", "0")]),
            out(1, "httpd", true, &[]),
        ];
        let p = evaluate(&status("dsd64:2", None), &outs, "off");
        assert!(p.is_bitperfect(), "{:?}", p.issues);
        assert_eq!(p.output.unwrap().plugin, "alsa");
        assert_eq!(p.dsd, Some(DsdTransport::Native));
    }

    #[test]
    fn reasons_are_reported() {
        let outs = [out(
            0,
            "pipewire",
            true,
            &[("allowed_formats", "48000:16:2")],
        )];
        let p = evaluate(&status("44100:16:2", Some(80)), &outs, "album");
        assert!(!p.is_bitperfect());
        assert_eq!(p.issues.len(), 4);
        assert_eq!(p.dsd, None);
    }

    #[test]
    fn dop_and_full_volume() {
        let outs = [out(0, "alsa", true, &[("dop", "1")])];
        let p = evaluate(&status("dsd128:2", Some(100)), &outs, "off");
        assert!(p.is_bitperfect());
        assert_eq!(p.dsd, Some(DsdTransport::DoP));
    }

    #[test]
    fn no_output() {
        let p = evaluate(
            &status("44100:16:2", None),
            &[out(0, "alsa", false, &[])],
            "off",
        );
        assert!(!p.is_bitperfect());
    }
}
