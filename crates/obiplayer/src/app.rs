//! Main window: library browser, navigation stack, queue and player bar.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use obiplayer_core::catalog::Library;
use obiplayer_core::config::Config;
use obiplayer_core::library::{self, Album};
use obiplayer_core::mpd::{self, Client, Event, PlayState, Single, Song, Status, Subsystem};
use relm4::adw::{self, prelude::*};
use relm4::gtk::{self, gio, glib};
use relm4::typed_view::grid::TypedGridView;
use relm4::typed_view::list::TypedListView;
use relm4::{Component, ComponentController, ComponentParts, ComponentSender, Controller};

use crate::albums::AlbumItem;
use crate::covers::CoverLoader;
use crate::ctx::{Action, AlbumFilter, Ctx, Route};
use crate::pages::album::AlbumPage;
use crate::pages::browse::Browse;
use crate::pages::group::{GroupPage, Kind};
use crate::pages::search::SearchPage;
use crate::pages::track::TrackPage;
use crate::pages::tracks::TracksView;
use crate::pages::{self, Page};
use crate::player_bar::{BarInput, BarOutput, PlayerBar, Transport};
use crate::queue::QueueItem;
use crate::tr;

const ROOT_TAG: &str = "library";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LibraryState {
    NotLoaded,
    Loading,
    Loaded,
}

pub struct App {
    client: Client,
    ctx: Rc<Ctx>,
    connected: bool,
    disconnect_reason: Option<String>,
    status: Status,
    song: Option<Song>,
    library: LibraryState,
    current_folder: Option<String>,
    grid: TypedGridView<AlbumItem, gtk::NoSelection>,
    filter: Rc<RefCell<Option<AlbumFilter>>>,
    queue: TypedListView<QueueItem, gtk::NoSelection>,
    queue_version: Option<u32>,
    queue_shown: bool,
    bar: Controller<PlayerBar>,
    toasts: adw::ToastOverlay,
    nav: adw::NavigationView,
    modes: adw::ViewStack,
    pages: Vec<Box<dyn Page>>,
    browse: Vec<Browse>,
    tracks: TracksView,
}

#[derive(Debug)]
pub enum AppMsg {
    Mpd(Event),
    Transport(Transport),
    AlbumActivated(u32),
    PlayQueuePos(u32),
    RemoveQueuePos(u32),
    ClearQueue,
    ShowQueue(bool),
    ReloadLibrary,
    ShowCurrentAlbum,
    ClearFilter,
    Act(Action),
    Popped(String),
}

#[derive(Debug)]
pub enum Cmd {
    Status(Result<(Box<Status>, Option<Box<Song>>), String>),
    Queue(u32, Result<Vec<Song>, String>),
    Library(Result<Vec<Arc<Album>>, String>),
    Ratings(HashMap<String, u8>),
    Preferred(HashSet<String>),
    PreferredSet(String, Vec<String>, Result<(), String>),
    Done(Result<(), String>),
}

pub struct AppWidgets {
    count: gtk::Label,
    banner: adw::Banner,
    albums_stack: gtk::Stack,
    empty: adw::StatusPage,
    filter_bar: gtk::Revealer,
    filter_icon: gtk::Image,
    filter_label: gtk::Label,
    queue_revealer: gtk::Revealer,
    queue_count: gtk::Label,
}

fn err(e: mpd::Error) -> String {
    e.to_string()
}

impl App {
    fn refresh_status(&self, sender: &ComponentSender<Self>) {
        let c = self.client.clone();
        sender.oneshot_command(async move {
            let r = async {
                let st = c.status().await?;
                let song = c.current_song().await?;
                mpd::Result::Ok((Box::new(st), song.map(Box::new)))
            }
            .await;
            Cmd::Status(r.map_err(err))
        });
    }

    fn refresh_queue(&self, sender: &ComponentSender<Self>, version: u32) {
        let c = self.client.clone();
        sender.oneshot_command(async move { Cmd::Queue(version, c.queue().await.map_err(err)) });
    }

    fn load_library(&mut self, sender: &ComponentSender<Self>) {
        if self.library == LibraryState::Loading {
            return;
        }
        self.library = LibraryState::Loading;
        let c = self.client.clone();
        sender.oneshot_command(async move {
            let t0 = std::time::Instant::now();
            let r = c.list_all_info().await.map(library::group_albums);
            tracing::info!(ms = t0.elapsed().as_millis() as u64, "library fetched");
            Cmd::Library(r.map_err(err))
        });
    }

    /// Run an MPD command in the background; failures become a toast.
    fn run<F, Fut>(&self, sender: &ComponentSender<Self>, f: F)
    where
        F: FnOnce(Client) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = mpd::Result<()>> + Send + 'static,
    {
        let c = self.client.clone();
        sender.oneshot_command(async move { Cmd::Done(f(c).await.map_err(err)) });
    }

    fn toast(&self, text: &str) {
        let t = adw::Toast::new(text);
        t.set_timeout(3);
        self.toasts.add_toast(t);
    }

    /// Re-insert a grid item so its cell is re-bound with new state.
    fn rebind_album(&mut self, folder: &str, playing: Option<bool>) {
        let Some(pos) = self.ctx.lib().album_index(folder) else {
            return;
        };
        let Some(item) = self.grid.get(pos) else {
            return;
        };
        let mut v = item.borrow().clone();
        if let Some(p) = playing {
            if v.playing == p {
                return;
            }
            v.playing = p;
        }
        self.grid.remove(pos);
        self.grid.insert(pos, v);
    }

    fn set_queue_current(&mut self, pos: Option<u32>, current: bool) {
        let Some(pos) = pos else { return };
        let Some(item) = self.queue.get(pos) else {
            return;
        };
        let mut v = item.borrow().clone();
        if v.current == current {
            return;
        }
        v.current = current;
        self.queue.remove(pos);
        self.queue.insert(pos, v);
    }

    fn push_bar(&self) {
        let album = self
            .current_folder
            .as_ref()
            .and_then(|f| self.ctx.lib().album_by_folder(f).cloned());
        self.bar.emit(BarInput::Update {
            status: Box::new(self.status.clone()),
            song: self.song.clone().map(Box::new),
            album,
        });
    }

    fn update_current(&mut self) {
        let file = self.song.as_ref().map(|s| s.file.clone());
        if *self.ctx.current_file.borrow() != file {
            *self.ctx.current_file.borrow_mut() = file;
            self.refresh_pages();
        }
        let lib = self.ctx.lib();
        let folder = self
            .song
            .as_ref()
            .map(|s| library::album_key(&s.file).to_owned())
            .filter(|f| lib.album_index(f).is_some());
        if folder != self.current_folder {
            if let Some(old) = self.current_folder.take() {
                self.rebind_album(&old, Some(false));
            }
            if let Some(new) = &folder {
                self.rebind_album(new, Some(true));
            }
            self.current_folder = folder;
        }
    }

    fn refresh_pages(&self) {
        for p in &self.pages {
            p.refresh();
        }
    }

    fn open(&mut self, route: Route, sender: &ComponentSender<Self>) {
        let tag = pages::tag(&route);
        if let Route::Search(q) = &route
            && let Some(p) = self
                .pages
                .iter()
                .find(|p| matches!(p.route(), Route::Search(_)))
        {
            // Reuse the open search page.
            self.nav.pop_to_tag(&tag);
            if !q.is_empty() {
                p.set_query(q);
            }
            return;
        }
        if self.nav.visible_page().and_then(|p| p.tag()).as_deref() == Some(tag.as_str()) {
            return;
        }
        let page: Option<Box<dyn Page>> = match &route {
            Route::Album(f) => {
                self.load_ratings(sender, f.clone());
                AlbumPage::new(&self.ctx, f).map(|p| Box::new(p) as _)
            }
            Route::Track(f) => {
                self.load_track_rating(sender, f.clone());
                TrackPage::new(&self.ctx, f).map(|p| Box::new(p) as _)
            }
            Route::Artist(n) => {
                GroupPage::new(&self.ctx, Kind::Artist, n).map(|p| Box::new(p) as _)
            }
            Route::Label(n) => GroupPage::new(&self.ctx, Kind::Label, n).map(|p| Box::new(p) as _),
            Route::Composer(n) => {
                GroupPage::new(&self.ctx, Kind::Composer, n).map(|p| Box::new(p) as _)
            }
            Route::Search(q) => Some(Box::new(SearchPage::new(&self.ctx, q))),
        };
        match page {
            Some(p) => {
                self.nav.push(p.nav());
                self.pages.push(p);
            }
            None => self.toast(&tr("Not found in the library")),
        }
    }

    /// Development aid: `OBIPLAYER_ROUTE=album:<folder>|track:<file>|artist:<name>|
    /// label:<name>|composer:<name>|search:<query>|mode:<albums|artists|…>`
    /// opens that view once the library has loaded (used for screenshots).
    fn open_debug_route(&mut self, sender: &ComponentSender<Self>) {
        let Ok(spec) = std::env::var("OBIPLAYER_ROUTE") else {
            return;
        };
        let Some((kind, arg)) = spec.split_once(':') else {
            return;
        };
        let arg = arg.to_owned();
        let route = match kind {
            "album" => Route::Album(arg),
            "track" => Route::Track(arg),
            "artist" => Route::Artist(arg),
            "label" => Route::Label(arg),
            "composer" => Route::Composer(arg),
            "search" => Route::Search(arg),
            "mode" => {
                self.modes.set_visible_child_name(&arg);
                return;
            }
            _ => return,
        };
        self.open(route, sender);
    }

    fn load_ratings(&self, sender: &ComponentSender<Self>, folder: String) {
        let c = self.client.clone();
        sender.oneshot_command(async move {
            match c.sticker_find(&folder, "rating").await {
                Ok(v) => Cmd::Ratings(
                    v.into_iter()
                        .filter_map(|(f, r)| r.parse::<u8>().ok().map(|r| (f, r.min(5))))
                        .collect(),
                ),
                Err(e) => {
                    tracing::debug!(error = %e, "ratings unavailable");
                    Cmd::Ratings(HashMap::new())
                }
            }
        });
    }

    fn load_track_rating(&self, sender: &ComponentSender<Self>, file: String) {
        let c = self.client.clone();
        sender.oneshot_command(async move {
            let r = c
                .sticker_get(&file, "rating")
                .await
                .ok()
                .flatten()
                .and_then(|v| v.parse::<u8>().ok());
            Cmd::Ratings(r.map(|r| (file, r.min(5))).into_iter().collect())
        });
    }

    fn load_preferred(&self, sender: &ComponentSender<Self>) {
        let c = self.client.clone();
        sender.oneshot_command(async move {
            let set = c
                .sticker_find("", "preferred")
                .await
                .map(|v| {
                    v.into_iter()
                        .filter(|(_, val)| val == "1")
                        .map(|(f, _)| f)
                        .collect()
                })
                .unwrap_or_default();
            Cmd::Preferred(set)
        });
    }

    fn act(&mut self, a: Action, sender: &ComponentSender<Self>, root: &adw::ApplicationWindow) {
        match a {
            Action::Open(r) => self.open(r, sender),
            Action::PlayAlbum(f) => self.run(sender, move |c| async move {
                c.replace_queue_and_play(&[f]).await
            }),
            Action::EnqueueAlbum(f) => {
                self.toast(&tr("Added to queue"));
                self.run(sender, move |c| async move { c.add(&f).await })
            }
            Action::PlayFile(f) => {
                self.run(sender, move |c| async move { c.add_and_play(&f).await })
            }
            Action::EnqueueFile(f) => {
                self.toast(&tr("Added to queue"));
                self.run(sender, move |c| async move { c.add(&f).await })
            }
            Action::Filter(f) => {
                self.nav.pop_to_tag(ROOT_TAG);
                self.modes.set_visible_child_name("albums");
                *self.filter.borrow_mut() = Some(f);
                self.grid.notify_filter_changed(0);
            }
            Action::SetRating(file, rating) => {
                if rating == 0 {
                    self.ctx.ratings.borrow_mut().remove(&file);
                } else {
                    self.ctx.ratings.borrow_mut().insert(file.clone(), rating);
                }
                self.refresh_pages();
                self.run(sender, move |c| async move {
                    if rating == 0 {
                        c.sticker_delete(&file, "rating").await
                    } else {
                        c.sticker_set(&file, "rating", &rating.to_string()).await
                    }
                });
            }
            Action::SetPreferred(file) => {
                let lib = self.ctx.lib();
                let Some(r) = lib.find_file(&file) else {
                    return;
                };
                let others: Vec<String> = lib
                    .versions(r)
                    .iter()
                    .map(|v| lib.song(*v).file.clone())
                    .filter(|f| *f != file)
                    .collect();
                let c = self.client.clone();
                sender.oneshot_command(async move {
                    let res = async {
                        c.sticker_set(&file, "preferred", "1").await?;
                        for o in &others {
                            c.sticker_delete(o, "preferred").await?;
                        }
                        mpd::Result::Ok(())
                    }
                    .await;
                    Cmd::PreferredSet(file, others, res.map_err(err))
                });
            }
            Action::UpdateDb(folder) => {
                self.toast(&tr("Updating MPD database…"));
                self.run(sender, move |c| async move {
                    c.update(Some(&folder)).await.map(drop)
                });
            }
            Action::RetryCover(folder) => {
                self.ctx.covers.invalidate(&folder);
                self.rebind_album(&folder, None);
                self.push_bar();
            }
            Action::OpenFolder(folder) => {
                if let Some(p) = self.ctx.local_path(&folder) {
                    let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(p)));
                    launcher.launch(Some(root), gio::Cancellable::NONE, |r| {
                        if let Err(e) = r {
                            tracing::warn!(error = %e, "could not open folder");
                        }
                    });
                }
            }
        }
    }
}

fn queue_panel(
    sender: &ComponentSender<App>,
    queue: &TypedListView<QueueItem, gtk::NoSelection>,
) -> (gtk::Revealer, gtk::Label) {
    queue.view.set_single_click_activate(true);
    queue.view.add_css_class("navigation-sidebar");
    {
        let s = sender.clone();
        queue
            .view
            .connect_activate(move |_, pos| s.input(AppMsg::PlayQueuePos(pos)));
    }
    let queue_count = gtk::Label::builder()
        .css_classes(["dim-label", "caption"])
        .hexpand(true)
        .xalign(0.0)
        .build();
    let clear = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text(tr("Clear queue"))
        .css_classes(["flat"])
        .build();
    {
        let s = sender.clone();
        clear.connect_clicked(move |_| s.input(AppMsg::ClearQueue));
    }
    let qheader = gtk::Box::builder()
        .spacing(8)
        .margin_start(12)
        .margin_end(6)
        .margin_top(6)
        .margin_bottom(6)
        .build();
    qheader.append(
        &gtk::Label::builder()
            .label(tr("Queue"))
            .css_classes(["heading"])
            .build(),
    );
    qheader.append(&queue_count);
    qheader.append(&clear);
    let qbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .width_request(300)
        .css_classes(["queue-panel"])
        .build();
    qbox.append(&qheader);
    qbox.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    qbox.append(
        &gtk::ScrolledWindow::builder()
            .child(&queue.view)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build(),
    );
    let revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideLeft)
        // Explicit, so expanding children inside don't make the hidden
        // panel claim half of the spare width.
        .hexpand(false)
        .child(&qbox)
        .build();
    (revealer, queue_count)
}

impl Component for App {
    type Init = Config;
    type Input = AppMsg;
    type Output = ();
    type CommandOutput = Cmd;
    type Root = adw::ApplicationWindow;
    type Widgets = AppWidgets;

    fn init_root() -> adw::ApplicationWindow {
        adw::ApplicationWindow::builder()
            .title("ObiPlayer")
            .default_width(1200)
            .default_height(800)
            .width_request(600)
            .height_request(500)
            .build()
    }

    fn init(
        config: Config,
        root: adw::ApplicationWindow,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let cfg = config.connect_config();
        let client = Client::new(cfg.clone());
        // Separate connection so artwork transfers never delay transport commands.
        let covers = CoverLoader::new(Client::new(cfg.clone()), config.library.music_dir.clone());
        let ctx = Ctx::new(
            covers.clone(),
            config.library.music_dir.clone(),
            sender.input_sender().clone(),
        );

        let bar = PlayerBar::builder()
            .launch(covers.clone())
            .forward(sender.input_sender(), |o| match o {
                BarOutput::Transport(t) => AppMsg::Transport(t),
                BarOutput::ShowQueue(v) => AppMsg::ShowQueue(v),
                BarOutput::OpenCurrentAlbum => AppMsg::ShowCurrentAlbum,
            });

        // -- Albums grid (with optional filter) ------------------------------------------
        let filter: Rc<RefCell<Option<AlbumFilter>>> = Rc::default();
        let mut grid: TypedGridView<AlbumItem, gtk::NoSelection> = TypedGridView::new();
        {
            let f = filter.clone();
            grid.add_filter(move |item| f.borrow().as_ref().is_none_or(|f| f.matches(&item.album)));
        }
        grid.view.set_single_click_activate(true);
        grid.view.set_min_columns(2);
        grid.view.set_max_columns(16);
        {
            let s = sender.clone();
            grid.view
                .connect_activate(move |_, pos| s.input(AppMsg::AlbumActivated(pos)));
        }
        let grid_scroll = gtk::ScrolledWindow::builder()
            .child(&grid.view)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();

        let spinner = gtk::Spinner::builder()
            .spinning(true)
            .width_request(32)
            .height_request(32)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        let empty = adw::StatusPage::builder()
            .icon_name("folder-music-symbolic")
            .build();
        let load_btn = gtk::Button::builder()
            .label(tr("Load library"))
            .halign(gtk::Align::Center)
            .css_classes(["pill", "suggested-action"])
            .build();
        {
            let s = sender.clone();
            load_btn.connect_clicked(move |_| s.input(AppMsg::ReloadLibrary));
        }
        empty.set_child(Some(&load_btn));
        let albums_stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .vexpand(true)
            .build();
        albums_stack.add_named(&spinner, Some("loading"));
        albums_stack.add_named(&empty, Some("empty"));
        albums_stack.add_named(&grid_scroll, Some("grid"));

        // Active filter chip
        let filter_icon = gtk::Image::new();
        let filter_label = gtk::Label::builder().css_classes(["heading"]).build();
        let filter_clear = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text(tr("Clear filter"))
            .css_classes(["flat", "circular"])
            .build();
        {
            let s = sender.clone();
            filter_clear.connect_clicked(move |_| s.input(AppMsg::ClearFilter));
        }
        let chip = gtk::Box::builder()
            .spacing(6)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(12)
            .css_classes(["filter-chip"])
            .halign(gtk::Align::Start)
            .build();
        chip.append(&filter_icon);
        chip.append(&filter_label);
        chip.append(&filter_clear);
        let filter_bar = gtk::Revealer::builder().child(&chip).build();
        let albums_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        albums_page.append(&filter_bar);
        albums_page.append(&albums_stack);

        // -- Browse modes ------------------------------------------------------------------
        let browse = vec![
            Browse::new(&ctx, Kind::Artist),
            Browse::new(&ctx, Kind::Label),
            Browse::new(&ctx, Kind::Composer),
        ];
        let tracks = TracksView::new(&ctx);
        let modes = adw::ViewStack::new();
        modes.add_titled_with_icon(
            &albums_page,
            Some("albums"),
            &tr("Albums"),
            "media-optical-symbolic",
        );
        modes.add_titled_with_icon(
            &browse[0].root,
            Some("artists"),
            &tr("Artists"),
            "avatar-default-symbolic",
        );
        modes.add_titled_with_icon(
            &browse[1].root,
            Some("labels"),
            &tr("Labels"),
            "building-symbolic",
        );
        modes.add_titled_with_icon(
            &tracks.root,
            Some("tracks"),
            &tr("Tracks"),
            "view-list-symbolic",
        );
        modes.add_titled_with_icon(
            &browse[2].root,
            Some("composers"),
            &tr("Composers"),
            "music-note-symbolic",
        );

        // -- Root page header ----------------------------------------------------------------
        let switcher = adw::ViewSwitcher::builder()
            .stack(&modes)
            .policy(adw::ViewSwitcherPolicy::Wide)
            .build();
        let header = adw::HeaderBar::builder().title_widget(&switcher).build();
        let search = gtk::Button::builder()
            .icon_name("system-search-symbolic")
            .tooltip_text(tr("Search (Ctrl+F)"))
            .build();
        {
            let c = ctx.clone();
            search.connect_clicked(move |_| c.act(Action::Open(Route::Search(String::new()))));
        }
        header.pack_start(&search);
        let reload = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text(tr("Reload library"))
            .build();
        {
            let s = sender.clone();
            reload.connect_clicked(move |_| s.input(AppMsg::ReloadLibrary));
        }
        header.pack_end(&reload);
        let count = gtk::Label::builder()
            .css_classes(["dim-label", "caption"])
            .build();
        header.pack_end(&count);

        let banner = adw::Banner::new("");
        let root_content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root_content.append(&banner);
        root_content.append(&modes);
        let root_tv = adw::ToolbarView::new();
        root_tv.add_top_bar(&header);
        root_tv.set_content(Some(&root_content));
        let root_page = adw::NavigationPage::builder()
            .title("ObiPlayer")
            .tag(ROOT_TAG)
            .child(&root_tv)
            .build();

        let nav = adw::NavigationView::builder().hexpand(true).build();
        nav.add(&root_page);
        {
            let s = sender.clone();
            nav.connect_popped(move |_, page| {
                if let Some(t) = page.tag() {
                    s.input(AppMsg::Popped(t.to_string()));
                }
            });
        }

        // -- Queue + window layout -------------------------------------------------------------
        let queue: TypedListView<QueueItem, gtk::NoSelection> = TypedListView::new();
        let (queue_revealer, queue_count) = queue_panel(&sender, &queue);
        let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        body.append(&nav);
        body.append(&queue_revealer);
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&body));
        let window_tv = adw::ToolbarView::new();
        window_tv.set_content(Some(&toasts));
        window_tv.add_bottom_bar(bar.widget());
        window_tv.set_bottom_bar_style(adw::ToolbarStyle::RaisedBorder);
        root.set_content(Some(&window_tv));

        // Ctrl+F: search
        let shortcuts = gtk::ShortcutController::new();
        shortcuts.set_scope(gtk::ShortcutScope::Global);
        {
            let c = ctx.clone();
            shortcuts.add_shortcut(gtk::Shortcut::new(
                gtk::ShortcutTrigger::parse_string("<Control>f"),
                Some(gtk::CallbackAction::new(move |_, _| {
                    c.act(Action::Open(Route::Search(String::new())));
                    glib::Propagation::Stop
                })),
            ));
        }
        root.add_controller(shortcuts);

        // -- Server events -----------------------------------------------------------------
        let (tx, rx) = async_channel::unbounded();
        relm4::spawn(mpd::watch(cfg, tx));
        {
            let s = sender.clone();
            relm4::spawn_local(async move {
                while let Ok(ev) = rx.recv().await {
                    s.input(AppMsg::Mpd(ev));
                }
            });
        }

        let model = App {
            client,
            ctx,
            connected: false,
            disconnect_reason: None,
            status: Status::default(),
            song: None,
            library: LibraryState::NotLoaded,
            current_folder: None,
            grid,
            filter,
            queue,
            queue_version: None,
            queue_shown: false,
            bar,
            toasts,
            nav,
            modes,
            pages: Vec::new(),
            browse,
            tracks,
        };
        let widgets = AppWidgets {
            count,
            banner,
            albums_stack,
            empty,
            filter_bar,
            filter_icon,
            filter_label,
            queue_revealer,
            queue_count,
        };
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: AppMsg, sender: ComponentSender<Self>, root: &Self::Root) {
        match msg {
            AppMsg::Mpd(Event::Connected { version }) => {
                tracing::info!(%version, host = %self.client.config().host, "connected to MPD");
                self.connected = true;
                self.disconnect_reason = None;
                self.queue_version = None;
                self.refresh_status(&sender);
                if self.library != LibraryState::Loaded {
                    self.load_library(&sender);
                }
            }
            AppMsg::Mpd(Event::Disconnected { reason }) => {
                self.connected = false;
                self.disconnect_reason = Some(reason);
            }
            AppMsg::Mpd(Event::Changed(subs)) => {
                if subs.contains(&Subsystem::Database) {
                    self.load_library(&sender);
                }
                if subs.contains(&Subsystem::Sticker) {
                    self.load_preferred(&sender);
                }
                if subs.iter().any(|s| {
                    matches!(
                        s,
                        Subsystem::Player | Subsystem::Playlist | Subsystem::Options
                    )
                }) {
                    self.refresh_status(&sender);
                }
            }
            AppMsg::Transport(t) => {
                let st = self.status.clone();
                match t {
                    Transport::TogglePause => self.run(&sender, move |c| async move {
                        match st.state {
                            PlayState::Play => c.set_pause(true).await,
                            PlayState::Pause => c.set_pause(false).await,
                            PlayState::Stop if st.playlist_length > 0 => c.play().await,
                            PlayState::Stop => Ok(()),
                        }
                    }),
                    Transport::Stop => self.run(&sender, |c| async move { c.stop().await }),
                    Transport::Next => self.run(&sender, |c| async move { c.next().await }),
                    Transport::Previous => self.run(&sender, |c| async move { c.previous().await }),
                    Transport::ToggleShuffle => {
                        self.run(
                            &sender,
                            move |c| async move { c.set_random(!st.random).await },
                        )
                    }
                    Transport::CycleRepeat => {
                        let (r, s) = match (st.repeat, st.single) {
                            (false, _) => (true, Single::Off),
                            (true, Single::Off) => (true, Single::On),
                            (true, _) => (false, Single::Off),
                        };
                        self.run(&sender, move |c| async move { c.set_repeat(r, s).await })
                    }
                }
            }
            AppMsg::AlbumActivated(pos) => {
                let Some(item) = self.grid.get_visible(pos) else {
                    return;
                };
                let folder = item.borrow().album.folder.clone();
                self.open(Route::Album(folder), &sender);
            }
            AppMsg::PlayQueuePos(pos) => {
                self.run(&sender, move |c| async move { c.play_pos(pos).await })
            }
            AppMsg::RemoveQueuePos(pos) => {
                self.run(&sender, move |c| async move { c.delete_pos(pos).await })
            }
            AppMsg::ClearQueue => self.run(&sender, |c| async move { c.clear().await }),
            AppMsg::ShowQueue(v) => {
                self.queue_shown = v;
                self.bar.emit(BarInput::QueueShown(v));
            }
            AppMsg::ReloadLibrary => self.load_library(&sender),
            AppMsg::ShowCurrentAlbum => {
                if let Some(f) = self.current_folder.clone() {
                    self.open(Route::Album(f), &sender);
                }
            }
            AppMsg::ClearFilter => {
                *self.filter.borrow_mut() = None;
                self.grid.notify_filter_changed(0);
            }
            AppMsg::Act(a) => self.act(a, &sender, root),
            AppMsg::Popped(tag) => {
                if let Some(i) = self
                    .pages
                    .iter()
                    .rposition(|p| pages::tag(p.route()) == tag)
                {
                    self.pages.remove(i);
                }
            }
        }
    }

    fn update_cmd(&mut self, msg: Cmd, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            Cmd::Status(Ok((status, song))) => {
                let old_pos = self.status.song_pos;
                self.status = *status;
                self.song = song.map(|s| *s);
                if self.queue_version != Some(self.status.playlist_version) {
                    self.refresh_queue(&sender, self.status.playlist_version);
                } else if old_pos != self.status.song_pos {
                    self.set_queue_current(old_pos, false);
                    self.set_queue_current(self.status.song_pos, true);
                }
                self.update_current();
                self.push_bar();
            }
            Cmd::Status(Err(e)) => tracing::warn!(error = %e, "status failed"),
            Cmd::Queue(version, Ok(songs)) => {
                self.queue_version = Some(version);
                let current = self.status.song_pos;
                let (tx, rx) = relm4::channel::<u32>();
                {
                    let s = sender.clone();
                    relm4::spawn_local(async move {
                        while let Some(pos) = rx.recv().await {
                            s.input(AppMsg::RemoveQueuePos(pos));
                        }
                    });
                }
                self.queue.clear();
                self.queue
                    .extend_from_iter(songs.into_iter().map(|song| QueueItem {
                        current: song.pos.is_some() && song.pos == current,
                        song,
                        on_remove: tx.clone(),
                    }));
            }
            Cmd::Queue(_, Err(e)) => tracing::warn!(error = %e, "queue failed"),
            Cmd::Library(Ok(albums)) => {
                let t0 = std::time::Instant::now();
                self.library = LibraryState::Loaded;
                // Pages hold positions into the old library; start over.
                self.nav.pop_to_tag(ROOT_TAG);
                self.pages.clear();
                let lib = Rc::new(Library::new(albums));
                *self.ctx.library.borrow_mut() = lib.clone();
                self.current_folder = None;
                self.grid.clear();
                self.grid
                    .extend_from_iter(lib.albums.iter().map(|a| AlbumItem {
                        album: a.clone(),
                        playing: false,
                        covers: self.ctx.covers.clone(),
                    }));
                for b in &self.browse {
                    b.reload(&self.ctx);
                }
                self.tracks.reload(&self.ctx);
                self.load_preferred(&sender);
                self.update_current();
                self.push_bar();
                tracing::info!(
                    albums = lib.albums.len(),
                    ms = t0.elapsed().as_millis() as u64,
                    "library indexed"
                );
                self.open_debug_route(&sender);
            }
            Cmd::Library(Err(e)) => {
                self.library = LibraryState::NotLoaded;
                self.toast(&format!("{}: {e}", tr("Could not load library")));
            }
            Cmd::Ratings(map) => {
                if !map.is_empty() {
                    self.ctx.ratings.borrow_mut().extend(map);
                    self.refresh_pages();
                }
            }
            Cmd::Preferred(set) => {
                *self.ctx.preferred.borrow_mut() = set;
                self.refresh_pages();
            }
            Cmd::PreferredSet(file, others, res) => match res {
                Ok(()) => {
                    {
                        let mut p = self.ctx.preferred.borrow_mut();
                        for o in &others {
                            p.remove(o);
                        }
                        p.insert(file);
                    }
                    self.toast(&tr("Preferred version set"));
                    self.refresh_pages();
                }
                Err(e) => self.toast(&e),
            },
            Cmd::Done(Ok(())) => {}
            Cmd::Done(Err(e)) => self.toast(&e),
        }
    }

    fn update_view(&self, w: &mut AppWidgets, _sender: ComponentSender<Self>) {
        let lib = self.ctx.lib();
        w.count.set_label(&if lib.albums.is_empty() {
            String::new()
        } else {
            format!(
                "{} {} · {} {}",
                lib.albums.len(),
                tr("albums"),
                lib.track_count(),
                tr("tracks")
            )
        });

        match &self.disconnect_reason {
            Some(r) if !self.connected => {
                let cfg = self.client.config();
                w.banner.set_title(&format!(
                    "{} {}:{} — {r}",
                    tr("Cannot reach MPD at"),
                    cfg.host,
                    cfg.port
                ));
                w.banner.set_revealed(true);
            }
            _ => w.banner.set_revealed(false),
        }

        let page = match self.library {
            LibraryState::Loading if lib.albums.is_empty() => "loading",
            _ if !lib.albums.is_empty() => "grid",
            LibraryState::Loaded => {
                w.empty.set_title(&tr("MPD's library is empty"));
                w.empty.set_description(Some(&tr(
                    "Is the music disk mounted on the server? Run an MPD update and reload.",
                )));
                "empty"
            }
            _ => {
                w.empty.set_title(&tr("No library loaded"));
                w.empty.set_description(None);
                "empty"
            }
        };
        w.albums_stack.set_visible_child_name(page);

        match &*self.filter.borrow() {
            Some(f) => {
                w.filter_icon.set_icon_name(Some(f.icon()));
                w.filter_label.set_label(&f.label());
                w.filter_bar.set_reveal_child(true);
            }
            None => w.filter_bar.set_reveal_child(false),
        }

        w.queue_revealer.set_reveal_child(self.queue_shown);
        w.queue_count
            .set_label(&format!("{} {}", self.queue.len(), tr("tracks")));
    }
}
