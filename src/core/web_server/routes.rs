use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use futures_util::StreamExt;
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::{Method, Request, Response, StatusCode};
use log::debug;
use rust_embed::RustEmbed;
use tokio_stream::wrappers::BroadcastStream;

use super::api;
use super::{AppState, ResponseBody};

#[derive(RustEmbed)]
#[folder = "src/web/dist"]
struct WebAssets;

pub(super) async fn handle(
    request: Request<Incoming>,
    state: Arc<AppState>,
) -> Result<Response<ResponseBody>, Infallible> {
    let path = request.uri().path().to_string();
    let method = request.method().clone();

    if method == Method::GET && path == "/api/events" {
        return Ok(sse_response(&state));
    }

    if path.starts_with("/api") {
        let started_at = Instant::now();
        let response = handle_api(request, &state).await;
        let status = response
            .as_ref()
            .map(|response| response.status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        debug!("API {method} {path} -> {status} in {:?}", started_at.elapsed());
        return Ok(response
            .unwrap_or_else(|| json(StatusCode::INTERNAL_SERVER_ERROR, "{\"error\":\"internal\"}")));
    }

    if method == Method::GET {
        Ok(serve_spa(&path))
    } else {
        Ok(text(StatusCode::METHOD_NOT_ALLOWED, "method not allowed"))
    }
}

async fn handle_api(
    request: Request<Incoming>,
    state: &Arc<AppState>,
) -> Option<Response<ResponseBody>> {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let query = request.uri().query().unwrap_or("").to_string();

    match (&method, path.as_str()) {
        (&Method::GET, "/api/info") => {
            let body = serde_json::json!({
                "pwd": state.working_directory.display().to_string(),
                "version": env!("BITFORGE_VERSION"),
            })
            .to_string();
            Some(json(StatusCode::OK, &body))
        }
        (&Method::GET, "/api/poll") => {
            let cursor = parse_cursor(&query);
            let (next_cursor, messages) = state.poll_buffer.lock().unwrap().since(cursor);
            let body = serde_json::json!({ "cursor": next_cursor, "messages": messages }).to_string();
            Some(json(StatusCode::OK, &body))
        }
        _ => api::handle_domain(request, state, &method, path.as_str()).await,
    }
}

fn sse_response(state: &Arc<AppState>) -> Response<ResponseBody> {
    let receiver = state.outbound.subscribe();
    let stream = BroadcastStream::new(receiver).filter_map(|item| async move {
        match item {
            Ok(message) => {
                let data = serde_json::to_string(&message).unwrap_or_default();
                let frame = Frame::data(Bytes::from(format!("data: {data}\n\n")));
                Some(Ok::<_, Infallible>(frame))
            }
            Err(_) => None,
        }
    });
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("connection", "keep-alive")
        .body(BodyExt::boxed(StreamBody::new(stream)))
        .unwrap()
}

fn serve_spa(path: &str) -> Response<ResponseBody> {
    let relative_path = path.trim_start_matches('/');
    let candidate = if relative_path.is_empty() {
        "index.html"
    } else {
        relative_path
    };

    if let Some(asset) = WebAssets::get(candidate) {
        return asset_response(candidate, asset.data.into_owned());
    }
    match WebAssets::get("index.html") {
        Some(index) => asset_response("index.html", index.data.into_owned()),
        None => text(StatusCode::NOT_FOUND, "web bundle missing"),
    }
}

fn asset_response(path: &str, data: Vec<u8>) -> Response<ResponseBody> {
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", content_type(path))
        .body(Full::new(Bytes::from(data)).boxed())
        .unwrap()
}

pub(crate) fn json(status: StatusCode, body: &str) -> Response<ResponseBody> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body.to_owned())).boxed())
        .unwrap()
}

pub(crate) fn text(status: StatusCode, body: &str) -> Response<ResponseBody> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from(body.to_owned())).boxed())
        .unwrap()
}

fn parse_cursor(query: &str) -> u64 {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("cursor="))
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("map") => "application/json",
        _ => "application/octet-stream",
    }
}
