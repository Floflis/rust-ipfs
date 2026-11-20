//! Minimal inbound HTTP Path Gateway.
//!
//! This module is intentionally separate from `gateway`, which retrieves content
//! from remote HTTP gateways. The listener binds to loopback by default.

use std::{net::SocketAddr, str::FromStr, sync::Arc};

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderValue, Method, Request, StatusCode, header},
    response::Response,
    routing::any,
};
use futures::StreamExt;
use tokio::net::TcpListener;

use crate::{Ipfs, IpfsPath};

/// Default local HTTP Gateway address, matching Kubo's path gateway.
pub const DEFAULT_LISTEN_ADDR: &str = "127.0.0.1:8080";

/// Run the HTTP Gateway on an already-bound listener.
///
/// The caller owns listener creation and shutdown policy. This keeps the
/// server reusable by applications and a future long-running daemon.
pub async fn serve(ipfs: Ipfs, listener: TcpListener) -> std::io::Result<()> {
    let app = Router::new()
        .route("/", any(handle_request))
        .route("/{*path}", any(handle_request))
        .with_state(Arc::new(ipfs));

    axum::serve(listener, app).await
}

/// Bind the default loopback listener and start serving until the task is aborted.
pub async fn serve_default(ipfs: Ipfs) -> std::io::Result<()> {
    let address: SocketAddr = DEFAULT_LISTEN_ADDR
        .parse()
        .expect("the built-in gateway address is valid");
    let listener = TcpListener::bind(address).await?;
    serve(ipfs, listener).await
}

async fn handle_request(
    State(ipfs): State<Arc<Ipfs>>,
    request: Request<Body>,
) -> Response<Body> {
    let method = request.method();
    if method != Method::GET && method != Method::HEAD {
        return response(StatusCode::METHOD_NOT_ALLOWED, "only GET and HEAD are supported");
    }

    let uri_path = request.uri().path();
    let Some(path) = uri_path.strip_prefix("/ipfs/") else {
        return response(StatusCode::NOT_FOUND, "expected /ipfs/{cid}/{path}");
    };

    // Reject encoded separators and dot segments rather than allowing ambiguous
    // path normalization to change the content root.
    if path.contains("%2f")
        || path.contains("%2F")
        || path.split('/').any(|segment| segment == "." || segment == "..")
    {
        return response(StatusCode::BAD_REQUEST, "invalid IPFS path");
    }

    let ipfs_path = match IpfsPath::from_str(&format!("/ipfs/{path}")) {
        Ok(path) => path,
        Err(_) => return response(StatusCode::BAD_REQUEST, "invalid CID or IPFS path"),
    };

    let stream = ipfs
        .unixfs()
        .cat(ipfs_path)
        .map(|chunk| chunk.map_err(|error| std::io::Error::other(error.to_string())));

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=29030400, immutable"))
        .header(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));

    if method == Method::HEAD {
        return builder
            .body(Body::empty())
            .expect("static response headers are valid");
    }

    builder
        .body(Body::from_stream(stream))
        .expect("static response headers are valid")
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

    #[test]
    fn default_listener_is_loopback_and_uses_kubo_port() {
        let address: SocketAddr = DEFAULT_LISTEN_ADDR.parse().unwrap();
        assert!(address.ip().is_loopback());
        assert_eq!(address.port(), 8080);
    }
}
