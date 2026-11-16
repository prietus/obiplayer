//! MPRIS2 (`org.mpris.MediaPlayer2`) on the session bus, so desktop shells,
//! media keys and `playerctl` can see and drive ObiPlayer.
//!
//! The D-Bus server (`mpris-server`, on top of `zbus`) is a `!Send` local
//! server driven from the GLib main loop: method calls are dispatched by a
//! future spawned with `glib::spawn_future_local`, so `on_command` always
//! runs on the GTK main thread. State pushes (`update`, `seeked`) go through
//! an unbounded `async_channel` to a single local task that applies them in
//! order (the property setters are async and only emit `PropertiesChanged`
//! when the value actually changes).
//!
//! On anything other than Linux the handle is a no-op stub.

// Until the app wires the handle in, its API is unused in the binary.

use std::path::Path;

use obiplayer_core::mpd::{Song, Status};

/// A request coming from an MPRIS client, to be turned into an app command.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub enum MprisCommand {
    PlayPause,
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    /// Relative seek in seconds (may be negative). Per the spec, a negative
    /// result should clamp to 0 and seeking past the end should skip to the
    /// next track; that is left to the receiver, which knows the duration.
    Seek(f64),
    /// Absolute position in seconds within the current track (already
    /// validated against the current track id and length).
    SetPosition(f64),
    Raise,
}

/// Handle to the MPRIS server. Dropping it stops processing updates.
pub struct Mpris {
    #[cfg(target_os = "linux")]
    tx: Option<async_channel::Sender<imp::Update>>,
}

#[cfg(not(target_os = "linux"))]
impl Mpris {
    /// MPRIS is a Linux/D-Bus interface: this is a no-op elsewhere.
    pub fn start(on_command: impl Fn(MprisCommand) + 'static) -> Mpris {
        let _ = on_command;
        tracing::debug!("MPRIS disabled: not running on Linux");
        Mpris {}
    }

    pub fn update(&self, _status: &Status, _song: Option<&Song>, _art_path: Option<&Path>) {}

    pub fn seeked(&self, _position_secs: f64) {}
}

#[cfg(target_os = "linux")]
impl Mpris {
    /// Register `org.mpris.MediaPlayer2.obiplayer` on the session bus. Never
    /// fails hard: without a session bus the handle just logs and ignores
    /// updates. Must be called on the GTK main thread (it spawns local futures).
    pub fn start(on_command: impl Fn(MprisCommand) + 'static) -> Mpris {
        Mpris {
            tx: Some(imp::start(on_command)),
        }
    }

    /// Push the current MPD state. `art_path` is a local image file for
    /// `mpris:artUrl`, if any.
    pub fn update(&self, status: &Status, song: Option<&Song>, art_path: Option<&Path>) {
        self.send(imp::Update::State(Box::new(imp::Snapshot::new(
            status, song, art_path,
        ))));
    }

    /// The elapsed time changed discontinuously (seek, track change).
    pub fn seeked(&self, position_secs: f64) {
        self.send(imp::Update::Seeked(position_secs));
    }

    fn send(&self, update: imp::Update) {
        if let Some(tx) = &self.tx {
            // Unbounded; only fails once the server task has given up.
            let _ = tx.try_send(update);
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::cell::{Cell, RefCell};
    use std::path::Path;
    use std::rc::{Rc, Weak};
    use std::time::{Duration, Instant};

    use mpris_server::{LoopStatus, Metadata, PlaybackStatus, Player, Time, TrackId};
    use obiplayer_core::mpd::{PlayState, Single, Song, Status};
    use relm4::gtk::glib;

    use super::MprisCommand;

    const BUS_SUFFIX: &str = "obiplayer";
    const IDENTITY: &str = "ObiPlayer";
    const DESKTOP_ENTRY: &str = "us.priet.ObiPlayer";
    const TRACK_PREFIX: &str = "/us/priet/ObiPlayer/track/";

    pub enum Update {
        State(Box<Snapshot>),
        Seeked(f64),
    }

    /// Everything MPRIS exposes, derived from one MPD status/song pair.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Snapshot {
        state: PlayState,
        elapsed: f64,
        shuffle: bool,
        loop_status: LoopStatus,
        /// Queue is non-empty: transport controls make sense.
        has_queue: bool,
        metadata: Metadata,
    }

    impl Snapshot {
        pub fn new(status: &Status, song: Option<&Song>, art_path: Option<&Path>) -> Snapshot {
            let loop_status = match (status.repeat, status.single) {
                (true, Single::On) => LoopStatus::Track,
                (true, _) => LoopStatus::Playlist,
                (false, _) => LoopStatus::None,
            };
            Snapshot {
                state: status.state,
                elapsed: status.elapsed.unwrap_or(0.0).max(0.0),
                shuffle: status.random,
                loop_status,
                has_queue: status.playlist_length > 0,
                metadata: song
                    .map(|s| metadata(s, status, art_path))
                    .unwrap_or_else(no_track),
            }
        }

        fn playback_status(&self) -> PlaybackStatus {
            match self.state {
                PlayState::Play => PlaybackStatus::Playing,
                PlayState::Pause => PlaybackStatus::Paused,
                PlayState::Stop => PlaybackStatus::Stopped,
            }
        }
    }

    fn no_track() -> Metadata {
        Metadata::builder().trackid(TrackId::NO_TRACK).build()
    }

    pub fn track_id(queue_id: Option<u32>) -> TrackId {
        queue_id
            .and_then(|id| TrackId::try_from(format!("{TRACK_PREFIX}{id}")).ok())
            .unwrap_or(TrackId::NO_TRACK)
    }

    /// Leading integer of a `track`/`disc` tag (`"3"`, `"3/12"`, `" 03 "`).
    pub fn leading_number(v: &str) -> Option<i32> {
        let v = v.trim();
        let end = v.find(|c: char| !c.is_ascii_digit()).unwrap_or(v.len());
        v[..end].parse().ok()
    }

    fn secs_to_time(secs: f64) -> Time {
        Time::from_micros((secs * 1_000_000.0).round() as i64)
    }

    fn metadata(song: &Song, status: &Status, art_path: Option<&Path>) -> Metadata {
        let mut m = Metadata::new();
        m.set_trackid(Some(track_id(song.id.or(status.song_id))));
        let length = song.duration.or(status.duration).filter(|d| *d > 0.0);
        m.set_length(length.map(secs_to_time));
        m.set_title(Some(song.display_title()));
        let non_empty = |v: &[String]| -> Option<Vec<String>> {
            let v: Vec<String> = v.iter().filter(|s| !s.is_empty()).cloned().collect();
            (!v.is_empty()).then_some(v)
        };
        m.set_artist(non_empty(song.tag_values("artist")));
        m.set_album(song.album());
        m.set_album_artist(non_empty(song.tag_values("albumartist")));
        m.set_track_number(song.track().and_then(leading_number));
        m.set_disc_number(song.disc().and_then(leading_number));
        m.set_genre(non_empty(song.genres()));
        m.set_composer(non_empty(song.tag_values("composer")));
        m.set_art_url(art_path.and_then(|p| glib::filename_to_uri(p, None).ok().map(String::from)));
        m
    }

    /// Last known position, interpolated while playing (Position is queried,
    /// never signalled).
    #[derive(Clone, Copy)]
    struct Clock {
        base: f64,
        at: Instant,
        playing: bool,
    }

    impl Clock {
        fn now(&self) -> f64 {
            if self.playing {
                self.base + self.at.elapsed().as_secs_f64()
            } else {
                self.base
            }
        }
    }

    pub fn start(on_command: impl Fn(MprisCommand) + 'static) -> async_channel::Sender<Update> {
        let (tx, rx) = async_channel::unbounded();
        glib::spawn_future_local(async move {
            let player = match build().await {
                Ok(p) => Rc::new(p),
                Err(e) => {
                    tracing::warn!("MPRIS unavailable: {e}");
                    rx.close();
                    return;
                }
            };
            tracing::info!("MPRIS registered as org.mpris.MediaPlayer2.{BUS_SUFFIX}");
            connect(&player, Rc::new(on_command));
            glib::spawn_future_local(player.run());

            let clock = Rc::new(Cell::new(Clock {
                base: 0.0,
                at: Instant::now(),
                playing: false,
            }));
            // Keep the queried Position roughly current between MPD updates.
            {
                let player: Weak<Player> = Rc::downgrade(&player);
                let clock = clock.clone();
                glib::timeout_add_local(Duration::from_secs(1), move || {
                    let Some(player) = player.upgrade() else {
                        return glib::ControlFlow::Break;
                    };
                    let c = clock.get();
                    if c.playing {
                        player.set_position(secs_to_time(c.now()));
                    }
                    glib::ControlFlow::Continue
                });
            }

            let last = RefCell::new(None::<Snapshot>);
            while let Ok(update) = rx.recv().await {
                let res = match update {
                    Update::State(s) => apply(&player, &clock, &last, *s).await,
                    Update::Seeked(pos) => {
                        let mut c = clock.get();
                        c.base = pos.max(0.0);
                        c.at = Instant::now();
                        clock.set(c);
                        player.set_position(secs_to_time(c.base));
                        player.seeked(secs_to_time(c.base)).await
                    }
                };
                if let Err(e) = res {
                    tracing::debug!("MPRIS update failed: {e}");
                }
            }
        });
        tx
    }

    async fn build() -> mpris_server::zbus::Result<Player> {
        Player::builder(BUS_SUFFIX)
            .identity(IDENTITY)
            .desktop_entry(DESKTOP_ENTRY)
            .can_quit(false)
            .can_raise(true)
            .can_set_fullscreen(false)
            .has_track_list(false)
            .can_control(true)
            .rate(1.0)
            .minimum_rate(1.0)
            .maximum_rate(1.0)
            .volume(1.0)
            .metadata(no_track())
            .build()
            .await
    }

    fn connect(player: &Player, cmd: Rc<dyn Fn(MprisCommand)>) {
        let c = cmd.clone();
        player.connect_play_pause(move |_| c(MprisCommand::PlayPause));
        let c = cmd.clone();
        player.connect_play(move |_| c(MprisCommand::Play));
        let c = cmd.clone();
        player.connect_pause(move |_| c(MprisCommand::Pause));
        let c = cmd.clone();
        player.connect_stop(move |_| c(MprisCommand::Stop));
        let c = cmd.clone();
        player.connect_next(move |_| c(MprisCommand::Next));
        let c = cmd.clone();
        player.connect_previous(move |_| c(MprisCommand::Previous));
        let c = cmd.clone();
        player.connect_raise(move |_| c(MprisCommand::Raise));
        let c = cmd.clone();
        player.connect_seek(move |p, offset| {
            if p.can_seek() {
                c(MprisCommand::Seek(offset.as_micros() as f64 / 1_000_000.0));
            }
        });
        let c = cmd;
        player.connect_set_position(move |p, id, pos| {
            // Spec: ignore stale track ids and out-of-range positions.
            let (current, length) = {
                let m = p.metadata();
                (m.trackid(), m.length())
            };
            if !p.can_seek()
                || current.as_ref() != Some(id)
                || *id == TrackId::NO_TRACK
                || pos.is_negative()
                || length.is_some_and(|l| pos > l)
            {
                return;
            }
            c(MprisCommand::SetPosition(
                pos.as_micros() as f64 / 1_000_000.0,
            ));
        });
        // Volume, rate, shuffle and loop status are read-only here: the
        // setters are not connected, so writes are ignored.
    }

    async fn apply(
        player: &Player,
        clock: &Cell<Clock>,
        last: &RefCell<Option<Snapshot>>,
        s: Snapshot,
    ) -> mpris_server::zbus::Result<()> {
        clock.set(Clock {
            base: s.elapsed,
            at: Instant::now(),
            playing: s.state == PlayState::Play,
        });
        player.set_position(secs_to_time(s.elapsed));
        if last.borrow().as_ref() == Some(&s) {
            return Ok(());
        }
        // Each setter compares with the current value and only emits
        // PropertiesChanged on an actual change.
        player.set_metadata(s.metadata.clone()).await?;
        player.set_playback_status(s.playback_status()).await?;
        player.set_shuffle(s.shuffle).await?;
        player.set_loop_status(s.loop_status).await?;
        let q = s.has_queue;
        player.set_can_play(q).await?;
        player.set_can_pause(q).await?;
        player.set_can_go_next(q).await?;
        player.set_can_go_previous(q).await?;
        let seekable = q && s.state != PlayState::Stop && s.metadata.length().is_some();
        player.set_can_seek(seekable).await?;
        last.replace(Some(s));
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn numbers() {
            assert_eq!(leading_number("3/12"), Some(3));
            assert_eq!(leading_number(" 07 "), Some(7));
            assert_eq!(leading_number("A1"), None);
        }

        #[test]
        fn track_ids() {
            assert_eq!(track_id(Some(42)).as_str(), "/us/priet/ObiPlayer/track/42");
            assert_eq!(track_id(None), TrackId::NO_TRACK);
        }
    }
}
