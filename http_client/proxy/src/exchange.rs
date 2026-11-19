use std::time::{Duration, Instant};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use bytes::Bytes;
use http::{Method, StatusCode, header};
use percent_encoding::percent_decode_str;
use pingora::{
    connectors::http::Connector, protocols::http::client::HttpSession, upstreams::peer::HttpPeer,
};
use pingora_http::RequestHeader;
use serde::Serialize;
use url::{Position, Url};

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

pub(super) type ExchangeError = (StatusCode, String);

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
        self,
        connector: &Connector,
        start: Instant,
        timeline: &mut Vec<TimelineEvent>,
    ) -> Result<(HttpSession, HttpPeer, Url, Option<Url>), ExchangeError> {
        if let Some(timeout) = self.timeout {
            tokio::time::timeout(timeout, self.run(connector, start, timeline))
                .await
                .map_err(|_| timeout_error())?
        } else {
            self.run(connector, start, timeline).await
        }
    }

    async fn run(
        mut self,
        connector: &Connector,
        start: Instant,
        timeline: &mut Vec<TimelineEvent>,
    ) -> Result<(HttpSession, HttpPeer, Url, Option<Url>), ExchangeError> {
        let mut redirects = 0;
        let original_http_host = (self.url.scheme() == "http")
            .then(|| self.url.host_str().map(str::to_owned))
            .flatten();
        let mut https_upgrade_url = None;
        loop {
            let sending = Instant::now();
            timeline.push(TimelineEvent::new(
                format!("sending {} request to {}", self.method, self.url),
                start,
                sending,
            ));
            let (response, peer) = self.send_once(connector, start, timeline).await?;
            let response_header = response
                .response_header()
                .expect("Response headers were read");
            let status = response_header.status;
            if !matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308) {
                return Ok((response, peer, self.url, https_upgrade_url));
            }
            let destination = response_header
                .headers
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|location| self.url.join(location).ok());
            let Some(destination) = destination else {
                return Ok((response, peer, self.url, https_upgrade_url));
            };
            // Keep the original host even when a later redirect leads elsewhere.
            if https_upgrade_url.is_none()
                && self.url.scheme() == "http"
                && let Some(host) = original_http_host.as_deref()
                && self.url.host_str() == Some(host)
                && destination.scheme() == "https"
                && destination.host_str() == Some(host)
            {
                https_upgrade_url = Some(destination.clone());
            }
            if !self.follow_redirects {
                return Ok((response, peer, self.url, https_upgrade_url));
            }
            if redirects == 10 || !matches!(destination.scheme(), "http" | "https") {
                let message = if redirects == 10 {
                    "Too many redirects"
                } else {
                    "Redirect requires an HTTP or HTTPS URL"
                };
                return Err((StatusCode::BAD_GATEWAY, message.into()));
            }
            let mut response = response;
            while response
                .read_response_body()
                .await
                .map_err(upstream_error)?
                .is_some()
            {}
            connector
                .release_http_session(response, &peer, Some(Duration::from_secs(30)))
                .await;
            timeline.push(TimelineEvent::new(
                format!("{} response received", status.as_u16()),
                start,
                Instant::now(),
            ));
            redirects += 1;
            self.redirect(status, destination);
        }
    }

    async fn send_once(
        &self,
        connector: &Connector,
        start: Instant,
        timeline: &mut Vec<TimelineEvent>,
    ) -> Result<(HttpSession, HttpPeer), ExchangeError> {
        let host = self.url.host_str().expect("HTTP URL has a host");
        let host = host.trim_matches(['[', ']']);
        let port = self
            .url
            .port_or_known_default()
            .expect("HTTP URL has a port");
        let addresses = tokio::net::lookup_host((host, port)).await.map_err(|_| {
            (
                StatusCode::BAD_GATEWAY,
                "Could not resolve upstream host".into(),
            )
        })?;
        let mut connection = Err((
            StatusCode::BAD_GATEWAY,
            "Could not resolve upstream host".into(),
        ));
        for address in addresses {
            let peer = HttpPeer::new(address, self.url.scheme() == "https", host.to_owned());
            match connector.get_http_session(&peer).await {
                Ok((session, _)) => {
                    connection = Ok((session, peer));
                    break;
                }
                Err(error) => connection = Err(upstream_error(error)),
            }
        }
        let (mut session, peer) = connection?;
        let path = &self.url[Position::BeforePath..Position::AfterQuery];
        let mut request = RequestHeader::build(
            self.method.clone(),
            path.as_bytes(),
            Some(self.headers.len() + 3),
        )
        .map_err(upstream_error)?;
        for (name, value) in &self.headers {
            let name = name
                .as_str()
                .split('-')
                .map(|segment| {
                    let mut segment = segment.to_owned();
                    if let Some(first) = segment.get_mut(..1) {
                        first.make_ascii_uppercase();
                    }
                    segment
                })
                .collect::<Vec<_>>()
                .join("-");
            request
                .append_header(name, value.clone())
                .map_err(upstream_error)?;
        }
        if !self.headers.contains_key(header::HOST) {
            request
                .insert_header("Host", &self.url[Position::BeforeHost..Position::AfterPort])
                .map_err(upstream_error)?;
        }
        if !self.headers.contains_key(header::ACCEPT) {
            request
                .insert_header("Accept", "*/*")
                .map_err(upstream_error)?;
        }
        if !self.headers.contains_key(header::AUTHORIZATION)
            && (!self.url.username().is_empty() || self.url.password().is_some())
        {
            let username = percent_decode_str(self.url.username()).decode_utf8_lossy();
            let password =
                percent_decode_str(self.url.password().unwrap_or_default()).decode_utf8_lossy();
            let token = STANDARD.encode(format!("{username}:{password}"));
            request
                .insert_header("Authorization", format!("Basic {token}"))
                .map_err(upstream_error)?;
        }
        if !self.headers.contains_key(header::ACCEPT_ENCODING) {
            request
                .insert_header("Accept-Encoding", "gzip, br, deflate, zstd")
                .map_err(upstream_error)?;
        }
        if let Some(body) = &self.body {
            request
                .insert_header("Content-Length", body.len().to_string())
                .map_err(upstream_error)?;
        }
        session
            .write_request_header(Box::new(request))
            .await
            .map_err(upstream_error)?;
        if let Some(body) = &self.body
            && !body.is_empty()
        {
            session
                .write_request_body(body.clone(), false)
                .await
                .map_err(upstream_error)?;
        }
        session
            .finish_request_body()
            .await
            .map_err(upstream_error)?;
        timeline.push(TimelineEvent::new("request sent", start, Instant::now()));
        loop {
            session
                .read_response_header()
                .await
                .map_err(upstream_error)?;
            let status = session
                .response_header()
                .expect("Response headers were read")
                .status;
            if !status.is_informational() || status == StatusCode::SWITCHING_PROTOCOLS {
                break;
            }
        }
        timeline.push(TimelineEvent::new(
            "receiving response",
            start,
            Instant::now(),
        ));
        Ok((session, peer))
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

fn upstream_error(error: Box<pingora::Error>) -> ExchangeError {
    (
        StatusCode::BAD_GATEWAY,
        format!("Upstream request failed: {error}"),
    )
}

pub(super) fn timeout_error() -> ExchangeError {
    (
        StatusCode::GATEWAY_TIMEOUT,
        "The upstream request timed out".into(),
    )
}
