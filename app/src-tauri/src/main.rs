//! Entry point of the `termbay` binary.
//!
//! The command line is parsed (by `termbay_core::cli`) before anything
//! else, so `termbay --version` and `termbay --help` never start Tauri or
//! open a window. Only a plain `termbay` opens the app.

// Release builds on Windows use the GUI subsystem, so no console window
// flashes when the app is opened from the Start menu or Explorer.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Write;
use std::process::ExitCode;

use termbay_core::cli::{self, Command};

mod bridge;
mod commands;
mod complete;
#[cfg(any(target_os = "macos", test))]
mod menu;
#[cfg(all(test, unix))]
mod security_tests;

fn main() -> ExitCode {
    let command = match cli::parse(std::env::args_os().skip(1)) {
        Ok(command) => command,
        Err(err) => {
            console::attach_to_parent();
            let _ = writeln!(std::io::stderr(), "termbay: {err}");
            return ExitCode::from(u8::try_from(err.exit_code()).unwrap_or(2));
        }
    };

    match command {
        Command::Version => print_and_exit(&cli::version_text()),
        Command::Help => print_and_exit(&cli::help_text()),
        Command::Gui => run_app(),
    }
}

fn print_and_exit(text: &str) -> ExitCode {
    console::attach_to_parent();
    // A closed stdout (for example `termbay --version | true`) is not an
    // error worth reporting.
    let _ = writeln!(std::io::stdout(), "{text}");
    ExitCode::SUCCESS
}

fn run_app() -> ExitCode {
    let builder = tauri::Builder::default();
    // Replaces Tauri's default macOS menu, whose "Close Window" takes Cmd+W.
    #[cfg(target_os = "macos")]
    let builder = builder.menu(menu::build);
    let app = commands::register(builder, bridge::PtyBridge::new()).build(context());
    match app {
        Ok(app) => {
            app.run(|app, event| commands::on_run_event(app, &event));
            ExitCode::SUCCESS
        }
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "termbay: could not start the app: {err}");
            ExitCode::FAILURE
        }
    }
}

/// The app's context (config, assets, capabilities), generated from
/// `tauri.conf.json`. Kept in one place because the macro may only be
/// expanded once per binary; the tests use it with the mock runtime.
fn context<R: tauri::Runtime>() -> tauri::Context<R> {
    tauri::generate_context!()
}

mod console {
    /// On Windows, a GUI-subsystem process started from a terminal has no
    /// standard output. Attaching to the parent's console makes
    /// `termbay --version` print there. When output is already redirected
    /// (pipes, files, tests), nothing is changed.
    #[cfg(windows)]
    pub fn attach_to_parent() {
        use windows_sys::Win32::System::Console::{
            ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE,
        };
        // SAFETY: plain Win32 calls without pointers; failure (no parent
        // console) is harmless and ignored.
        unsafe {
            if GetStdHandle(STD_OUTPUT_HANDLE).is_null() {
                AttachConsole(ATTACH_PARENT_PROCESS);
            }
        }
    }

    #[cfg(not(windows))]
    pub fn attach_to_parent() {}
}
