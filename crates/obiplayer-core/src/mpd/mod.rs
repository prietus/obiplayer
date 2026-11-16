//! MPD client.
//!
//! Two connections are used, as MPD recommends:
//! - a command connection ([`Client`]), shared and reconnected transparently;
//! - an `idle` connection ([`watch`]) that pushes change notifications, so the
//!   UI reacts to server events instead of polling every second.

mod client;
mod connection;
mod error;
mod idle;
mod protocol;
mod types;

pub use client::Client;
pub use connection::{ConnectConfig, Connection};
pub use error::{Ack, Error, Result};
pub use idle::{Event, Subsystem, watch};
pub use protocol::{Command, Pair, quote};
pub use types::{
    AudioFormat, Entry, Output, PcmBits, PlayState, SampleFormat, Single, Song, Stats, Status,
};
