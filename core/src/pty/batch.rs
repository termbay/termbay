//! Output batching: turns the many small reads of a pty into fewer, larger
//! messages for the UI (decision D6 in `docs/FASE1-PLANO.md`).
//!
//! Sending every read on its own costs one IPC message (and one xterm.js
//! write) per read, which is wasteful when a program prints a lot. Holding
//! bytes for a few milliseconds and sending them together keeps typing
//! responsive and large outputs cheap.
//!
//! Two layers:
//!
//! - [`OutputBatcher`] is the pure state machine. It never looks at the
//!   clock: every call takes `now`, so tests drive time by hand.
//! - [`spawn_batcher`] runs an [`OutputBatcher`] on its own thread with the
//!   real clock and returns a [`BatchSender`] to feed it. This is what a pty
//!   callback uses.
//!
//! Rules, in both layers:
//!
//! - bytes leave in the order they came in, and none are dropped;
//! - pending bytes are sent once [`BatchLimits::max_delay`] has passed since
//!   the **first** pending byte arrived, or as soon as
//!   [`BatchLimits::max_bytes`] are pending, whichever comes first;
//! - no chunk is ever larger than [`BatchLimits::max_bytes`];
//! - at the end, whatever is pending is sent before the end is reported.

use std::fmt;
use std::io;
use std::mem;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

/// When pending output is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchLimits {
    /// Longest time a byte waits before being sent, counted from the first
    /// pending byte.
    pub max_delay: Duration,
    /// Size of a full chunk: reaching it sends at once. Also the largest
    /// chunk ever sent. Values below 1 are treated as 1.
    pub max_bytes: usize,
}

impl Default for BatchLimits {
    /// About 8 ms or 64 KiB, as decided in D6.
    fn default() -> Self {
        Self {
            max_delay: Duration::from_millis(8),
            max_bytes: 64 * 1024,
        }
    }
}

/// Pure batching state machine. See the [module docs](self) for the rules.
#[derive(Debug, Clone)]
pub struct OutputBatcher {
    limits: BatchLimits,
    pending: Vec<u8>,
    /// When the oldest pending byte arrived; `None` when nothing is pending.
    since: Option<Instant>,
}

impl OutputBatcher {
    /// An empty batcher.
    pub fn new(limits: BatchLimits) -> Self {
        let limits = BatchLimits {
            max_bytes: limits.max_bytes.max(1),
            ..limits
        };
        Self {
            limits,
            pending: Vec::new(),
            since: None,
        }
    }

    /// The limits in use.
    pub fn limits(&self) -> BatchLimits {
        self.limits
    }

    /// Adds bytes read at `now` and returns the chunks that are ready, in
    /// order (often none). Full chunks are returned at once; if the oldest
    /// pending byte is already older than `max_delay`, everything pending is
    /// returned too.
    pub fn push(&mut self, bytes: &[u8], now: Instant) -> Vec<Vec<u8>> {
        let mut ready = Vec::new();
        let mut rest = bytes;
        while !rest.is_empty() {
            if self.since.is_none() {
                self.since = Some(now);
            }
            let room = self.limits.max_bytes - self.pending.len();
            let (head, tail) = rest.split_at(room.min(rest.len()));
            self.pending.extend_from_slice(head);
            rest = tail;
            if self.pending.len() >= self.limits.max_bytes {
                ready.extend(self.flush());
            }
        }
        ready.extend(self.poll(now));
        ready
    }

    /// Returns everything pending if `max_delay` has passed since the oldest
    /// pending byte arrived.
    pub fn poll(&mut self, now: Instant) -> Option<Vec<u8>> {
        let since = self.since?;
        if now.saturating_duration_since(since) >= self.limits.max_delay {
            self.flush()
        } else {
            None
        }
    }

    /// Returns everything pending right away (for the end of a stream).
    pub fn flush(&mut self) -> Option<Vec<u8>> {
        self.since = None;
        if self.pending.is_empty() {
            None
        } else {
            Some(mem::take(&mut self.pending))
        }
    }

    /// When the pending bytes are due, or `None` when nothing is pending.
    pub fn deadline(&self) -> Option<Instant> {
        self.since.map(|since| since + self.limits.max_delay)
    }

    /// Number of bytes waiting to be sent.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

enum Message<E> {
    Output(Vec<u8>),
    End(E),
}

/// Feeds a batcher thread started by [`spawn_batcher`].
///
/// Dropping it without calling [`BatchSender::end`] still sends the pending
/// bytes, then stops the thread without calling `on_end`.
pub struct BatchSender<E> {
    tx: mpsc::Sender<Message<E>>,
}

impl<E> fmt::Debug for BatchSender<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BatchSender").finish_non_exhaustive()
    }
}

impl<E> BatchSender<E> {
    /// Queues bytes for batching. Never blocks.
    pub fn output(&self, bytes: &[u8]) {
        if !bytes.is_empty() {
            // A send error means the thread is gone (it already ended), so
            // there is nobody left to deliver to.
            let _ = self.tx.send(Message::Output(bytes.to_vec()));
        }
    }

    /// Ends the stream: the thread sends what is pending, calls `on_end`
    /// with `value` and stops. Output queued afterwards is ignored.
    pub fn end(&self, value: E) {
        let _ = self.tx.send(Message::End(value));
    }
}

/// Starts a thread that batches bytes with the real clock.
///
/// `on_chunk` receives every chunk, in order. `on_end` is called once,
/// after the last chunk, when [`BatchSender::end`] is used. Both run on the
/// batcher thread.
pub fn spawn_batcher<E, C, X>(
    limits: BatchLimits,
    on_chunk: C,
    on_end: X,
) -> io::Result<BatchSender<E>>
where
    E: Send + 'static,
    C: FnMut(Vec<u8>) + Send + 'static,
    X: FnOnce(E) + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("termbay-pty-batcher".into())
        .spawn(move || run(OutputBatcher::new(limits), &rx, on_chunk, on_end))?;
    Ok(BatchSender { tx })
}

fn run<E>(
    mut batcher: OutputBatcher,
    rx: &mpsc::Receiver<Message<E>>,
    mut on_chunk: impl FnMut(Vec<u8>),
    on_end: impl FnOnce(E),
) {
    loop {
        let message = match batcher.deadline() {
            None => rx.recv().ok(),
            Some(deadline) => {
                let wait = deadline.saturating_duration_since(Instant::now());
                match rx.recv_timeout(wait) {
                    Ok(message) => Some(message),
                    Err(RecvTimeoutError::Timeout) => {
                        if let Some(chunk) = batcher.poll(Instant::now()) {
                            on_chunk(chunk);
                        }
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => None,
                }
            }
        };

        match message {
            Some(Message::Output(bytes)) => {
                for chunk in batcher.push(&bytes, Instant::now()) {
                    on_chunk(chunk);
                }
            }
            Some(Message::End(value)) => {
                if let Some(chunk) = batcher.flush() {
                    on_chunk(chunk);
                }
                // Release whatever `on_chunk` holds (for the UI, the output
                // channel) before reporting the end, so nothing about the
                // output can come after it.
                drop(on_chunk);
                on_end(value);
                return;
            }
            None => {
                if let Some(chunk) = batcher.flush() {
                    on_chunk(chunk);
                }
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    fn limits(max_delay_ms: u64, max_bytes: usize) -> BatchLimits {
        BatchLimits {
            max_delay: Duration::from_millis(max_delay_ms),
            max_bytes,
        }
    }

    #[test]
    fn default_limits_are_8_ms_and_64_kib() {
        assert_eq!(
            BatchLimits::default(),
            BatchLimits {
                max_delay: Duration::from_millis(8),
                max_bytes: 65_536
            }
        );
    }

    #[test]
    fn small_pushes_are_joined_into_one_chunk() {
        let t0 = Instant::now();
        let mut b = OutputBatcher::new(limits(8, 1024));
        assert!(b.push(b"ab", t0).is_empty());
        assert!(b.push(b"cd", t0 + MS).is_empty());
        assert!(b.push(b"ef", t0 + 2 * MS).is_empty());
        assert_eq!(b.pending_len(), 6);
        assert_eq!(b.poll(t0 + 8 * MS), Some(b"abcdef".to_vec()));
        assert_eq!(b.pending_len(), 0);
    }

    #[test]
    fn nothing_is_sent_before_the_delay() {
        let t0 = Instant::now();
        let mut b = OutputBatcher::new(limits(8, 1024));
        b.push(b"x", t0);
        assert_eq!(b.poll(t0), None);
        assert_eq!(b.poll(t0 + 7 * MS), None);
        assert_eq!(b.deadline(), Some(t0 + 8 * MS));
        assert_eq!(b.poll(t0 + 8 * MS), Some(b"x".to_vec()));
        assert_eq!(b.deadline(), None);
        assert_eq!(b.poll(t0 + 100 * MS), None);
    }

    #[test]
    fn the_delay_counts_from_the_first_pending_byte() {
        let t0 = Instant::now();
        let mut b = OutputBatcher::new(limits(8, 1024));
        b.push(b"a", t0);
        b.push(b"b", t0 + 6 * MS);
        // Not 6 + 8: the oldest byte decides.
        assert_eq!(b.poll(t0 + 8 * MS), Some(b"ab".to_vec()));
        // The next byte starts a new window.
        b.push(b"c", t0 + 9 * MS);
        assert_eq!(b.deadline(), Some(t0 + 17 * MS));
    }

    #[test]
    fn a_push_after_the_deadline_sends_everything() {
        let t0 = Instant::now();
        let mut b = OutputBatcher::new(limits(8, 1024));
        b.push(b"old", t0);
        assert_eq!(b.push(b"new", t0 + 9 * MS), vec![b"oldnew".to_vec()]);
        assert_eq!(b.pending_len(), 0);
    }

    #[test]
    fn reaching_max_bytes_sends_at_once() {
        let t0 = Instant::now();
        let mut b = OutputBatcher::new(limits(8, 4));
        assert!(b.push(b"ab", t0).is_empty());
        assert_eq!(b.push(b"cd", t0), vec![b"abcd".to_vec()]);
        assert_eq!(b.pending_len(), 0);
        assert_eq!(b.deadline(), None);
    }

    #[test]
    fn a_full_64_kib_chunk_is_sent_without_waiting() {
        let t0 = Instant::now();
        let mut b = OutputBatcher::new(BatchLimits::default());
        assert!(b.push(&[1; 65_535], t0).is_empty());
        let ready = b.push(&[2], t0);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].len(), 65_536);
        assert_eq!(ready[0][65_535], 2);
    }

    #[test]
    fn large_pushes_are_split_into_full_chunks_and_a_remainder() {
        let t0 = Instant::now();
        let mut b = OutputBatcher::new(limits(8, 4));
        let ready = b.push(b"abcdefghij", t0);
        assert_eq!(ready, vec![b"abcd".to_vec(), b"efgh".to_vec()]);
        assert_eq!(b.pending_len(), 2);
        // The remainder arrived at t0, so it is due at t0 + 8 ms.
        assert_eq!(b.deadline(), Some(t0 + 8 * MS));
        assert_eq!(b.flush(), Some(b"ij".to_vec()));
    }

    #[test]
    fn flush_sends_whatever_is_pending() {
        let t0 = Instant::now();
        let mut b = OutputBatcher::new(limits(8, 1024));
        assert_eq!(b.flush(), None);
        b.push(b"tail", t0);
        assert_eq!(b.flush(), Some(b"tail".to_vec()));
        assert_eq!(b.flush(), None);
        assert_eq!(b.deadline(), None);
    }

    #[test]
    fn empty_pushes_change_nothing() {
        let t0 = Instant::now();
        let mut b = OutputBatcher::new(limits(8, 1024));
        assert!(b.push(b"", t0).is_empty());
        assert_eq!(b.deadline(), None);
    }

    #[test]
    fn zero_max_bytes_is_treated_as_one() {
        let t0 = Instant::now();
        let mut b = OutputBatcher::new(limits(8, 0));
        assert_eq!(b.limits().max_bytes, 1);
        assert_eq!(b.push(b"ab", t0), vec![b"a".to_vec(), b"b".to_vec()]);
    }

    /// Deterministic pseudo-random numbers (no extra crate needed).
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self, bound: u64) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 33) % bound
        }
    }

    #[test]
    fn bytes_are_never_lost_or_reordered() {
        for seed in 0..20 {
            let mut rng = Lcg(seed);
            let max_bytes = 1 + rng.next(300) as usize;
            let mut b = OutputBatcher::new(limits(8, max_bytes));
            let mut now = Instant::now();
            let mut input = Vec::new();
            let mut output = Vec::new();
            for _ in 0..500 {
                let len = rng.next(700) as usize;
                let bytes: Vec<u8> = (0..len).map(|_| rng.next(256) as u8).collect();
                input.extend_from_slice(&bytes);
                now += Duration::from_micros(rng.next(5_000));
                let mut chunks = b.push(&bytes, now);
                if rng.next(4) == 0 {
                    now += Duration::from_micros(rng.next(12_000));
                    chunks.extend(b.poll(now));
                }
                for chunk in chunks {
                    assert!(!chunk.is_empty());
                    assert!(chunk.len() <= max_bytes, "chunk larger than max_bytes");
                    output.extend(chunk);
                }
            }
            output.extend(b.flush().unwrap_or_default());
            assert_eq!(output, input, "seed {seed}");
        }
    }

    // The thread tests below use the real clock with generous margins.

    enum Seen {
        Chunk(Vec<u8>),
        End(&'static str),
    }

    fn thread_batcher(limits: BatchLimits) -> (BatchSender<&'static str>, mpsc::Receiver<Seen>) {
        let (tx, rx) = mpsc::channel();
        let end_tx = tx.clone();
        let sender = spawn_batcher(
            limits,
            move |chunk| tx.send(Seen::Chunk(chunk)).unwrap(),
            move |value| end_tx.send(Seen::End(value)).unwrap(),
        )
        .unwrap();
        (sender, rx)
    }

    const WAIT: Duration = Duration::from_secs(5);

    #[test]
    fn thread_sends_pending_bytes_after_the_delay_without_more_input() {
        let (sender, rx) = thread_batcher(limits(8, 1024));
        sender.output(b"hello");
        sender.output(b" world");
        let started = Instant::now();
        match rx.recv_timeout(WAIT).unwrap() {
            Seen::Chunk(chunk) => assert_eq!(chunk, b"hello world"),
            Seen::End(_) => panic!("unexpected end"),
        }
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn thread_sends_full_chunks_without_waiting_for_the_delay() {
        // A delay this long would make the test time out if size did not
        // trigger the send.
        let (sender, rx) = thread_batcher(limits(60_000, 4));
        sender.output(b"abcdef");
        match rx.recv_timeout(WAIT).unwrap() {
            Seen::Chunk(chunk) => assert_eq!(chunk, b"abcd"),
            Seen::End(_) => panic!("unexpected end"),
        }
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
    }

    #[test]
    fn thread_flushes_before_reporting_the_end() {
        let (sender, rx) = thread_batcher(limits(60_000, 1024));
        sender.output(b"last");
        sender.end("bye");
        sender.output(b"ignored");
        assert!(matches!(rx.recv_timeout(WAIT).unwrap(), Seen::Chunk(c) if c == b"last"));
        assert!(matches!(rx.recv_timeout(WAIT).unwrap(), Seen::End("bye")));
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
    }

    #[test]
    fn thread_flushes_when_the_sender_is_dropped() {
        let (sender, rx) = thread_batcher(limits(60_000, 1024));
        sender.output(b"tail");
        drop(sender);
        assert!(matches!(rx.recv_timeout(WAIT).unwrap(), Seen::Chunk(c) if c == b"tail"));
        // The thread is gone and `on_end` was never called.
        assert!(matches!(
            rx.recv_timeout(WAIT),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }

    #[test]
    fn thread_keeps_order_and_loses_nothing() {
        let (sender, rx) = thread_batcher(limits(1, 100));
        let mut input = Vec::new();
        for i in 0..2_000u32 {
            let bytes = i.to_le_bytes();
            input.extend_from_slice(&bytes);
            sender.output(&bytes);
            if i % 300 == 0 {
                thread::sleep(Duration::from_millis(2));
            }
        }
        sender.end("done");
        let mut output = Vec::new();
        loop {
            match rx.recv_timeout(WAIT).unwrap() {
                Seen::Chunk(chunk) => {
                    assert!(chunk.len() <= 100);
                    output.extend(chunk);
                }
                Seen::End(value) => {
                    assert_eq!(value, "done");
                    break;
                }
            }
        }
        assert_eq!(output, input);
    }
}
