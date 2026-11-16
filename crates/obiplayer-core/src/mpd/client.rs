use std::sync::Arc;

use tokio::sync::Mutex;

use super::connection::{ConnectConfig, Connection};
use super::error::{Ack, Error, Result};
use super::protocol::{Command, Pair};
use super::types::{Entry, Output, Single, Song, Stats, Status};

/// Shared MPD command client.
///
/// Holds one persistent connection, created lazily. If the connection turns
/// out to be dead (MPD drops idle clients after `connection_timeout`), it is
/// re-established and the command retried once. Server errors (`ACK`) are
/// returned as [`Error::Ack`], never swallowed.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

struct Inner {
    cfg: ConnectConfig,
    conn: Mutex<Option<Connection>>,
}

enum Request<'a> {
    One(&'a Command),
    List(&'a [Command]),
    Binary(&'a Command),
}

impl Client {
    /// Create a client without connecting.
    pub fn new(cfg: ConnectConfig) -> Client {
        Client {
            inner: Arc::new(Inner {
                cfg,
                conn: Mutex::new(None),
            }),
        }
    }

    /// Create a client and connect immediately (validates host and password).
    pub async fn connect(cfg: ConnectConfig) -> Result<Client> {
        let conn = Connection::connect(&cfg).await?;
        Ok(Client {
            inner: Arc::new(Inner {
                cfg,
                conn: Mutex::new(Some(conn)),
            }),
        })
    }

    pub fn config(&self) -> &ConnectConfig {
        &self.inner.cfg
    }

    async fn request(&self, req: Request<'_>) -> Result<Vec<Pair>> {
        self.request_full(req).await.map(|(p, _)| p)
    }

    async fn request_full(&self, req: Request<'_>) -> Result<(Vec<Pair>, Vec<u8>)> {
        let mut guard = self.inner.conn.lock().await;
        let mut retried = false;
        loop {
            let conn = match guard.as_mut() {
                Some(c) => c,
                None => guard.insert(Connection::connect(&self.inner.cfg).await?),
            };
            let res = match req {
                Request::One(c) => conn.execute(c).await.map(|p| (p, Vec::new())),
                Request::List(cs) => conn.execute_list(cs).await.map(|p| (p, Vec::new())),
                Request::Binary(c) => conn.execute_binary(c).await,
            };
            match res {
                Err(e) if e.is_connection_error() => {
                    *guard = None;
                    if retried {
                        return Err(e);
                    }
                    tracing::debug!(error = %e, "MPD connection stale, reconnecting");
                    retried = true;
                }
                r => return r,
            }
        }
    }

    /// Run a raw command and return its key/value pairs.
    pub async fn command(&self, cmd: Command) -> Result<Vec<Pair>> {
        self.request(Request::One(&cmd)).await
    }

    /// Run several commands atomically.
    pub async fn command_list(&self, cmds: &[Command]) -> Result<()> {
        if cmds.is_empty() {
            return Ok(());
        }
        self.request(Request::List(cmds)).await.map(drop)
    }

    async fn simple(&self, cmd: Command) -> Result<()> {
        self.command(cmd).await.map(drop)
    }

    // -- Querying -----------------------------------------------------------

    pub async fn ping(&self) -> Result<()> {
        self.simple(Command::new("ping")).await
    }

    pub async fn status(&self) -> Result<Status> {
        Ok(Status::from_pairs(
            &self.command(Command::new("status")).await?,
        ))
    }

    pub async fn stats(&self) -> Result<Stats> {
        Ok(Stats::from_pairs(
            &self.command(Command::new("stats")).await?,
        ))
    }

    pub async fn current_song(&self) -> Result<Option<Song>> {
        let pairs = self.command(Command::new("currentsong")).await?;
        Ok(Entry::songs(&pairs).into_iter().next())
    }

    /// The whole queue.
    pub async fn queue(&self) -> Result<Vec<Song>> {
        Ok(Entry::songs(
            &self.command(Command::new("playlistinfo")).await?,
        ))
    }

    /// Every song in the database, with tags.
    pub async fn list_all_info(&self) -> Result<Vec<Song>> {
        Ok(Entry::songs(
            &self.command(Command::new("listallinfo")).await?,
        ))
    }

    /// Direct children of a directory (`""` = root).
    pub async fn lsinfo(&self, path: &str) -> Result<Vec<Entry>> {
        let cmd = if path.is_empty() {
            Command::new("lsinfo")
        } else {
            Command::new("lsinfo").arg(path)
        };
        Ok(Entry::parse_all(&self.command(cmd).await?))
    }

    /// Case-insensitive substring search across all tags.
    pub async fn search_any(&self, query: &str) -> Result<Vec<Song>> {
        let cmd = Command::new("search").args(["any", query]);
        Ok(Entry::songs(&self.command(cmd).await?))
    }

    // -- Playback -----------------------------------------------------------

    pub async fn play(&self) -> Result<()> {
        self.simple(Command::new("play")).await
    }

    pub async fn play_pos(&self, pos: u32) -> Result<()> {
        self.simple(Command::new("play").arg(pos)).await
    }

    pub async fn play_id(&self, id: u32) -> Result<()> {
        self.simple(Command::new("playid").arg(id)).await
    }

    pub async fn set_pause(&self, paused: bool) -> Result<()> {
        self.simple(Command::new("pause").arg(u8::from(paused)))
            .await
    }

    /// Pause if playing, resume if paused, start if stopped with a queue.
    pub async fn toggle_pause(&self) -> Result<()> {
        let st = self.status().await?;
        match st.state {
            super::PlayState::Play => self.set_pause(true).await,
            super::PlayState::Pause => self.set_pause(false).await,
            super::PlayState::Stop if st.playlist_length > 0 => self.play().await,
            super::PlayState::Stop => Ok(()),
        }
    }

    pub async fn stop(&self) -> Result<()> {
        self.simple(Command::new("stop")).await
    }

    pub async fn next(&self) -> Result<()> {
        self.simple(Command::new("next")).await
    }

    pub async fn previous(&self) -> Result<()> {
        self.simple(Command::new("previous")).await
    }

    /// Seek within the current song (seconds, fractional allowed).
    pub async fn seek_current(&self, seconds: f64) -> Result<()> {
        self.simple(Command::new("seekcur").arg(format!("{:.3}", seconds.max(0.0))))
            .await
    }

    pub async fn set_random(&self, on: bool) -> Result<()> {
        self.simple(Command::new("random").arg(u8::from(on))).await
    }

    /// Set `repeat` and `single` together (atomically).
    pub async fn set_repeat(&self, repeat: bool, single: Single) -> Result<()> {
        let single = match single {
            Single::Off => "0",
            Single::On => "1",
            Single::Oneshot => "oneshot",
        };
        self.command_list(&[
            Command::new("repeat").arg(u8::from(repeat)),
            Command::new("single").arg(single),
        ])
        .await
    }

    // -- Queue --------------------------------------------------------------

    pub async fn clear(&self) -> Result<()> {
        self.simple(Command::new("clear")).await
    }

    /// Append a file or a whole directory.
    pub async fn add(&self, uri: &str) -> Result<()> {
        self.simple(Command::new("add").arg(uri)).await
    }

    /// Append a single file and return its queue id.
    pub async fn add_id(&self, uri: &str) -> Result<u32> {
        let pairs = self.command(Command::new("addid").arg(uri)).await?;
        pairs
            .iter()
            .find(|(k, _)| k == "Id")
            .and_then(|(_, v)| v.parse().ok())
            .ok_or_else(|| Error::Protocol("addid returned no Id".into()))
    }

    /// Append a file and start playing it right away.
    pub async fn add_and_play(&self, uri: &str) -> Result<()> {
        let id = self.add_id(uri).await?;
        self.play_id(id).await
    }

    pub async fn delete_pos(&self, pos: u32) -> Result<()> {
        self.simple(Command::new("delete").arg(pos)).await
    }

    /// Replace the queue with `uris` and start playing — one round trip.
    pub async fn replace_queue_and_play(&self, uris: &[String]) -> Result<()> {
        let mut cmds = Vec::with_capacity(uris.len() + 2);
        cmds.push(Command::new("clear"));
        cmds.extend(uris.iter().map(|u| Command::new("add").arg(u)));
        cmds.push(Command::new("play"));
        self.command_list(&cmds).await
    }

    // -- Database -----------------------------------------------------------

    /// Start a database update (optionally limited to `path`); returns the job id.
    pub async fn update(&self, path: Option<&str>) -> Result<u32> {
        let cmd = match path {
            Some(p) => Command::new("update").arg(p),
            None => Command::new("update"),
        };
        let pairs = self.command(cmd).await?;
        pairs
            .iter()
            .find(|(k, _)| k == "updating_db")
            .and_then(|(_, v)| v.parse().ok())
            .ok_or_else(|| Error::Protocol("update returned no job id".into()))
    }

    // -- Outputs ------------------------------------------------------------

    pub async fn outputs(&self) -> Result<Vec<Output>> {
        Ok(Output::parse_all(
            &self.command(Command::new("outputs")).await?,
        ))
    }

    pub async fn enable_output(&self, id: u32) -> Result<()> {
        self.simple(Command::new("enableoutput").arg(id)).await
    }

    pub async fn disable_output(&self, id: u32) -> Result<()> {
        self.simple(Command::new("disableoutput").arg(id)).await
    }

    pub async fn toggle_output(&self, id: u32) -> Result<()> {
        self.simple(Command::new("toggleoutput").arg(id)).await
    }

    // -- Artwork ------------------------------------------------------------

    /// Cover image for a song: the cover file in its directory (`albumart`),
    /// falling back to a picture embedded in the file (`readpicture`).
    /// Works with remote servers, no local mount needed.
    pub async fn cover_art(&self, uri: &str) -> Result<Option<Vec<u8>>> {
        for cmd in ["albumart", "readpicture"] {
            match self.read_binary(cmd, uri).await {
                Ok(Some(data)) => return Ok(Some(data)),
                Ok(None) => {}
                Err(Error::Ack(a)) if a.code == Ack::NO_EXIST || a.code == Ack::UNKNOWN => {}
                Err(e) => return Err(e),
            }
        }
        Ok(None)
    }

    async fn read_binary(&self, cmd: &str, uri: &str) -> Result<Option<Vec<u8>>> {
        let mut data = Vec::new();
        loop {
            let c = Command::new(cmd).arg(uri).arg(data.len());
            let (pairs, chunk) = self.request_full(Request::Binary(&c)).await?;
            let size: Option<usize> = pairs
                .iter()
                .find(|(k, _)| k == "size")
                .and_then(|(_, v)| v.parse().ok());
            let Some(size) = size else { return Ok(None) };
            if chunk.is_empty() {
                // Defensive: never loop forever on a misbehaving server.
                return Ok((!data.is_empty()).then_some(data));
            }
            data.extend_from_slice(&chunk);
            if data.len() >= size {
                return Ok(Some(data));
            }
        }
    }

    // -- Stickers (song) ----------------------------------------------------

    /// Read a song sticker; `None` if it is not set.
    pub async fn sticker_get(&self, uri: &str, name: &str) -> Result<Option<String>> {
        let cmd = Command::new("sticker").args(["get", "song", uri, name]);
        match self.command(cmd).await {
            Ok(pairs) => Ok(pairs
                .into_iter()
                .find(|(k, _)| k == "sticker")
                .and_then(|(_, v)| v.split_once('=').map(|(_, val)| val.to_owned()))),
            Err(Error::Ack(a)) if a.code == Ack::NO_EXIST => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub async fn sticker_set(&self, uri: &str, name: &str, value: &str) -> Result<()> {
        self.simple(Command::new("sticker").args(["set", "song", uri, name, value]))
            .await
    }

    /// Delete a song sticker; deleting one that does not exist is not an error.
    pub async fn sticker_delete(&self, uri: &str, name: &str) -> Result<()> {
        let cmd = Command::new("sticker").args(["delete", "song", uri, name]);
        match self.simple(cmd).await {
            Err(Error::Ack(a)) if a.code == Ack::NO_EXIST => Ok(()),
            r => r,
        }
    }

    /// All songs under `base` (`""` = everything) that have sticker `name`,
    /// as `(uri, value)`.
    pub async fn sticker_find(&self, base: &str, name: &str) -> Result<Vec<(String, String)>> {
        let cmd = Command::new("sticker").args(["find", "song", base, name]);
        let pairs = match self.command(cmd).await {
            Ok(p) => p,
            Err(Error::Ack(a)) if a.code == Ack::NO_EXIST => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut out = Vec::new();
        let mut file: Option<String> = None;
        for (k, v) in pairs {
            match k.as_str() {
                "file" => file = Some(v),
                "sticker" => {
                    if let (Some(f), Some((_, val))) = (file.take(), v.split_once('=')) {
                        out.push((f, val.to_owned()));
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }

    /// Add `by` to a numeric sticker (missing counts as 0). Not atomic across
    /// clients — the protocol has no increment command.
    pub async fn sticker_increment(&self, uri: &str, name: &str, by: i64) -> Result<i64> {
        let cur = self
            .sticker_get(uri, name)
            .await?
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        let new = cur + by;
        self.sticker_set(uri, name, &new.to_string()).await?;
        Ok(new)
    }
}
