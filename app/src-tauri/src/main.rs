//! Entry point of the `termbay` binary.
//!
//! The command line is parsed (by `termbay_core::cli`) before anything
//! else, so `termbay --version` and `termbay --help` never start Tauri or
//! open a window. Only a plain `termbay` opens the app.

// Release builds on Windows use the GUI subsystem, so no console window
// flashes when the app is opened from the Start menu or Explorer.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use termbay_core::cli::{self, CliError, Command};

mod bridge;
mod commands;
mod complete;
#[cfg(any(target_os = "macos", test))]
mod menu;
#[cfg(all(test, unix))]
mod security_tests;

fn main() -> ExitCode {
    let command = match parse_command(std::env::args_os().skip(1), launched_by_webview_driver()) {
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

/// Variable that `msedgedriver` sets when it starts the app for the e2e
/// tests, together with browser options in the command line.
const WEBVIEW_DRIVER_ENV: &str = "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS";

/// Only e2e builds (`--features e2e`) started by the WebView2 driver
/// tolerate unknown options; release builds always reject them.
fn launched_by_webview_driver() -> bool {
    cfg!(feature = "e2e") && std::env::var_os(WEBVIEW_DRIVER_ENV).is_some()
}

/// [`cli::parse`], except that with `driver` set the browser options the
/// driver adds (`--remote-debugging-port=0`, `--user-data-dir=...`) open
/// the app instead of failing.
fn parse_command(
    args: impl IntoIterator<Item = OsString>,
    driver: bool,
) -> Result<Command, CliError> {
    match cli::parse(args) {
        Err(_) if driver => Ok(Command::Gui),
        result => result,
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
    let builder = tauri::Builder::default().setup(|app| {
        disable_browser_keys(app);
        Ok(())
    });
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

/// Turns off WebView2's browser keys (F5, Ctrl+R, Ctrl+P, Ctrl+F, F12...)
/// in the main window, so they reach the terminal or nothing at all.
/// Failures leave the defaults; the UI's shortcut handler still consumes
/// the keys that matter.
#[cfg(windows)]
fn disable_browser_keys(app: &tauri::App) {
    use tauri::Manager;
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
    use windows::core::Interface;

    let Some(window) = app.get_webview_window(commands::MAIN_WINDOW) else {
        return;
    };
    let _ = window.with_webview(|webview| {
        // SAFETY: COM calls on the webview's own controller, on the thread
        // Tauri runs this closure on (the UI thread).
        let result = unsafe {
            webview
                .controller()
                .CoreWebView2()
                .and_then(|core| core.Settings())
                .and_then(|settings| settings.cast::<ICoreWebView2Settings3>())
                .and_then(|settings| settings.SetAreBrowserAcceleratorKeysEnabled(false))
        };
        if let Err(err) = result {
            eprintln!("termbay: could not turn off the browser keys: {err}");
        }
    });
}

#[cfg(not(windows))]
fn disable_browser_keys(_app: &tauri::App) {}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn browser_options_open_the_app_only_for_the_driver() {
        let driver_args = args(&["--remote-debugging-port=0", "--user-data-dir=C:\\tmp"]);
        assert_eq!(parse_command(driver_args.clone(), true), Ok(Command::Gui));
        let err = parse_command(driver_args, false).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        // Known options keep working either way.
        assert_eq!(
            parse_command(args(&["--version"]), true),
            Ok(Command::Version)
        );
        assert_eq!(parse_command(args(&[]), false), Ok(Command::Gui));
    }

    #[test]
    fn release_builds_never_tolerate_unknown_options() {
        if !cfg!(feature = "e2e") {
            assert!(!launched_by_webview_driver());
        }
    }
}
