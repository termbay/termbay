//! Core library of termbay.
//!
//! This crate has no dependency on Tauri or any UI toolkit, so it can be
//! reused by the desktop app, by `termbay serve` and by `termbay hook`.
//!
//! - [`cli`]: command line parsing for the `termbay` binary.
//! - [`pty`]: pseudo terminal sessions running the user's shell.

pub mod cli;
pub mod pty;

/// Version of termbay, taken from the workspace `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
