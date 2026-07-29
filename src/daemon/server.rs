use super::api;
use super::state::{Command, SharedState};
use crate::error::AppError;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;
use tiny_http::{Header, Method, Request, Response, Server};
use tracing::warn;

const INDEX_HTML: &str = include_str!("web/index.html");
const STYLE_CSS: &str = include_str!("web/style.css");
const APP_JS: &str = include_str!("web/app.js");

/// Maximum accepted request body size (settings JSON is well under this).
const MAX_BODY: u64 = 1024 * 1024;

/// Bind the web server, failing early (e.g. port already in use).
pub fn bind(listen: &str) -> Result<Server, AppError> {
    Server::http(listen)
        .map_err(|e| AppError::CommandExecution(format!("Failed to bind web server: {e}")))
}

/// Accept loop. Single-threaded: handlers only read shared state or push
/// commands, so no request ever blocks on encoding work.
pub fn serve(
    server: &Server,
    shared: &SharedState,
    cmd_tx: &Sender<Command>,
    shutdown: &AtomicBool,
) {
    while !shutdown.load(Ordering::SeqCst) {
        match server.recv_timeout(Duration::from_millis(500)) {
            Ok(Some(request)) => handle_request(request, shared, cmd_tx),
            Ok(None) => {}
            Err(e) => warn!("HTTP accept error: {e}"),
        }
    }
}

/// Whether a request carries the configured shared secret.
///
/// Accepted as `Authorization: Bearer <token>` or a `token=` query parameter,
/// so a plain URL is enough to open the UI. Only `/api` paths are guarded: the
/// page itself has to load before it can send anything.
fn authorized(request: &Request, query: &str, token: &str) -> bool {
    if token.is_empty() {
        return true;
    }
    let header = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
        .map(|h| h.value.as_str().to_string())
        .unwrap_or_default();
    header.strip_prefix("Bearer ").map(str::trim) == Some(token)
        || query_param(query, "token").as_deref() == Some(token)
}

fn handle_request(mut request: Request, shared: &SharedState, cmd_tx: &Sender<Command>) {
    let url = request.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));

    if path.starts_with("/api") {
        let token = super::state::lock(shared).config.daemon.auth_token.clone();
        if !authorized(&request, query, &token) {
            return respond_json(
                request,
                401,
                &serde_json::json!({"error": "missing or invalid token"}),
            );
        }
    }

    let (status, body) = match (request.method(), path) {
        (Method::Get, "/") => {
            return respond_asset(request, INDEX_HTML, "text/html; charset=utf-8");
        }
        (Method::Get, "/style.css") => {
            return respond_asset(request, STYLE_CSS, "text/css; charset=utf-8");
        }
        (Method::Get, "/app.js") => {
            return respond_asset(request, APP_JS, "application/javascript; charset=utf-8");
        }
        (Method::Get, "/api/status") => (200, api::status(shared)),
        (Method::Get, "/api/queue") => (200, api::queue(shared)),
        (Method::Get, "/api/fs") => api::fs_browse(
            shared,
            &query_param(query, "path").unwrap_or_default(),
            query_param(query, "hidden").as_deref() == Some("1"),
        ),
        (Method::Get, "/api/settings") => (200, api::settings_get(shared)),
        (Method::Get, "/api/job/tracks") => {
            api::job_tracks(shared, &query_param(query, "id").unwrap_or_default())
        }
        (Method::Post, "/api/job/tracks") => match read_json_body(&mut request) {
            Ok(body) => api::job_tracks_set(shared, cmd_tx, &body),
            Err(resp) => resp,
        },
        (Method::Post, "/api/queue/add") => match read_json_body(&mut request) {
            Ok(body) => api::queue_add(shared, cmd_tx, &body),
            Err(resp) => resp,
        },
        (Method::Post, "/api/queue/remove") => match read_json_body(&mut request) {
            Ok(body) => api::queue_remove(shared, cmd_tx, &body),
            Err(resp) => resp,
        },
        (Method::Post, "/api/queue/pause") => match read_json_body(&mut request) {
            Ok(body) => api::queue_pause(cmd_tx, &body),
            Err(resp) => resp,
        },
        (Method::Post, "/api/queue/cancel") => api::queue_cancel(cmd_tx),
        (Method::Post, "/api/queue/clear_finished") => api::queue_clear_finished(shared, cmd_tx),
        (Method::Post, "/api/settings") => match read_json_body(&mut request) {
            Ok(body) => api::settings_post(cmd_tx, &body),
            Err(resp) => resp,
        },
        _ => (404, serde_json::json!({"error": "not found"})),
    };

    respond_json(request, status, &body);
}

fn respond_json(request: Request, status: u16, body: &serde_json::Value) {
    let response = Response::from_string(body.to_string())
        .with_status_code(status)
        .with_header(content_type("application/json"));
    if let Err(e) = request.respond(response) {
        warn!("Failed to send HTTP response: {e}");
    }
}

fn respond_asset(request: Request, body: &'static str, mime: &str) {
    let response = Response::from_string(body).with_header(content_type(mime));
    if let Err(e) = request.respond(response) {
        warn!("Failed to send HTTP response: {e}");
    }
}

fn content_type(mime: &str) -> Header {
    Header::from_bytes(&b"Content-Type"[..], mime.as_bytes()).expect("valid header")
}

/// Read and parse a JSON request body, capped at [`MAX_BODY`].
fn read_json_body(request: &mut Request) -> Result<serde_json::Value, (u16, serde_json::Value)> {
    let mut body = String::new();
    request
        .as_reader()
        .take(MAX_BODY)
        .read_to_string(&mut body)
        .map_err(|e| (400, serde_json::json!({"error": format!("bad body: {e}")})))?;
    serde_json::from_str(&body).map_err(|e| {
        (
            400,
            serde_json::json!({"error": format!("invalid JSON: {e}")}),
        )
    })
}

/// Extract and percent-decode a query-string parameter.
fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| percent_decode(v))
    })
}

/// Decode `%XX` escapes and `+` in a URL component.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len()
                && bytes[i + 1].is_ascii_hexdigit()
                && bytes[i + 2].is_ascii_hexdigit() =>
            {
                let hi = (bytes[i + 1] as char).to_digit(16).unwrap_or(0);
                let lo = (bytes[i + 2] as char).to_digit(16).unwrap_or(0);
                out.push(u8::try_from(hi * 16 + lo).unwrap_or(b'%'));
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decoding() {
        assert_eq!(
            percent_decode("/home/user/My%20Videos"),
            "/home/user/My Videos"
        );
        assert_eq!(percent_decode("a+b%2Fc"), "a b/c");
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("bad%2"), "bad%2");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn query_params() {
        assert_eq!(
            query_param("path=%2Ftmp&hidden=1", "path").as_deref(),
            Some("/tmp")
        );
        assert_eq!(query_param("path=%2Ftmp", "hidden"), None);
        assert_eq!(query_param("", "path"), None);
    }
}
