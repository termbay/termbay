//! `termbay-hook`: what Claude Code hooks run to send office events to the
//! termbay app (tasks A3 and E11 in `docs/FASE2-PLANO.md`).
//!
//! Contract with Claude Code (decision E3): the process always exits with
//! code 0 and never writes to stdout, whatever happens. Exit code 2 would
//! block the tool, and some hooks read stdout as a decision; the office must
//! never get in the way of `claude`. Sending has a 200 ms budget; past it the
//! event is dropped.
//!
//! Steps:
//! 1. Read where the app is from the environment ([`HookEnv`]). Without it,
//!    stop at once: this `claude` is not running inside termbay.
//! 2. Read the payload from stdin, up to [`MAX_STDIN`].
//! 3. Reduce it ([`reduce`]): only a closed list of fields is kept.
//! 4. Send it to the app's event server and wait briefly for the answer.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use termbay_core::events::server::EVENTS_PATH;
use termbay_core::events::{HookEnv, ReducedEvent, reduce};

/// Largest payload read from stdin. Bigger ones are dropped.
pub const MAX_STDIN: u64 = 8 * 1024 * 1024;
/// Time budget to connect, send and get the answer.
pub const SEND_BUDGET: Duration = Duration::from_millis(200);
/// Variable that makes the hook explain on stderr why it did nothing.
pub const DEBUG_VAR: &str = "TERMBAY_HOOK_DEBUG";

/// What happened, for tests and for `TERMBAY_HOOK_DEBUG`.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// No (valid) `TERMBAY_EVENTS`/`TERMBAY_TOKEN`: not inside termbay.
    NotInTermbay,
    /// stdin could not be read, was too big or was not JSON.
    BadInput,
    /// An event the office does not use.
    Skipped,
    /// The event server answered with this status code.
    Sent(u16),
    /// The event could not be delivered in time.
    NotSent(String),
}

/// Runs the hook with the given environment and stdin.
pub fn run(env: impl Fn(&str) -> Option<String>, stdin: impl Read) -> Outcome {
    let Some((hook_env, pane)) = HookEnv::from_vars(&env) else {
        return Outcome::NotInTermbay;
    };
    let mut raw = Vec::new();
    if stdin.take(MAX_STDIN + 1).read_to_end(&mut raw).is_err() || raw.len() as u64 > MAX_STDIN {
        return Outcome::BadInput;
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&raw) else {
        return Outcome::BadInput;
    };
    drop(raw);
    let Some(event) = reduce(&value, pane.as_deref()) else {
        return Outcome::Skipped;
    };
    match send(&hook_env, &event, Instant::now() + SEND_BUDGET) {
        Ok(status) => Outcome::Sent(status),
        Err(err) => Outcome::NotSent(err.to_string()),
    }
}

/// Sends one event with a hand-written HTTP/1.1 request, all within
/// `deadline`, and returns the status code of the answer.
pub fn send(hook_env: &HookEnv, event: &ReducedEvent, deadline: Instant) -> io::Result<u16> {
    let body = serde_json::to_vec(event).map_err(io::Error::other)?;
    let addr = hook_env.addr();
    let mut stream = TcpStream::connect_timeout(&addr, remaining(deadline)?)?;
    stream.set_nodelay(true)?;
    stream.set_write_timeout(Some(remaining(deadline)?))?;
    let head = format!(
        "POST {EVENTS_PATH} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        hook_env.token(),
        body.len()
    );
    let mut request = head.into_bytes();
    request.extend_from_slice(&body);
    stream.write_all(&request)?;

    // Waiting for the answer makes sure the server read the whole request
    // before this process exits and closes the socket.
    stream.set_read_timeout(Some(remaining(deadline)?))?;
    let mut answer = Vec::new();
    let mut buf = [0u8; 256];
    while !answer.ends_with(b"\r\n\r\n") && answer.len() < 4096 {
        match stream.read(&mut buf)? {
            0 => break,
            n => answer.extend_from_slice(&buf[..n]),
        }
    }
    status_code(&answer)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no HTTP status line"))
}

/// Time left before `deadline`, or a timeout error once it passed.
fn remaining(deadline: Instant) -> io::Result<Duration> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        Err(io::Error::new(io::ErrorKind::TimedOut, "out of time"))
    } else {
        Ok(left)
    }
}

/// `HTTP/1.1 204 No Content` → 204.
fn status_code(answer: &[u8]) -> Option<u16> {
    let line = answer.split(|&b| b == b'\n').next()?;
    let line = std::str::from_utf8(line).ok()?;
    let mut parts = line.split(' ');
    parts.next().filter(|v| v.starts_with("HTTP/1."))?;
    parts.next()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_line_parsing() {
        assert_eq!(status_code(b"HTTP/1.1 204 No Content\r\n\r\n"), Some(204));
        assert_eq!(status_code(b"HTTP/1.0 401 Unauthorized\r\n"), Some(401));
        assert_eq!(status_code(b"garbage"), None);
        assert_eq!(status_code(b""), None);
    }

    #[test]
    fn without_the_variables_nothing_is_read() {
        struct Panics;
        impl Read for Panics {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                panic!("stdin must not be read outside termbay");
            }
        }
        assert_eq!(run(|_| None, Panics), Outcome::NotInTermbay);
    }
}
