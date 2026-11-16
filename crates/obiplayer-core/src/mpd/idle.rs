use std::time::Duration;

use super::connection::{ConnectConfig, Connection};

/// MPD subsystems reported by `idle`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Subsystem {
    Database,
    Update,
    StoredPlaylist,
    /// The queue.
    Playlist,
    Player,
    Mixer,
    Output,
    Options,
    Partition,
    Sticker,
    Subscription,
    Message,
    Neighbor,
    Mount,
    Other(String),
}

impl Subsystem {
    pub fn as_str(&self) -> &str {
        match self {
            Subsystem::Database => "database",
            Subsystem::Update => "update",
            Subsystem::StoredPlaylist => "stored_playlist",
            Subsystem::Playlist => "playlist",
            Subsystem::Player => "player",
            Subsystem::Mixer => "mixer",
            Subsystem::Output => "output",
            Subsystem::Options => "options",
            Subsystem::Partition => "partition",
            Subsystem::Sticker => "sticker",
            Subsystem::Subscription => "subscription",
            Subsystem::Message => "message",
            Subsystem::Neighbor => "neighbor",
            Subsystem::Mount => "mount",
            Subsystem::Other(s) => s,
        }
    }

    pub fn from_name(s: &str) -> Subsystem {
        match s {
            "database" => Subsystem::Database,
            "update" => Subsystem::Update,
            "stored_playlist" => Subsystem::StoredPlaylist,
            "playlist" => Subsystem::Playlist,
            "player" => Subsystem::Player,
            "mixer" => Subsystem::Mixer,
            "output" => Subsystem::Output,
            "options" => Subsystem::Options,
            "partition" => Subsystem::Partition,
            "sticker" => Subsystem::Sticker,
            "subscription" => Subsystem::Subscription,
            "message" => Subsystem::Message,
            "neighbor" => Subsystem::Neighbor,
            "mount" => Subsystem::Mount,
            other => Subsystem::Other(other.to_owned()),
        }
    }
}

/// Notifications from [`watch`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// (Re)connected; the receiver should refresh all state.
    Connected {
        version: String,
    },
    Changed(Vec<Subsystem>),
    /// Connection lost or refused; a retry is scheduled.
    Disconnected {
        reason: String,
    },
}

const MAX_BACKOFF: Duration = Duration::from_secs(10);

/// Keep an `idle` connection open and forward changes to `tx`, reconnecting
/// with exponential backoff (1 s → 10 s). Returns when `tx` is closed.
///
/// Must run on a Tokio runtime; the receiving side can be any executor
/// (e.g. the GLib main loop), since `async_channel` is runtime-agnostic.
pub async fn watch(cfg: ConnectConfig, tx: async_channel::Sender<Event>) {
    let mut backoff = Duration::from_secs(1);
    let mut was_connected = true; // report the first failure too
    loop {
        match Connection::connect(&cfg).await {
            Ok(mut conn) => {
                backoff = Duration::from_secs(1);
                was_connected = true;
                let ev = Event::Connected {
                    version: conn.version().to_owned(),
                };
                if tx.send(ev).await.is_err() {
                    return;
                }
                let reason = loop {
                    tokio::select! {
                        r = conn.idle(&[]) => match r {
                            Ok(changed) => {
                                if tx.send(Event::Changed(changed)).await.is_err() {
                                    return;
                                }
                            }
                            Err(e) => break e.to_string(),
                        },
                        _ = tx_closed(&tx) => return,
                    }
                };
                tracing::warn!(%reason, "MPD idle connection lost");
                if tx.send(Event::Disconnected { reason }).await.is_err() {
                    return;
                }
            }
            Err(e) => {
                tracing::debug!(error = %e, "MPD connect failed");
                if was_connected {
                    was_connected = false;
                    let ev = Event::Disconnected {
                        reason: e.to_string(),
                    };
                    if tx.send(ev).await.is_err() {
                        return;
                    }
                }
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(backoff) => {}
            _ = tx_closed(&tx) => return,
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// Resolves once the receiving side has been dropped.
async fn tx_closed(tx: &async_channel::Sender<Event>) {
    while !tx.is_closed() {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}
