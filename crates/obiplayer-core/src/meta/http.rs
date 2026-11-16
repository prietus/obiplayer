//! HTTP client shared by metadata services.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use reqwest::StatusCode;
use tokio::sync::Mutex;

/// Identifies us to services (MusicBrainz requires a meaningful agent).
pub const USER_AGENT: &str = "ObiPlayer/0.1 ( https://github.com/prietus/obiplayer )";

/// Services with a request-rate policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Service {
    /// 1 request/second (MusicBrainz policy).
    MusicBrainz,
    /// 60/minute authenticated.
    Discogs,
    /// No explicit limit; be gentle anyway.
    Other,
}

impl Service {
    fn interval(self) -> Duration {
        match self {
            Service::MusicBrainz => Duration::from_millis(1100),
            Service::Discogs => Duration::from_millis(1000),
            Service::Other => Duration::ZERO,
        }
    }
}

pub struct Http {
    client: reqwest::Client,
    last: Mutex<HashMap<Service, Instant>>,
}

impl Default for Http {
    fn default() -> Self {
        Http::new()
    }
}

impl Http {
    pub fn new() -> Http {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(8))
            .build()
            .expect("http client");
        Http {
            client,
            last: Mutex::new(HashMap::new()),
        }
    }

    /// Wait until a request to `service` is allowed by its rate policy.
    pub async fn throttle(&self, service: Service) {
        let interval = service.interval();
        if interval.is_zero() {
            return;
        }
        let mut last = self.last.lock().await;
        if let Some(t) = last.get(&service) {
            let wait = interval.saturating_sub(t.elapsed());
            if !wait.is_zero() {
                tokio::time::sleep(wait).await;
            }
        }
        last.insert(service, Instant::now());
    }

    async fn send(
        &self,
        service: Service,
        url: &str,
        query: &[(&str, &str)],
        accept_json: bool,
    ) -> Option<reqwest::Response> {
        for attempt in 0..2 {
            self.throttle(service).await;
            let mut req = self.client.get(url).query(query);
            if accept_json {
                req = req.header(reqwest::header::ACCEPT, "application/json");
            }
            let res = req.send().await;
            // MusicBrainz answers 503 when busy; one polite retry.
            if attempt == 0
                && let Ok(r) = &res
                && r.status() == StatusCode::SERVICE_UNAVAILABLE
                && service == Service::MusicBrainz
            {
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            return Self::accept(url, res);
        }
        None
    }

    fn accept(url: &str, res: reqwest::Result<reqwest::Response>) -> Option<reqwest::Response> {
        match res {
            Ok(r) if r.status() == StatusCode::OK => Some(r),
            Ok(r) => {
                tracing::debug!(%url, status = %r.status(), "metadata request failed");
                None
            }
            Err(e) => {
                // The full URL can carry API keys in its query: never log it.
                tracing::debug!(%url, error = %e.without_url(), "metadata request error");
                None
            }
        }
    }

    /// GET and return the body text (HTTP 200 only).
    pub async fn get_text(
        &self,
        service: Service,
        url: &str,
        query: &[(&str, &str)],
    ) -> Option<String> {
        self.send(service, url, query, false)
            .await?
            .text()
            .await
            .ok()
    }

    /// GET JSON (HTTP 200 only). Returns the raw text too, for caching.
    pub async fn get_json(
        &self,
        service: Service,
        url: &str,
        query: &[(&str, &str)],
    ) -> Option<(serde_json::Value, String)> {
        let text = self
            .send(service, url, query, true)
            .await?
            .text()
            .await
            .ok()?;
        let v = serde_json::from_str(&text).ok()?;
        Some((v, text))
    }

    /// GET binary content (images), following redirects.
    pub async fn get_bytes(&self, service: Service, url: &str) -> Option<Vec<u8>> {
        let r = self.send(service, url, &[], false).await?;
        r.bytes().await.ok().map(|b| b.to_vec())
    }

    /// Raw status for credential checks (`None` = network error).
    pub async fn status(&self, service: Service, url: &str, query: &[(&str, &str)]) -> Option<u16> {
        self.throttle(service).await;
        self.client
            .get(url)
            .query(query)
            .send()
            .await
            .ok()
            .map(|r| r.status().as_u16())
    }
}
