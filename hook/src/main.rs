//! `termbay-hook <event>`: run by Claude Code hooks. See the library docs.
//!
//! Arguments are ignored (the event name also comes in the payload), so the
//! command written by `termbay hooks install` can carry the event name for
//! readability. Always exits with 0 and prints nothing on stdout, even if
//! something panics.

use std::io::Write;
use std::panic::{self, AssertUnwindSafe};
use std::process::ExitCode;

fn main() -> ExitCode {
    let debug = std::env::var_os(termbay_hook::DEBUG_VAR).is_some();
    if !debug {
        // A panic message on stderr is noise in Claude Code's output.
        panic::set_hook(Box::new(|_| {}));
    }
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
        termbay_hook::run(|key| std::env::var(key).ok(), std::io::stdin().lock())
    }));
    if debug {
        // `eprintln!` would panic on a closed stderr; this cannot.
        let _ = writeln!(std::io::stderr(), "termbay-hook: {outcome:?}");
    }
    ExitCode::SUCCESS
}
