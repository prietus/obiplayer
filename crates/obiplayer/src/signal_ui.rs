//! Signal-path indicator (dot in the player bar) and its popover.

use obiplayer_core::mpd::{AudioFormat, PcmBits, SampleFormat, Status};
use obiplayer_core::signal::{DsdTransport, SignalPath};
use relm4::gtk::{self, prelude::*};

use crate::tr;
use crate::ui;

pub struct SignalIndicator {
    pub button: gtk::MenuButton,
    dot: gtk::Box,
    content: gtk::Box,
}

fn source_text(a: Option<AudioFormat>) -> String {
    let Some(a) = a else { return "—".into() };
    let ch = match a.channels {
        1 => "mono".to_owned(),
        2 => "stereo".to_owned(),
        n => format!("{n} ch"),
    };
    match a.sample {
        SampleFormat::Dsd { multiplier } => format!("DSD{multiplier} · {ch}"),
        SampleFormat::Pcm { rate, bits } => {
            let khz = if rate % 1000 == 0 {
                format!("{} kHz", rate / 1000)
            } else {
                format!("{:.1} kHz", rate as f64 / 1000.0)
            };
            let b = match bits {
                PcmBits::Int(b) => format!("{b} bit"),
                PcmBits::Float => "32 bit float".into(),
            };
            format!("{khz} · {b} · {ch}")
        }
    }
}

fn node(icon: &str, title: &str, detail: &str, ok: bool) -> gtk::Box {
    let row = ui::hbox(10);
    let img = gtk::Image::builder()
        .icon_name(icon)
        .pixel_size(20)
        .valign(gtk::Align::Start)
        .css_classes([if ok { "signal-ok" } else { "signal-warn" }])
        .build();
    row.append(&img);
    let text = ui::vbox(2);
    text.append(&ui::label(title, &["heading"]));
    if !detail.is_empty() {
        let d = ui::label(detail, &["dim-label", "caption"]);
        d.set_width_chars(28);
        d.set_max_width_chars(40);
        text.append(&d);
    }
    row.append(&text);
    row
}

fn connector() -> gtk::Box {
    gtk::Box::builder()
        .width_request(2)
        .height_request(12)
        .margin_start(9)
        .halign(gtk::Align::Start)
        .css_classes(["signal-connector"])
        .build()
}

impl SignalIndicator {
    pub fn new() -> SignalIndicator {
        let dot = gtk::Box::builder()
            .width_request(10)
            .height_request(10)
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Center)
            .css_classes(["signal-dot", "signal-idle"])
            .build();
        let content = ui::vbox(4);
        content.set_margin_top(10);
        content.set_margin_bottom(10);
        content.set_margin_start(10);
        content.set_margin_end(10);
        let pop = gtk::Popover::builder().child(&content).build();
        let button = gtk::MenuButton::builder()
            .child(&dot)
            .popover(&pop)
            .valign(gtk::Align::Center)
            .tooltip_text(tr("Signal path"))
            .css_classes(["flat", "circular"])
            .build();
        SignalIndicator {
            button,
            dot,
            content,
        }
    }

    pub fn update(&self, path: Option<&SignalPath>, status: &Status, connected: bool) {
        for c in ["signal-idle", "signal-perfect", "signal-processed"] {
            self.dot.remove_css_class(c);
        }
        let playing = connected && status.state != obiplayer_core::mpd::PlayState::Stop;
        let (class, tip) = match path {
            Some(p) if playing && p.is_bitperfect() => ("signal-perfect", "Bit-perfect"),
            Some(_) if playing => ("signal-processed", "Signal is processed"),
            _ => ("signal-idle", "Signal path"),
        };
        self.dot.add_css_class(class);
        self.button.set_tooltip_text(Some(&tr(tip)));

        ui::clear(&self.content);
        let header = ui::hbox(8);
        header.append(&ui::label(&tr("Signal path"), &["title-4"]));
        if let Some(p) = path.filter(|p| p.is_bitperfect()) {
            let _ = p;
            header.append(&ui::badge(&tr("Bit-perfect"), "badge-best"));
        }
        self.content.append(&header);
        let Some(p) = path else {
            self.content
                .append(&ui::label(&tr("No data from MPD"), &["dim-label"]));
            return;
        };

        self.content.append(&node(
            "audio-x-generic-symbolic",
            &tr("Source"),
            &source_text(p.source),
            true,
        ));
        self.content.append(&connector());

        let mut mpd_detail = status
            .bitrate
            .map(|b| format!("{b} kbps · "))
            .unwrap_or_default();
        if p.issues.is_empty() {
            mpd_detail.push_str(&tr("No processing"));
        } else {
            mpd_detail.push_str(&p.issues.join("\n"));
        }
        self.content.append(&node(
            "network-server-symbolic",
            "MPD",
            &mpd_detail,
            p.issues.is_empty(),
        ));
        self.content.append(&connector());

        match &p.output {
            Some(o) => {
                let mut d = vec![o.plugin.to_uppercase()];
                d.push(match status.volume {
                    None => tr("no mixer"),
                    Some(v) => format!("{} {v}%", tr("volume")),
                });
                match p.dsd {
                    Some(DsdTransport::Native) => d.push(tr("native DSD")),
                    Some(DsdTransport::DoP) => d.push("DoP".into()),
                    None => {}
                }
                self.content.append(&node(
                    "audio-speakers-symbolic",
                    &o.name,
                    &d.join(" · "),
                    p.is_bitperfect(),
                ));
            }
            None => self.content.append(&node(
                "audio-speakers-symbolic",
                &tr("Output"),
                &tr("No active output"),
                false,
            )),
        }
    }
}
