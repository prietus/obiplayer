//! Player bar: cover, track info, audio format/DR, waveform seek bar,
//! transport, signal path and panel toggles.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use obiplayer_core::library::Album;
use obiplayer_core::mpd::{PlayState, Single, Song, Status};
use obiplayer_core::signal::SignalPath;
use relm4::gtk::{self, glib, prelude::*};
use relm4::{Component, ComponentParts, ComponentSender};

use crate::ctx::{Action, Ctx};
use crate::signal_ui::SignalIndicator;
use crate::tr;
use crate::ui;
use crate::viz::Waveform;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    TogglePause,
    Stop,
    Next,
    Previous,
    ToggleShuffle,
    CycleRepeat,
}

#[derive(Debug)]
pub enum BarInput {
    Update {
        status: Box<Status>,
        song: Option<Box<Song>>,
        album: Option<Arc<Album>>,
    },
    Peaks(Option<Arc<Vec<f32>>>),
    Signal(Option<Box<SignalPath>>, bool),
    QueueShown(bool),
    VizShown(bool),
    LyricsShown(bool),
    Tasks(crate::tasks_ui::TasksState),
    RadioOn(bool),
    /// Re-read shared state (e.g. a DR value arrived).
    Refresh,
    Tick,
}

#[derive(Debug)]
pub enum BarOutput {
    Transport(Transport),
    ShowQueue(bool),
    ShowViz(bool),
    ShowLyrics(bool),
    OpenCurrentAlbum,
}

pub struct PlayerBar {
    ctx: Rc<Ctx>,
    status: Status,
    song: Option<Song>,
    album_folder: Option<String>,
    queue_shown: bool,
    viz_shown: bool,
    lyrics_shown: bool,
    radio_on: bool,
}

pub struct BarWidgets {
    cover: gtk::Picture,
    cover_placeholder: gtk::Image,
    title: gtk::Label,
    artist_album: gtk::Label,
    album_button: gtk::Button,
    format: gtk::Label,
    bitrate: gtk::Label,
    dr: gtk::Label,
    waveform: Waveform,
    elapsed: gtk::Label,
    total: gtk::Label,
    play: gtk::Button,
    shuffle: gtk::Button,
    repeat: gtk::Button,
    queue: gtk::ToggleButton,
    queue_handler: glib::SignalHandlerId,
    viz: gtk::ToggleButton,
    viz_handler: glib::SignalHandlerId,
    lyrics: gtk::ToggleButton,
    lyrics_handler: glib::SignalHandlerId,
    radio: gtk::ToggleButton,
    radio_handler: glib::SignalHandlerId,
    signal: SignalIndicator,
    tasks: std::rc::Rc<crate::tasks_ui::TasksIndicator>,
    cover_folder: Rc<std::cell::RefCell<Option<String>>>,
}

/// CSS class for a DR value (DrPlayer's colour bands).
pub fn dr_class(dr: i32) -> &'static str {
    match dr {
        14.. => "dr-green",
        10..=13 => "dr-yellow",
        7..=9 => "dr-orange",
        _ => "dr-red",
    }
}

fn toggle(icon: &str, tip: &str) -> gtk::ToggleButton {
    gtk::ToggleButton::builder()
        .icon_name(icon)
        .css_classes(["flat"])
        .tooltip_text(tr(tip))
        .valign(gtk::Align::Center)
        .build()
}

impl Component for PlayerBar {
    type Init = Rc<Ctx>;
    type Input = BarInput;
    type Output = BarOutput;
    type CommandOutput = ();
    type Root = gtk::Box;
    type Widgets = BarWidgets;

    fn init_root() -> gtk::Box {
        gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .margin_start(12)
            .margin_end(12)
            .margin_top(8)
            .margin_bottom(8)
            .css_classes(["player-bar"])
            .build()
    }

    fn init(ctx: Rc<Ctx>, root: gtk::Box, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        // -- Row 1: cover · text · audio info ----------------------------------
        let cover = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Cover)
            .width_request(48)
            .height_request(48)
            .build();
        let cover_placeholder = gtk::Image::builder()
            .icon_name("audio-x-generic-symbolic")
            .pixel_size(24)
            .css_classes(["dim-label"])
            .build();
        let cover_frame = gtk::Overlay::builder()
            .width_request(48)
            .height_request(48)
            .overflow(gtk::Overflow::Hidden)
            .valign(gtk::Align::Center)
            .css_classes(["bar-cover"])
            .tooltip_text(tr("Now playing (full window)"))
            .build();
        cover_frame.set_child(Some(&cover_placeholder));
        cover_frame.add_overlay(&cover);
        cover_frame.set_cursor_from_name(Some("pointer"));
        {
            let c = ctx.clone();
            let click = gtk::GestureClick::new();
            click.connect_released(move |_, _, _, _| c.act(Action::ShowNowPlaying(true)));
            cover_frame.add_controller(click);
        }

        let title = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["heading"])
            .build();
        let artist_album = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["dim-label"])
            .build();
        let album_button = gtk::Button::builder()
            .child(&artist_album)
            .halign(gtk::Align::Start)
            .css_classes(["flat", "link-like"])
            .tooltip_text(tr("Show album"))
            .build();
        {
            let s = sender.clone();
            album_button.connect_clicked(move |_| {
                let _ = s.output(BarOutput::OpenCurrentAlbum);
            });
        }
        let text = ui::vbox(0);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        text.append(&title);
        text.append(&album_button);

        let format = gtk::Label::builder()
            .xalign(1.0)
            .css_classes(["monospace", "caption"])
            .build();
        let bitrate = gtk::Label::builder()
            .xalign(1.0)
            .css_classes(["monospace", "caption", "dim-label"])
            .build();
        let dr = gtk::Label::builder()
            .xalign(1.0)
            .css_classes(["monospace", "caption", "heading"])
            .tooltip_text(tr("Dynamic range (DR14)"))
            .build();
        let info = ui::vbox(0);
        info.set_valign(gtk::Align::Center);
        info.append(&format);
        let info2 = ui::hbox(8);
        info2.set_halign(gtk::Align::End);
        info2.append(&bitrate);
        info2.append(&dr);
        info.append(&info2);

        let signal = SignalIndicator::new();

        let row1 = ui::hbox(12);
        row1.append(&cover_frame);
        row1.append(&text);
        row1.append(&info);
        row1.append(&signal.button);

        // -- Row 2: waveform -----------------------------------------------------
        let elapsed = gtk::Label::builder()
            .css_classes(["monospace", "caption", "dim-label"])
            .width_chars(6)
            .xalign(0.0)
            .build();
        let total = gtk::Label::builder()
            .css_classes(["monospace", "caption", "dim-label"])
            .width_chars(6)
            .xalign(1.0)
            .build();
        let c = ctx.clone();
        let waveform = Waveform::new(&ctx, 32, move |f| {
            let d = c.clock.get().duration;
            if d > 0.0 {
                c.act(Action::Seek(f * d));
            }
        });
        let row2 = ui::hbox(8);
        row2.append(&elapsed);
        row2.append(&waveform.area);
        row2.append(&total);

        // -- Row 3: transport · toggles -----------------------------------------
        let button = |icon: &str, tip: &str, t: Transport| {
            let b = gtk::Button::builder()
                .icon_name(icon)
                .tooltip_text(tr(tip))
                .css_classes(["flat", "circular"])
                .build();
            let s = sender.clone();
            b.connect_clicked(move |_| {
                let _ = s.output(BarOutput::Transport(t));
            });
            b
        };
        let shuffle = button(
            "media-playlist-shuffle-symbolic",
            "Shuffle",
            Transport::ToggleShuffle,
        );
        let prev = button(
            "media-skip-backward-symbolic",
            "Previous",
            Transport::Previous,
        );
        let play = button(
            "media-playback-start-symbolic",
            "Play",
            Transport::TogglePause,
        );
        play.set_css_classes(&["circular", "suggested-action", "play-button"]);
        let stop = button("media-playback-stop-symbolic", "Stop", Transport::Stop);
        let next = button("media-skip-forward-symbolic", "Next", Transport::Next);
        let repeat = button(
            "media-playlist-repeat-symbolic",
            "Repeat: off",
            Transport::CycleRepeat,
        );
        let transport = ui::hbox(6);
        transport.set_halign(gtk::Align::Center);
        transport.set_hexpand(true);
        for b in [&shuffle, &prev, &play, &stop, &next, &repeat] {
            transport.append(b);
        }

        let viz = toggle("utilities-system-monitor-symbolic", "Oscilloscope");
        let viz_handler = {
            let s = sender.clone();
            viz.connect_toggled(move |b| {
                let _ = s.output(BarOutput::ShowViz(b.is_active()));
            })
        };
        let lyrics = toggle("format-justify-left-symbolic", "Lyrics");
        let lyrics_handler = {
            let s = sender.clone();
            lyrics.connect_toggled(move |b| {
                let _ = s.output(BarOutput::ShowLyrics(b.is_active()));
            })
        };
        let queue = toggle("view-list-symbolic", "Play queue");
        let queue_handler = {
            let s = sender.clone();
            queue.connect_toggled(move |b| {
                let _ = s.output(BarOutput::ShowQueue(b.is_active()));
            })
        };
        let toggles = ui::hbox(2);
        let radio = toggle("network-wireless-symbolic", "Radio");
        let radio_handler = {
            let c = ctx.clone();
            radio.connect_toggled(move |b| {
                c.act(if b.is_active() {
                    Action::StartRadioFromCurrent
                } else {
                    Action::StopRadio
                })
            })
        };
        let tasks = crate::tasks_ui::TasksIndicator::new();
        toggles.append(&tasks.button);
        toggles.append(&radio);
        toggles.append(&viz);
        toggles.append(&lyrics);
        toggles.append(&queue);
        // Balance the toggles so the transport stays centred.
        let spacer = gtk::Box::builder().width_request(180).build();
        let row3 = ui::hbox(0);
        row3.append(&spacer);
        row3.append(&transport);
        row3.append(&toggles);

        root.append(&row1);
        root.append(&row2);
        root.append(&row3);

        {
            let s = sender.clone();
            glib::timeout_add_local(Duration::from_millis(500), move || {
                s.input(BarInput::Tick);
                glib::ControlFlow::Continue
            });
        }

        let model = PlayerBar {
            ctx,
            status: Status::default(),
            song: None,
            album_folder: None,
            queue_shown: false,
            viz_shown: false,
            lyrics_shown: false,
            radio_on: false,
        };
        let widgets = BarWidgets {
            cover,
            cover_placeholder,
            title,
            artist_album,
            album_button,
            format,
            bitrate,
            dr,
            waveform,
            elapsed,
            total,
            play,
            shuffle,
            repeat,
            queue,
            queue_handler,
            viz,
            viz_handler,
            lyrics,
            lyrics_handler,
            radio,
            radio_handler,
            signal,
            tasks,
            cover_folder: Rc::default(),
        };
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: BarInput, _sender: ComponentSender<Self>, _root: &gtk::Box) {
        match msg {
            BarInput::Update {
                status,
                song,
                album,
            } => {
                self.status = *status;
                self.song = song.map(|s| *s);
                self.album_folder = album.as_ref().map(|a| {
                    self.ctx.covers.request(a, |_| {});
                    a.folder.clone()
                });
            }
            BarInput::QueueShown(v) => self.queue_shown = v,
            BarInput::VizShown(v) => self.viz_shown = v,
            BarInput::LyricsShown(v) => self.lyrics_shown = v,
            BarInput::RadioOn(v) => self.radio_on = v,
            // Handled in update_with_view-like fashion below.
            BarInput::Peaks(_)
            | BarInput::Signal(..)
            | BarInput::Tasks(_)
            | BarInput::Refresh
            | BarInput::Tick => {}
        }
    }

    fn update_with_view(
        &mut self,
        w: &mut BarWidgets,
        msg: BarInput,
        sender: ComponentSender<Self>,
        root: &gtk::Box,
    ) {
        match &msg {
            BarInput::Peaks(p) => w.waveform.set_peaks(p.clone()),
            BarInput::Signal(path, connected) => {
                w.signal.update(path.as_deref(), &self.status, *connected)
            }
            BarInput::Tasks(t) => w.tasks.update(*t),
            _ => {}
        }
        self.update(msg, sender.clone(), root);
        self.update_view(w, sender);
    }

    fn update_view(&self, w: &mut BarWidgets, _sender: ComponentSender<Self>) {
        let st = &self.status;
        let stopped = st.state == PlayState::Stop && self.song.is_none();

        match &self.song {
            Some(s) if !stopped => {
                w.title.set_label(s.display_title());
                let line = [s.artist(), s.album()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" — ");
                w.artist_album.set_label(&line);
                w.album_button.set_sensitive(self.album_folder.is_some());
            }
            _ => {
                w.title.set_label(&tr("Not playing"));
                w.artist_album.set_label("");
                w.album_button.set_sensitive(false);
            }
        }

        w.format
            .set_label(&st.audio.map(|a| a.to_string()).unwrap_or_default());
        w.bitrate.set_label(
            &st.bitrate
                .filter(|_| st.state != PlayState::Stop)
                .map(|b| format!("{b} kbps"))
                .unwrap_or_default(),
        );
        for c in ["dr-green", "dr-yellow", "dr-orange", "dr-red"] {
            w.dr.remove_css_class(c);
        }
        match self.song.as_ref().and_then(|s| self.ctx.dr(&s.file)) {
            Some(d) => {
                w.dr.set_label(&format!("DR{d}"));
                w.dr.add_css_class(dr_class(d));
            }
            None => w.dr.set_label(""),
        }

        let clock = self.ctx.clock.get();
        let has = clock.duration > 0.0;
        w.waveform.area.set_visible(has);
        w.elapsed.set_label(&if has {
            ui::mmss(clock.now())
        } else {
            String::new()
        });
        w.total.set_label(&if has {
            ui::mmss(clock.duration)
        } else {
            String::new()
        });

        let playing = st.state == PlayState::Play;
        w.play.set_icon_name(if playing {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        });
        w.play
            .set_tooltip_text(Some(&tr(if playing { "Pause" } else { "Play" })));
        set_accent(&w.shuffle, st.random);
        w.shuffle.set_tooltip_text(Some(&tr(if st.random {
            "Shuffle: on"
        } else {
            "Shuffle: off"
        })));
        let (icon, tip, on) = match (st.repeat, st.single) {
            (true, Single::On) => ("media-playlist-repeat-song-symbolic", "Repeat: track", true),
            (true, _) => ("media-playlist-repeat-symbolic", "Repeat: queue", true),
            (false, _) => ("media-playlist-repeat-symbolic", "Repeat: off", false),
        };
        w.repeat.set_icon_name(icon);
        w.repeat.set_tooltip_text(Some(&tr(tip)));
        set_accent(&w.repeat, on);

        w.queue.block_signal(&w.queue_handler);
        w.queue.set_active(self.queue_shown);
        w.queue.unblock_signal(&w.queue_handler);
        w.radio.block_signal(&w.radio_handler);
        w.radio.set_active(self.radio_on);
        w.radio.unblock_signal(&w.radio_handler);
        if self.radio_on {
            w.radio.add_css_class("radio-on");
        } else {
            w.radio.remove_css_class("radio-on");
        }
        w.lyrics.block_signal(&w.lyrics_handler);
        w.lyrics.set_active(self.lyrics_shown);
        w.lyrics.unblock_signal(&w.lyrics_handler);
        w.viz.block_signal(&w.viz_handler);
        w.viz.set_active(self.viz_shown);
        w.viz.unblock_signal(&w.viz_handler);

        // Cover: retried on each tick until the loader has it.
        let mut bound = w.cover_folder.borrow_mut();
        let want = self.album_folder.clone().filter(|_| !stopped);
        let cached = want.as_deref().and_then(|f| self.ctx.covers.cached(f));
        if *bound != want {
            let loaded = cached.is_some();
            let tex = cached.flatten();
            w.cover.set_paintable(tex.as_ref());
            w.cover_placeholder.set_visible(tex.is_none());
            if loaded || want.is_none() {
                *bound = want;
            }
        }
    }
}

fn set_accent(b: &gtk::Button, on: bool) {
    if on {
        b.add_css_class("accent");
        b.remove_css_class("dim-label");
    } else {
        b.remove_css_class("accent");
        b.add_css_class("dim-label");
    }
}
