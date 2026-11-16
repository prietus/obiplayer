//! Main window.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use obiplayer_core::config::Config;
use obiplayer_core::library::{self, Album};
use obiplayer_core::mpd::{self, Client, Event, PlayState, Single, Song, Status, Subsystem};
use relm4::adw::{self, prelude::*};
use relm4::gtk;
use relm4::typed_view::grid::TypedGridView;
use relm4::typed_view::list::TypedListView;
use relm4::{Component, ComponentController, ComponentParts, ComponentSender, Controller};

use crate::albums::AlbumItem;
use crate::covers::CoverLoader;
use crate::player_bar::{BarInput, BarOutput, PlayerBar, Transport};
use crate::queue::QueueItem;
use crate::tr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LibraryState {
    NotLoaded,
    Loading,
    Loaded,
}

pub struct App {
    client: Client,
    connected: bool,
    disconnect_reason: Option<String>,
    status: Status,
    song: Option<Song>,
    library: LibraryState,
    albums: Vec<Arc<Album>>,
    album_index: HashMap<String, u32>,
    current_folder: Option<String>,
    grid: TypedGridView<AlbumItem, gtk::NoSelection>,
    queue: TypedListView<QueueItem, gtk::NoSelection>,
    queue_version: Option<u32>,
    queue_shown: bool,
    covers: Rc<CoverLoader>,
    bar: Controller<PlayerBar>,
    toasts: adw::ToastOverlay,
}

#[derive(Debug)]
pub enum AppMsg {
    Mpd(Event),
    Transport(Transport),
    PlayAlbum(u32),
    PlayQueuePos(u32),
    RemoveQueuePos(u32),
    ClearQueue,
    ShowQueue(bool),
    ReloadLibrary,
    RevealCurrentAlbum,
}

#[derive(Debug)]
pub enum Cmd {
    Status(Result<(Box<Status>, Option<Box<Song>>), String>),
    Queue(u32, Result<Vec<Song>, String>),
    Library(Result<Vec<Arc<Album>>, String>),
    Done(Result<(), String>),
}

pub struct AppWidgets {
    title: adw::WindowTitle,
    banner: adw::Banner,
    stack: gtk::Stack,
    empty: adw::StatusPage,
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
            let r = c.list_all_info().await.map(library::group_albums);
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
        self.toasts.add_toast(adw::Toast::new(text));
    }

    /// Re-insert a grid item so its cell is re-bound with new state.
    fn set_album_playing(&mut self, folder: &str, playing: bool) {
        let Some(&pos) = self.album_index.get(folder) else {
            return;
        };
        let Some(item) = self.grid.get(pos) else {
            return;
        };
        let mut v = item.borrow().clone();
        if v.playing == playing {
            return;
        }
        v.playing = playing;
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

    fn current_album(&self) -> Option<Arc<Album>> {
        let f = self.current_folder.as_ref()?;
        let &i = self.album_index.get(f)?;
        self.albums.get(i as usize).cloned()
    }

    fn push_bar(&self) {
        self.bar.emit(BarInput::Update {
            status: Box::new(self.status.clone()),
            song: self.song.clone().map(Box::new),
            album: self.current_album(),
        });
    }

    fn update_current_album(&mut self) {
        let folder = self
            .song
            .as_ref()
            .map(|s| library::album_key(&s.file).to_owned())
            .filter(|f| self.album_index.contains_key(f));
        if folder != self.current_folder {
            if let Some(old) = self.current_folder.take() {
                self.set_album_playing(&old, false);
            }
            if let Some(new) = &folder {
                self.set_album_playing(new, true);
            }
            self.current_folder = folder;
        }
    }
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
            .default_width(1100)
            .default_height(760)
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

        let bar = PlayerBar::builder()
            .launch(covers.clone())
            .forward(sender.input_sender(), |o| match o {
                BarOutput::Transport(t) => AppMsg::Transport(t),
                BarOutput::ShowQueue(v) => AppMsg::ShowQueue(v),
                BarOutput::OpenCurrentAlbum => AppMsg::RevealCurrentAlbum,
            });

        // -- Header -------------------------------------------------------------
        let title = adw::WindowTitle::new("ObiPlayer", "");
        let header = adw::HeaderBar::builder().title_widget(&title).build();
        let reload = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text(tr("Reload library"))
            .build();
        {
            let s = sender.clone();
            reload.connect_clicked(move |_| s.input(AppMsg::ReloadLibrary));
        }
        header.pack_end(&reload);

        // -- Album grid ---------------------------------------------------------
        let grid: TypedGridView<AlbumItem, gtk::NoSelection> = TypedGridView::new();
        grid.view.set_single_click_activate(true);
        grid.view.set_min_columns(2);
        grid.view.set_max_columns(16);
        grid.view.add_css_class("album-grid");
        {
            let s = sender.clone();
            grid.view
                .connect_activate(move |_, pos| s.input(AppMsg::PlayAlbum(pos)));
        }
        let grid_scroll = gtk::ScrolledWindow::builder()
            .child(&grid.view)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();

        // -- Stack: loading / empty / grid ---------------------------------------
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

        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .hexpand(true)
            .build();
        stack.add_named(&spinner, Some("loading"));
        stack.add_named(&empty, Some("empty"));
        stack.add_named(&grid_scroll, Some("grid"));

        // -- Queue panel ---------------------------------------------------------
        let queue: TypedListView<QueueItem, gtk::NoSelection> = TypedListView::new();
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
        let queue_revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideLeft)
            // Explicit, so expanding children inside don't make the hidden
            // panel claim half of the spare width.
            .hexpand(false)
            .child(&qbox)
            .build();

        // -- Layout --------------------------------------------------------------
        let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        body.append(&stack);
        body.append(&queue_revealer);

        let banner = adw::Banner::new("");
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&banner);
        content.append(&body);

        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&content));

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.add_top_bar(bar.widget());
        toolbar.set_top_bar_style(adw::ToolbarStyle::RaisedBorder);
        toolbar.set_content(Some(&toasts));
        root.set_content(Some(&toolbar));

        // -- Server events -------------------------------------------------------
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
            connected: false,
            disconnect_reason: None,
            status: Status::default(),
            song: None,
            library: LibraryState::NotLoaded,
            albums: Vec::new(),
            album_index: HashMap::new(),
            current_folder: None,
            grid,
            queue,
            queue_version: None,
            queue_shown: false,
            covers,
            bar,
            toasts,
        };
        let widgets = AppWidgets {
            title,
            banner,
            stack,
            empty,
            queue_revealer,
            queue_count,
        };
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: AppMsg, sender: ComponentSender<Self>, _root: &Self::Root) {
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
            AppMsg::PlayAlbum(pos) => {
                let Some(item) = self.grid.get(pos) else {
                    return;
                };
                let folder = item.borrow().album.folder.clone();
                self.run(&sender, move |c| async move {
                    c.replace_queue_and_play(&[folder]).await
                });
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
            AppMsg::RevealCurrentAlbum => {
                if let Some(&pos) = self
                    .current_folder
                    .as_ref()
                    .and_then(|f| self.album_index.get(f))
                {
                    self.grid
                        .view
                        .scroll_to(pos, gtk::ListScrollFlags::NONE, None);
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
                self.update_current_album();
                self.push_bar();
            }
            Cmd::Status(Err(e)) => tracing::warn!(error = %e, "status failed"),
            Cmd::Queue(version, Ok(songs)) => {
                self.queue_version = Some(version);
                let current = self.status.song_pos;
                let tx = {
                    let s = sender.clone();
                    let (tx, rx) = relm4::channel::<u32>();
                    relm4::spawn_local(async move {
                        while let Some(pos) = rx.recv().await {
                            s.input(AppMsg::RemoveQueuePos(pos));
                        }
                    });
                    tx
                };
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
                self.library = LibraryState::Loaded;
                self.album_index = albums
                    .iter()
                    .enumerate()
                    .map(|(i, a)| (a.folder.clone(), i as u32))
                    .collect();
                self.current_folder = None;
                self.grid.clear();
                self.grid.extend_from_iter(albums.iter().map(|a| AlbumItem {
                    album: a.clone(),
                    playing: false,
                    covers: self.covers.clone(),
                }));
                self.albums = albums;
                self.update_current_album();
                self.push_bar();
            }
            Cmd::Library(Err(e)) => {
                self.library = LibraryState::NotLoaded;
                self.toast(&format!("{}: {e}", tr("Could not load library")));
            }
            Cmd::Done(Ok(())) => {}
            Cmd::Done(Err(e)) => self.toast(&e),
        }
    }

    fn update_view(&self, w: &mut AppWidgets, _sender: ComponentSender<Self>) {
        let tracks: usize = self.albums.iter().map(|a| a.tracks.len()).sum();
        w.title.set_subtitle(&if self.albums.is_empty() {
            String::new()
        } else {
            format!("{} albums · {} tracks", self.albums.len(), tracks)
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
            LibraryState::Loading if self.albums.is_empty() => "loading",
            _ if !self.albums.is_empty() => "grid",
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
        w.stack.set_visible_child_name(page);

        w.queue_revealer.set_reveal_child(self.queue_shown);
        w.queue_count
            .set_label(&format!("{} tracks", self.queue.len()));
    }
}
