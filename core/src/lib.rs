//! Core library of termbay.
//!
//! This crate has no dependency on Tauri or any UI toolkit, so it can be
//! reused by the desktop app, by `termbay serve` and by `termbay-hook`.
//!
//! - [`cli`]: command line parsing for the `termbay` binary.
//! - [`pty`]: pseudo terminal sessions running the user's shell.
//! - [`config`]: the user's configuration file (only `[autocomplete]` so far).
//! - [`shell_integration`]: scripts, nonce and marks of integrated shells.
//! - [`complete`]: the autocomplete engine and history readers.
//! - [`events`]: office events: hook payload reducer and local event server.
//! - [`hooks`]: `termbay hooks install`: registers `termbay-hook` in Claude Code.
//! - [`state`]: the office: who is there and what each one does (phase 2).

pub mod cli;
pub mod complete;
pub mod config;
pub mod events;
pub mod hooks;
pub mod pty;
pub mod shell_integration;
pub mod state;

/// Version of termbay, taken from the workspace `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
