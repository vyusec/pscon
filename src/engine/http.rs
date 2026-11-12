use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::core::{Config, Scope};
use anyhow::{Context, Result, bail};
use bytes::Bytes;
use futures_util::StreamExt;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue, LOCATION, USER_AGENT};
use reqwest::{Client, Method};
use tokio::sync::Mutex;
use tracing::{debug, warn};
use url::Url;

#[derive(Debug, Clone)]
pub struct Fetched {
    pub url: Url,
    pub final_url: Url,
    pub status: u16,
    pub headers: HeaderMap,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    pub duration_ms: u64,
    pub redirects: Vec<String>,
}

impl Fetched {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
}

pub struct HttpClient {
    client: Client,
    scope: Arc<Scope>,
    config: Config,
    limiter: HostRateLimiter,
    jar: Arc<reqwest::cookie::Jar>,
}

impl HttpClient {
    pub fn new(config: Config, scope: Arc<Scope>) -> Result<Self> {
        let jar = Arc::new(reqwest::cookie::Jar::default());
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&config.general.user_agent)
                .unwrap_or_else(|_| HeaderValue::from_static("pscon/0.1")),
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/json,text/javascript,*/*;q=0.8",
            ),
        );
        let client = Client::builder()
            .default_headers(headers)
            .cookie_provider(jar.clone())
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(config.general.timeout_secs))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .context("building HTTP client")?;
        let limiter =
            HostRateLimiter::new(Duration::from_millis(config.recon.rate_limit_per_host_ms));
        Ok(Self {
            client,
            scope,
            config,
            limiter,
            jar,
        })
    }

    pub fn add_cookie(&self, raw: &str) {
        // reqwest's Jar expects a URL; callers that pass a raw Cookie header should
        // use request_headers instead. This helper is retained for future use.
        let _ = (raw, &self.jar);
    }

    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    pub async fn fetch(
        &self,
        url: &Url,
        method: Method,
        extra_headers: Option<HeaderMap>,
        body: Option<Bytes>,
    ) -> Result<Fetched> {
        self.scope.check(url)?;
        self.limiter.wait(url).await;
        let started = Instant::now();
        let mut current = url.clone();
        let mut redirects = Vec::new();

        for _ in 0..=self.config.recon.max_redirects {
            let mut req = self.client.request(method.clone(), current.clone());
            if let Some(headers) = &extra_headers {
                req = req.headers(headers.clone());
            }
            if let Some(body) = &body {
                req = req.body(body.clone());
            }
            let resp = req
                .send()
                .await
                .with_context(|| format!("requesting {current}"))?;
            let status = resp.status().as_u16();

            if (300..400).contains(&status)
                && let Some(location) = resp.headers().get(LOCATION)
            {
                let location = location.to_str().context("non-UTF8 redirect location")?;
                let next = current
                    .join(location)
                    .with_context(|| format!("resolving redirect {location}"))?;
                if !self.scope.is_allowed(&next) {
                    bail!("redirect out of scope: {next}");
                }
                redirects.push(next.to_string());
                current = next;
                continue;
            }

            let headers = resp.headers().clone();
            let content_type = headers
                .get(CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.split(';').next().unwrap_or(s).trim().to_string());
            let body = read_capped(resp, self.config.general.max_body_bytes).await?;
            debug!(
                url = %url,
                final_url = %current,
                status,
                bytes = body.len(),
                "fetched"
            );
            return Ok(Fetched {
                url: url.clone(),
                final_url: current,
                status,
                headers,
                content_type,
                body,
                duration_ms: started.elapsed().as_millis() as u64,
                redirects,
            });
        }

        bail!("too many redirects for {url}")
    }

    pub async fn get(&self, url: &Url) -> Result<Fetched> {
        self.fetch(url, Method::GET, None, None).await
    }

    pub async fn head(&self, url: &Url) -> Result<Fetched> {
        self.fetch(url, Method::HEAD, None, None).await
    }

    pub async fn options(&self, url: &Url) -> Result<Fetched> {
        self.fetch(url, Method::OPTIONS, None, None).await
    }
}

async fn read_capped(resp: reqwest::Response, max: usize) -> Result<Vec<u8>> {
    let mut stream = resp.bytes_stream();
    let mut out = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if out.len() + chunk.len() > max {
            let remaining = max.saturating_sub(out.len());
            out.extend_from_slice(&chunk[..remaining]);
            warn!(max, "response body truncated");
            break;
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

struct HostRateLimiter {
    delay: Duration,
    last: Mutex<HashMap<String, Instant>>,
}

impl HostRateLimiter {
    fn new(delay: Duration) -> Self {
        Self {
            delay,
            last: Mutex::new(HashMap::new()),
        }
    }

    async fn wait(&self, url: &Url) {
        if self.delay.is_zero() {
            return;
        }
        let host = url.host_str().unwrap_or_default().to_string();
        let wait_until = {
            let guard = self.last.lock().await;
            guard
                .get(&host)
                .map(|last| *last + self.delay)
                .unwrap_or_else(Instant::now)
        };
        let now = Instant::now();
        if wait_until > now {
            tokio::time::sleep(wait_until - now).await;
        }
        let mut guard = self.last.lock().await;
        guard.insert(host, Instant::now());
    }
}
