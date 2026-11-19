use std::{
    convert::Infallible,
    pin::Pin,
    sync::{Arc, OnceLock},
    task::{Context, Poll},
    time::{Duration, Instant},
};

use bytes::Bytes;
use http_body::{Body, Frame, SizeHint};
use http_body_util::Full;
use reqwest::{Client, Method, Response, StatusCode, Url, header};
use serde::Serialize;

#[derive(Serialize)]
pub(super) struct TimelineEvent {
    label: String,
    elapsed_ms: f64,
}

impl TimelineEvent {
    fn new(label: impl Into<String>, start: Instant, time: Instant) -> Self {
        Self {
            label: label.into(),
            elapsed_ms: time.duration_since(start).as_secs_f64() * 1000.0,
        }
    }
}

struct Upload {
    body: Full<Bytes>,
    sent: Arc<OnceLock<Instant>>,
}

impl Body for Upload {
    type Data = Bytes;
    type Error = Infallible;

    fn size_hint(&self) -> SizeHint {
        self.body.size_hint()
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let frame = Pin::new(&mut self.body).poll_frame(cx);
        if frame.is_ready() && self.body.is_end_stream() {
            let _ = self.sent.set(Instant::now());
        }
        frame
    }
}

pub(super) struct Exchange {
    pub(super) method: Method,
    pub(super) url: Url,
    pub(super) headers: header::HeaderMap,
    pub(super) body: Option<Bytes>,
    pub(super) timeout: Option<Duration>,
    pub(super) follow_redirects: bool,
}

impl Exchange {
    pub(super) async fn send(
        mut self,
        client: &Client,
        start: Instant,
        timeline: &mut Vec<TimelineEvent>,
    ) -> Result<Response, (StatusCode, String)> {
        let mut redirects = 0;
        loop {
            let sending = Instant::now();
            timeline.push(TimelineEvent::new(
                format!("sending {} request to {}", self.method, self.url),
                start,
                sending,
            ));
            let sent = Arc::new(OnceLock::new());
            let mut request = client
                .request(self.method.clone(), self.url.clone())
                .headers(self.headers.clone());
            if let Some(body) = self.body.clone() {
                if !body.is_empty() {
                    request = request.body(reqwest::Body::wrap(Upload {
                        body: Full::new(body),
                        sent: sent.clone(),
                    }));
                } else {
                    request = request.body(body);
                }
            }
            if let Some(timeout) = self.timeout {
                let remaining = timeout.saturating_sub(start.elapsed());
                if remaining.is_zero() {
                    return Err(timeout_error());
                }
                request = request.timeout(remaining);
            }
            let dispatched = Instant::now();
            let response = request.send().await;
            let received = Instant::now();
            let end = sent.get().copied().unwrap_or(dispatched);
            let upload_incomplete =
                sent.get().is_none() && self.body.as_ref().is_some_and(|body| !body.is_empty());
            if !upload_incomplete {
                timeline.push(TimelineEvent::new("request sent", start, end));
            }
            let mut response = response.map_err(upstream_error)?;
            timeline.push(TimelineEvent::new("receiving response", start, received));
            let status = response.status();
            if !self.follow_redirects || !matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308) {
                return Ok(response);
            }
            let destination = response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|location| self.url.join(location).ok());
            let Some(destination) = destination else {
                return Ok(response);
            };
            if redirects == 10 || !matches!(destination.scheme(), "http" | "https") {
                let message = if redirects == 10 {
                    "Too many redirects"
                } else {
                    "Redirect requires an HTTP or HTTPS URL"
                };
                return Err((StatusCode::BAD_GATEWAY, message.into()));
            }
            while response.chunk().await.map_err(upstream_error)?.is_some() {}
            timeline.push(TimelineEvent::new(
                format!("{} response received", status.as_u16()),
                start,
                Instant::now(),
            ));
            redirects += 1;
            self.redirect(status, destination);
        }
    }

    fn redirect(&mut self, status: StatusCode, destination: Url) {
        if self.url.origin() != destination.origin() {
            for name in [
                header::AUTHORIZATION,
                header::COOKIE,
                header::HOST,
                header::REFERER,
                header::ORIGIN,
            ] {
                self.headers.remove(name);
            }
        }
        let change_to_get = match status.as_u16() {
            301 | 302 => self.method == Method::POST,
            303 => self.method != Method::GET && self.method != Method::HEAD,
            _ => false,
        };
        if change_to_get {
            self.method = Method::GET;
            self.body = None;
            for name in [
                header::CONTENT_TYPE,
                header::CONTENT_ENCODING,
                header::CONTENT_LANGUAGE,
                header::CONTENT_LOCATION,
            ] {
                self.headers.remove(name);
            }
        }
        self.url = destination;
    }
}

fn upstream_error(error: reqwest::Error) -> (StatusCode, String) {
    if error.is_timeout() {
        timeout_error()
    } else {
        (
            StatusCode::BAD_GATEWAY,
            format!("Upstream request failed: {}", error.without_url()),
        )
    }
}

fn timeout_error() -> (StatusCode, String) {
    (
        StatusCode::GATEWAY_TIMEOUT,
        "The upstream request timed out".into(),
    )
}
