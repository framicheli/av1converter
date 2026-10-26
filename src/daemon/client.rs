//! Blocking HTTP client for the daemon's JSON API.

use crate::disc::{DiscDrive, DiscSource, DiscTitle};
use crate::queue::{JobStatus, PersistedQueue, QueueState};
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// Read timeout of the action thread's requests.
const ACTION_READ_TIMEOUT: Duration = Duration::from_secs(600);
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// Wait between snapshot attempts while the daemon is unreachable.
const RETRY_WAIT: Duration = Duration::from_secs(1);

/// Failure of a request to the daemon.
#[derive(Debug)]
pub enum ClientError {
    /// Connecting, writing, reading or a timeout failed.
    Unreachable(String),
    /// The daemon answered a non-2xx status.
    Http { status: u16, error: String },
    /// The response status line or JSON body could not be parsed.
    BadResponse(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(e) | Self::BadResponse(e) => f.write_str(e),
            Self::Http { status, error } => write!(f, "HTTP {status}: {error}"),
        }
    }
}

/// Client for one daemon address and access token. Clones share the token.
#[derive(Debug, Clone)]
pub struct DaemonClient {
    addr: SocketAddr,
    token: Arc<Mutex<String>>,
    read_timeout: Duration,
    /// Reads the current access token after a 401; `None` keeps the token.
    reload_token: fn() -> Option<String>,
}

/// `daemon.auth_token` from config.toml; `None` when the file is missing or
/// unreadable.
fn configured_token() -> Option<String> {
    crate::config::AppConfig::auth_token_at(&crate::config::AppConfig::config_path())
}

impl DaemonClient {
    #[must_use]
    pub fn new(addr: SocketAddr, token: String) -> Self {
        Self {
            addr,
            token: Arc::new(Mutex::new(token)),
            read_timeout: READ_TIMEOUT,
            reload_token: || None,
        }
    }

    /// This client with `reload_token` as its token source after a 401.
    #[must_use]
    pub fn with_token_source(self, reload_token: fn() -> Option<String>) -> Self {
        Self {
            reload_token,
            ..self
        }
    }

    /// A client sharing this one's token, with the action read timeout.
    fn for_actions(&self) -> Self {
        Self {
            read_timeout: ACTION_READ_TIMEOUT,
            ..self.clone()
        }
    }

    fn token(&self) -> String {
        self.token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Sends a GET and returns the parsed JSON body.
    ///
    /// # Errors
    /// Returns a [`ClientError`] on connection, status or parse failure.
    pub fn get(&self, path_and_query: &str) -> Result<Value, ClientError> {
        self.request("GET", path_and_query, None)
    }

    /// Sends a POST with a JSON body and returns the parsed JSON body.
    ///
    /// # Errors
    /// Returns a [`ClientError`] on connection, status or parse failure.
    pub fn post(&self, path: &str, body: &Value) -> Result<Value, ClientError> {
        self.request("POST", path, Some(&body.to_string()))
    }

    /// Sends the request; on a 401 re-reads the token and, when it changed,
    /// stores it and sends the request once more.
    fn request(&self, method: &str, path: &str, body: Option<&str>) -> Result<Value, ClientError> {
        let token = self.token();
        let result = self.send(method, path, body, &token);
        if !matches!(result, Err(ClientError::Http { status: 401, .. })) {
            return result;
        }
        let Some(fresh) = (self.reload_token)().filter(|fresh| *fresh != token) else {
            return result;
        };
        fresh.clone_into(
            &mut self
                .token
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        self.send(method, path, body, &fresh)
    }

    fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
        token: &str,
    ) -> Result<Value, ClientError> {
        let io = |e: std::io::Error| ClientError::Unreachable(e.to_string());
        let mut stream = TcpStream::connect_timeout(&self.addr, CONNECT_TIMEOUT).map_err(io)?;
        stream
            .set_read_timeout(Some(self.read_timeout))
            .map_err(io)?;
        stream.set_write_timeout(Some(WRITE_TIMEOUT)).map_err(io)?;
        let body_headers = body.map_or(String::new(), |b| {
            format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                b.len()
            )
        });
        let request = format!(
            "{method} {path} HTTP/1.0\r\nHost: {}\r\nAuthorization: Bearer {}\r\n\
             Connection: close\r\n{body_headers}\r\n{}",
            self.addr,
            token,
            body.unwrap_or("")
        );
        stream.write_all(request.as_bytes()).map_err(io)?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).map_err(io)?;
        parse_response(&raw)
    }
}

fn parse_response(raw: &[u8]) -> Result<Value, ClientError> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| ClientError::BadResponse("incomplete response".to_string()))?;
    let status_line = head.lines().next().unwrap_or_default();
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| ClientError::BadResponse(format!("bad status line: {status_line}")))?;
    let json = serde_json::from_str::<Value>(body);
    if (200..300).contains(&status) {
        return json.map_err(|e| ClientError::BadResponse(e.to_string()));
    }
    let error = json
        .ok()
        .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| status_line.to_string());
    Err(ClientError::Http { status, error })
}

/// Daemon state as served by `GET /api/snapshot`.
#[derive(Deserialize)]
pub struct RemoteSnapshot {
    pub version: String,
    /// Random id of the daemon run that took the snapshot.
    pub instance: u64,
    pub tag: String,
    pub taken_ms: u64,
    pub queue: PersistedQueue,
    pub progress: Vec<JobProgress>,
    /// Encoding time of the current or last session.
    pub elapsed_secs: Option<u64>,
    pub encoding_active: bool,
    pub disc: RemoteDisc,
}

/// Progress of one encoding or ripping job.
#[derive(Debug, Deserialize)]
pub struct JobProgress {
    pub id: u64,
    pub progress: f64,
}

/// Disc state of the daemon.
#[derive(Debug, Deserialize)]
pub struct RemoteDisc {
    pub drives: Vec<DiscDrive>,
    pub scanned_source: Option<DiscSource>,
    pub titles: Vec<DiscTitle>,
    pub active: bool,
    pub scanning: bool,
    pub listing: bool,
    pub error: Option<String>,
}

impl RemoteSnapshot {
    /// The queue with `Encoding`/`Ripping` progress restored from `progress`
    /// and the session clock set from `elapsed_secs`, stopped while no encode
    /// runs; returns it with its ids.
    #[must_use]
    pub fn queue_state(&self) -> (QueueState, Vec<u64>) {
        let source = &self.queue.state;
        let mut jobs = source.jobs.clone();
        for (job, id) in jobs.iter_mut().zip(&self.queue.ids) {
            let Some(reported) = self.progress.iter().find(|p| p.id == *id) else {
                continue;
            };
            if let JobStatus::Encoding { progress } | JobStatus::Ripping { progress } =
                &mut job.status
            {
                *progress = reported.progress;
            }
        }
        let now = std::time::Instant::now();
        let start_time = self
            .elapsed_secs
            .and_then(|secs| now.checked_sub(Duration::from_secs(secs)));
        let end_time = (start_time.is_some() && !self.encoding_active).then_some(now);
        let state = QueueState {
            jobs,
            current_job_index: source.current_job_index,
            config_job_index: source.config_job_index,
            start_time,
            end_time,
            total_jobs_to_encode: source.total_jobs_to_encode,
            converted_count: source.converted_count,
            skipped_count: source.skipped_count,
            cancelled_count: source.cancelled_count,
            error_count: source.error_count,
            encoding_progress_done: source.encoding_progress_done,
            cleared_saved_bytes: source.cleared_saved_bytes,
        };
        (state, self.queue.ids.clone())
    }
}

/// Parses a `GET /api/snapshot` body.
///
/// # Errors
/// Returns [`ClientError::BadResponse`] when the body is not a snapshot.
pub fn parse_snapshot(body: Value) -> Result<RemoteSnapshot, ClientError> {
    serde_json::from_value(body).map_err(|e| ClientError::BadResponse(e.to_string()))
}

/// The snapshot the daemon serves for `state`.
#[cfg(test)]
pub fn snapshot_of(state: crate::daemon::state::DaemonState) -> RemoteSnapshot {
    let shared = Arc::new(std::sync::Mutex::new(state));
    let body = crate::daemon::api::snapshot_wait(&shared, None, Duration::ZERO, Duration::ZERO);
    parse_snapshot(body).expect("a snapshot")
}

/// An action the TUI asks the daemon to perform.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientRequest {
    Browse {
        path: String,
        show_hidden: bool,
    },
    /// Folder expansion without queueing (`dry_run`).
    AddPreview {
        path: String,
        mode: &'static str,
    },
    /// One `file` add per path, in order, stopping at the first error.
    Add {
        paths: Vec<String>,
    },
    AddFolder {
        path: String,
        mode: &'static str,
    },
    Remove(u64),
    MoveUp(u64),
    Cancel,
    CancelAnalysis,
    ClearFinished(Value),
    SetTracks(Value),
    DiscsList,
    DiscsScan(Value),
    DiscsRip(Value),
    DiscsCancel,
    SettingsGet,
    SettingsAccess,
    SettingsSet(Value),
    ServiceSet(Value),
}

/// Which [`ClientRequest`] a reply answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    Browse,
    AddPreview,
    Add,
    AddFolder,
    Remove,
    MoveUp,
    Cancel,
    CancelAnalysis,
    ClearFinished,
    SetTracks,
    DiscsList,
    DiscsScan,
    DiscsRip,
    DiscsCancel,
    SettingsGet,
    SettingsAccess,
    SettingsSet,
    ServiceSet,
}

impl ClientRequest {
    #[must_use]
    pub fn kind(&self) -> RequestKind {
        match self {
            Self::Browse { .. } => RequestKind::Browse,
            Self::AddPreview { .. } => RequestKind::AddPreview,
            Self::Add { .. } => RequestKind::Add,
            Self::AddFolder { .. } => RequestKind::AddFolder,
            Self::Remove(_) => RequestKind::Remove,
            Self::MoveUp(_) => RequestKind::MoveUp,
            Self::Cancel => RequestKind::Cancel,
            Self::CancelAnalysis => RequestKind::CancelAnalysis,
            Self::ClearFinished(_) => RequestKind::ClearFinished,
            Self::SetTracks(_) => RequestKind::SetTracks,
            Self::DiscsList => RequestKind::DiscsList,
            Self::DiscsScan(_) => RequestKind::DiscsScan,
            Self::DiscsRip(_) => RequestKind::DiscsRip,
            Self::DiscsCancel => RequestKind::DiscsCancel,
            Self::SettingsGet => RequestKind::SettingsGet,
            Self::SettingsAccess => RequestKind::SettingsAccess,
            Self::SettingsSet(_) => RequestKind::SettingsSet,
            Self::ServiceSet(_) => RequestKind::ServiceSet,
        }
    }

    /// The HTTP calls for this request, in order: a path and, for a POST, its
    /// JSON body (`None` is a GET).
    fn calls(self) -> Vec<(String, Option<Value>)> {
        let post = |path: &str, body: Value| vec![(path.to_string(), Some(body))];
        let get = |path: String| vec![(path, None)];
        match self {
            Self::Browse { path, show_hidden } => get(format!(
                "/api/fs?path={}&hidden={}",
                percent_encode(&path),
                u8::from(show_hidden)
            )),
            Self::AddPreview { path, mode } => post(
                "/api/queue/add",
                json!({"path": path, "mode": mode, "dry_run": true}),
            ),
            Self::Add { paths } => paths
                .into_iter()
                .map(|path| {
                    let body = json!({"path": path, "mode": "file"});
                    ("/api/queue/add".to_string(), Some(body))
                })
                .collect(),
            Self::AddFolder { path, mode } => {
                post("/api/queue/add", json!({"path": path, "mode": mode}))
            }
            Self::Remove(id) => post("/api/queue/remove", json!({"id": id})),
            Self::MoveUp(id) => post("/api/queue/move_up", json!({"id": id})),
            Self::Cancel => post("/api/queue/cancel", json!({})),
            Self::CancelAnalysis => post("/api/queue/cancel_analysis", json!({})),
            Self::ClearFinished(body) => post("/api/queue/clear_finished", body),
            Self::SetTracks(body) => post("/api/job/tracks", body),
            Self::DiscsList => post("/api/discs/list", json!({})),
            Self::DiscsScan(body) => post("/api/discs/scan", body),
            Self::DiscsRip(body) => post("/api/discs/rip", body),
            Self::DiscsCancel => post("/api/discs/cancel", json!({})),
            Self::SettingsGet => get("/api/settings".to_string()),
            Self::SettingsAccess => get("/api/settings/access".to_string()),
            Self::SettingsSet(body) => post("/api/settings", body),
            Self::ServiceSet(body) => post("/api/settings/service", body),
        }
    }
}

/// Encodes every byte outside the URL unreserved set as `%XX`.
fn percent_encode(s: &str) -> String {
    use std::fmt::Write as _;
    s.bytes()
        .fold(String::with_capacity(s.len()), |mut out, b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                out.push(char::from(b));
            } else {
                let _ = write!(out, "%{b:02X}");
            }
            out
        })
}

/// Runs the HTTP calls of `request`; returns the last reply with `added`
/// summed over all replies, or the first error.
fn perform(client: &DaemonClient, request: ClientRequest) -> Result<Value, ClientError> {
    let mut reply = Value::Null;
    let mut added = 0;
    for (path, body) in request.calls() {
        reply = match body {
            Some(body) => client.post(&path, &body)?,
            None => client.get(&path)?,
        };
        added += reply["added"].as_u64().unwrap_or(0);
    }
    if reply.get("added").is_some() {
        reply["added"] = json!(added);
    }
    Ok(reply)
}

/// Something the client threads report to the TUI.
pub enum ClientEvent {
    Snapshot(Box<RemoteSnapshot>),
    Reply {
        request: RequestKind,
        result: Result<Value, ClientError>,
    },
    /// The snapshot request failed; sent once per outage.
    Unreachable(String),
    /// The daemon answered 401 to the snapshot request with the configured
    /// token; sent once per outage.
    TokenRejected,
    /// A snapshot request succeeded after an outage.
    Reachable,
}

/// Spawns the long-poll and action threads. Dropping the returned sender
/// stops the action thread, which then raises the stop flag the poller exits on.
pub fn spawn(client: DaemonClient, events: Sender<ClientEvent>) -> Sender<ClientRequest> {
    spawn_threads(client, events).0
}

/// [`spawn`], also returning the poller's handle. The action thread raises a
/// shared stop flag when it returns; the poller exits once it sees the flag.
fn spawn_threads(
    client: DaemonClient,
    events: Sender<ClientEvent>,
) -> (Sender<ClientRequest>, JoinHandle<()>) {
    let (requests_tx, requests) = std::sync::mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let action_client = client.for_actions();
    let (poll_events, poll_stop) = (events.clone(), stop.clone());
    let poller = std::thread::spawn(move || run_poller(&client, &poll_events, &poll_stop));
    std::thread::spawn(move || {
        run_actions(&action_client, &requests, &events);
        stop.store(true, Ordering::SeqCst);
    });
    (requests_tx, poller)
}

/// Long-polls `/api/snapshot` with the last tag and forwards each changed
/// snapshot. A 401 is a rejected token; any other error counts as
/// unreachable. Returns when `stop` is set or `events` is disconnected.
fn run_poller(client: &DaemonClient, events: &Sender<ClientEvent>, stop: &AtomicBool) {
    let mut since: Option<String> = None;
    let mut down = false;
    let mut rejected = false;
    while !stop.load(Ordering::SeqCst) {
        let path = since.as_deref().map_or_else(
            || "/api/snapshot".to_string(),
            |tag| format!("/api/snapshot?since={tag}"),
        );
        match client.get(&path).and_then(parse_snapshot) {
            Ok(snapshot) => {
                if down {
                    down = false;
                    rejected = false;
                    if events.send(ClientEvent::Reachable).is_err() {
                        return;
                    }
                }
                if since.as_deref() == Some(snapshot.tag.as_str()) {
                    continue;
                }
                since = Some(snapshot.tag.clone());
                if events
                    .send(ClientEvent::Snapshot(Box::new(snapshot)))
                    .is_err()
                {
                    return;
                }
            }
            Err(error) => {
                since = None;
                let token = matches!(error, ClientError::Http { status: 401, .. });
                if !down || rejected != token {
                    down = true;
                    rejected = token;
                    let event = if token {
                        ClientEvent::TokenRejected
                    } else {
                        ClientEvent::Unreachable(error.to_string())
                    };
                    if events.send(event).is_err() {
                        return;
                    }
                }
                std::thread::sleep(RETRY_WAIT);
            }
        }
    }
}

/// Performs each request in turn, sends its `Reply`, then a fresh snapshot.
/// `AddPreview` requests go in order to a preview thread of their own, which
/// exits once this function returns and its queue is drained. Returns when
/// `requests` is disconnected or `events` is.
fn run_actions(
    client: &DaemonClient,
    requests: &Receiver<ClientRequest>,
    events: &Sender<ClientEvent>,
) {
    let (previews_tx, previews) = std::sync::mpsc::channel::<ClientRequest>();
    let (preview_client, preview_events) = (client.clone(), events.clone());
    std::thread::spawn(move || {
        for request in previews {
            if answer(&preview_client, request, &preview_events).is_err() {
                return;
            }
        }
    });
    for request in requests {
        if matches!(request, ClientRequest::AddPreview { .. }) {
            if previews_tx.send(request).is_err() {
                return;
            }
        } else if answer(client, request, events).is_err() {
            return;
        }
    }
}

/// Performs `request`, sends its `Reply`, then a fresh snapshot. Errs when
/// `events` is disconnected.
fn answer(
    client: &DaemonClient,
    request: ClientRequest,
    events: &Sender<ClientEvent>,
) -> Result<(), std::sync::mpsc::SendError<ClientEvent>> {
    let kind = request.kind();
    let result = perform(client, request);
    events.send(ClientEvent::Reply {
        request: kind,
        result,
    })?;
    if let Ok(snapshot) = client.get("/api/snapshot").and_then(parse_snapshot) {
        events.send(ClientEvent::Snapshot(Box::new(snapshot)))?;
    }
    Ok(())
}

/// The TUI's client for the daemon at `addr`: after a 401 it retries with
/// `daemon.auth_token` from config.toml.
#[must_use]
pub fn attached_client(addr: SocketAddr, token: String) -> DaemonClient {
    DaemonClient::new(addr, token).with_token_source(configured_token)
}

/// Address to connect to for a daemon listening on `listen`: wildcard
/// addresses map to loopback.
#[must_use]
pub fn attach_addr(listen: SocketAddr) -> SocketAddr {
    let ip = match listen.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    SocketAddr::new(ip, listen.port())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::daemon::state::{DaemonState, SharedState, lock};
    use crate::daemon::{api, server};
    use crate::queue::{EncodingJob, JobStatus};
    use serde_json::json;
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, mpsc};
    use std::thread::JoinHandle;

    const HANG_GUARD: Duration = Duration::from_secs(10);

    /// Serves one request on an ephemeral port: runs `check` on it, then
    /// answers with `status` and `body`.
    fn serve_once(
        status: u16,
        body: &'static str,
        check: impl FnOnce(&mut tiny_http::Request) + Send + 'static,
    ) -> (SocketAddr, JoinHandle<()>) {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("an ephemeral port");
        let addr = server.server_addr().to_ip().expect("an IP listener");
        let handle = std::thread::spawn(move || {
            let mut request = server.recv().expect("a request");
            check(&mut request);
            request
                .respond(tiny_http::Response::from_string(body).with_status_code(status))
                .expect("respond");
        });
        (addr, handle)
    }

    fn header(request: &tiny_http::Request, name: &'static str) -> Option<String> {
        request
            .headers()
            .iter()
            .find(|h| h.field.equiv(name))
            .map(|h| h.value.as_str().to_string())
    }

    #[test]
    fn get_returns_the_json_body_on_200() {
        let (addr, server) = serve_once(200, r#"{"a":1}"#, |request| {
            assert_eq!(request.url(), "/api/x?y=1");
            assert_eq!(
                header(request, "Authorization").as_deref(),
                Some("Bearer tok")
            );
        });
        let client = DaemonClient::new(addr, "tok".to_string());
        assert_eq!(client.get("/api/x?y=1").unwrap(), json!({"a":1}));
        server.join().expect("server thread");
    }

    #[test]
    fn a_401_becomes_an_http_error_with_the_body_message() {
        let (addr, server) = serve_once(401, r#"{"error":"missing or invalid token"}"#, |_| {});
        let err = DaemonClient::new(addr, "bad".to_string())
            .get("/api/x")
            .unwrap_err();
        assert!(matches!(
            &err,
            ClientError::Http { status: 401, error } if error == "missing or invalid token"
        ));
        server.join().expect("server thread");
    }

    #[test]
    fn a_non_json_error_body_falls_back_to_the_status_line() {
        let (addr, server) = serve_once(500, "oops", |_| {});
        let err = DaemonClient::new(addr, "t".to_string())
            .get("/api/x")
            .unwrap_err();
        assert!(
            matches!(
                &err,
                ClientError::Http { status: 500, error } if error == "HTTP/1.0 500 Internal Server Error"
            ),
            "{err:?}"
        );
        server.join().expect("server thread");
    }

    #[test]
    fn a_non_json_success_body_is_a_bad_response() {
        let (addr, server) = serve_once(200, "not json", |_| {});
        let err = DaemonClient::new(addr, "t".to_string())
            .get("/api/x")
            .unwrap_err();
        assert!(matches!(err, ClientError::BadResponse(_)));
        server.join().expect("server thread");
    }

    #[test]
    fn post_sends_json_with_content_type() {
        let (addr, server) = serve_once(200, "{}", |request| {
            assert_eq!(request.method(), &tiny_http::Method::Post);
            assert_eq!(
                header(request, "Content-Type").as_deref(),
                Some("application/json")
            );
            let mut body = String::new();
            std::io::Read::read_to_string(request.as_reader(), &mut body).expect("body");
            assert_eq!(body, r#"{"x":true}"#);
        });
        let client = DaemonClient::new(addr, "tok".to_string());
        assert_eq!(
            client.post("/api/y", &json!({"x":true})).unwrap(),
            json!({})
        );
        server.join().expect("server thread");
    }

    #[test]
    fn a_closed_port_is_unreachable() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        drop(listener);
        let result = DaemonClient::new(addr, "t".to_string()).get("/api/x");
        assert!(matches!(result, Err(ClientError::Unreachable(_))));
    }

    fn shared_with_jobs(statuses: Vec<JobStatus>) -> (SharedState, Vec<u64>) {
        let mut state = DaemonState::new(AppConfig::default());
        let ids = statuses
            .into_iter()
            .enumerate()
            .map(|(i, status)| {
                let mut job = EncodingJob::new(PathBuf::from(format!("/nonexistent/{i}.mkv")));
                job.status = status;
                state.queue.push(job)
            })
            .collect();
        (Arc::new(Mutex::new(state)), ids)
    }

    fn snapshot_body(shared: &SharedState) -> String {
        api::snapshot_wait(shared, None, Duration::ZERO, Duration::ZERO).to_string()
    }

    #[test]
    fn a_snapshot_round_trips_into_queue_state() {
        let (shared, ids) = shared_with_jobs(vec![
            JobStatus::Encoding { progress: 42.5 },
            JobStatus::Ripping { progress: 10.0 },
            JobStatus::Ready,
        ]);
        lock(&shared).queue.state.converted_count = 3;
        lock(&shared).instance = u64::MAX;

        let snapshot: RemoteSnapshot =
            serde_json::from_str(&snapshot_body(&shared)).expect("a snapshot");
        let (queue, queue_ids) = snapshot.queue_state();

        assert_eq!(queue_ids, ids);
        assert_eq!(queue.converted_count, 3);
        assert_eq!(queue.jobs.len(), 3);
        assert!(matches!(
            queue.jobs[0].status,
            JobStatus::Encoding { progress } if (progress - 42.5).abs() < f64::EPSILON
        ));
        assert!(matches!(
            queue.jobs[1].status,
            JobStatus::Ripping { progress } if (progress - 10.0).abs() < f64::EPSILON
        ));
        assert!(matches!(queue.jobs[2].status, JobStatus::Ready));
        assert_eq!(snapshot.tag.len(), 16);
        assert_eq!(snapshot.instance, u64::MAX);
    }

    #[test]
    fn a_snapshot_missing_a_field_fails_to_parse() {
        let (shared, _) = shared_with_jobs(vec![]);
        let mut body: Value = serde_json::from_str(&snapshot_body(&shared)).expect("JSON");
        body.as_object_mut().expect("an object").remove("tag");
        assert!(matches!(
            parse_snapshot(body),
            Err(ClientError::BadResponse(_))
        ));
    }

    /// Answers every request on an ephemeral port: 503 for the first
    /// `failures`, then 200 with `body`, until `stop` is set.
    fn scripted_server(
        failures: usize,
        body: String,
        stop: Arc<AtomicBool>,
    ) -> (SocketAddr, JoinHandle<()>) {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("an ephemeral port");
        let addr = server.server_addr().to_ip().expect("an IP listener");
        let handle = std::thread::spawn(move || {
            let mut answered = 0;
            while !stop.load(Ordering::SeqCst) {
                let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(50)) else {
                    continue;
                };
                let response = if answered < failures {
                    tiny_http::Response::from_string(r#"{"error":"down"}"#).with_status_code(503)
                } else {
                    tiny_http::Response::from_string(body.as_str()).with_status_code(200)
                };
                answered += 1;
                let _ = request.respond(response);
            }
        });
        (addr, handle)
    }

    #[test]
    fn the_poller_reports_unreachable_then_reachable() {
        let (shared, ids) = shared_with_jobs(vec![JobStatus::Ready]);
        let stop = Arc::new(AtomicBool::new(false));
        let (addr, server) = scripted_server(2, snapshot_body(&shared), stop.clone());
        let (events_tx, events) = mpsc::channel();
        let requests = spawn(DaemonClient::new(addr, "t".to_string()), events_tx);

        let next = || events.recv_timeout(HANG_GUARD).expect("an event");
        assert!(matches!(next(), ClientEvent::Unreachable(_)));
        assert!(matches!(next(), ClientEvent::Reachable));
        let ClientEvent::Snapshot(snapshot) = next() else {
            panic!("expected a snapshot");
        };
        assert_eq!(snapshot.queue.ids, ids);

        drop(requests);
        drop(events);
        stop.store(true, Ordering::SeqCst);
        server.join().expect("server thread");
    }

    const TOKEN: &str = "a-token-long-enough-to-be-a-real-one";

    /// Runs the daemon's real HTTP server on an ephemeral port.
    fn real_server(shared: &SharedState) -> (SocketAddr, Arc<AtomicBool>, Vec<JoinHandle<()>>) {
        lock(shared).config.daemon.auth_token = TOKEN.to_string();
        let server = Arc::new(server::bind("127.0.0.1:0").expect("an ephemeral port"));
        let addr = server.server_addr().to_ip().expect("an IP listener");
        let shutdown = Arc::new(AtomicBool::new(false));
        let (probe_tx, probe_rx) = mpsc::channel();
        let (disc_tx, disc_rx) = mpsc::channel();
        let handles = (0..2)
            .map(|_| {
                let (server, shared, shutdown) = (server.clone(), shared.clone(), shutdown.clone());
                let (probe_tx, disc_tx) = (probe_tx.clone(), disc_tx.clone());
                std::thread::spawn(move || {
                    server::serve(&server, &shared, &probe_tx, &disc_tx, &shutdown);
                })
            })
            .collect();
        std::mem::forget((probe_rx, disc_rx));
        (addr, shutdown, handles)
    }

    fn stop_server(shutdown: &AtomicBool, handles: Vec<JoinHandle<()>>) {
        shutdown.store(true, Ordering::SeqCst);
        for handle in handles {
            handle.join().expect("a server thread");
        }
    }

    #[test]
    fn an_action_is_followed_by_a_fresh_snapshot() {
        let (shared, ids) = shared_with_jobs(vec![JobStatus::Ready, JobStatus::Ready]);
        let (addr, shutdown, handles) = real_server(&shared);
        let (requests_tx, requests) = mpsc::channel();
        let (events_tx, events) = mpsc::channel();
        let client = DaemonClient::new(addr, TOKEN.to_string());
        let actions = std::thread::spawn(move || run_actions(&client, &requests, &events_tx));

        requests_tx
            .send(ClientRequest::Remove(ids[0]))
            .expect("send");
        let next = || events.recv_timeout(HANG_GUARD).expect("an event");
        assert!(matches!(
            next(),
            ClientEvent::Reply {
                request: RequestKind::Remove,
                result: Ok(_)
            }
        ));
        let ClientEvent::Snapshot(snapshot) = next() else {
            panic!("expected a snapshot");
        };
        assert_eq!(snapshot.queue.ids, vec![ids[1]]);

        requests_tx
            .send(ClientRequest::Remove(ids[0]))
            .expect("send");
        assert!(matches!(
            next(),
            ClientEvent::Reply {
                request: RequestKind::Remove,
                result: Err(ClientError::Http { status: 404, .. })
            }
        ));
        assert!(matches!(next(), ClientEvent::Snapshot(_)));

        drop(requests_tx);
        actions.join().expect("the action thread");
        stop_server(&shutdown, handles);
    }

    #[test]
    fn the_poller_stops_once_the_handles_are_dropped() {
        let (shared, _) = shared_with_jobs(vec![JobStatus::Ready]);
        let (addr, shutdown, handles) = real_server(&shared);
        let (events_tx, events) = mpsc::channel();
        let (requests, poller) =
            spawn_threads(DaemonClient::new(addr, TOKEN.to_string()), events_tx);
        assert!(matches!(
            events.recv_timeout(HANG_GUARD),
            Ok(ClientEvent::Snapshot(_))
        ));

        drop(requests);
        drop(events);
        let (joined_tx, joined) = mpsc::channel();
        std::thread::spawn(move || {
            poller.join().expect("the poller");
            let _ = joined_tx.send(());
        });
        assert!(joined.recv_timeout(HANG_GUARD).is_ok());
        stop_server(&shutdown, handles);
    }

    #[test]
    fn browse_sends_the_path_percent_encoded() {
        assert_eq!(
            ClientRequest::Browse {
                path: "/a b/ü&x".to_string(),
                show_hidden: true
            }
            .calls(),
            vec![(
                "/api/fs?path=%2Fa%20b%2F%C3%BC%26x&hidden=1".to_string(),
                None
            )]
        );
        assert_eq!(
            ClientRequest::Browse {
                path: "/m".to_string(),
                show_hidden: false
            }
            .calls(),
            vec![("/api/fs?path=%2Fm&hidden=0".to_string(), None)]
        );
    }

    #[test]
    fn add_preview_is_a_dry_run_add() {
        assert_eq!(
            ClientRequest::AddPreview {
                path: "/m".to_string(),
                mode: "folder"
            }
            .calls(),
            vec![(
                "/api/queue/add".to_string(),
                Some(json!({"path": "/m", "mode": "folder", "dry_run": true}))
            )]
        );
    }

    #[test]
    fn add_posts_each_file_in_order_and_stops_at_the_first_error() {
        let dir = std::env::temp_dir().join(format!("av1c-client-add-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let (a, b) = (dir.join("a.mkv"), dir.join("b.mkv"));
        std::fs::write(&a, b"").expect("a.mkv");
        std::fs::write(&b, b"").expect("b.mkv");
        let shared = shared_with_jobs(vec![]).0;
        let (addr, shutdown, handles) = real_server(&shared);
        let client = DaemonClient::new(addr, TOKEN.to_string());

        let paths = [&a, &dir.join("missing.mkv"), &b]
            .map(|p| p.to_string_lossy().into_owned())
            .to_vec();
        let result = perform(&client, ClientRequest::Add { paths });

        assert!(matches!(result, Err(ClientError::Http { status: 400, .. })));
        let queued: Vec<String> = lock(&shared)
            .queue
            .state
            .jobs
            .iter()
            .map(|job| job.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(queued, vec!["a.mkv".to_string()]);
        stop_server(&shutdown, handles);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adding_several_files_reports_the_total_added() {
        let dir = std::env::temp_dir().join(format!("av1c-client-total-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let paths: Vec<String> = ["a.mkv", "b.mkv"]
            .iter()
            .map(|name| {
                let path = dir.join(name);
                std::fs::write(&path, b"").expect("a file");
                path.to_string_lossy().into_owned()
            })
            .collect();
        let shared = shared_with_jobs(vec![]).0;
        let (addr, shutdown, handles) = real_server(&shared);
        let client = DaemonClient::new(addr, TOKEN.to_string());

        let first = perform(
            &client,
            ClientRequest::Add {
                paths: paths.clone(),
            },
        );
        let again = perform(&client, ClientRequest::Add { paths });

        assert_eq!(first.expect("a reply")["added"], json!(2));
        assert_eq!(again.expect("a reply")["added"], json!(0));
        stop_server(&shutdown, handles);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Answers `count` requests: 200 to `Bearer accepted`, 401 to any other
    /// token; returns the address and the tokens it saw.
    fn token_server(accepted: &'static str, count: usize) -> (SocketAddr, JoinHandle<Vec<String>>) {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("an ephemeral port");
        let addr = server.server_addr().to_ip().expect("an IP listener");
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for _ in 0..count {
                let request = server.recv().expect("a request");
                let auth = header(&request, "Authorization").unwrap_or_default();
                let (status, body) = if auth == format!("Bearer {accepted}") {
                    (200, r#"{"ok":true}"#)
                } else {
                    (401, r#"{"error":"missing or invalid token"}"#)
                };
                seen.push(auth);
                request
                    .respond(tiny_http::Response::from_string(body).with_status_code(status))
                    .expect("respond");
            }
            seen
        });
        (addr, handle)
    }

    #[allow(clippy::unnecessary_wraps)]
    fn token_b() -> Option<String> {
        Some("B".to_string())
    }

    #[allow(clippy::unnecessary_wraps)]
    fn token_a() -> Option<String> {
        Some("A".to_string())
    }

    #[test]
    fn a_401_retries_once_with_the_configured_token_and_keeps_it() {
        let (addr, server) = token_server("B", 3);
        let client = DaemonClient::new(addr, "A".to_string()).with_token_source(token_b);
        let actions = client.for_actions();

        assert_eq!(client.get("/api/x").unwrap(), json!({"ok": true}));
        assert_eq!(actions.get("/api/y").unwrap(), json!({"ok": true}));

        assert_eq!(
            server.join().expect("server thread"),
            vec!["Bearer A", "Bearer B", "Bearer B"]
        );
    }

    #[test]
    fn a_401_with_an_unchanged_configured_token_is_not_retried() {
        let (addr, server) = token_server("B", 1);
        let client = DaemonClient::new(addr, "A".to_string()).with_token_source(token_a);

        let result = client.get("/api/x");

        assert!(matches!(result, Err(ClientError::Http { status: 401, .. })));
        assert_eq!(server.join().expect("server thread"), vec!["Bearer A"]);
    }

    #[test]
    fn the_poller_reports_a_rejected_token_apart_from_unreachable() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("an ephemeral port");
        let addr = server.server_addr().to_ip().expect("an IP listener");
        let stop = Arc::new(AtomicBool::new(false));
        let server_stop = stop.clone();
        let server = std::thread::spawn(move || {
            while !server_stop.load(Ordering::SeqCst) {
                if let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(50)) {
                    let _ = request
                        .respond(tiny_http::Response::from_string("{}").with_status_code(401));
                }
            }
        });
        let (events_tx, events) = mpsc::channel();
        let poll_stop = stop.clone();
        let client = DaemonClient::new(addr, "A".to_string()).with_token_source(token_b);
        let poller = std::thread::spawn(move || run_poller(&client, &events_tx, &poll_stop));

        assert!(matches!(
            events.recv_timeout(HANG_GUARD),
            Ok(ClientEvent::TokenRejected)
        ));

        stop.store(true, Ordering::SeqCst);
        poller.join().expect("the poller");
        server.join().expect("server thread");
    }

    const CONFIGURED: &str = "b-token-long-enough-to-be-a-real-one";

    fn write_config(content: &str) {
        let path = AppConfig::config_path();
        std::fs::create_dir_all(path.parent().expect("a config dir")).expect("the config dir");
        std::fs::write(path, content).expect("config.toml");
    }

    #[test]
    fn the_configured_token_is_read_from_config_toml() {
        let _ = std::fs::remove_file(AppConfig::config_path());
        assert_eq!(configured_token(), None);

        write_config(&format!("[daemon]\nauth_token = \"  {CONFIGURED} \"\n"));
        assert_eq!(configured_token().as_deref(), Some(CONFIGURED));

        write_config("[daemon\nnot toml");
        assert_eq!(configured_token(), None);
        let _ = std::fs::remove_file(AppConfig::config_path());
    }

    #[test]
    fn the_attached_client_retries_a_401_with_the_token_in_config_toml() {
        write_config(&format!("[daemon]\nauth_token = \"{CONFIGURED}\"\n"));
        let (addr, server) = token_server(CONFIGURED, 2);

        let result = attached_client(addr, "A".to_string()).get("/api/x");

        assert_eq!(result.unwrap(), json!({"ok": true}));
        assert_eq!(
            server.join().expect("server thread"),
            vec!["Bearer A".to_string(), format!("Bearer {CONFIGURED}")]
        );
        let _ = std::fs::remove_file(AppConfig::config_path());
    }

    #[test]
    fn a_new_client_does_not_reload_the_token_after_a_401() {
        write_config(&format!("[daemon]\nauth_token = \"{CONFIGURED}\"\n"));
        let (addr, server) = token_server(CONFIGURED, 1);

        let result = DaemonClient::new(addr, "A".to_string()).get("/api/x");

        assert!(matches!(result, Err(ClientError::Http { status: 401, .. })));
        assert_eq!(server.join().expect("server thread"), vec!["Bearer A"]);
        let _ = std::fs::remove_file(AppConfig::config_path());
    }

    #[test]
    fn a_cancel_sent_after_a_preview_is_answered_before_the_preview() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("an ephemeral port");
        let addr = server.server_addr().to_ip().expect("an IP listener");
        let (release_tx, release) = mpsc::channel::<()>();
        let stop = Arc::new(AtomicBool::new(false));
        let server_stop = stop.clone();
        let server = std::thread::spawn(move || {
            let mut held = Vec::new();
            while !server_stop.load(Ordering::SeqCst) {
                if release.try_recv().is_ok() {
                    for request in held.drain(..) {
                        let _ = tiny_http::Request::respond(
                            request,
                            tiny_http::Response::from_string(r#"{"files":[]}"#),
                        );
                    }
                }
                let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(20)) else {
                    continue;
                };
                if request.url() == "/api/queue/add" {
                    held.push(request);
                } else {
                    let _ = request.respond(tiny_http::Response::from_string("{}"));
                }
            }
        });
        let (requests_tx, requests) = mpsc::channel();
        let (events_tx, events) = mpsc::channel();
        let client = DaemonClient::new(addr, "t".to_string()).for_actions();
        let actions = std::thread::spawn(move || run_actions(&client, &requests, &events_tx));

        requests_tx
            .send(ClientRequest::AddPreview {
                path: "/m".to_string(),
                mode: "recursive",
            })
            .expect("send");
        requests_tx.send(ClientRequest::Cancel).expect("send");
        let next = || events.recv_timeout(HANG_GUARD).expect("an event");
        assert!(matches!(
            next(),
            ClientEvent::Reply {
                request: RequestKind::Cancel,
                result: Ok(_)
            }
        ));
        release_tx.send(()).expect("release");
        assert!(matches!(
            next(),
            ClientEvent::Reply {
                request: RequestKind::AddPreview,
                result: Ok(_)
            }
        ));

        drop(requests_tx);
        actions.join().expect("the action thread");
        stop.store(true, Ordering::SeqCst);
        server.join().expect("server thread");
    }

    #[test]
    fn previews_are_answered_in_the_order_they_were_sent() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("an ephemeral port");
        let addr = server.server_addr().to_ip().expect("an IP listener");
        let (release_tx, release) = mpsc::channel::<()>();
        let stop = Arc::new(AtomicBool::new(false));
        let server_stop = stop.clone();
        let server = std::thread::spawn(move || {
            let mut held = None;
            while !server_stop.load(Ordering::SeqCst) {
                if release.try_recv().is_ok()
                    && let Some(request) = held.take()
                {
                    let _ = tiny_http::Request::respond(
                        request,
                        tiny_http::Response::from_string(r#"{"path":"/A"}"#),
                    );
                }
                let Ok(Some(mut request)) = server.recv_timeout(Duration::from_millis(20)) else {
                    continue;
                };
                let mut body = String::new();
                let _ = std::io::Read::read_to_string(request.as_reader(), &mut body);
                if body.contains(r#""path":"/A""#) {
                    held = Some(request);
                } else if body.contains(r#""path":"/B""#) {
                    let _ = request.respond(tiny_http::Response::from_string(r#"{"path":"/B"}"#));
                } else {
                    let _ = request.respond(tiny_http::Response::from_string("{}"));
                }
            }
        });
        let (requests_tx, requests) = mpsc::channel();
        let (events_tx, events) = mpsc::channel();
        let client = DaemonClient::new(addr, "t".to_string()).for_actions();
        let actions = std::thread::spawn(move || run_actions(&client, &requests, &events_tx));
        let preview = |path: &str| ClientRequest::AddPreview {
            path: path.to_string(),
            mode: "recursive",
        };

        requests_tx.send(preview("/A")).expect("send");
        requests_tx.send(preview("/B")).expect("send");
        requests_tx.send(ClientRequest::Cancel).expect("send");
        let next = || events.recv_timeout(HANG_GUARD).expect("an event");
        let preview_path = |event: ClientEvent| match event {
            ClientEvent::Reply {
                request: RequestKind::AddPreview,
                result: Ok(value),
            } => value["path"].as_str().map(str::to_string),
            _ => None,
        };
        loop {
            let event = next();
            if matches!(
                event,
                ClientEvent::Reply {
                    request: RequestKind::Cancel,
                    ..
                }
            ) {
                break;
            }
            assert_eq!(preview_path(event), None);
        }
        release_tx.send(()).expect("release");
        let replies: Vec<String> = std::iter::from_fn(|| Some(next()))
            .filter_map(preview_path)
            .take(2)
            .collect();
        assert_eq!(replies, vec!["/A".to_string(), "/B".to_string()]);

        drop(requests_tx);
        actions.join().expect("the action thread");
        stop.store(true, Ordering::SeqCst);
        server.join().expect("server thread");
    }

    #[test]
    fn actions_wait_longer_for_a_reply_than_the_snapshot_poll() {
        let client = DaemonClient::new("127.0.0.1:1".parse().unwrap(), "t".to_string());
        assert_eq!(client.read_timeout, Duration::from_secs(5));
        assert_eq!(client.for_actions().read_timeout, Duration::from_secs(600));
    }

    #[test]
    fn wildcard_listen_addresses_attach_on_loopback() {
        let attach = |s: &str| attach_addr(s.parse().unwrap());
        assert_eq!(attach("0.0.0.0:8080"), "127.0.0.1:8080".parse().unwrap());
        assert_eq!(attach("[::]:8080"), "[::1]:8080".parse().unwrap());
        assert_eq!(
            attach("192.168.1.5:8080"),
            "192.168.1.5:8080".parse().unwrap()
        );
    }
}
