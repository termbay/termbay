//! Pseudo terminal sessions running the user's shell.
//!
//! Built on [`portable-pty`](https://docs.rs/portable-pty), which uses a
//! Unix pty on macOS and Linux and ConPTY on Windows (10 1809 or newer).
//! Nothing here depends on Tauri: the desktop app, `termbay serve` and the
//! tests all drive sessions through the same API.
//!
//! # Overview
//!
//! - [`default_shell`] finds the user's shell (`$SHELL` on Unix; `pwsh`,
//!   then `powershell`, then `cmd` on Windows). On macOS it is started as a
//!   login shell, because apps opened from Finder get an almost empty `PATH`.
//! - [`PtySession::spawn`] starts a shell in a new pty. Output and the final
//!   exit status are delivered to a callback as [`PtyEvent`]s, from a
//!   background thread owned by the session.
//! - [`PtySession::write`], [`PtySession::resize`] and [`PtySession::kill`]
//!   control a running session. Dropping a session kills its shell, so no
//!   process is left behind.
//! - [`PtyManager`] keeps several sessions by [`PtyId`] (one per tab or
//!   split pane) and forgets a session once its shell exits.
//! - [`OutputBatcher`] and [`spawn_batcher`] group the output into fewer,
//!   larger chunks for the UI (about 8 ms or 64 KiB per chunk).
//! - [`FlowControl`] and [`ReadGate`] pause reading while the consumer is
//!   too far behind (decision D25).
//! - [`PtySession::info`] reports the shell's directory and foreground
//!   program ([`PtyInfo`]).
//!
//! Every child gets `TERM=xterm-256color`, `COLORTERM=truecolor`,
//! `TERM_PROGRAM=termbay` and `TERM_PROGRAM_VERSION=<termbay version>`.
//!
//! # Example
//!
//! ```no_run
//! use termbay_core::pty::{PtyEvent, PtySession, PtySize, SpawnOptions};
//!
//! let session = PtySession::spawn(SpawnOptions::default(), |event| match event {
//!     PtyEvent::Output(bytes) => print!("{}", String::from_utf8_lossy(bytes)),
//!     PtyEvent::Exit(status) => println!("\n[shell exited: {status}]"),
//! })?;
//! session.resize(PtySize { rows: 40, cols: 120 })?;
//! session.write(b"echo hello\r")?;
//! session.write(b"exit\r")?;
//! # Ok::<(), termbay_core::pty::PtyError>(())
//! ```

mod batch;
mod error;
mod flow;
mod manager;
mod procinfo;
mod session;
mod shell;

pub use batch::{BatchLimits, BatchSender, OutputBatcher, spawn_batcher};
pub use error::PtyError;
pub use flow::{FLOW_HIGH_WATER, FLOW_LOW_WATER, FlowControl, ReadGate};
pub use manager::{PtyId, PtyManager};
pub use procinfo::{MAX_NAME_CHARS, PtyInfo};
pub use session::{ExitStatus, PtyEvent, PtySession, PtySize, SpawnOptions};
pub use shell::{ShellSpec, default_shell};
