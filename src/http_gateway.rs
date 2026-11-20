//! Native inbound HTTP Path and Subdomain Gateway.
//!
//! This module is intentionally separate from the outbound HTTP gateway client.
//! The listener binds to loopback by default.

use std::{net::SocketAddr, ops::Range, str::FromStr, sync::Arc};

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{
        header, uri::Authority, HeaderValue, Method, Request, StatusCode, Uri,
    },
    response::Response,
    routing::any,
};
use futures::StreamExt;
use ipld_core::cid::{Cid, Version};
use percent_encoding::percent_decode_str;
use tokio::net::TcpListener;

use crate::{
    Block, Ipfs, IpfsPath,
    unixfs::ll::file::{FileReadFailed, visit::IdleFileVisit},
};

/// Default local HTTP Gateway address, matching Kubo's path gateway.
pub const DEFAULT_LISTEN_ADDR: &str = "127.0.0.1:8080";

/// Run the HTTP Gateway on an already-bound listener.
///
/// The caller owns listener creation and shutdown policy. This keeps the
/// server reusable by applications and a future long-running daemon.
pub async fn serve(ipfs: Ipfs, listener: TcpListener) -> std::io::Result<()> {
    axum::serve(listener, router(ipfs)).await
}

fn router(ipfs: Ipfs) -> Router {
    Router::new()
        .route("/", any(handle_request))
        .route("/{*path}", any(handle_request))
        .with_state(Arc::new(ipfs))
}

/// Bind the default loopback listener and start serving until the task is aborted.
pub async fn serve_default(ipfs: Ipfs) -> std::io::Result<()> {
    let address: SocketAddr = DEFAULT_LISTEN_ADDR
        .parse()
        .expect("the built-in gateway address is valid");
    let listener = TcpListener::bind(address).await?;
    serve(ipfs, listener).await
}

#[derive(Clone)]
enum GatewayHost {
    Path,
    Subdomain(Cid),
}

async fn handle_request(
    State(ipfs): State<Arc<Ipfs>>,
    request: Request<Body>,
) -> Response<Body> {
    let method = request.method();
    if method != Method::GET && method != Method::HEAD {
        return response(StatusCode::METHOD_NOT_ALLOWED, "only GET and HEAD are supported");
    }

    let host = match parse_host(&request) {
        Ok(host) => host,
        Err(()) => return response(StatusCode::MISDIRECTED_REQUEST, "unrecognized gateway host"),
    };

    let (ipfs_path, trailing_slash) = match parse_request_path(request.uri().path(), host) {
        Ok(path) => path,
        Err(status) => return response(status, "invalid or unsupported IPFS path"),
    };

    // Resolve only the selected root block. UnixfsLs walks file chunks while
    // enumerating entries, which would defeat the point of ranged retrieval.
    let root_block = match resolve_unixfs_block(&ipfs, ipfs_path.clone()).await {
        Ok(block) => block,
        Err(status) => return response(status, "content not found"),
    };

    let (resolved_path, file_size) = match unixfs_file_size(&root_block) {
        Ok(size) => (ipfs_path.clone(), size),
        Err(FileReadFailed::UnexpectedType(node)) if node.is_directory() => {
            if !trailing_slash {
                return redirect_to_directory(request.uri());
            }

            let index_path = match ipfs_path.sub_path("index.html") {
                Ok(path) => path,
                Err(_) => return response(StatusCode::BAD_REQUEST, "invalid index path"),
            };
            let index_block = match resolve_unixfs_block(&ipfs, index_path.clone()).await {
                Ok(block) => block,
                Err(_) => return response(StatusCode::NOT_FOUND, "directory has no index.html"),
            };
            let size = match unixfs_file_size(&index_block) {
                Ok(size) => size,
                Err(_) => return response(StatusCode::NOT_FOUND, "directory has no index.html"),
            };
            (index_path, size)
        }
        Err(_) => return response(StatusCode::NOT_FOUND, "unsupported or missing UnixFS content"),
    };

    let selected_range = if method == Method::GET {
        match request.headers().get(header::RANGE) {
            Some(value) => match value
                .to_str()
                .ok()
                .and_then(|value| parse_single_range(value, file_size).ok())
            {
                Some(range) => Some(range),
                None => return range_not_satisfiable(file_size),
            },
            None => None,
        }
    } else {
        None
    };

    let response_length = selected_range
        .as_ref()
        .map_or(file_size, |range| range.end.saturating_sub(range.start));

    let mut builder = Response::builder()
        .status(if selected_range.is_some() {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        })
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=29030400, immutable"),
        )
        .header(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"))
        .header(header::CONTENT_TYPE, content_type(&resolved_path))
        .header(header::CONTENT_LENGTH, response_length.to_string());

    if let Some(range) = &selected_range {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!(
                "bytes {}-{}/{}",
                range.start,
                range.end.saturating_sub(1),
                file_size
            ),
        );
    }

    let body = if method == Method::HEAD {
        Body::empty()
    } else {
        let cat = ipfs.unixfs().cat(resolved_path);
        let cat = match selected_range {
            Some(range) => cat.range(range),
            None => cat,
        };
        let stream = cat.map(|chunk| {
            chunk.map_err(|error| std::io::Error::other(error.to_string()))
        });
        Body::from_stream(stream)
    };

    builder.body(body).expect("static response headers are valid")
}

async fn resolve_unixfs_block(ipfs: &Ipfs, path: IpfsPath) -> Result<Block, StatusCode> {
    let (resolved, _) = ipfs
        .dag()
        .resolve(path, true, &[], false)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    resolved
        .into_unixfs_block()
        .map_err(|_| StatusCode::NOT_FOUND)
}

fn unixfs_file_size(block: &Block) -> Result<u64, FileReadFailed> {
    if block.cid().codec() == 0x55 {
        Ok(IdleFileVisit::default().start_from_raw(block.data()).1)
    } else {
        IdleFileVisit::default()
            .start(block.data())
            .map(|(_, size, _, _)| size)
    }
}

fn parse_host(request: &Request<Body>) -> Result<GatewayHost, ()> {
    let Some(value) = request.headers().get(header::HOST) else {
        // HTTP/1.0 clients may omit Host. Such requests can only use path addressing.
        return Ok(GatewayHost::Path);
    };
    let authority = Authority::from_str(value.to_str().map_err(|_| ())?).map_err(|_| ())?;
    let host = authority.host().to_ascii_lowercase();

    if matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]") {
        return Ok(GatewayHost::Path);
    }

    let label = host.strip_suffix(".ipfs.localhost").ok_or(())?;
    if label.is_empty() || label.contains('.') {
        return Err(());
    }

    let cid = Cid::from_str(label).map_err(|_| ())?;
    // DNS subdomains use the canonical lower-case base32 CIDv1 label. Parsing
    // other multibase spellings would accept hostnames outside that format.
    if cid.version() != Version::V1 || cid.to_string() != label {
        return Err(());
    }

    Ok(GatewayHost::Subdomain(cid))
}

fn parse_request_path(
    uri_path: &str,
    host: GatewayHost,
) -> Result<(IpfsPath, bool), StatusCode> {
    let (cid, raw_path) = match host {
        GatewayHost::Path => {
            let Some(raw) = uri_path.strip_prefix("/ipfs/") else {
                return Err(StatusCode::NOT_FOUND);
            };
            let (segments, trailing_slash) =
                decode_path_segments(raw).map_err(|_| StatusCode::BAD_REQUEST)?;
            let Some(cid) = segments.first() else {
                return Err(StatusCode::BAD_REQUEST);
            };
            let parsed = Cid::from_str(cid).map_err(|_| StatusCode::BAD_REQUEST)?;
            let tail = segments.iter().skip(1).cloned().collect::<Vec<_>>().join("/");
            let raw_path = if tail.is_empty() {
                String::new()
            } else {
                format!("/{tail}")
            };
            return build_ipfs_path(parsed, &raw_path, trailing_slash);
        }
        GatewayHost::Subdomain(cid) => {
            // The origin root is a directory URL, even though trimming its
            // leading slash would otherwise turn it into an empty string.
            let raw = if uri_path == "/" {
                "/"
            } else {
                uri_path.strip_prefix('/').unwrap_or(uri_path)
            };
            (cid, raw.to_owned())
        }
    };

    let (segments, trailing_slash) =
        decode_path_segments(&raw_path).map_err(|_| StatusCode::BAD_REQUEST)?;
    let tail = segments.join("/");
    let path = if tail.is_empty() {
        String::new()
    } else {
        format!("/{tail}")
    };
    build_ipfs_path(cid, &path, trailing_slash)
}

fn build_ipfs_path(
    cid: Cid,
    tail: &str,
    trailing_slash: bool,
) -> Result<(IpfsPath, bool), StatusCode> {
    let path = format!("/ipfs/{cid}{tail}");
    IpfsPath::from_str(&path)
        .map(|path| (path, trailing_slash))
        .map_err(|_| StatusCode::BAD_REQUEST)
}

fn decode_path_segments(raw: &str) -> Result<(Vec<String>, bool), ()> {
    let lowercase = raw.to_ascii_lowercase();
    if lowercase.contains("%2f") || lowercase.contains("%5c") || raw.contains('\\') {
        return Err(());
    }

    let trailing_slash = raw.ends_with('/');
    let content = if trailing_slash {
        &raw[..raw.len() - 1]
    } else {
        raw
    };

    if content.is_empty() {
        return Ok((Vec::new(), trailing_slash));
    }

    let mut segments = Vec::new();
    for segment in content.split('/') {
        if segment.is_empty() || !valid_percent_encoding(segment) {
            return Err(());
        }
        let decoded = percent_decode_str(segment)
            .decode_utf8()
            .map_err(|_| ())?
            .into_owned();
        if decoded.is_empty()
            || decoded == "."
            || decoded == ".."
            || decoded.contains('/')
            || decoded.contains('\\')
            || decoded.contains('\0')
        {
            return Err(());
        }
        segments.push(decoded);
    }

    Ok((segments, trailing_slash))
}

fn valid_percent_encoding(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    true
}

fn parse_single_range(value: &str, length: u64) -> Result<Range<u64>, ()> {
    let value = value.trim();
    let value = value.strip_prefix("bytes=").ok_or(())?;
    if value.contains(',') {
        return Err(());
    }

    let (start, end) = value.split_once('-').ok_or(())?;
    if start.is_empty() {
        let suffix = end.parse::<u64>().map_err(|_| ())?;
        if suffix == 0 || length == 0 {
            return Err(());
        }
        let start = length.saturating_sub(suffix);
        return Ok(start..length);
    }

    let start = start.parse::<u64>().map_err(|_| ())?;
    if start >= length {
        return Err(());
    }
    let end = if end.is_empty() {
        length - 1
    } else {
        end.parse::<u64>().map_err(|_| ())?.min(length - 1)
    };
    if end < start {
        return Err(());
    }

    Ok(start..end + 1)
}

fn content_type(path: &IpfsPath) -> &'static str {
    let extension = path
        .iter()
        .last()
        .and_then(|name| name.rsplit_once('.').map(|(_, extension)| extension))
        .unwrap_or("");

    if extension.eq_ignore_ascii_case("html") || extension.eq_ignore_ascii_case("htm") {
        "text/html; charset=utf-8"
    } else if extension.eq_ignore_ascii_case("css") {
        "text/css; charset=utf-8"
    } else if extension.eq_ignore_ascii_case("js") || extension.eq_ignore_ascii_case("mjs") {
        "text/javascript; charset=utf-8"
    } else if extension.eq_ignore_ascii_case("json") || extension.eq_ignore_ascii_case("map") {
        "application/json"
    } else if extension.eq_ignore_ascii_case("svg") {
        "image/svg+xml"
    } else if extension.eq_ignore_ascii_case("png") {
        "image/png"
    } else if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") {
        "image/jpeg"
    } else if extension.eq_ignore_ascii_case("gif") {
        "image/gif"
    } else if extension.eq_ignore_ascii_case("webp") {
        "image/webp"
    } else if extension.eq_ignore_ascii_case("ico") {
        "image/vnd.microsoft.icon"
    } else if extension.eq_ignore_ascii_case("avif") {
        "image/avif"
    } else if extension.eq_ignore_ascii_case("woff") {
        "font/woff"
    } else if extension.eq_ignore_ascii_case("woff2") {
        "font/woff2"
    } else if extension.eq_ignore_ascii_case("ttf") {
        "font/ttf"
    } else if extension.eq_ignore_ascii_case("otf") {
        "font/otf"
    } else if extension.eq_ignore_ascii_case("wasm") {
        "application/wasm"
    } else if extension.eq_ignore_ascii_case("pdf") {
        "application/pdf"
    } else if extension.eq_ignore_ascii_case("xml") {
        "application/xml"
    } else if extension.eq_ignore_ascii_case("txt")
        || extension.eq_ignore_ascii_case("md")
        || extension.eq_ignore_ascii_case("csv")
    {
        "text/plain; charset=utf-8"
    } else if extension.eq_ignore_ascii_case("webmanifest") {
        "application/manifest+json"
    } else if extension.eq_ignore_ascii_case("mp4") {
        "video/mp4"
    } else if extension.eq_ignore_ascii_case("webm") {
        "video/webm"
    } else if extension.eq_ignore_ascii_case("mp3") {
        "audio/mpeg"
    } else if extension.eq_ignore_ascii_case("ogg") {
        "audio/ogg"
    } else {
        "application/octet-stream"
    }
}

fn range_not_satisfiable(length: u64) -> Response<Body> {
    Response::builder()
        .status(StatusCode::RANGE_NOT_SATISFIABLE)
        .header(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"))
        .header(header::CONTENT_RANGE, format!("bytes */{length}"))
        .body(Body::empty())
        .expect("static range error response is valid")
}

fn redirect_to_directory(uri: &Uri) -> Response<Body> {
    let mut location = uri.path().to_owned();
    location.push('/');
    if let Some(query) = uri.query() {
        location.push('?');
        location.push_str(query);
    }
    Response::builder()
        .status(StatusCode::PERMANENT_REDIRECT)
        .header(header::LOCATION, location)
        .body(Body::empty())
        .expect("directory redirect is valid")
}

fn response(status: StatusCode, message: &'static str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(message))
        .expect("static error response is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::DefaultIpfsBuilder;
    use axum::body::to_bytes;
    use bytes::Bytes;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn gateway_request(
        ipfs: &Ipfs,
        method: Method,
        uri: &str,
        host: &str,
        range: Option<&str>,
    ) -> Response<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, host);
        if let Some(range) = range {
            builder = builder.header(header::RANGE, range);
        }
        let request = builder.body(Body::empty()).unwrap();
        handle_request(State(Arc::new(ipfs.clone())), request).await
    }

    async fn test_node() -> Ipfs {
        DefaultIpfsBuilder::new()
            .with_default()
            .enable_memory_transport()
            .start()
            .await
            .expect("in-memory IPFS node should start")
    }

    #[test]
    fn default_listener_is_loopback_and_uses_kubo_port() {
        let address: SocketAddr = DEFAULT_LISTEN_ADDR.parse().unwrap();
        assert!(address.ip().is_loopback());
        assert_eq!(address.port(), 8080);
    }

    #[test]
    fn rejects_unconfigured_hosts() {
        let request = Request::builder()
            .uri("/")
            .header(header::HOST, "anything.example")
            .body(Body::empty())
            .unwrap();
        assert!(parse_host(&request).is_err());
    }

    #[tokio::test]
    async fn serves_a_local_unixfs_file_over_the_http_listener() {
        let ipfs = test_node().await;
        let content = b"served through a real TCP HTTP connection";
        let path = ipfs
            .unixfs()
            .add(Bytes::from_static(content))
            .await
            .unwrap();
        let cid = path.root().cid().unwrap().to_string();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(ipfs, listener));

        let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
        let request = format!(
            "GET /ipfs/{cid} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
        );
        client.write_all(request.as_bytes()).await.unwrap();

        let mut response_bytes = Vec::new();
        client.read_to_end(&mut response_bytes).await.unwrap();
        server.abort();

        let response = String::from_utf8(response_bytes).unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with("HTTP/1.1 200"));
        assert!(headers.to_ascii_lowercase().contains("content-type: application/octet-stream"));
        assert_eq!(body.as_bytes(), content);
    }

    #[tokio::test]
    async fn serves_a_local_unixfs_index_with_mime_type() {
        let ipfs = test_node().await;
        let path = ipfs
            .unixfs()
            .add((
                "index.html".to_owned(),
                Bytes::from_static(b"<!doctype html><title>local UnixFS</title>"),
            ))
            .wrap()
            .await
            .unwrap();
        let cid = path.root().cid().unwrap().to_string();

        let response = gateway_request(
            &ipfs,
            Method::GET,
            &format!("/ipfs/{cid}/"),
            "localhost:8080",
            None,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/html; charset=utf-8"
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body.as_ref(), b"<!doctype html><title>local UnixFS</title>");
    }

    #[tokio::test]
    async fn serves_single_byte_ranges_without_buffering_the_file() {
        let ipfs = test_node().await;
        let path = ipfs
            .unixfs()
            .add(Bytes::from_static(b"0123456789"))
            .await
            .unwrap();
        let cid = path.root().cid().unwrap().to_string();

        let response = gateway_request(
            &ipfs,
            Method::GET,
            &format!("/ipfs/{cid}"),
            "localhost:8080",
            Some("bytes=2-5"),
        )
        .await;

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            response.headers().get(header::CONTENT_RANGE).unwrap(),
            "bytes 2-5/10"
        );
        assert_eq!(
            response.headers().get(header::CONTENT_LENGTH).unwrap(),
            "4"
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body.as_ref(), b"2345");
    }

    #[tokio::test]
    async fn serves_a_site_through_its_cid_subdomain() {
        let ipfs = test_node().await;
        let path = ipfs
            .unixfs()
            .add((
                "index.html".to_owned(),
                Bytes::from_static(b"<h1>subdomain origin</h1>"),
            ))
            .wrap()
            .await
            .unwrap();
        let cid = path.root().cid().unwrap().to_string();

        let response = gateway_request(
            &ipfs,
            Method::GET,
            "/",
            &format!("{cid}.ipfs.localhost:8080"),
            None,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body.as_ref(), b"<h1>subdomain origin</h1>");
    }

    #[test]
    fn parses_and_rejects_range_boundaries() {
        assert_eq!(parse_single_range("bytes=2-5", 10).unwrap(), 2..6);
        assert_eq!(parse_single_range("bytes=4-", 10).unwrap(), 4..10);
        assert_eq!(parse_single_range("bytes=-3", 10).unwrap(), 7..10);
        assert!(parse_single_range("bytes=10-12", 10).is_err());
        assert!(parse_single_range("bytes=4-2", 10).is_err());
        assert!(parse_single_range("bytes=0-1,4-5", 10).is_err());
        assert!(parse_single_range("bytes=-0", 10).is_err());
        assert!(parse_single_range("bytes=0-0", 0).is_err());
    }

    #[test]
    fn rejects_encoded_separators_and_dot_segments() {
        assert!(decode_path_segments("cid/a%2fb").is_err());
        assert!(decode_path_segments("cid/%2E%2E/secret").is_err());
        assert!(decode_path_segments("cid/%5csecret").is_err());
        assert!(decode_path_segments("cid/%GG").is_err());
    }
}
