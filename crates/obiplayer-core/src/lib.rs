//! Platform-independent core of ObiPlayer: MPD protocol client, library model,
//! audio analysis and metadata services. No GTK dependencies live here.

pub mod analysis;
pub mod catalog;
pub mod config;
pub mod covers;
pub mod history;
pub mod home;
pub mod library;
pub mod meta;
pub mod mpd;
pub mod paths;
pub mod probe;
pub mod quality;
pub mod radio;
pub mod signal;
pub mod smart;
pub mod stats;
pub mod text;
