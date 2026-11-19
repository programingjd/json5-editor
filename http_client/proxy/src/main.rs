use std::{
    env, io,
    net::{Ipv4Addr, SocketAddrV4},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use async_compression::tokio::bufread::{BrotliDecoder, GzipDecoder, ZlibDecoder, ZstdDecoder};
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use futures_util::stream;
use http::{Method, StatusCode, header};
use pingora::{
    apps::{HttpServerApp, ReusedHttpStream},
    connectors::http::Connector,
    protocols::http::ServerSession,
    server::{Server, ShutdownWatch, configuration::Opt},
    services::listening::Service,
};
use pingora_http::ResponseHeader;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::{AsyncBufRead, AsyncReadExt, BufReader};
use tokio_util::io::StreamReader;
use url::Url;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    https_upgrade_url: Option<String>,
}

struct Proxy {
    port: u16,
    connector: Arc<Connector>,
    dist: PathBuf,
}

#[async_trait]
impl HttpServerApp for Proxy {
    async fn process_new_http(
        self: &Arc<Self>,
        mut session: ServerSession,
        _: &ShutdownWatch,
    ) -> Option<ReusedHttpStream> {
        if !session.read_request().await.ok()? {
            return None;
        }
        // Each browser request contains a single exchange. Close incomplete uploads too.
        session.set_keepalive(None);
        let request = session.req_header();
        let result = match (request.uri.path(), &request.method) {
            ("/proxy", &Method::POST) => self.forward(&mut session).await,
            ("/proxy", _) => {
                write_json(
                    &mut session,
                    StatusCode::METHOD_NOT_ALLOWED,
                    json!({
                        "error": "Use POST /proxy"
                    }),
                )
                .await
            }
            (_, &Method::GET | &Method::HEAD) => self.serve_file(&mut session).await,
            _ => {
                write_json(
                    &mut session,
                    StatusCode::METHOD_NOT_ALLOWED,
                    json!({
                        "error": "Method not allowed"
                    }),
                )
                .await
            }
        };
        result.ok()?;
        let _ = session.finish().await;
        None
    }
}

type ProxyError = (StatusCode, String);

fn bad_request(message: impl Into<String>) -> ProxyError {
    (StatusCode::BAD_REQUEST, message.into())
}

impl Proxy {
    async fn serve_file(&self, session: &mut ServerSession) -> pingora::Result<()> {
        let request = session.req_header();
        let head = request.method == Method::HEAD;
        let path = request.uri.path().trim_start_matches('/');
        let path = if path.is_empty() { "index.html" } else { path };
        let path = percent_encoding::percent_decode_str(path)
            .decode_utf8()
            .ok();
        let Some(path) = path.filter(|path| {
            !path.contains(['\\', ':', '\0'])
                && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
        }) else {
            return write_json(
                session,
                StatusCode::NOT_FOUND,
                json!({ "error": "Not found" }),
            )
            .await;
        };
        let Ok(path) = tokio::fs::canonicalize(self.dist.join(path.as_ref())).await else {
            return write_json(
                session,
                StatusCode::NOT_FOUND,
                json!({ "error": "Not found" }),
            )
            .await;
        };
        if !path.starts_with(&self.dist) {
            return write_json(
                session,
                StatusCode::NOT_FOUND,
                json!({ "error": "Not found" }),
            )
            .await;
        }
        let Ok(mut file) = tokio::fs::File::open(&path).await else {
            return write_json(
                session,
                StatusCode::NOT_FOUND,
                json!({ "error": "Not found" }),
            )
            .await;
        };
        let length = match file.metadata().await {
            Ok(metadata) if metadata.is_file() => metadata.len(),
            _ => {
                return write_json(
                    session,
                    StatusCode::NOT_FOUND,
                    json!({ "error": "Not found" }),
                )
                .await;
            }
        };
        let mut headers = ResponseHeader::build(StatusCode::OK, Some(3))?;
        headers.insert_header(
            "Content-Type",
            mime_guess::from_path(&path)
                .first_or_octet_stream()
                .as_ref(),
        )?;
        headers.insert_header("Content-Length", length.to_string())?;
        if path == self.dist.join("index.html") {
            headers.insert_header("Cache-Control", "no-cache")?;
        }
        session.write_response_header(Box::new(headers)).await?;
        if !head {
            let mut buffer = vec![0; 64 * 1024];
            let mut remaining = length;
            while remaining > 0 {
                let size = remaining.min(buffer.len() as u64) as usize;
                let read = file.read(&mut buffer[..size]).await.map_err(io_error)?;
                if read == 0 {
                    return Err(io_error(io::Error::from(io::ErrorKind::UnexpectedEof)));
                }
                remaining -= read as u64;
                session
                    .write_response_body(Bytes::copy_from_slice(&buffer[..read]), remaining == 0)
                    .await?;
            }
        }
        Ok(())
    }

    async fn forward(&self, session: &mut ServerSession) -> pingora::Result<()> {
        let exchange = match self.read_exchange(session).await {
            Ok(exchange) => exchange,
            Err((status, error)) => {
                return write_json(session, status, json!({ "error": error })).await;
            }
        };
        let timeout = exchange.timeout;
        let head = exchange.method == Method::HEAD;
        let start = Instant::now();
        let mut timeline = Vec::new();
        let (upstream, peer, url, https_upgrade_url) =
            match exchange.send(&self.connector, start, &mut timeline).await {
                Ok(response) => response,
                Err((status, error)) => {
                    return write_json(
                        session,
                        status,
                        json!({ "error": error, "timeline": timeline }),
                    )
                    .await;
                }
            };
        let response = upstream
            .response_header()
            .expect("Response headers were read");
        let status = response.status;
        let encoding = response
            .headers
            .get_all(header::CONTENT_ENCODING)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','))
            .map(|value| value.trim().to_ascii_lowercase())
            .collect::<Vec<_>>();
        let stringify = |name: &[u8], value: &header::HeaderValue| {
            (
                String::from_utf8_lossy(name).into_owned(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        };
        let headers = if response.has_case() {
            response
                .case_header_iter()
                .map(|(name, value)| stringify(name.as_slice(), value))
                .collect()
        } else {
            response
                .headers
                .iter()
                .map(|(name, value)| stringify(name.as_str().as_bytes(), value))
                .collect()
        };
        let metadata = serde_json::to_vec(&UpstreamResponse {
            status: status.as_u16(),
            status_text: status.canonical_reason().unwrap_or_default(),
            url: url.into(),
            headers,
            timeline,
            https_upgrade_url: https_upgrade_url.map(Into::into),
        })
        .expect("Response metadata can be serialized");
        let mut prefix = Vec::with_capacity(4 + metadata.len());
        prefix.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
        prefix.extend_from_slice(&metadata);

        let no_body = head
            || matches!(status.as_u16(), 204 | 304)
            || response
                .headers
                .get(header::CONTENT_LENGTH)
                .is_some_and(|value| value == "0");
        let connector = self.connector.clone();
        let chunks = stream::try_unfold(Some(upstream), move |state| {
            let peer = peer.clone();
            let connector = connector.clone();
            async move {
                let Some(mut upstream) = state else {
                    return Ok::<_, io::Error>(None);
                };
                match upstream
                    .read_response_body()
                    .await
                    .map_err(io::Error::other)?
                {
                    Some(chunk) => Ok(Some((chunk, Some(upstream)))),
                    None => {
                        connector
                            .release_http_session(upstream, &peer, Some(Duration::from_secs(30)))
                            .await;
                        Ok(None)
                    }
                }
            }
        });
        let chunks = StreamReader::new(Box::pin(chunks));
        let mut body: Box<dyn AsyncBufRead + Send + Unpin> = Box::new(chunks);
        if !no_body
            && !encoding.is_empty()
            && encoding.iter().all(|name| {
                matches!(
                    name.as_str(),
                    "identity" | "gzip" | "x-gzip" | "br" | "deflate" | "zstd"
                )
            })
        {
            for encoding in encoding.iter().rev() {
                body = match encoding.as_str() {
                    "gzip" | "x-gzip" => Box::new(BufReader::new(GzipDecoder::new(body))),
                    "br" => Box::new(BufReader::new(BrotliDecoder::new(body))),
                    "zstd" => Box::new(BufReader::new(ZstdDecoder::new(body))),
                    "deflate" => Box::new(BufReader::new(ZlibDecoder::new(body))),
                    _ => body,
                };
            }
        }
        let mut header = ResponseHeader::build(StatusCode::OK, Some(3))?;
        header.insert_header("Content-Type", PROXY_MEDIA_TYPE)?;
        header.insert_header("Cache-Control", "no-store")?;
        header.insert_header("Transfer-Encoding", "chunked")?;
        session.write_response_header(Box::new(header)).await?;
        session.write_response_body(prefix.into(), false).await?;
        let mut buffer = vec![0; 64 * 1024];
        loop {
            let read = if let Some(timeout) = timeout {
                tokio::time::timeout(
                    timeout.saturating_sub(start.elapsed()),
                    body.read(&mut buffer),
                )
                .await
                .map_err(|_| io_error(io::Error::from(io::ErrorKind::TimedOut)))?
            } else {
                body.read(&mut buffer).await
            }
            .map_err(io_error)?;
            if read == 0 {
                break;
            }
            session
                .write_response_body(Bytes::copy_from_slice(&buffer[..read]), false)
                .await?;
        }
        let elapsed = start.elapsed().as_micros() as u64;
        session
            .write_response_body(Bytes::copy_from_slice(&elapsed.to_be_bytes()), true)
            .await
    }

    async fn read_exchange(&self, session: &mut ServerSession) -> Result<Exchange, ProxyError> {
        let req = session.req_header();
        let host = req
            .headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok());
        let origin = req.headers.get(header::ORIGIN);
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
            .headers
            .get(header::CONTENT_TYPE)
            .is_none_or(|value| value != PROXY_MEDIA_TYPE)
        {
            return Err((
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Invalid proxy request content type".into(),
            ));
        }

        let mut packet = BytesMut::new();
        while let Some(chunk) = session
            .read_request_body()
            .await
            .map_err(|_| bad_request("Could not read the proxy request"))?
        {
            if chunk.len() > MAX_REQUEST_BYTES - packet.len() {
                return Err((
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "The proxy request exceeds 64 MiB".into(),
                ));
            }
            packet.extend_from_slice(&chunk);
        }
        let packet = packet.freeze();
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
        Ok(Exchange {
            method,
            url,
            headers: request_headers(&request.headers)?,
            body: request.has_body.then(|| packet.slice(4 + length..)),
            timeout: (request.timeout_ms > 0)
                .then(|| Duration::from_millis(request.timeout_ms.min(999_000))),
            follow_redirects: request.follow_redirects,
        })
    }
}

fn io_error(error: io::Error) -> Box<pingora::Error> {
    pingora::Error::because(
        pingora::ErrorType::InternalError,
        "HTTP body I/O failed",
        error,
    )
}

async fn write_json(
    session: &mut ServerSession,
    status: StatusCode,
    value: serde_json::Value,
) -> pingora::Result<()> {
    let body = value.to_string();
    let mut header = ResponseHeader::build(status, Some(2))?;
    header.insert_header("Content-Type", "application/json")?;
    header.insert_header("Content-Length", body.len().to_string())?;
    session.write_response_header(Box::new(header)).await?;
    if session.req_header().method != Method::HEAD {
        session.write_response_body(body.into(), true).await?;
    }
    Ok(())
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    let mut server = Server::new(Some(Opt::default()))?;
    server.bootstrap();
    let mut service = Service::new(
        "HTTP Client".into(),
        Proxy {
            port,
            connector: Arc::new(Connector::new(None)),
            dist: dist.canonicalize()?,
        },
    );
    service.add_tcp(&address.to_string());
    server.add_service(service);
    println!("HTTP Client listening on http://{address}");
    server.run_forever()
}
