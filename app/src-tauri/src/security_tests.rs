//! Security checks of the autocomplete at the bridge level (AC8,
//! `docs/SEGURANCA.md`). The checks with real fish and zsh live in
//! `core/tests/shell_integration.rs`.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::bridge::PtyBridge;
use crate::bridge::test_support::{TIMEOUT, plain_shell};
use crate::complete::CompleteResponse;

/// With `[autocomplete] enabled = false` there is no scanner: the reader
/// hands every byte to the batcher as before, so even a private sequence
/// reaches the UI unchanged, and `pty_complete` never writes to the pty.
#[test]
fn disabled_bridge_passes_private_sequences_through() {
    let bridge = PtyBridge::with_options(plain_shell());
    let (tx, rx) = mpsc::channel();
    let id = bridge
        .open(
            80,
            24,
            move |chunk| {
                let _ = tx.send(chunk);
            },
            |_| {},
        )
        .unwrap();
    // The echoed command has `\033`, not ESC; only printf's output has the
    // real sequence.
    bridge
        .write(
            id,
            b"printf '\\033]6973;0123;prompt;\\007\\033]133;A\\007DONE''DONE\\n'\r".to_vec(),
        )
        .unwrap();
    let expected: &[u8] = b"\x1b]6973;0123;prompt;\x07\x1b]133;A\x07DONEDONE";
    let mut output = Vec::new();
    let deadline = Instant::now() + TIMEOUT;
    while !output.windows(expected.len()).any(|w| w == expected) {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left.max(Duration::from_millis(1))) {
            Ok(chunk) => output.extend(chunk),
            Err(_) => panic!(
                "the private sequence did not arrive unchanged: {:?}",
                String::from_utf8_lossy(&output)
            ),
        }
    }
    assert_eq!(bridge.complete(id), CompleteResponse::Unavailable);
    bridge.close(id).unwrap();
}
