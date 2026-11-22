//! A shell never inherits the hook variables from termbay's own environment
//! (decision E1, `docs/FASE2-PLANO.md`): a termbay started inside another
//! one must not send its events to the other app.
//!
//! Its own test binary, with a single test, because it changes the process
//! environment, which is not safe while other threads read it.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use termbay_core::events::env::HOOK_VARS;
use termbay_core::pty::{PtyEvent, PtySession, ShellSpec, SpawnOptions};

const TIMEOUT: Duration = Duration::from_secs(30);

#[test]
fn shells_do_not_inherit_the_hook_variables() {
    for key in HOOK_VARS {
        // SAFETY: this test binary has a single test and no other thread
        // runs yet, so nothing reads the environment concurrently.
        unsafe { std::env::set_var(key, "inherited-from-the-parent") };
    }

    // Prints HAS_VAR if any variable is set, NO_VAR otherwise; the typed
    // line never contains either marker.
    #[cfg(windows)]
    let shell = ShellSpec::new("cmd.exe").arg("/D").arg("/C").arg(
        "if defined TERMBAY_EVENTS (echo HAS_^VAR) else if defined TERMBAY_TOKEN (echo HAS_^VAR) \
         else if defined TERMBAY_PANE (echo HAS_^VAR) else (echo NO_^VAR)",
    );
    #[cfg(not(windows))]
    let shell = ShellSpec::new("/bin/sh").arg("-c").arg(
        r#"if [ -n "$TERMBAY_EVENTS$TERMBAY_TOKEN$TERMBAY_PANE" ]; then echo HAS_""VAR; else echo NO_""VAR; fi"#,
    );

    let (tx, rx) = mpsc::channel::<Option<Vec<u8>>>();
    let session = PtySession::spawn(
        SpawnOptions {
            shell: Some(shell),
            ..SpawnOptions::default()
        },
        move |event| {
            let _ = tx.send(match event {
                PtyEvent::Output(bytes) => Some(bytes.to_vec()),
                PtyEvent::Exit(_) => None,
            });
        },
    )
    .expect("failed to spawn the shell");

    let mut output = String::new();
    let deadline = Instant::now() + TIMEOUT;
    loop {
        assert!(Instant::now() < deadline, "output so far:\n{output}");
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Some(bytes)) => {
                // ConPTY waits for an answer to its cursor query before
                // running the command (see tests/pty.rs).
                if bytes.windows(4).any(|w| w == b"\x1b[6n") {
                    let _ = session.write(b"\x1b[1;1R");
                }
                output.push_str(&String::from_utf8_lossy(&bytes));
            }
            Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    assert!(output.contains("NO_VAR"), "output:\n{output}");
    assert!(!output.contains("HAS_VAR"), "output:\n{output}");
}
