//! Flow control between a pty and a slower consumer (the UI), decision D25
//! in `docs/ARQUITETURA.md` and section 6.5 of `docs/FASE1-PLANO.md`.
//!
//! Two pieces, joined by the caller (the Tauri bridge):
//!
//! - [`FlowControl`]: a pure state machine that counts the bytes in flight
//!   (delivered but not yet acknowledged) and says when to pause and when
//!   to resume. No clock, no thread.
//! - [`ReadGate`]: checked by the reader thread of a [`PtySession`]
//!   *before* each `read()`, never while the event callback runs. Opening
//!   it for good ([`ReadGate::open`]) lets the reader drain the pty when
//!   the shell exits or is killed.
//!
//! [`PtySession`]: super::PtySession

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

/// Bytes in flight at which reading pauses (8 output batches of 64 KiB).
pub const FLOW_HIGH_WATER: u64 = 512 * 1024;

/// Bytes in flight at or below which reading resumes.
pub const FLOW_LOW_WATER: u64 = 128 * 1024;

/// Counts bytes in flight and decides when to pause and resume, with
/// hysteresis between a high and a low mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowControl {
    high: u64,
    low: u64,
    in_flight: u64,
    paused: bool,
}

impl Default for FlowControl {
    /// [`FLOW_HIGH_WATER`] and [`FLOW_LOW_WATER`].
    fn default() -> Self {
        Self::new(FLOW_HIGH_WATER, FLOW_LOW_WATER)
    }
}

impl FlowControl {
    /// A counter that pauses at `high` bytes in flight and resumes at `low`
    /// or fewer. `low` is clamped to `high`.
    pub fn new(high: u64, low: u64) -> Self {
        Self {
            high,
            low: low.min(high),
            in_flight: 0,
            paused: false,
        }
    }

    /// `n` bytes were delivered. Returns `true` when reading must pause now
    /// (only on the transition, not while already paused).
    pub fn sent(&mut self, n: u64) -> bool {
        self.in_flight = self.in_flight.saturating_add(n);
        if !self.paused && self.in_flight >= self.high {
            self.paused = true;
            return true;
        }
        false
    }

    /// `n` bytes were acknowledged. An ack larger than what is in flight
    /// resets the count to zero. Returns `true` when reading must resume
    /// now (only on the transition).
    pub fn acked(&mut self, n: u64) -> bool {
        self.in_flight = self.in_flight.saturating_sub(n);
        if self.paused && self.in_flight <= self.low {
            self.paused = false;
            return true;
        }
        false
    }

    /// Bytes delivered and not yet acknowledged.
    pub fn in_flight(&self) -> u64 {
        self.in_flight
    }

    /// Whether reading is paused.
    pub fn is_paused(&self) -> bool {
        self.paused
    }
}

#[derive(Debug, Default)]
struct GateState {
    paused: bool,
    open: bool,
}

/// Lets the reader thread of a session wait while the consumer is behind.
///
/// Shared (in an `Arc`) between the session, which waits on it, and the
/// code that counts and acknowledges bytes, which pauses and resumes it.
#[derive(Debug, Default)]
pub struct ReadGate {
    state: Mutex<GateState>,
    changed: Condvar,
}

impl ReadGate {
    /// A gate that lets reads through.
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes the next [`ReadGate::wait`] block (unless the gate is open).
    pub fn pause(&self) {
        self.lock().paused = true;
    }

    /// Lets the reader continue.
    pub fn resume(&self) {
        self.lock().paused = false;
        self.changed.notify_all();
    }

    /// Opens the gate for good: pausing has no effect anymore. Used when
    /// the shell exits or is killed, so the rest of the output is drained.
    pub fn open(&self) {
        self.lock().open = true;
        self.changed.notify_all();
    }

    /// Whether a read would wait now.
    pub fn is_paused(&self) -> bool {
        let state = self.lock();
        state.paused && !state.open
    }

    /// Blocks while paused and not open.
    pub fn wait(&self) {
        let state = self.lock();
        let _state = self
            .changed
            .wait_while(state, |state| state.paused && !state.open)
            .unwrap_or_else(PoisonError::into_inner);
    }

    fn lock(&self) -> MutexGuard<'_, GateState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::*;

    #[test]
    fn sent_and_acked_follow_the_table() {
        let mut flow = FlowControl::new(100, 25);
        // (operation, bytes, returned, in flight after, paused after)
        let steps: &[(&str, u64, bool, u64, bool)] = &[
            ("sent", 60, false, 60, false),
            ("sent", 39, false, 99, false),
            ("sent", 1, true, 100, true),
            ("sent", 50, false, 150, true),
            ("acked", 100, false, 50, true),
            ("acked", 25, true, 25, false),
            ("acked", 10, false, 15, false),
            ("sent", 85, true, 100, true),
        ];
        for &(op, n, returned, in_flight, paused) in steps {
            let got = match op {
                "sent" => flow.sent(n),
                _ => flow.acked(n),
            };
            assert_eq!(got, returned, "{op}({n})");
            assert_eq!(flow.in_flight(), in_flight, "{op}({n})");
            assert_eq!(flow.is_paused(), paused, "{op}({n})");
        }
    }

    #[test]
    fn an_ack_larger_than_pending_resets_to_zero() {
        let mut flow = FlowControl::new(100, 25);
        assert!(flow.sent(120));
        assert!(flow.acked(10_000));
        assert_eq!(flow.in_flight(), 0);
        assert!(!flow.acked(u64::MAX));
        assert_eq!(flow.in_flight(), 0);
        // Counting saturates instead of wrapping around.
        assert!(flow.sent(u64::MAX));
        assert!(!flow.sent(1));
        assert_eq!(flow.in_flight(), u64::MAX);
    }

    #[test]
    fn hysteresis_keeps_paused_between_the_marks() {
        let mut flow = FlowControl::default();
        assert!(flow.sent(FLOW_HIGH_WATER));
        assert!(!flow.acked(FLOW_HIGH_WATER - FLOW_LOW_WATER - 1));
        assert!(flow.is_paused());
        assert!(flow.acked(1));
        assert!(!flow.is_paused());
        // Below the high mark again, nothing pauses.
        assert!(!flow.sent(FLOW_HIGH_WATER - FLOW_LOW_WATER - 1));
    }

    #[test]
    fn low_is_clamped_to_high() {
        let mut flow = FlowControl::new(10, 50);
        assert!(flow.sent(10));
        assert!(flow.acked(0));
    }

    #[test]
    fn gate_blocks_while_paused_until_resumed_or_opened() {
        for finish in [ReadGate::resume, ReadGate::open] {
            let gate = Arc::new(ReadGate::new());
            gate.wait();
            gate.pause();
            assert!(gate.is_paused());
            let (tx, rx) = mpsc::channel();
            let waiter = Arc::clone(&gate);
            let handle = thread::spawn(move || {
                waiter.wait();
                tx.send(()).unwrap();
            });
            assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
            finish(&gate);
            rx.recv_timeout(Duration::from_secs(5)).expect("not woken");
            handle.join().unwrap();
            assert!(!gate.is_paused());
        }
    }

    #[test]
    fn an_open_gate_ignores_pause() {
        let gate = ReadGate::new();
        gate.open();
        gate.pause();
        assert!(!gate.is_paused());
        gate.wait();
    }
}
