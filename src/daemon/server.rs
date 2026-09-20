use super::api;
use super::state::SharedState;
use crate::disc::worker::DiscEvent;
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
const FAVICON_PNG: &[u8] = include_bytes!("web/favicon.png");

/// Maximum accepted request body size (settings JSON is well under this).
const MAX_BODY: u64 = 1024 * 1024;

/// Bind the web server, failing early (e.g. port already in use).
pub fn bind(listen: &str) -> Result<Server, AppError> {
    Server::http(listen)
        .map_err(|e| AppError::CommandExecution(format!("Failed to bind web server: {e}")))
}

/// Accept loop. Several of these run at once; state mutations only hold the
/// shared lock for short in-memory updates.
pub fn serve(
    server: &Server,
    shared: &SharedState,
    probe_tx: &Sender<(u64, String)>,
    disc_tx: &Sender<DiscEvent>,
    shutdown: &AtomicBool,
) {
    while !shutdown.load(Ordering::SeqCst) {
        match server.recv_timeout(Duration::from_millis(500)) {
            // A panic costs one request, not the worker thread, which is never
            // replaced. The request is consumed either way, so the client sees
            // a dropped connection rather than a hung one.
            Ok(Some(request)) => {
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    handle_request(request, shared, probe_tx, disc_tx, shutdown);
                }))
                .is_err()
                {
                    warn!("HTTP handler panicked; connection dropped");
                }
            }
            Ok(None) => {}
            Err(e) => warn!("HTTP accept error: {e}"),
        }
    }
}

/// Compare two secrets without giving away how much of a guess was right.
/// The length is not hidden, which tells an attacker nothing useful here.
fn secret_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// Whether a request carries the configured shared secret.
///
/// Accepted as `Authorization: Bearer <token>`. The launch URL carries the
/// token in its fragment, which browsers never send to this HTTP server.
fn authorized(request: &Request, token: &str) -> bool {
    // An empty token is never accepted: startup always regenerates one, and a
    // cleared runtime token must not open the API.
    if token.is_empty() {
        return false;
    }
    let header = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
        .map(|h| h.value.as_str().to_string())
        .unwrap_or_default();
    header
        .strip_prefix("Bearer ")
        .is_some_and(|value| secret_eq(value.trim(), token))
}

/// Whether the connection and requested origin both identify this host.
fn request_is_local(request: &Request, behind_proxy: bool) -> bool {
    !behind_proxy && is_local(request.remote_addr(), request.headers())
}

/// A loopback peer with a loopback `Host` and no reverse-proxy forwarding
/// header. IPv4-mapped IPv6 loopback counts as loopback.
fn is_local(peer: Option<&std::net::SocketAddr>, headers: &[Header]) -> bool {
    let peer_is_loopback = peer.is_some_and(|address| address.ip().to_canonical().is_loopback());
    let host_is_loopback = headers
        .iter()
        .find(|header| header.field.equiv("Host"))
        .is_some_and(|header| host_is_loopback(header.value.as_str()));
    let forwarded = headers.iter().any(|header| {
        [
            "X-Forwarded-For",
            "X-Forwarded-Host",
            "X-Real-IP",
            "Forwarded",
        ]
        .iter()
        .any(|name| header.field.equiv(name))
    });
    peer_is_loopback && host_is_loopback && !forwarded
}

/// Whether a `Host` header names a loopback origin.
fn host_is_loopback(host: &str) -> bool {
    let name = host.strip_prefix('[').map_or_else(
        || host.split(':').next().unwrap_or(""),
        |rest| rest.split(']').next().unwrap_or(""),
    );
    name.eq_ignore_ascii_case("localhost")
        || name
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.to_canonical().is_loopback())
}

/// Requiring JSON makes browser cross-origin POSTs preflight instead of being
/// silently accepted as form/text requests. This server grants no CORS access.
fn has_json_content_type(headers: &[Header]) -> bool {
    headers
        .iter()
        .find(|h| h.field.equiv("Content-Type"))
        .and_then(|h| h.value.as_str().split(';').next())
        .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
}

/// Cancel endpoints still run so an in-flight encode/rip can be told to stop.
fn blocks_on_shutdown(method: &Method, path: &str) -> bool {
    if matches!(
        path,
        "/api/queue/cancel" | "/api/discs/cancel" | "/api/queue/cancel_analysis"
    ) {
        return false;
    }
    method == &Method::Post
}

#[allow(clippy::too_many_lines)]
fn handle_request(
    mut request: Request,
    shared: &SharedState,
    probe_tx: &Sender<(u64, String)>,
    disc_tx: &Sender<DiscEvent>,
    shutdown: &AtomicBool,
) {
    let url = request.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let behind_proxy = super::state::lock(shared).config.daemon.behind_proxy;
    let local_request = request_is_local(&request, behind_proxy);

    if path.starts_with("/api") {
        let token = super::state::lock(shared).config.daemon.auth_token.clone();
        if !authorized(&request, &token) {
            return respond_json(
                request,
                401,
                &serde_json::json!({"error": "missing or invalid token"}),
            );
        }
        if request.method() == &Method::Post && !has_json_content_type(request.headers()) {
            return respond_json(
                request,
                415,
                &serde_json::json!({"error": "Content-Type must be application/json"}),
            );
        }
        if shutdown.load(Ordering::SeqCst) && blocks_on_shutdown(request.method(), path) {
            return respond_json(
                request,
                503,
                &serde_json::json!({"error": "daemon is shutting down"}),
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
        (Method::Get, "/favicon.png") => {
            return respond_bytes(request, FAVICON_PNG, "image/png");
        }
        (Method::Get, "/api/status") => (200, api::status(shared)),
        (Method::Get, "/api/queue") => (200, api::queue(shared)),
        (Method::Get, "/api/fs") => api::fs_browse(
            shared,
            &query_param(query, "path").unwrap_or_default(),
            query_param(query, "hidden").as_deref() == Some("1"),
        ),
        (Method::Get, "/api/settings") => (200, api::settings_get(shared)),
        (Method::Get, "/api/settings/access") => (200, api::settings_access(shared, local_request)),
        (Method::Get, "/api/strings") => (200, api::strings(shared)),
        (Method::Get, "/api/job/tracks") => {
            api::job_tracks(shared, &query_param(query, "id").unwrap_or_default())
        }
        (Method::Post, "/api/job/tracks") => match read_json_body(&mut request) {
            Ok(body) => api::job_tracks_set(shared, &body),
            Err(resp) => resp,
        },
        (Method::Post, "/api/queue/add") => match read_json_body(&mut request) {
            Ok(body) => api::queue_add(shared, probe_tx, &body, shutdown),
            Err(resp) => resp,
        },
        (Method::Post, "/api/queue/remove") => match read_json_body(&mut request) {
            Ok(body) => api::queue_remove(shared, &body),
            Err(resp) => resp,
        },
        (Method::Post, "/api/queue/move_up") => match read_json_body(&mut request) {
            Ok(body) => api::queue_move_up(shared, &body),
            Err(resp) => resp,
        },
        (Method::Post, "/api/discs/list") => api::discs_list(shared),
        (Method::Post, "/api/discs/scan") => match read_json_body(&mut request) {
            Ok(body) => api::discs_scan(shared, disc_tx, &body),
            Err(resp) => resp,
        },
        (Method::Post, "/api/discs/rip") => match read_json_body(&mut request) {
            Ok(body) => api::discs_rip(shared, disc_tx, &body),
            Err(resp) => resp,
        },
        (Method::Post, "/api/discs/cancel") => api::discs_cancel(shared),
        (Method::Post, "/api/queue/cancel") => api::queue_cancel(shared),
        (Method::Post, "/api/queue/cancel_analysis") => api::queue_cancel_analysis(shared),
        (Method::Post, "/api/queue/clear_finished") => match read_json_body(&mut request) {
            Ok(body) => api::queue_clear_finished(shared, &body),
            Err(resp) => resp,
        },
        (Method::Post, "/api/settings") => match read_json_body(&mut request) {
            Ok(body) => api::settings_post(shared, &body, local_request),
            Err(resp) => resp,
        },
        (Method::Post, "/api/settings/service") => match read_json_body(&mut request) {
            Ok(body) => api::settings_service_post(shared, &body, local_request),
            Err(resp) => resp,
        },
        _ => (404, serde_json::json!({"error": "not found"})),
    };

    respond_json(request, status, &body);
}

fn respond_json(request: Request, status: u16, body: &serde_json::Value) {
    let response = secure_response(
        Response::from_string(body.to_string())
            .with_status_code(status)
            .with_header(content_type("application/json")),
    );
    if let Err(e) = request.respond(response) {
        warn!("Failed to send HTTP response: {e}");
    }
}

fn respond_asset(request: Request, body: &'static str, mime: &str) {
    let response = secure_response(Response::from_string(body).with_header(content_type(mime)));
    if let Err(e) = request.respond(response) {
        warn!("Failed to send HTTP response: {e}");
    }
}

fn respond_bytes(request: Request, body: &'static [u8], mime: &str) {
    let response = secure_response(Response::from_data(body).with_header(content_type(mime)));
    if let Err(e) = request.respond(response) {
        warn!("Failed to send HTTP response: {e}");
    }
}

fn content_type(mime: &str) -> Header {
    Header::from_bytes(&b"Content-Type"[..], mime.as_bytes()).expect("valid header")
}

fn secure_response<R: Read>(mut response: Response<R>) -> Response<R> {
    for (name, value) in [
        ("Cache-Control", "no-store"),
        (
            "Content-Security-Policy",
            "default-src 'self'; frame-ancestors 'none'; object-src 'none'; base-uri 'none'; form-action 'self'; style-src 'self'",
        ),
        ("Referrer-Policy", "no-referrer"),
        ("X-Content-Type-Options", "nosniff"),
        ("X-Frame-Options", "DENY"),
    ] {
        response.add_header(Header::from_bytes(name, value).expect("valid security header"));
    }
    response
}

/// Read and parse a JSON request body, capped at [`MAX_BODY`].
fn read_json_body(request: &mut Request) -> Result<serde_json::Value, (u16, serde_json::Value)> {
    let mut body = String::new();
    request
        .as_reader()
        .take(MAX_BODY + 1)
        .read_to_string(&mut body)
        .map_err(|e| (400, serde_json::json!({"error": format!("bad body: {e}")})))?;
    if body.len() as u64 > MAX_BODY {
        return Err((413, serde_json::json!({"error": "request body too large"})));
    }
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

    #[test]
    fn local_admin_hosts_are_limited_to_loopback_origins() {
        assert!(host_is_loopback("127.0.0.1:8399"));
        assert!(host_is_loopback("[::1]:8399"));
        assert!(host_is_loopback("localhost:8399"));
        assert!(!host_is_loopback("192.168.1.10:8399"));
        assert!(!host_is_loopback("media.example.com"));
    }

    #[test]
    fn a_proxied_loopback_request_is_not_local() {
        let loopback: std::net::SocketAddr = "127.0.0.1:50000".parse().unwrap();
        let host = Header::from_bytes("Host", "127.0.0.1:8399").unwrap();
        assert!(is_local(Some(&loopback), std::slice::from_ref(&host)));
        for name in [
            "X-Forwarded-For",
            "x-forwarded-host",
            "X-Real-IP",
            "Forwarded",
        ] {
            let forwarded = Header::from_bytes(name, "203.0.113.7").unwrap();
            assert!(
                !is_local(Some(&loopback), &[host.clone(), forwarded]),
                "{name} still counted as local"
            );
        }
        let remote: std::net::SocketAddr = "192.168.1.5:50000".parse().unwrap();
        assert!(!is_local(Some(&remote), &[host]));
    }

    #[test]
    fn ipv4_mapped_loopback_is_local() {
        let mapped: std::net::SocketAddr = "[::ffff:127.0.0.1]:50000".parse().unwrap();
        let host = Header::from_bytes("Host", "[::ffff:127.0.0.1]:8399").unwrap();
        assert!(is_local(Some(&mapped), std::slice::from_ref(&host)));
        assert!(host_is_loopback("[::ffff:127.0.0.1]:8399"));
    }

    #[test]
    fn secret_comparison_matches_only_the_whole_token() {
        assert!(secret_eq("s3cret", "s3cret"));
        assert!(!secret_eq("s3cret", "s3cres"));
        assert!(!secret_eq("s3cre", "s3cret"));
        assert!(!secret_eq("", "s3cret"));
    }

    #[test]
    fn mutation_content_type_must_be_json() {
        let json = Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap();
        let text = Header::from_bytes("Content-Type", "text/plain").unwrap();
        assert!(has_json_content_type(&[json]));
        assert!(!has_json_content_type(&[text]));
        assert!(!has_json_content_type(&[]));
    }

    /// Every disc route sits behind the token, and none of them is a 404 once
    /// the token is there.
    #[test]
    fn disc_routes_are_registered_and_token_guarded() {
        use crate::config::AppConfig;
        use crate::daemon::state::DaemonState;
        use std::io::Write;
        use std::net::TcpStream;
        use std::sync::mpsc;
        use std::sync::{Arc, Mutex};

        const TOKEN: &str = "a-token-long-enough-to-be-a-real-one";
        let mut config = AppConfig::default();
        config.daemon.auth_token = TOKEN.to_string();
        let shared: SharedState = Arc::new(Mutex::new(DaemonState::new(config)));
        let server = Arc::new(bind("127.0.0.1:0").expect("an ephemeral port"));
        let port = server.server_addr().to_ip().expect("an IP listener").port();

        let shutdown = Arc::new(AtomicBool::new(false));
        let (probe_tx, _probe_rx) = mpsc::channel();
        let (disc_tx, _disc_rx) = mpsc::channel();
        let worker = {
            let server = server.clone();
            let shared = shared.clone();
            let shutdown = shutdown.clone();
            std::thread::spawn(move || serve(&server, &shared, &probe_tx, &disc_tx, &shutdown))
        };

        let request = |method: &str, path: &str, token: Option<&str>| {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
            let auth = token.map_or(String::new(), |t| format!("Authorization: Bearer {t}\r\n"));
            write!(
                stream,
                "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}\
                 Content-Type: application/json\r\nContent-Length: 2\r\n\
                 Connection: close\r\n\r\n{{}}"
            )
            .expect("write");
            let mut response = String::new();
            std::io::Read::read_to_string(&mut stream, &mut response).expect("read");
            response
        };

        let routes = [
            ("POST", "/api/discs/list"),
            ("POST", "/api/discs/scan"),
            ("POST", "/api/discs/rip"),
            ("POST", "/api/discs/cancel"),
        ];
        for (method, path) in routes {
            let anonymous = request(method, path, None);
            assert!(
                anonymous.starts_with("HTTP/1.1 401"),
                "{method} {path} answered an unauthenticated request: {anonymous}"
            );
            let authorized = request(method, path, Some(TOKEN));
            assert!(
                !authorized.starts_with("HTTP/1.1 404") && !authorized.starts_with("HTTP/1.1 401"),
                "{method} {path} is not registered: {authorized}"
            );
        }

        shutdown.store(true, Ordering::SeqCst);
        worker.join().expect("the server thread");
    }

    /// With no token configured the API answers 401 to every request,
    /// whatever `Host` it carries and whether or not it sends a bearer token.
    #[test]
    fn an_empty_token_accepts_nothing() {
        use crate::config::AppConfig;
        use crate::daemon::state::DaemonState;
        use std::io::Write;
        use std::net::TcpStream;
        use std::sync::mpsc;
        use std::sync::{Arc, Mutex};

        let mut config = AppConfig::default();
        config.daemon.auth_token = String::new();
        let shared: SharedState = Arc::new(Mutex::new(DaemonState::new(config)));
        let server = Arc::new(bind("127.0.0.1:0").expect("an ephemeral port"));
        let port = server.server_addr().to_ip().expect("an IP listener").port();

        let shutdown = Arc::new(AtomicBool::new(false));
        let (probe_tx, _probe_rx) = mpsc::channel();
        let (disc_tx, _disc_rx) = mpsc::channel();
        let worker = {
            let server = server.clone();
            let shared = shared.clone();
            let shutdown = shutdown.clone();
            std::thread::spawn(move || serve(&server, &shared, &probe_tx, &disc_tx, &shutdown))
        };

        let request = |host: &str, auth: &str| {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
            write!(
                stream,
                "GET /api/status HTTP/1.1\r\nHost: {host}\r\n{auth}\
                 Connection: close\r\n\r\n"
            )
            .expect("write");
            let mut response = String::new();
            std::io::Read::read_to_string(&mut stream, &mut response).expect("read");
            response
        };

        for (host, auth) in [
            ("127.0.0.1", ""),
            ("evil.example.com", ""),
            ("127.0.0.1", "Authorization: Bearer \r\n"),
            ("evil.example.com", "Authorization: Bearer \r\n"),
            ("127.0.0.1", "Authorization: Bearer anything\r\n"),
        ] {
            let response = request(host, auth);
            assert!(
                response.starts_with("HTTP/1.1 401"),
                "Host {host} with {auth:?} was not refused: {response}"
            );
        }

        shutdown.store(true, Ordering::SeqCst);
        worker.join().expect("the server thread");
    }

    #[test]
    fn every_response_gets_browser_security_headers() {
        let response = secure_response(Response::from_string("ok"));
        for expected in [
            "Cache-Control",
            "Content-Security-Policy",
            "Referrer-Policy",
            "X-Content-Type-Options",
            "X-Frame-Options",
        ] {
            assert!(
                response
                    .headers()
                    .iter()
                    .any(|header| header.field.equiv(expected)),
                "missing {expected}"
            );
        }
        let csp = response
            .headers()
            .iter()
            .find(|header| header.field.equiv("Content-Security-Policy"))
            .unwrap()
            .value
            .as_str();
        assert!(!csp.contains("unsafe-inline"));
    }
}
