//! Coordination between the pty reader (marks) and completion requests.

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::complete::CompletionReply;

use super::ShellKind;
use super::osc::{ShellInfo, ShellMark};
use super::state::ShellState;

/// How long a completion request waits for the shell (decision in
/// `docs/AUTOCOMPLETE.md`, section 3).
pub const COMPLETION_TIMEOUT: Duration = Duration::from_millis(1500);

/// After a line break typed while a command ran (typeahead), how long the
/// shell must stay at its prompt, with no `exec` mark, before a trigger is
/// sent. A typed-ahead command is read right after the prompt shows, so its
/// `exec` mark arrives first. zsh checks this itself ($PENDING); fish
/// cannot see pending input, so this is its guard.
pub const TYPEAHEAD_SETTLE: Duration = Duration::from_millis(250);

/// First private-use character of the trigger: U+10FFF0 + digit types one
/// digit of the request number.
const TRIGGER_DIGIT_BASE: u32 = 0x10_FFF0;
/// Private-use character that runs the completion widget.
const TRIGGER_RUN: char = '\u{10FFFD}';

/// Bytes written to the pty to ask for completions: the digits of
/// `request`, each as U+10FFF0..U+10FFF9, then U+10FFFD. These characters
/// are in Unicode's last private-use plane; no keyboard or input method
/// sends them, and the widgets are bound to them in every keymap.
pub fn trigger_bytes(request: u64) -> Vec<u8> {
    let mut text = String::new();
    for digit in request.to_string().bytes() {
        let c = char::from_u32(TRIGGER_DIGIT_BASE + u32::from(digit - b'0'))
            .expect("U+10FFF0..U+10FFF9 are valid characters");
        text.push(c);
    }
    text.push(TRIGGER_RUN);
    text.into_bytes()
}

/// How a completion request ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestOutcome {
    /// The shell answered.
    Reply(CompletionReply),
    /// Not at the prompt (no handshake, a command running, a line just
    /// submitted, the pty gone) or the trigger could not be written. The
    /// UI gives the key to the shell.
    Unavailable,
    /// No answer in time.
    Timeout,
    /// A newer request replaced this one.
    Cancelled,
}

#[derive(Debug)]
struct Inner {
    state: ShellState,
    last_request: u64,
    waiting: Option<u64>,
    reply: Option<CompletionReply>,
    closed: bool,
    /// A line break was sent while the shell was not at its prompt.
    typeahead: bool,
    /// When the last trusted prompt mark arrived.
    prompt_at: Option<Instant>,
}

/// Shared state of one integrated pty: fed by the reader thread, queried
/// by completion requests.
#[derive(Debug)]
pub struct ShellLink {
    kind: ShellKind,
    inner: Mutex<Inner>,
    changed: Condvar,
}

impl ShellLink {
    /// A link for a shell of this kind, before any mark.
    pub fn new(kind: ShellKind) -> Self {
        Self {
            kind,
            inner: Mutex::new(Inner {
                state: ShellState::new(),
                last_request: 0,
                waiting: None,
                reply: None,
                closed: false,
                typeahead: false,
                prompt_at: None,
            }),
            changed: Condvar::new(),
        }
    }

    /// The shell this link was prepared for.
    pub fn kind(&self) -> ShellKind {
        self.kind
    }

    /// Applies trusted marks (from the reader thread). Returns the
    /// handshake the first time it arrives.
    pub fn apply(&self, marks: impl IntoIterator<Item = ShellMark>) -> Option<ShellInfo> {
        let mut first_ready = None;
        let mut inner = self.lock();
        for mark in marks {
            if let ShellMark::Completion(reply) = mark {
                // Late answers (timed out or replaced) are dropped.
                if inner.waiting == Some(reply.request) {
                    inner.reply = Some(reply);
                }
                continue;
            }
            if matches!(mark, ShellMark::Prompt { .. }) {
                inner.prompt_at = Some(Instant::now());
            }
            if inner.state.apply(&mark) {
                first_ready = inner.state.info().cloned();
            }
        }
        drop(inner);
        self.changed.notify_all();
        first_ready
    }

    /// Input sent to the shell. A line break means the line may start
    /// running before its `exec` mark arrives.
    pub fn note_input(&self, data: &[u8]) {
        if data.iter().any(|&b| b == b'\r' || b == b'\n') {
            let mut inner = self.lock();
            if !inner.state.is_at_prompt() {
                inner.typeahead = true;
            }
            inner.state.note_line_submitted();
            drop(inner);
            self.changed.notify_all();
        }
    }

    /// The pty is gone: every waiting request ends as unavailable.
    pub fn close(&self) {
        self.lock().closed = true;
        self.changed.notify_all();
    }

    /// Whether [`ShellLink::close`] was called.
    pub fn is_closed(&self) -> bool {
        self.lock().closed
    }

    /// The handshake, once received.
    pub fn info(&self) -> Option<ShellInfo> {
        self.lock().state.info().cloned()
    }

    /// Whether a completion request would be sent now.
    pub fn is_at_prompt(&self) -> bool {
        let inner = self.lock();
        !inner.closed && inner.state.is_at_prompt()
    }

    /// Asks the shell for completions and waits for the answer.
    ///
    /// The trigger is written with `write` only when the shell is at its
    /// prompt, according to the trusted marks. A newer request cancels
    /// this one.
    pub fn request<W>(&self, write: W, timeout: Duration) -> RequestOutcome
    where
        W: FnOnce(&[u8]) -> Result<(), String>,
    {
        let deadline = Instant::now() + timeout;
        let request = {
            let mut inner = self.lock();
            if inner.closed || !inner.state.is_at_prompt() {
                return RequestOutcome::Unavailable;
            }
            // After typeahead, wait until the prompt has been quiet for a
            // moment: a typed-ahead command would have started by then.
            while inner.typeahead {
                let settled = inner
                    .prompt_at
                    .map_or(Instant::now(), |at| at + TYPEAHEAD_SETTLE);
                let now = Instant::now();
                if now >= settled {
                    inner.typeahead = false;
                    break;
                }
                if now >= deadline {
                    return RequestOutcome::Timeout;
                }
                inner = self
                    .changed
                    .wait_timeout(inner, settled.min(deadline) - now)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
                if inner.closed || !inner.state.is_at_prompt() {
                    return RequestOutcome::Unavailable;
                }
            }
            inner.last_request += 1;
            inner.waiting = Some(inner.last_request);
            inner.reply = None;
            inner.last_request
        };
        // Wake an older request so it ends as cancelled.
        self.changed.notify_all();

        if write(&trigger_bytes(request)).is_err() {
            let mut inner = self.lock();
            if inner.waiting == Some(request) {
                inner.waiting = None;
            }
            return RequestOutcome::Unavailable;
        }

        let mut inner = self.lock();
        loop {
            if inner.waiting != Some(request) {
                return RequestOutcome::Cancelled;
            }
            if let Some(reply) = inner.reply.take() {
                inner.waiting = None;
                return RequestOutcome::Reply(reply);
            }
            if inner.closed || !inner.state.is_at_prompt() {
                inner.waiting = None;
                return RequestOutcome::Unavailable;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                inner.waiting = None;
                return RequestOutcome::Timeout;
            }
            inner = self
                .changed
                .wait_timeout(inner, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::thread;

    use super::*;
    use crate::complete::EditMode;

    fn ready() -> ShellMark {
        ShellMark::Ready(ShellInfo {
            protocol: 1,
            shell: "zsh".into(),
            version: "5.9".into(),
            history_file: None,
        })
    }

    fn reply(request: u64) -> ShellMark {
        ShellMark::Completion(CompletionReply {
            request,
            mode: EditMode::Insert,
            line: "ls".into(),
            cursor: 2,
            replace_from: 0,
            items: Vec::new(),
        })
    }

    fn at_prompt() -> Arc<ShellLink> {
        let link = Arc::new(ShellLink::new(ShellKind::Zsh));
        assert!(
            link.apply([ready(), ShellMark::Prompt { exit_code: None }])
                .is_some()
        );
        link
    }

    #[test]
    fn trigger_is_private_use_digits_and_run() {
        assert_eq!(
            String::from_utf8(trigger_bytes(907)).unwrap(),
            "\u{10FFF9}\u{10FFF0}\u{10FFF7}\u{10FFFD}"
        );
        assert_eq!(trigger_bytes(1), "\u{10FFF1}\u{10FFFD}".as_bytes());
    }

    #[test]
    fn nothing_is_written_unless_at_the_prompt() {
        let link = ShellLink::new(ShellKind::Fish);
        let never = |_: &[u8]| -> Result<(), String> { panic!("must not write") };
        assert_eq!(
            link.request(never, COMPLETION_TIMEOUT),
            RequestOutcome::Unavailable
        );
        link.apply([ShellMark::Prompt { exit_code: None }]);
        assert_eq!(
            link.request(never, COMPLETION_TIMEOUT),
            RequestOutcome::Unavailable
        );
        link.apply([ready(), ShellMark::Exec]);
        assert_eq!(
            link.request(never, COMPLETION_TIMEOUT),
            RequestOutcome::Unavailable
        );
        link.apply([ShellMark::Prompt { exit_code: Some(0) }]);
        link.note_input(b"sleep 5\r");
        assert_eq!(
            link.request(never, COMPLETION_TIMEOUT),
            RequestOutcome::Unavailable
        );
        link.apply([ShellMark::Prompt { exit_code: Some(0) }]);
        assert!(link.is_at_prompt());
        link.close();
        assert_eq!(
            link.request(never, COMPLETION_TIMEOUT),
            RequestOutcome::Unavailable
        );
    }

    #[test]
    fn the_matching_reply_is_returned() {
        let link = at_prompt();
        let (tx, rx) = mpsc::channel();
        let feeder = Arc::clone(&link);
        let handle = thread::spawn(move || {
            let written: Vec<u8> = rx.recv().unwrap();
            assert_eq!(written, trigger_bytes(1));
            feeder.apply([reply(7), reply(1)]);
        });
        let outcome = link.request(
            move |bytes| {
                tx.send(bytes.to_vec()).unwrap();
                Ok(())
            },
            Duration::from_secs(10),
        );
        handle.join().unwrap();
        let RequestOutcome::Reply(r) = outcome else {
            panic!("{outcome:?}")
        };
        assert_eq!(r.request, 1);
    }

    #[test]
    fn timeout_and_late_replies() {
        let link = at_prompt();
        let start = Instant::now();
        let outcome = link.request(|_| Ok(()), Duration::from_millis(50));
        assert_eq!(outcome, RequestOutcome::Timeout);
        assert!(start.elapsed() >= Duration::from_millis(50));
        // The late reply for request 1 is ignored by request 2.
        let feeder = Arc::clone(&link);
        let outcome = link.request(
            move |_| {
                feeder.apply([reply(1)]);
                Ok(())
            },
            Duration::from_millis(50),
        );
        assert_eq!(outcome, RequestOutcome::Timeout);
    }

    #[test]
    fn a_new_request_cancels_the_old_one() {
        let link = at_prompt();
        let first = Arc::clone(&link);
        let (started_tx, started_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            first.request(
                move |_| {
                    started_tx.send(()).unwrap();
                    Ok(())
                },
                Duration::from_secs(10),
            )
        });
        started_rx.recv().unwrap();
        let feeder = Arc::clone(&link);
        let second = link.request(
            move |_| {
                feeder.apply([reply(2)]);
                Ok(())
            },
            Duration::from_secs(10),
        );
        assert!(matches!(second, RequestOutcome::Reply(r) if r.request == 2));
        assert_eq!(handle.join().unwrap(), RequestOutcome::Cancelled);
    }

    #[test]
    fn after_typeahead_the_prompt_must_settle() {
        // `sleep` runs; `cat` + Enter is typed ahead.
        let link = at_prompt();
        link.note_input(b"sleep 1\r");
        link.apply([ShellMark::Exec]);
        link.note_input(b"cat\r");
        // The prompt of `sleep` arrives, then (typeahead) `cat` starts.
        link.apply([ShellMark::Prompt { exit_code: Some(0) }]);
        let feeder = Arc::clone(&link);
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            feeder.apply([ShellMark::Exec]);
        });
        let outcome = link.request(
            |_| panic!("the trigger must not reach `cat`"),
            Duration::from_secs(10),
        );
        handle.join().unwrap();
        assert_eq!(outcome, RequestOutcome::Unavailable);

        // `cat` ends; this prompt stays quiet, so the request goes out
        // after the settle time.
        link.apply([ShellMark::Prompt { exit_code: Some(0) }]);
        let start = Instant::now();
        let feeder = Arc::clone(&link);
        let outcome = link.request(
            move |_| {
                feeder.apply([reply(1)]);
                Ok(())
            },
            Duration::from_secs(10),
        );
        assert!(matches!(outcome, RequestOutcome::Reply(_)), "{outcome:?}");
        assert!(start.elapsed() >= TYPEAHEAD_SETTLE - Duration::from_millis(5));

        // Without typeahead there is no wait.
        let start = Instant::now();
        let feeder = Arc::clone(&link);
        let outcome = link.request(
            move |_| {
                feeder.apply([reply(2)]);
                Ok(())
            },
            Duration::from_secs(10),
        );
        assert!(matches!(outcome, RequestOutcome::Reply(_)));
        assert!(start.elapsed() < TYPEAHEAD_SETTLE);
    }

    #[test]
    fn leaving_the_prompt_or_closing_ends_the_wait() {
        let link = at_prompt();
        let feeder = Arc::clone(&link);
        let outcome = link.request(
            move |_| {
                feeder.apply([ShellMark::Exec]);
                Ok(())
            },
            Duration::from_secs(10),
        );
        assert_eq!(outcome, RequestOutcome::Unavailable);

        let link = at_prompt();
        let closer = Arc::clone(&link);
        let outcome = link.request(
            move |_| {
                closer.close();
                Ok(())
            },
            Duration::from_secs(10),
        );
        assert_eq!(outcome, RequestOutcome::Unavailable);

        let link = at_prompt();
        let outcome = link.request(|_| Err("gone".into()), Duration::from_secs(10));
        assert_eq!(outcome, RequestOutcome::Unavailable);
    }
}
