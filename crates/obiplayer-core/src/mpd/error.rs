use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("connection closed by server")]
    Closed,
    #[error("timed out")]
    Timeout,
    #[error("not an MPD server (greeting: {0:?})")]
    BadGreeting(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("invalid argument: {0:?} contains a newline")]
    InvalidArgument(String),
    #[error("MPD error: {0}")]
    Ack(Ack),
}

impl Error {
    /// True when the connection is unusable and should be re-established.
    pub fn is_connection_error(&self) -> bool {
        matches!(
            self,
            Error::Io(_) | Error::Closed | Error::Timeout | Error::Protocol(_)
        )
    }

    /// The MPD `ACK` code, if this is a server-side error.
    pub fn ack_code(&self) -> Option<u32> {
        match self {
            Error::Ack(a) => Some(a.code),
            _ => None,
        }
    }
}

/// A server error line: `ACK [code@index] {command} message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ack {
    pub code: u32,
    /// Position in a command list (0 for single commands).
    pub index: u32,
    pub command: String,
    pub message: String,
}

impl Ack {
    pub const NOT_LIST: u32 = 1;
    pub const ARG: u32 = 2;
    pub const PASSWORD: u32 = 3;
    pub const PERMISSION: u32 = 4;
    pub const UNKNOWN: u32 = 5;
    pub const NO_EXIST: u32 = 50;

    pub(crate) fn parse(line: &str) -> Option<Ack> {
        let rest = line.strip_prefix("ACK [")?;
        let (codes, rest) = rest.split_once(']')?;
        let (code, index) = codes.split_once('@')?;
        let rest = rest.trim_start().strip_prefix('{')?;
        let (command, message) = rest.split_once('}')?;
        Some(Ack {
            code: code.parse().ok()?,
            index: index.parse().ok()?,
            command: command.to_owned(),
            message: message.trim_start().to_owned(),
        })
    }
}

impl fmt::Display for Ack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.command.is_empty() {
            write!(f, "{} (code {})", self.message, self.code)
        } else {
            write!(f, "{}: {} (code {})", self.command, self.message, self.code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ack() {
        let a = Ack::parse("ACK [50@0] {sticker} no such sticker").unwrap();
        assert_eq!(a.code, 50);
        assert_eq!(a.index, 0);
        assert_eq!(a.command, "sticker");
        assert_eq!(a.message, "no such sticker");

        let a = Ack::parse("ACK [3@0] {password} incorrect password").unwrap();
        assert_eq!(a.code, Ack::PASSWORD);

        let a = Ack::parse("ACK [5@2] {} unknown command \"foo\"").unwrap();
        assert_eq!(a.index, 2);
        assert_eq!(a.command, "");
    }

    #[test]
    fn rejects_garbage() {
        assert!(Ack::parse("ACK nonsense").is_none());
        assert!(Ack::parse("OK").is_none());
    }
}
