use std::{
    env, io,
    net::{Ipv4Addr, SocketAddrV4},
    path::Path,
    time::{Duration, Instant},
};

use futures_util::{StreamExt, stream};
use reqwest::{Client, Method, Url, redirect::Policy};
use salvo::{
    http::{ParseError, header},
    prelude::*,
    serve_static::{StaticDir, StaticFile},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

mod exchange;
use exchange::{Exchange, TimelineEvent};

const PROXY_MEDIA_TYPE: &str = "application/vnd.http-client.proxy";
const MAX_REQUEST_BYTES: usize = 64 * 1024 * 1024;
const DEFAULT_PORT: u16 = 8080;

#[derive(Deserialize)]
struct OutboundRequest {
    url: String,
    method: String,
    headers: String,
    has_body: bool,
    timeout_ms: u64,
    follow_redirects: bool,
}

#[derive(Serialize)]
struct UpstreamResponse {
    status: u16,
    status_text: &'static str,
    url: String,
    headers: Vec<(String, String)>,
    timeline: Vec<TimelineEvent>,
}

struct Proxy {
    port: u16,
    client: Client,
}

#[handler]
impl Proxy {
    async fn handle(&self, req: &mut Request, res: &mut Response) {
        if let Err((status, message)) = self.forward(req, res).await {
            res.status_code(status);
            res.render(Json(json!({ "error": message })));
        }
    }
}

type ProxyError = (StatusCode, String);

fn bad_request(message: impl Into<String>) -> ProxyError {
    (StatusCode::BAD_REQUEST, message.into())
}

impl Proxy {
    async fn forward(&self, req: &mut Request, res: &mut Response) -> Result<(), ProxyError> {
        let host = req
            .headers()
            .get(header::HOST)
            .and_then(|value| value.to_str().ok());
        let origin = req.headers().get(header::ORIGIN);
        let local_request = ["127.0.0.1", "localhost"].into_iter().any(|hostname| {
            let authority = if self.port == 80 {
                hostname.to_owned()
            } else {
                format!("{hostname}:{}", self.port)
            };
            host == Some(authority.as_str())
                && origin.is_none_or(|value| {
                    value.as_bytes() == format!("http://{authority}").as_bytes()
                })
        });
        if !local_request {
            return Err((
                StatusCode::FORBIDDEN,
                "The proxy accepts requests from its local client only".into(),
            ));
        }
        if req
            .headers()
            .get(header::CONTENT_TYPE)
            .is_none_or(|value| value != PROXY_MEDIA_TYPE)
        {
            return Err((
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Invalid proxy request content type".into(),
            ));
        }

        let packet =
            req.payload_with_max_size(MAX_REQUEST_BYTES)
                .await
                .map_err(|error| match error {
                    ParseError::PayloadTooLarge => (
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "The proxy request exceeds 64 MiB".into(),
                    ),
                    _ => bad_request("Could not read the proxy request"),
                })?;
        // The packet is a big-endian JSON length, metadata, then raw body bytes.
        let (length, data) = packet
            .split_first_chunk::<4>()
            .ok_or_else(|| bad_request("Missing proxy metadata"))?;
        let length = u32::from_be_bytes(*length) as usize;
        let metadata = data
            .get(..length)
            .ok_or_else(|| bad_request("Incomplete proxy metadata"))?;
        let request: OutboundRequest = serde_json::from_slice(metadata)
            .map_err(|error| bad_request(format!("Invalid proxy metadata: {error}")))?;
        let url = Url::parse(&request.url)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https") && url.has_host())
            .ok_or_else(|| bad_request("Enter an HTTP or HTTPS URL"))?;
        let method = Method::from_bytes(request.method.as_bytes())
            .map_err(|_| bad_request("Invalid HTTP method"))?;
        if method == Method::CONNECT {
            return Err(bad_request("CONNECT tunnels are unavailable"));
        }
        let exchange = Exchange {
            method,
            url,
            headers: request_headers(&request.headers)?,
            body: request.has_body.then(|| packet.slice(4 + length..)),
            timeout: (request.timeout_ms > 0)
                .then(|| Duration::from_millis(request.timeout_ms.min(999_000))),
            follow_redirects: request.follow_redirects,
        };
        let start = Instant::now();
        let mut timeline = Vec::new();
        let upstream = match exchange.send(&self.client, start, &mut timeline).await {
            Ok(response) => response,
            Err((status, message)) => {
                res.status_code(status);
                res.render(Json(json!({ "error": message, "timeline": timeline })));
                return Ok(());
            }
        };
        let status = upstream.status();
        let metadata = serde_json::to_vec(&UpstreamResponse {
            status: status.as_u16(),
            status_text: status.canonical_reason().unwrap_or_default(),
            url: upstream.url().to_string(),
            headers: upstream
                .headers()
                .iter()
                .map(|(name, value)| {
                    (
                        name.to_string(),
                        String::from_utf8_lossy(value.as_bytes()).into_owned(),
                    )
                })
                .collect(),
            timeline,
        })
        .expect("Response metadata can be serialized");
        let mut prefix = Vec::with_capacity(4 + metadata.len());
        prefix.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
        prefix.extend_from_slice(&metadata);

        res.status_code(StatusCode::OK);
        res.headers_mut().insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static(PROXY_MEDIA_TYPE),
        );
        res.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-store"),
        );
        // The final eight bytes carry the upstream completion time in microseconds.
        let done = stream::once(async move {
            let elapsed = start.elapsed().as_micros() as u64;
            Ok(bytes::Bytes::copy_from_slice(&elapsed.to_be_bytes()))
        });
        res.stream(
            stream::once(async { Ok(prefix.into()) })
                .chain(upstream.bytes_stream())
                .chain(done),
        );
        Ok(())
    }
}

fn request_headers(lines: &str) -> Result<header::HeaderMap, ProxyError> {
    let mut headers = header::HeaderMap::new();
    for line in lines.lines().filter(|line| !line.trim().is_empty()) {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| bad_request("Expected an HTTP header name and value"))?;
        let name = header::HeaderName::from_bytes(name.trim().as_bytes())
            .map_err(|_| bad_request("Invalid HTTP header name"))?;
        let value = header::HeaderValue::from_str(value.trim())
            .map_err(|_| bad_request(format!("Invalid value for HTTP header {name}")))?;
        headers.append(name, value);
    }
    let connection_headers: Vec<_> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|name| header::HeaderName::try_from(name.trim()).ok())
        .collect();
    for name in connection_headers {
        headers.remove(name);
    }
    for name in [
        "connection",
        "content-length",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "proxy-connection",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ] {
        headers.remove(name);
    }
    Ok(headers)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = env::args()
        .nth(1)
        .map_or(Ok(DEFAULT_PORT), |value| value.parse::<u16>())?;
    if port == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Port must be nonzero").into());
    }
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dist");
    let index = dist.join("index.html");
    if !index.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Build http_client/dist with npm run build:http-client first",
        )
        .into());
    }
    let proxy = Proxy {
        port,
        client: Client::builder().redirect(Policy::none()).build()?,
    };
    let router = Router::new()
        .get(StaticFile::new(index))
        .push(Router::with_path("proxy").post(proxy))
        .push(Router::with_path("{**path}").get(StaticDir::new(dist)));
    let acceptor = TcpListener::new(address).try_bind().await?;
    println!("HTTP Client listening on http://{address}");
    Server::new(acceptor).serve(router).await;
    Ok(())
}
