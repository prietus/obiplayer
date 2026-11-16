//! Background-tasks indicator (player bar) and popover.

use obiplayer_core::analysis::{dr14, waveform};
use obiplayer_core::meta::genres;
use relm4::gtk::{self, prelude::*};

use crate::tr;
use crate::ui;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TasksState {
    pub genres_done: usize,
    pub genres_total: usize,
    pub genres_running: bool,
    pub dr_pending: usize,
    pub outputs: usize,
}

pub struct TasksIndicator {
    pub button: gtk::MenuButton,
    icon: gtk::Image,
    content: gtk::Box,
    state: std::cell::Cell<TasksState>,
}

fn row(icon: &str, title: &str, status: &str, active: bool, fraction: Option<f64>) -> gtk::Box {
    let r = ui::hbox(10);
    r.append(
        &gtk::Image::builder()
            .icon_name(icon)
            .valign(gtk::Align::Start)
            .css_classes([if active { "task-active" } else { "dim-label" }])
            .build(),
    );
    let t = ui::vbox(2);
    t.set_hexpand(true);
    let head = ui::hbox(6);
    head.append(&ui::label(title, &["heading"]));
    if active {
        head.append(&gtk::Spinner::builder().spinning(true).build());
    }
    t.append(&head);
    t.append(&ui::label(status, &["dim-label", "caption"]));
    if let Some(f) = fraction {
        t.append(
            &gtk::ProgressBar::builder()
                .fraction(f)
                .css_classes(["task-progress"])
                .build(),
        );
    }
    r.append(&t);
    r
}

impl TasksIndicator {
    pub fn new() -> std::rc::Rc<TasksIndicator> {
        let icon = gtk::Image::from_icon_name("emblem-system-symbolic");
        let content = ui::vbox(10);
        content.set_margin_top(10);
        content.set_margin_bottom(10);
        content.set_margin_start(10);
        content.set_margin_end(10);
        content.set_width_request(320);
        let pop = gtk::Popover::builder().child(&content).build();
        let button = gtk::MenuButton::builder()
            .child(&icon)
            .popover(&pop)
            .valign(gtk::Align::Center)
            .tooltip_text(tr("Background tasks"))
            .css_classes(["flat"])
            .build();
        let me = std::rc::Rc::new(TasksIndicator {
            button,
            icon,
            content,
            state: Default::default(),
        });
        {
            let m = me.clone();
            pop.connect_show(move |_| m.render());
        }
        me
    }

    pub fn update(&self, s: TasksState) {
        self.state.set(s);
        let busy = s.genres_running || s.dr_pending > 0;
        if busy {
            self.icon.add_css_class("task-active");
        } else {
            self.icon.remove_css_class("task-active");
        }
        if self.button.popover().is_some_and(|p| p.is_visible()) {
            self.render();
        }
    }

    fn render(&self) {
        let s = self.state.get();
        ui::clear(&self.content);
        self.content
            .append(&ui::label(&tr("Background tasks"), &["title-4"]));
        let (status, frac) = if s.genres_running {
            (
                format!(
                    "{} {}/{} {}",
                    tr("Processing"),
                    s.genres_done,
                    s.genres_total,
                    tr("albums…")
                ),
                Some(if s.genres_total > 0 {
                    s.genres_done as f64 / s.genres_total as f64
                } else {
                    0.0
                }),
            )
        } else {
            (
                format!(
                    "{} · {} {}",
                    tr("Completed"),
                    genres::cache_count(),
                    tr("albums cached")
                ),
                None,
            )
        };
        self.content.append(&row(
            "tag-symbolic",
            &tr("Genre enrichment"),
            &status,
            s.genres_running,
            frac,
        ));
        let dr_status = if s.dr_pending > 0 {
            format!(
                "{} {} · {} {}",
                s.dr_pending,
                tr("pending"),
                dr14::cache_count(),
                tr("tracks cached")
            )
        } else {
            format!(
                "{} · {} {}",
                tr("On demand"),
                dr14::cache_count(),
                tr("tracks cached")
            )
        };
        self.content.append(&row(
            "utilities-system-monitor-symbolic",
            &tr("DR14 analysis"),
            &dr_status,
            s.dr_pending > 0,
            None,
        ));
        self.content.append(&row(
            "media-playlist-shuffle-symbolic",
            &tr("Waveforms"),
            &format!(
                "{} · {} {}",
                tr("On demand"),
                waveform::cache_count(),
                tr("tracks cached")
            ),
            false,
            None,
        ));
        self.content.append(&row(
            "audio-speakers-symbolic",
            &tr("Audio outputs"),
            &format!(
                "{} {} · {}",
                s.outputs,
                tr("outputs"),
                tr("live via MPD idle")
            ),
            false,
            None,
        ));
        self.content.append(&ui::label(
            &obiplayer_core::paths::cache_dir().display().to_string(),
            &["dim-label", "caption", "monospace"],
        ));
    }
}
