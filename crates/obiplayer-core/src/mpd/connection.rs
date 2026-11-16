use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::time::timeout;

use super::error::{Ack, Error, Result};
use super::idle::Subsystem;
use super::protocol::{Command, Pair, parse_pair};

const BINARY_LIMIT: u32 = 1 << 20;

/// Where and how to reach MPD.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectConfig {
    pub host: String,
    pub port: u16,
    pub password: Option<String>,
    /// Applies to connecting and to each command (not to `idle`).
    pub timeout: Duration,
}

impl Default for ConnectConfig {
    fn default() -> Self {
        ConnectConfig {
            host: "localhost".into(),
            port: 6600,
            password: None,
            timeout: Duration::from_secs(60),
        }
    }
}

impl ConnectConfig {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        ConnectConfig {
            host: host.into(),
            port,
            ..Default::default()
        }
    }

    pub fn with_password(mut self, password: impl Into<String>) -> Self {
        let p = password.into();
        self.password = (!p.is_empty()).then_some(p);
        self
    }

    /// Defaults overridden by the standard `MPD_HOST` (`[password@]host`) and
    /// `MPD_PORT` environment variables, as used by `mpc` and other clients.
    pub fn from_env() -> Self {
        let mut cfg = ConnectConfig::default();
        if let Ok(h) = std::env::var("MPD_HOST")
            && !h.is_empty()
        {
            match h.rsplit_once('@') {
                Some((pw, host)) if !host.is_empty() => {
                    cfg.host = host.to_owned();
                    cfg = cfg.with_password(pw);
                }
                _ => cfg.host = h,
            }
        }
        if let Some(p) = std::env::var("MPD_PORT").ok().and_then(|p| p.parse().ok()) {
            cfg.port = p;
        }
        cfg
    }

    /// True unless the host is the local machine.
    pub fn is_remote(&self) -> bool {
        !matches!(self.host.as_str(), "" | "localhost" | "127.0.0.1" | "::1")
    }
}

/// A single authenticated protocol connection.
pub struct Connection {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
    version: String,
    timeout: Duration,
}

impl Connection {
    pub async fn connect(cfg: &ConnectConfig) -> Result<Connection> {
        let stream = timeout(
            cfg.timeout,
            TcpStream::connect((cfg.host.as_str(), cfg.port)),
        )
        .await
        .map_err(|_| Error::Timeout)??;
        stream.set_nodelay(true)?;
        let (r, w) = stream.into_split();
        let mut conn = Connection {
            reader: BufReader::new(r),
            writer: w,
            version: String::new(),
            timeout: cfg.timeout,
        };

        let greeting = timeout(cfg.timeout, conn.read_line())
            .await
            .map_err(|_| Error::Timeout)??;
        conn.version = greeting
            .strip_prefix("OK MPD ")
            .ok_or_else(|| Error::BadGreeting(greeting.clone()))?
            .to_owned();

        if let Some(pw) = &cfg.password {
            conn.execute(&Command::new("password").arg(pw)).await?;
        }
        // Larger binary chunks make `albumart` far faster (MPD >= 0.22.4).
        // Older servers reject it; that is harmless.
        if let Err(e) = conn
            .execute(&Command::new("binarylimit").arg(BINARY_LIMIT))
            .await
            && e.is_connection_error()
        {
            return Err(e);
        }
        Ok(conn)
    }

    /// Protocol version announced by the server, e.g. `0.24.0`.
    pub fn version(&self) -> &str {
        &self.version
    }

    pub async fn execute(&mut self, cmd: &Command) -> Result<Vec<Pair>> {
        cmd.validate()?;
        let limit = self.timeout;
        let fut = async {
            self.send(cmd.line()).await?;
            self.read_response().await
        };
        timeout(limit, fut).await.map_err(|_| Error::Timeout)?
    }

    /// Run a command whose response may carry a binary payload
    /// (`albumart`, `readpicture`).
    pub async fn execute_binary(&mut self, cmd: &Command) -> Result<(Vec<Pair>, Vec<u8>)> {
        cmd.validate()?;
        let limit = self.timeout;
        let fut = async {
            self.send(cmd.line()).await?;
            self.read_response_full().await
        };
        timeout(limit, fut).await.map_err(|_| Error::Timeout)?
    }

    /// Run several commands atomically (`command_list_begin` … `end`).
    pub async fn execute_list(&mut self, cmds: &[Command]) -> Result<Vec<Pair>> {
        for c in cmds {
            c.validate()?;
        }
        let mut body = String::from("command_list_begin\n");
        for c in cmds {
            body.push_str(c.line());
            body.push('\n');
        }
        body.push_str("command_list_end");
        let limit = self.timeout;
        let fut = async {
            self.send(&body).await?;
            self.read_response().await
        };
        timeout(limit, fut).await.map_err(|_| Error::Timeout)?
    }

    /// Block until one of `subsystems` (or any, if empty) changes. No timeout.
    pub async fn idle(&mut self, subsystems: &[Subsystem]) -> Result<Vec<Subsystem>> {
        let cmd = Command::new("idle").args(subsystems.iter().map(Subsystem::as_str));
        self.send(cmd.line()).await?;
        let pairs = self.read_response().await?;
        Ok(pairs
            .into_iter()
            .filter(|(k, _)| k == "changed")
            .map(|(_, v)| Subsystem::from_name(&v))
            .collect())
    }

    async fn send(&mut self, text: &str) -> Result<()> {
        let mut buf = Vec::with_capacity(text.len() + 1);
        buf.extend_from_slice(text.as_bytes());
        buf.push(b'\n');
        self.writer.write_all(&buf).await?;
        Ok(())
    }

    async fn read_line(&mut self) -> Result<String> {
        let mut buf = Vec::new();
        let n = self.reader.read_until(b'\n', &mut buf).await?;
        if n == 0 || buf.last() != Some(&b'\n') {
            return Err(Error::Closed);
        }
        buf.pop();
        // Tags with broken encodings must not kill the whole response.
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    async fn read_response(&mut self) -> Result<Vec<Pair>> {
        Ok(self.read_response_full().await?.0)
    }

    async fn read_response_full(&mut self) -> Result<(Vec<Pair>, Vec<u8>)> {
        let mut pairs = Vec::new();
        let mut binary = Vec::new();
        loop {
            let line = self.read_line().await?;
            if line == "OK" {
                return Ok((pairs, binary));
            }
            if line.starts_with("ACK ") {
                return Err(Ack::parse(&line)
                    .map(Error::Ack)
                    .unwrap_or_else(|| Error::Protocol(line)));
            }
            if line == "list_OK" {
                continue;
            }
            match parse_pair(&line) {
                Some((k, v)) if k == "binary" => {
                    let n: usize = v
                        .parse()
                        .map_err(|_| Error::Protocol(format!("bad binary size {v:?}")))?;
                    let start = binary.len();
                    binary.resize(start + n, 0);
                    self.reader.read_exact(&mut binary[start..]).await?;
                    // The payload is followed by a single newline.
                    if !self.read_line().await?.is_empty() {
                        return Err(Error::Protocol("missing newline after binary".into()));
                    }
                }
                Some(p) => pairs.push(p),
                None => return Err(Error::Protocol(format!("unexpected line {line:?}"))),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_detection() {
        assert!(!ConnectConfig::new("localhost", 6600).is_remote());
        assert!(!ConnectConfig::new("127.0.0.1", 6600).is_remote());
        assert!(!ConnectConfig::new("::1", 6600).is_remote());
        assert!(ConnectConfig::new("192.0.2.11", 6600).is_remote());
        assert!(ConnectConfig::new("nas.local", 6600).is_remote());
    }

    #[test]
    fn empty_password_is_none() {
        assert_eq!(ConnectConfig::default().with_password("").password, None);
    }
}
