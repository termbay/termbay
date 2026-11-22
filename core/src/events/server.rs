//! Local event server (task A1, decision E6 in `docs/FASE2-PLANO.md`).
//!
//! One endpoint, `POST /v1/events`, on `127.0.0.1` at a random port, for
//! [`ReducedEvent`]s sent by `termbay-hook`. Minimal HTTP/1.1 written by hand:
//! one request per connection, `Connection: close`, small limits everywhere.
//!
//! What it checks, in order: the request line (method and path), the headers
//! (size, `Host` to stop DNS rebinding, `Authorization: Bearer <token>`
//! compared in constant time, `Content-Length` within [`MAX_BODY`]), then the
//! body (a [`ReducedEvent`] of a kept event, cleaned again with
//! [`ReducedEvent::clamp`]). Only then is the event handed to the sink.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::model::ReducedEvent;

/// Path of the only endpoint.
pub const EVENTS_PATH: &str = "/v1/events";
/// Largest accepted body. Reduced events are a few hundred bytes.
pub const MAX_BODY: usize = 64 * 1024;
/// Largest request line plus headers.
pub const MAX_HEAD: usize = 8 * 1024;
/// How long a connection may take to send its request.
pub const READ_TIMEOUT: Duration = Duration::from_secs(2);
/// Connections handled at the same time; more are closed right away.
pub const MAX_CONNECTIONS: usize = 32;
/// Bytes of randomness in the session token.
const TOKEN_BYTES: usize = 32;

/// Receives every accepted event, on the connection's thread.
pub type EventSink = dyn Fn(ReducedEvent) + Send + Sync + 'static;

/// The running server. Dropping it stops the server and waits for it.
pub struct EventServer {
    addr: SocketAddr,
    token: String,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
}

impl EventServer {
    /// Binds `127.0.0.1:0`, makes a fresh token and starts accepting.
    pub fn start(sink: impl Fn(ReducedEvent) + Send + Sync + 'static) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let addr = listener.local_addr()?;
        let token = new_token()?;
        let stop = Arc::new(AtomicBool::new(false));

        let shared = Arc::new(Shared {
            token: token.clone(),
            addr,
            sink: Box::new(sink),
            active: AtomicUsize::new(0),
        });
        let accept = {
            let stop = Arc::clone(&stop);
            thread::Builder::new()
                .name("termbay-events".into())
                .spawn(move || accept_loop(&listener, &stop, &shared))?
        };
        Ok(Self {
            addr,
            token,
            stop,
            accept: Some(accept),
        })
    }

    /// Address to give to hooks (`TERMBAY_EVENTS`).
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Token to give to hooks (`TERMBAY_TOKEN`): 64 hex characters.
    pub fn token(&self) -> &str {
        &self.token
    }
}

impl Drop for EventServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the blocking accept() so the loop sees the flag.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(500));
        if let Some(handle) = self.accept.take() {
            let _ = handle.join();
        }
    }
}

struct Shared {
    token: String,
    addr: SocketAddr,
    sink: Box<EventSink>,
    active: AtomicUsize,
}

fn accept_loop(listener: &TcpListener, stop: &AtomicBool, shared: &Arc<Shared>) {
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(stream) = stream else { continue };
        // Bound to 127.0.0.1, but checked anyway.
        if !stream.peer_addr().is_ok_and(|peer| peer.ip().is_loopback()) {
            continue;
        }
        if shared.active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            shared.active.fetch_sub(1, Ordering::SeqCst);
            continue; // dropped: closes the connection
        }
        let conn = Arc::clone(shared);
        let spawned = thread::Builder::new()
            .name("termbay-events-conn".into())
            .spawn(move || {
                handle(stream, &conn);
                conn.active.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            shared.active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// HTTP status sent back. Hooks ignore the answer; it helps tests and debugging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    NoContent,
    BadRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    MethodNotAllowed,
    Timeout,
    LengthRequired,
    TooLarge,
    HeadersTooLarge,
    Unprocessable,
}

impl Status {
    fn line(self) -> &'static str {
        match self {
            Self::NoContent => "204 No Content",
            Self::BadRequest => "400 Bad Request",
            Self::Unauthorized => "401 Unauthorized",
            Self::Forbidden => "403 Forbidden",
            Self::NotFound => "404 Not Found",
            Self::MethodNotAllowed => "405 Method Not Allowed",
            Self::Timeout => "408 Request Timeout",
            Self::LengthRequired => "411 Length Required",
            Self::TooLarge => "413 Content Too Large",
            Self::HeadersTooLarge => "431 Request Header Fields Too Large",
            Self::Unprocessable => "422 Unprocessable Content",
        }
    }
}

fn handle(stream: TcpStream, shared: &Shared) {
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let _ = stream.set_write_timeout(Some(READ_TIMEOUT));
    let mut writer = match stream.try_clone() {
        Ok(writer) => writer,
        Err(_) => return,
    };
    let status = match read_request(BufReader::new(stream), shared) {
        Ok(event) => {
            (shared.sink)(event);
            Status::NoContent
        }
        Err(status) => status,
    };
    let response = format!(
        "HTTP/1.1 {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        status.line()
    );
    let _ = writer.write_all(response.as_bytes());
    let _ = writer.flush();
}

fn read_request(mut reader: impl BufRead, shared: &Shared) -> Result<ReducedEvent, Status> {
    let mut head_left = MAX_HEAD;
    let request_line = read_line(&mut reader, &mut head_left)?;
    let mut parts = request_line.split(' ');
    let (method, path, version) = (parts.next(), parts.next(), parts.next());
    if parts.next().is_some() || !matches!(version, Some("HTTP/1.1" | "HTTP/1.0")) {
        return Err(Status::BadRequest);
    }
    if path != Some(EVENTS_PATH) {
        return Err(Status::NotFound);
    }
    if method != Some("POST") {
        return Err(Status::MethodNotAllowed);
    }

    let mut host = None;
    let mut authorization = None;
    let mut length = None;
    loop {
        let line = read_line(&mut reader, &mut head_left)?;
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':').ok_or(Status::BadRequest)?;
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "host" => set_once(&mut host, value)?,
            "authorization" => set_once(&mut authorization, value)?,
            "content-length" => set_once(&mut length, value)?,
            "transfer-encoding" => return Err(Status::BadRequest), // no chunked bodies
            _ => {}
        }
    }

    if !host.is_some_and(|h| host_is_allowed(&h, shared.addr.port())) {
        return Err(Status::Forbidden);
    }
    let presented = authorization
        .as_deref()
        .and_then(|a| a.strip_prefix("Bearer "))
        .unwrap_or("");
    if !constant_time_eq(presented.as_bytes(), shared.token.as_bytes()) {
        return Err(Status::Unauthorized);
    }
    let length: usize = length
        .ok_or(Status::LengthRequired)?
        .parse()
        .map_err(|_| Status::BadRequest)?;
    if length > MAX_BODY {
        return Err(Status::TooLarge);
    }

    let mut body = vec![0; length];
    reader.read_exact(&mut body).map_err(|e| io_status(&e))?;
    let mut event: ReducedEvent =
        serde_json::from_slice(&body).map_err(|_| Status::Unprocessable)?;
    event.clamp();
    if !event.is_kept() || event.session.is_empty() {
        return Err(Status::Unprocessable);
    }
    Ok(event)
}

/// One CRLF-terminated line of the head, counted against `left`.
fn read_line(reader: &mut impl BufRead, left: &mut usize) -> Result<String, Status> {
    let mut line = Vec::new();
    let limit = u64::try_from(*left).unwrap_or(u64::MAX) + 1;
    let read = reader
        .take(limit)
        .read_until(b'\n', &mut line)
        .map_err(|e| io_status(&e))?;
    if read == 0 {
        return Err(Status::BadRequest);
    }
    if read > *left || !line.ends_with(b"\n") {
        return Err(if read > *left {
            Status::HeadersTooLarge
        } else {
            Status::BadRequest
        });
    }
    *left -= read;
    line.pop();
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    String::from_utf8(line).map_err(|_| Status::BadRequest)
}

fn set_once(slot: &mut Option<String>, value: &str) -> Result<(), Status> {
    if slot.is_some() {
        return Err(Status::BadRequest); // duplicated header
    }
    *slot = Some(value.to_owned());
    Ok(())
}

fn io_status(error: &io::Error) -> Status {
    match error.kind() {
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => Status::Timeout,
        _ => Status::BadRequest,
    }
}

/// `Host` must name this server by loopback address: a page that rebinds a
/// domain to 127.0.0.1 still sends its own domain here.
fn host_is_allowed(host: &str, port: u16) -> bool {
    let port = port.to_string();
    host.rsplit_once(':')
        .is_some_and(|(name, p)| p == port && matches!(name, "127.0.0.1" | "localhost"))
}

/// Compares without stopping at the first difference.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}

fn new_token() -> io::Result<String> {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|e| io::Error::other(format!("no system randomness: {e}")))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::mpsc::{self, Receiver};
    use std::time::Instant;

    use super::*;

    fn server() -> (EventServer, Receiver<ReducedEvent>) {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let server = EventServer::start(move |event| {
            let _ = tx.lock().unwrap().send(event);
        })
        .unwrap();
        (server, rx)
    }

    const EVENT: &str =
        r#"{"v":1,"event":"PreToolUse","session":"s1","tool":"Read","file":"api.ts"}"#;

    /// Sends raw bytes and returns the status line.
    fn send_raw(server: &EventServer, request: &[u8]) -> String {
        let mut stream = TcpStream::connect(server.addr()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let _ = stream.write_all(request);
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
        response.lines().next().unwrap_or_default().to_owned()
    }

    fn request(server: &EventServer, host: &str, auth: Option<&str>, body: &str) -> String {
        let auth = auth
            .map(|t| format!("Authorization: Bearer {t}\r\n"))
            .unwrap_or_default();
        let raw = format!(
            "POST /v1/events HTTP/1.1\r\nHost: {host}\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        send_raw(server, raw.as_bytes())
    }

    fn host(server: &EventServer) -> String {
        format!("127.0.0.1:{}", server.addr().port())
    }

    #[test]
    fn listens_on_loopback_with_a_fresh_token() {
        let (a, _) = server();
        let (b, _) = server();
        assert!(a.addr().ip().is_loopback());
        assert_ne!(a.addr().port(), 0);
        assert_eq!(a.token().len(), 64);
        assert!(a.token().bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a.token(), b.token());
    }

    #[test]
    fn accepts_a_valid_event_and_hands_it_to_the_sink() {
        let (server, rx) = server();
        let status = request(&server, &host(&server), Some(server.token()), EVENT);
        assert_eq!(status, "HTTP/1.1 204 No Content");
        let event = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            (event.event.as_str(), event.file.as_deref()),
            ("PreToolUse", Some("api.ts"))
        );

        let localhost = format!("localhost:{}", server.addr().port());
        assert_eq!(
            request(&server, &localhost, Some(server.token()), EVENT),
            "HTTP/1.1 204 No Content"
        );
    }

    #[test]
    fn refuses_a_missing_or_wrong_token() {
        let (server, rx) = server();
        let h = host(&server);
        assert_eq!(
            request(&server, &h, None, EVENT),
            "HTTP/1.1 401 Unauthorized"
        );
        assert_eq!(
            request(&server, &h, Some("nope"), EVENT),
            "HTTP/1.1 401 Unauthorized"
        );
        let mut almost = server.token().to_owned();
        almost.replace_range(63.., if almost.ends_with('0') { "1" } else { "0" });
        assert_eq!(
            request(&server, &h, Some(&almost), EVENT),
            "HTTP/1.1 401 Unauthorized"
        );
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn refuses_other_hosts_methods_and_paths() {
        let (server, rx) = server();
        let t = server.token().to_owned();
        let port = server.addr().port();
        assert_eq!(
            request(&server, &format!("evil.example:{port}"), Some(&t), EVENT),
            "HTTP/1.1 403 Forbidden"
        );
        assert_eq!(
            request(&server, "127.0.0.1:1", Some(&t), EVENT),
            "HTTP/1.1 403 Forbidden"
        );
        assert_eq!(
            request(&server, "127.0.0.1", Some(&t), EVENT),
            "HTTP/1.1 403 Forbidden"
        );

        let get = format!("GET /v1/events HTTP/1.1\r\nHost: {}\r\n\r\n", host(&server));
        assert_eq!(
            send_raw(&server, get.as_bytes()),
            "HTTP/1.1 405 Method Not Allowed"
        );
        let other = format!("POST /v1/other HTTP/1.1\r\nHost: {}\r\n\r\n", host(&server));
        assert_eq!(
            send_raw(&server, other.as_bytes()),
            "HTTP/1.1 404 Not Found"
        );
        assert_eq!(
            send_raw(&server, b"garbage\r\n\r\n"),
            "HTTP/1.1 400 Bad Request"
        );
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn refuses_bad_bodies() {
        let (server, rx) = server();
        let (h, t) = (host(&server), server.token().to_owned());
        assert_eq!(
            request(&server, &h, Some(&t), "not json"),
            "HTTP/1.1 422 Unprocessable Content"
        );
        let unused = r#"{"v":1,"event":"MessageDisplay","session":"s1"}"#;
        assert_eq!(
            request(&server, &h, Some(&t), unused),
            "HTTP/1.1 422 Unprocessable Content"
        );
        let no_session = r#"{"v":1,"event":"PreToolUse"}"#;
        assert_eq!(
            request(&server, &h, Some(&t), no_session),
            "HTTP/1.1 422 Unprocessable Content"
        );

        let huge = format!(
            "POST /v1/events HTTP/1.1\r\nHost: {h}\r\nAuthorization: Bearer {t}\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        );
        assert_eq!(
            send_raw(&server, huge.as_bytes()),
            "HTTP/1.1 413 Content Too Large"
        );
        let no_length =
            format!("POST /v1/events HTTP/1.1\r\nHost: {h}\r\nAuthorization: Bearer {t}\r\n\r\n");
        assert_eq!(
            send_raw(&server, no_length.as_bytes()),
            "HTTP/1.1 411 Length Required"
        );
        let chunked = format!(
            "POST /v1/events HTTP/1.1\r\nHost: {h}\r\nAuthorization: Bearer {t}\r\nTransfer-Encoding: chunked\r\n\r\n"
        );
        assert_eq!(
            send_raw(&server, chunked.as_bytes()),
            "HTTP/1.1 400 Bad Request"
        );
        let twice = format!("POST /v1/events HTTP/1.1\r\nHost: {h}\r\nHost: {h}\r\n\r\n");
        assert_eq!(
            send_raw(&server, twice.as_bytes()),
            "HTTP/1.1 400 Bad Request"
        );
        let big_head = format!(
            "POST /v1/events HTTP/1.1\r\nX-Pad: {}\r\n\r\n",
            "a".repeat(MAX_HEAD)
        );
        assert_eq!(
            send_raw(&server, big_head.as_bytes()),
            "HTTP/1.1 431 Request Header Fields Too Large"
        );
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn cleans_what_a_local_client_sends() {
        let (server, rx) = server();
        let body = format!(
            r#"{{"v":1,"event":"PreToolUse","session":"s1","description":"\u001b[2J{}","content":"SECRET"}}"#,
            "x".repeat(400)
        );
        assert_eq!(
            request(&server, &host(&server), Some(server.token()), &body),
            "HTTP/1.1 204 No Content"
        );
        let event = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let description = event.description.unwrap();
        assert!(!description.contains('\u{1b}'));
        assert_eq!(
            description.chars().count(),
            crate::events::text::MAX_TEXT_CHARS
        );
    }

    #[test]
    fn a_slow_client_times_out_without_blocking_others() {
        let (server, rx) = server();
        let mut slow = TcpStream::connect(server.addr()).unwrap();
        slow.write_all(b"POST /v1/events HTTP/1.1\r\n").unwrap(); // and nothing more

        let started = Instant::now();
        assert_eq!(
            request(&server, &host(&server), Some(server.token()), EVENT),
            "HTTP/1.1 204 No Content"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "blocked by the slow client"
        );
        assert!(rx.recv_timeout(Duration::from_secs(1)).is_ok());

        slow.set_read_timeout(Some(READ_TIMEOUT + Duration::from_secs(3)))
            .unwrap();
        let mut response = String::new();
        let _ = slow.read_to_string(&mut response);
        assert!(response.starts_with("HTTP/1.1 408"), "{response}");
    }

    #[test]
    fn drop_stops_the_server() {
        let (server, _) = server();
        let addr = server.addr();
        let started = Instant::now();
        drop(server);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_err());
    }

    #[test]
    fn helpers() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(host_is_allowed("127.0.0.1:8080", 8080));
        assert!(host_is_allowed("localhost:8080", 8080));
        assert!(!host_is_allowed("127.0.0.1:8081", 8080));
        assert!(!host_is_allowed("[::1]:8080", 8080));
        assert!(!host_is_allowed("example.com:8080", 8080));
    }
}
