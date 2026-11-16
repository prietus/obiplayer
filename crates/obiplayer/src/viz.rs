//! Visualizer sample source and drawing widgets (waveform seek bar,
//! oscilloscope, spectrum, circular).
//!
//! Samples come from decoding the current track locally (ffmpeg, mono
//! 44.1 kHz) and are read at the interpolated playback position — works with
//! remote servers as long as the library is mounted. DrPlayer's FIFO mode
//! (local MPD only) can be added on top of the same `Samples` interface.

use std::cell::RefCell;
use std::f64::consts::PI;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use obiplayer_core::analysis::spectrum::{FFT_SIZE, Smoother, Spectrum};
use relm4::gtk::{self, cairo, prelude::*};

use crate::ctx::Ctx;

pub const VIZ_RATE: u32 = 44_100;

#[derive(Default, Debug)]
pub struct Samples {
    pub file: Option<String>,
    pub data: Option<Arc<Vec<f32>>>,
}

impl Samples {
    /// The `FFT_SIZE` samples ending at `t` seconds.
    pub fn window(&self, t: f64) -> Option<&[f32]> {
        let d = self.data.as_ref()?;
        let end = ((t * VIZ_RATE as f64) as usize).min(d.len());
        let start = end.checked_sub(FFT_SIZE)?;
        Some(&d[start..end])
    }
}

/// Smooth path through `pts` (Catmull-Rom converted to cubic Béziers).
fn catmull_rom(cr: &cairo::Context, pts: &[(f64, f64)]) {
    if pts.len() < 2 {
        return;
    }
    cr.move_to(pts[0].0, pts[0].1);
    curve_through(cr, pts);
}

/// Continue the current path smoothly through `pts` (first point assumed reached).
fn curve_through(cr: &cairo::Context, pts: &[(f64, f64)]) {
    for i in 0..pts.len() - 1 {
        let p0 = pts[i.saturating_sub(1)];
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = pts[(i + 2).min(pts.len() - 1)];
        let c1 = (p1.0 + (p2.0 - p0.0) / 6.0, p1.1 + (p2.1 - p0.1) / 6.0);
        let c2 = (p2.0 - (p3.0 - p1.0) / 6.0, p2.1 - (p3.1 - p1.1) / 6.0);
        cr.curve_to(c1.0, c1.1, c2.0, c2.1, p2.0, p2.1);
    }
}

/// Accent colour (white in the full-window view, where it sits on dark art).
fn accent(w: &impl IsA<gtk::Widget>) -> gtk::gdk::RGBA {
    if w.as_ref()
        .ancestor(gtk::Overlay::static_type())
        .is_some_and(|o| o.has_css_class("now-playing"))
    {
        return gtk::gdk::RGBA::WHITE;
    }
    crate::ui::accent_rgba(w)
}

/// Regular foreground (text colour of the surroundings) for neutral parts.
fn foreground(w: &impl IsA<gtk::Widget>) -> gtk::gdk::RGBA {
    let w = w.as_ref();
    w.parent().map(|p| p.color()).unwrap_or_else(|| w.color())
}

fn set(cr: &cairo::Context, c: &gtk::gdk::RGBA, alpha: f64) {
    cr.set_source_rgba(
        c.red() as f64,
        c.green() as f64,
        c.blue() as f64,
        c.alpha() as f64 * alpha,
    );
}

/// Redraw on every frame, capped at ~30 fps.
fn animate(area: &gtk::DrawingArea) {
    let last = Rc::new(RefCell::new(Instant::now()));
    area.add_tick_callback(move |w, _| {
        let mut l = last.borrow_mut();
        if l.elapsed() >= Duration::from_millis(33) {
            *l = Instant::now();
            w.queue_draw();
        }
        gtk::glib::ControlFlow::Continue
    });
}

// -----------------------------------------------------------------------------
// Waveform seek bar
// -----------------------------------------------------------------------------

#[derive(Clone)]
pub struct Waveform {
    pub area: gtk::DrawingArea,
    peaks: Rc<RefCell<Option<Arc<Vec<f32>>>>>,
}

impl Waveform {
    /// `on_seek` gets a fraction 0..=1 of the track.
    pub fn new(ctx: &Rc<Ctx>, height: i32, on_seek: impl Fn(f64) + 'static) -> Waveform {
        let area = gtk::DrawingArea::builder()
            .content_height(height)
            .hexpand(true)
            .css_classes(["waveform"])
            .build();
        area.set_cursor_from_name(Some("pointer"));
        area.set_tooltip_text(Some(&crate::tr("Click to seek")));
        let peaks: Rc<RefCell<Option<Arc<Vec<f32>>>>> = Rc::default();
        {
            let (p, c) = (peaks.clone(), ctx.clone());
            area.set_draw_func(move |a, cr, w, h| {
                let frac = c.clock.get().fraction();
                draw_waveform(a, cr, w as f64, h as f64, p.borrow().as_deref(), frac);
            });
        }
        let click = gtk::GestureClick::new();
        {
            let a = area.clone();
            click.connect_pressed(move |_, _, x, _| {
                let w = a.width() as f64;
                if w > 0.0 {
                    on_seek((x / w).clamp(0.0, 1.0));
                }
            });
        }
        area.add_controller(click);
        animate(&area);
        Waveform { area, peaks }
    }

    pub fn set_peaks(&self, peaks: Option<Arc<Vec<f32>>>) {
        *self.peaks.borrow_mut() = peaks;
        self.area.queue_draw();
    }
}

fn draw_waveform(
    a: &gtk::DrawingArea,
    cr: &cairo::Context,
    w: f64,
    h: f64,
    peaks: Option<&Vec<f32>>,
    frac: f64,
) {
    let fg = foreground(a);
    let accent = accent(a);
    let x_played = w * frac;
    let mid = h / 2.0;

    let shape = |cr: &cairo::Context| match peaks.filter(|p| p.len() >= 2) {
        Some(p) => {
            let step = w / (p.len() - 1) as f64;
            let top: Vec<(f64, f64)> = p
                .iter()
                .enumerate()
                .map(|(i, &v)| (i as f64 * step, mid - (v as f64 * mid * 0.95).max(1.0)))
                .collect();
            let bottom: Vec<(f64, f64)> = top.iter().rev().map(|&(x, y)| (x, h - y)).collect();
            catmull_rom(cr, &top);
            cr.line_to(bottom[0].0, bottom[0].1);
            curve_through(cr, &bottom);
            cr.close_path();
        }
        None => {
            // Flat bar when there is no waveform (no local file yet).
            cr.rectangle(0.0, mid - 2.0, w, 4.0);
        }
    };

    // Unplayed part
    cr.save().ok();
    cr.rectangle(x_played, 0.0, w - x_played, h);
    cr.clip();
    shape(cr);
    set(cr, &fg, 0.22);
    cr.fill().ok();
    cr.restore().ok();
    // Played part
    cr.save().ok();
    cr.rectangle(0.0, 0.0, x_played, h);
    cr.clip();
    shape(cr);
    set(cr, &accent, 0.9);
    cr.fill().ok();
    cr.restore().ok();
}

// -----------------------------------------------------------------------------
// Visualizers
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VizMode {
    Oscilloscope,
    Spectrum,
    Circular,
}

impl VizMode {
    pub const ALL: [VizMode; 3] = [VizMode::Oscilloscope, VizMode::Spectrum, VizMode::Circular];

    pub fn label(self) -> &'static str {
        match self {
            VizMode::Oscilloscope => "Oscilloscope",
            VizMode::Spectrum => "Spectrum",
            VizMode::Circular => "Circular",
        }
    }
}

struct VizState {
    mode: VizMode,
    spectrum: Spectrum,
    smooth: Smoother,
}

#[derive(Clone)]
pub struct Visualizer {
    pub area: gtk::DrawingArea,
    state: Rc<RefCell<VizState>>,
}

impl Visualizer {
    pub fn new(ctx: &Rc<Ctx>, mode: VizMode) -> Visualizer {
        let area = gtk::DrawingArea::builder()
            .hexpand(true)
            .vexpand(true)
            .css_classes(["visualizer"])
            .build();
        let state = Rc::new(RefCell::new(VizState {
            mode,
            spectrum: Spectrum::new(),
            smooth: Smoother::default(),
        }));
        {
            let (st, c) = (state.clone(), ctx.clone());
            area.set_draw_func(move |a, cr, w, h| {
                let clock = c.clock.get();
                let samples = c.samples.borrow();
                let window = samples.window(clock.now()).filter(|_| clock.playing);
                let mut st = st.borrow_mut();
                draw_viz(a, cr, w as f64, h as f64, &mut st, window);
            });
        }
        animate(&area);
        Visualizer { area, state }
    }

    pub fn set_mode(&self, mode: VizMode) {
        let mut s = self.state.borrow_mut();
        s.mode = mode;
        s.smooth = Smoother::default();
    }
}

fn draw_viz(
    a: &gtk::DrawingArea,
    cr: &cairo::Context,
    w: f64,
    h: f64,
    st: &mut VizState,
    window: Option<&[f32]>,
) {
    let fg = foreground(a);
    let accent = accent(a);
    match st.mode {
        VizMode::Oscilloscope => {
            let mid = h / 2.0;
            set(cr, &fg, 0.12);
            cr.set_line_width(1.0);
            cr.move_to(0.0, mid);
            cr.line_to(w, mid);
            cr.stroke().ok();
            let Some(s) = window else { return };
            let n = 256usize;
            let step = s.len() / n;
            let pts: Vec<(f64, f64)> = (0..n)
                .map(|i| {
                    let v = s[i * step] as f64;
                    (i as f64 * w / (n - 1) as f64, mid - v * mid * 0.9)
                })
                .collect();
            // Glow: wide faint strokes under a sharp one.
            for (width, alpha) in [(4.0, 0.3), (2.0, 0.6), (1.0, 1.0)] {
                catmull_rom(cr, &pts);
                cr.set_line_width(width);
                set(cr, &accent, alpha);
                cr.stroke().ok();
            }
        }
        VizMode::Spectrum => {
            let bands = 64;
            let target = window
                .map(|s| st.spectrum.bands(s, bands))
                .unwrap_or_else(|| vec![0.0; bands]);
            st.smooth.update(&target);
            let bw = w / bands as f64;
            let gap = bw * 0.12;
            for (i, (&v, &p)) in st.smooth.values.iter().zip(&st.smooth.peaks).enumerate() {
                let x = i as f64 * bw + gap / 2.0;
                let bh = (v as f64 * h).max(1.0);
                set(cr, &accent, 0.6 + 0.4 * v as f64);
                cr.rectangle(x, h - bh, bw - gap, bh);
                cr.fill().ok();
                let ph = p as f64 * h;
                if ph - bh > 3.0 {
                    set(cr, &fg, 0.5);
                    cr.rectangle(x, h - ph - 2.0, bw - gap, 2.0);
                    cr.fill().ok();
                }
            }
        }
        VizMode::Circular => {
            let bands = 80;
            let target = window
                .map(|s| st.spectrum.bands(s, bands))
                .unwrap_or_else(|| vec![0.0; bands]);
            st.smooth.update(&target);
            let (cx, cy) = (w / 2.0, h / 2.0);
            let m = w.min(h);
            let r0 = m * 0.12;
            let rmax = m * 0.38;
            set(cr, &accent, 0.6);
            cr.set_line_width(2.0);
            cr.arc(cx, cy, r0, 0.0, 2.0 * PI);
            cr.stroke().ok();
            cr.set_line_cap(cairo::LineCap::Round);
            for (i, &v) in st.smooth.values.iter().enumerate() {
                let ang = i as f64 / bands as f64 * 2.0 * PI - PI / 2.0;
                let len = v as f64 * (rmax - r0);
                let (c, s) = (ang.cos(), ang.sin());
                let hue = (0.75 + 0.3 * i as f64 / bands as f64).fract();
                let (r, g, b) = hsv(hue, 0.7, 0.95);
                cr.set_source_rgba(r, g, b, 0.5 + 0.5 * v as f64);
                cr.set_line_width(if v > 0.4 { 4.0 } else { 3.0 });
                cr.move_to(cx + c * r0, cy + s * r0);
                cr.line_to(cx + c * (r0 + len), cy + s * (r0 + len));
                cr.stroke().ok();
                // Mirrored inward bar.
                cr.set_line_width(2.0);
                cr.move_to(cx + c * r0, cy + s * r0);
                cr.line_to(cx + c * (r0 - len * 0.3), cy + s * (r0 - len * 0.3));
                cr.stroke().ok();
            }
        }
    }
}

fn hsv(h: f64, s: f64, v: f64) -> (f64, f64, f64) {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}
