//! Smart playlists: rule editor and live results.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use obiplayer_core::catalog::TrackRef;
use obiplayer_core::smart::{self, Facts, Field, MatchMode, Op, Rule, SmartPlaylist};
use relm4::gtk::{self, prelude::*};

use crate::ctx::{Action, Ctx, Route};
use crate::tr;
use crate::ui;

pub struct SmartView {
    pub root: gtk::Paned,
    ctx: Rc<Ctx>,
    playlists: RefCell<Vec<SmartPlaylist>>,
    selected: Cell<Option<usize>>,
    sidebar: gtk::ListBox,
    editor: gtk::Box,
    results: gtk::Box,
    play_counts: RefCell<HashMap<String, u32>>,
}

impl SmartView {
    pub fn new(ctx: &Rc<Ctx>) -> Rc<SmartView> {
        let sidebar = gtk::ListBox::builder()
            .css_classes(["navigation-sidebar"])
            .build();
        let add = gtk::Button::builder()
            .label(tr("New playlist"))
            .css_classes(["flat"])
            .margin_bottom(6)
            .build();
        let side = ui::vbox(0);
        side.set_width_request(200);
        side.append(
            &gtk::ScrolledWindow::builder()
                .child(&sidebar)
                .vexpand(true)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .build(),
        );
        side.append(&add);

        let editor = ui::vbox(8);
        editor.set_margin_top(12);
        editor.set_margin_start(12);
        editor.set_margin_end(12);
        let results = ui::vbox(4);
        results.set_margin_start(12);
        results.set_margin_end(12);
        results.set_margin_bottom(12);
        let right = ui::vbox(8);
        right.append(&editor);
        right.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        right.append(&results);
        let root = gtk::Paned::builder()
            .start_child(&side)
            .end_child(
                &gtk::ScrolledWindow::builder()
                    .child(&right)
                    .hscrollbar_policy(gtk::PolicyType::Never)
                    .build(),
            )
            .shrink_start_child(false)
            .resize_start_child(false)
            .position(220)
            .build();

        let v = Rc::new(SmartView {
            root,
            ctx: ctx.clone(),
            playlists: RefCell::new(smart::load_all(&smart::default_path())),
            selected: Cell::new(None),
            sidebar,
            editor,
            results,
            play_counts: RefCell::default(),
        });
        {
            let me = v.clone();
            add.connect_clicked(move |_| {
                me.playlists.borrow_mut().push(SmartPlaylist::new_default());
                let n = me.playlists.borrow().len();
                me.save();
                me.select(Some(n - 1));
            });
        }
        {
            let me = v.clone();
            v.sidebar.connect_row_activated(move |_, row| {
                me.select(Some(row.index() as usize));
            });
        }
        v.render_sidebar();
        v.render_editor();
        v
    }

    /// Refresh play counts (for the "Play Count" field) and results.
    pub fn reload(self: &Rc<Self>) {
        let (me, client) = (self.clone(), self.ctx.client.clone());
        relm4::spawn_local(async move {
            let counts = relm4::spawn(async move {
                client
                    .sticker_find("", "play_count")
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|(f, v)| v.parse().ok().map(|n| (f, n)))
                    .collect::<HashMap<String, u32>>()
            })
            .await
            .unwrap_or_default();
            *me.play_counts.borrow_mut() = counts;
            me.render_results();
        });
    }

    fn save(&self) {
        if let Err(e) = smart::save_all(&smart::default_path(), &self.playlists.borrow()) {
            tracing::warn!(error = %e, "could not save smart playlists");
        }
    }

    fn select(self: &Rc<Self>, i: Option<usize>) {
        self.selected.set(i);
        self.render_sidebar();
        self.render_editor();
    }

    fn render_sidebar(self: &Rc<Self>) {
        self.sidebar.remove_all();
        for (i, p) in self.playlists.borrow().iter().enumerate() {
            let row = ui::hbox(8);
            row.set_margin_top(4);
            row.set_margin_bottom(4);
            row.append(&gtk::Image::from_icon_name("starred-symbolic"));
            let l = ui::ellipsized(&p.name, &[]);
            l.set_hexpand(true);
            row.append(&l);
            let me = self.clone();
            row.append(&ui::icon_button(
                "user-trash-symbolic",
                "Delete",
                move || {
                    me.playlists.borrow_mut().remove(i);
                    me.save();
                    me.select(None);
                },
            ));
            self.sidebar.append(&row);
        }
        if let Some(i) = self.selected.get()
            && let Some(r) = self.sidebar.row_at_index(i as i32)
        {
            self.sidebar.select_row(Some(&r));
        }
    }

    /// Apply a change to the selected playlist, save, re-evaluate.
    fn modify(self: &Rc<Self>, f: impl FnOnce(&mut SmartPlaylist), rebuild_editor: bool) {
        let Some(i) = self.selected.get() else { return };
        if let Some(p) = self.playlists.borrow_mut().get_mut(i) {
            f(p);
        }
        self.save();
        if rebuild_editor {
            self.render_editor();
        } else {
            self.render_results();
        }
    }

    fn render_editor(self: &Rc<Self>) {
        ui::clear(&self.editor);
        let Some(i) = self.selected.get() else {
            ui::clear(&self.results);
            self.editor.append(
                &relm4::adw::StatusPage::builder()
                    .icon_name("starred-symbolic")
                    .title(tr("Select or create a smart playlist"))
                    .build(),
            );
            return;
        };
        let Some(p) = self.playlists.borrow().get(i).cloned() else {
            return;
        };

        // Name
        let name = gtk::Entry::builder()
            .text(&p.name)
            .css_classes(["title-2"])
            .build();
        {
            let me = self.clone();
            name.connect_changed(move |e| {
                let t = e.text().to_string();
                me.modify(|p| p.name = t, false);
            });
            let me = self.clone();
            name.connect_activate(move |_| me.render_sidebar());
        }
        self.editor.append(&name);

        // Match all/any
        let m = ui::hbox(8);
        m.append(&ui::label(&tr("Match"), &[]));
        let all = gtk::ToggleButton::builder()
            .label(tr("all"))
            .active(p.match_mode == MatchMode::All)
            .build();
        let any = gtk::ToggleButton::builder()
            .label(tr("any"))
            .group(&all)
            .active(p.match_mode == MatchMode::Any)
            .build();
        let linked = gtk::Box::builder().css_classes(["linked"]).build();
        linked.append(&all);
        linked.append(&any);
        m.append(&linked);
        m.append(&ui::label(&tr("of the following rules:"), &[]));
        {
            let me = self.clone();
            all.connect_toggled(move |b| {
                let mode = if b.is_active() {
                    MatchMode::All
                } else {
                    MatchMode::Any
                };
                me.modify(|p| p.match_mode = mode, false);
            });
        }
        self.editor.append(&m);

        // Rules
        for (ri, rule) in p.rules.iter().enumerate() {
            self.editor
                .append(&self.rule_row(ri, rule, p.rules.len() > 1));
        }
        let add_rule = gtk::Button::builder()
            .label(tr("Add rule"))
            .halign(gtk::Align::Start)
            .css_classes(["flat"])
            .build();
        {
            let me = self.clone();
            add_rule.connect_clicked(move |_| {
                me.modify(
                    |p| p.rules.push(SmartPlaylist::new_default().rules.remove(0)),
                    true,
                )
            });
        }
        self.editor.append(&add_rule);

        // Limit
        let lim = ui::hbox(8);
        let check = gtk::CheckButton::builder()
            .label(tr("Limit to"))
            .active(p.limit.is_some())
            .build();
        let spin = gtk::SpinButton::with_range(1.0, 10_000.0, 1.0);
        spin.set_value(p.limit.unwrap_or(50) as f64);
        spin.set_sensitive(p.limit.is_some());
        lim.append(&check);
        lim.append(&spin);
        lim.append(&ui::label(&tr("tracks"), &[]));
        {
            let (me, sp) = (self.clone(), spin.clone());
            check.connect_toggled(move |c| {
                let on = c.is_active();
                sp.set_sensitive(on);
                let v = sp.value() as usize;
                me.modify(|p| p.limit = on.then_some(v), false);
            });
            let me = self.clone();
            spin.connect_value_changed(move |s| {
                let v = s.value() as usize;
                me.modify(
                    |p| {
                        if p.limit.is_some() {
                            p.limit = Some(v)
                        }
                    },
                    false,
                );
            });
        }
        self.editor.append(&lim);
        self.render_results();
    }

    fn rule_row(self: &Rc<Self>, ri: usize, rule: &Rule, removable: bool) -> gtk::Box {
        let row = ui::hbox(6);
        let fields: Vec<Field> = Field::all().to_vec();
        let field_dd =
            gtk::DropDown::from_strings(&fields.iter().map(|f| f.label()).collect::<Vec<_>>());
        field_dd.set_selected(fields.iter().position(|f| *f == rule.field).unwrap_or(0) as u32);
        let ops: Vec<Op> = Op::available(rule.field).to_vec();
        let op_dd = gtk::DropDown::from_strings(&ops.iter().map(|o| o.label()).collect::<Vec<_>>());
        op_dd.set_selected(ops.iter().position(|o| *o == rule.op).unwrap_or(0) as u32);
        row.append(&field_dd);
        row.append(&op_dd);
        {
            let me = self.clone();
            field_dd.connect_selected_notify(move |d| {
                let f = Field::all()[d.selected() as usize];
                me.modify(
                    |p| {
                        if let Some(r) = p.rules.get_mut(ri) {
                            r.field = f;
                            r.op = Op::available(f)[0];
                        }
                    },
                    true,
                );
            });
            let me = self.clone();
            op_dd.connect_selected_notify(move |d| {
                let op = ops[d.selected() as usize];
                me.modify(
                    |p| {
                        if let Some(r) = p.rules.get_mut(ri) {
                            r.op = op;
                        }
                    },
                    true,
                );
            });
        }
        if !rule.field.is_boolean() {
            let v = gtk::Entry::builder()
                .text(&rule.value)
                .hexpand(true)
                .build();
            let me = self.clone();
            v.connect_changed(move |e| {
                let t = e.text().to_string();
                me.modify(
                    |p| {
                        if let Some(r) = p.rules.get_mut(ri) {
                            r.value = t;
                        }
                    },
                    false,
                );
            });
            row.append(&v);
            if rule.op == Op::Between {
                row.append(&ui::label("–", &[]));
                let v2 = gtk::Entry::builder()
                    .text(&rule.value2)
                    .width_chars(8)
                    .build();
                let me = self.clone();
                v2.connect_changed(move |e| {
                    let t = e.text().to_string();
                    me.modify(
                        |p| {
                            if let Some(r) = p.rules.get_mut(ri) {
                                r.value2 = t;
                            }
                        },
                        false,
                    );
                });
                row.append(&v2);
            }
        }
        if removable {
            let me = self.clone();
            row.append(&ui::icon_button(
                "list-remove-symbolic",
                "Remove rule",
                move || {
                    me.modify(
                        |p| {
                            if ri < p.rules.len() {
                                p.rules.remove(ri);
                            }
                        },
                        true,
                    )
                },
            ));
        }
        row
    }

    fn render_results(self: &Rc<Self>) {
        ui::clear(&self.results);
        let Some(p) = self
            .selected
            .get()
            .and_then(|i| self.playlists.borrow().get(i).cloned())
        else {
            return;
        };
        let lib = self.ctx.lib();
        let ratings = self.ctx.ratings.borrow().clone();
        let dr = self.ctx.dr.borrow().clone();
        let extra = self.ctx.extra_genres.borrow().clone();
        let counts = self.play_counts.borrow();
        let found: Vec<TrackRef> = p.evaluate(
            &lib,
            &Facts {
                play_counts: &counts,
                ratings: &ratings,
                dr: &dr,
                extra_genres: &extra,
            },
        );
        let head = ui::hbox(8);
        let n = ui::label(
            &format!("{} {}", found.len(), tr("tracks matched")),
            &["heading"],
        );
        n.set_hexpand(true);
        head.append(&n);
        if !found.is_empty() {
            let files: Vec<String> = found.iter().map(|r| lib.song(*r).file.clone()).collect();
            let play = gtk::Button::builder()
                .label(format!("{} ({})", tr("Play all"), files.len()))
                .css_classes(["pill", "suggested-action"])
                .build();
            let c = self.ctx.clone();
            play.connect_clicked(move |_| c.act(Action::PlayFiles(files.clone())));
            head.append(&play);
        }
        self.results.append(&head);
        if found.is_empty() {
            self.results.append(&ui::label(
                &tr("No tracks match these rules"),
                &["dim-label"],
            ));
            return;
        }
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        let mut files = Vec::new();
        for r in found.iter().take(500) {
            let a = lib.album(r.album);
            let s = lib.song(*r);
            let row = ui::hbox(10);
            row.set_margin_top(3);
            row.set_margin_bottom(3);
            row.set_margin_start(6);
            row.set_margin_end(8);
            let (c, f) = (self.ctx.clone(), s.file.clone());
            row.append(&ui::icon_button(
                "media-playback-start-symbolic",
                "Play",
                move || c.act(Action::PlayFile(f.clone())),
            ));
            let t = ui::vbox(0);
            t.set_hexpand(true);
            t.append(&ui::ellipsized(s.display_title(), &[]));
            t.append(&ui::ellipsized(
                &format!("{} · {}", s.artist().unwrap_or(&a.artist), a.title),
                &["dim-label", "caption"],
            ));
            row.append(&t);
            if let Some(d) = self.ctx.dr(&s.file) {
                row.append(&ui::dr_badge(d));
            }
            row.append(&ui::label(
                &s.duration.map(ui::mmss).unwrap_or_default(),
                &["dim-label", "numeric"],
            ));
            list.append(&row);
            files.push(s.file.clone());
        }
        let c = self.ctx.clone();
        list.connect_row_activated(move |_, row| {
            if let Some(f) = files.get(row.index() as usize) {
                c.act(Action::Open(Route::Track(f.clone())));
            }
        });
        self.results.append(&list);
        if found.len() > 500 {
            self.results.append(&ui::label(
                &format!("… {} {}", found.len() - 500, tr("more")),
                &["dim-label", "caption"],
            ));
        }
    }
}
