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
    e2e_log::start();
    let command = match parse_command(std::env::args_os().skip(1), launched_by_webview_driver()) {
        Ok(command) => command,
        Err(err) => {
            e2e_log::write(&format!("command line rejected: {err}"));
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
/// tests (with the browser options, including the DevTools port).
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
        e2e_log::setup(app);
        Ok(())
    });
    let builder = e2e_log::on_page_load(builder);
    // Replaces Tauri's default macOS menu, whose "Close Window" takes Cmd+W.
    #[cfg(target_os = "macos")]
    let builder = builder.menu(menu::build);
    let mut context = context();
    pass_driver_browser_args(&mut context);
    let app = commands::register(builder, bridge::PtyBridge::new()).build(context);
    match app {
        Ok(app) => {
            e2e_log::write("app built, entering the event loop");
            app.run(|app, event| {
                e2e_log::run_event(&event);
                commands::on_run_event(app, &event);
            });
            e2e_log::write("event loop finished");
            ExitCode::SUCCESS
        }
        Err(err) => {
            e2e_log::write(&format!("could not start the app: {err}"));
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
        match result {
            Ok(()) => e2e_log::write("webview ready, browser keys off"),
            Err(err) => {
                e2e_log::write(&format!("could not turn off the browser keys: {err}"));
                eprintln!("termbay: could not turn off the browser keys: {err}");
            }
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

/// WebView2's own default switches, which wry uses when a window sets none.
const WEBVIEW2_DEFAULT_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/// The WebView2 driver (`msedgedriver`) passes `--remote-debugging-port`
/// and friends in `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`, but the browser
/// arguments wry sets for every window win over that variable, so the
/// driver never finds DevTools. e2e builds hand the driver's switches to
/// the windows explicitly; release builds ignore the variable.
fn pass_driver_browser_args<R: tauri::Runtime>(context: &mut tauri::Context<R>) {
    if !cfg!(feature = "e2e") {
        return;
    }
    let Some(driver) = std::env::var(WEBVIEW_DRIVER_ENV).ok() else {
        return;
    };
    for window in &mut context.config_mut().app.windows {
        let args = browser_args(window.additional_browser_args.as_deref(), &driver);
        window.additional_browser_args = Some(args);
    }
}

fn browser_args(window: Option<&str>, driver: &str) -> String {
    format!("{} {driver}", window.unwrap_or(WEBVIEW2_DEFAULT_ARGS))
}

/// Startup diagnostics for e2e builds: with `TERMBAY_E2E_LOG` set, the
/// command line, the WebView2 variables, panics and the app's lifecycle are
/// appended to that file, so CI can show why a WebDriver session failed
/// (the driver does not forward the app's stderr). Compiled out of
/// release builds.
mod e2e_log {
    #[cfg(feature = "e2e")]
    pub fn write(line: &str) {
        use std::io::Write;
        let Some(path) = std::env::var_os("TERMBAY_E2E_LOG") else {
            return;
        };
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path);
        if let Ok(mut file) = file {
            let _ = writeln!(file, "[pid {}] {line}", std::process::id());
        }
    }

    #[cfg(not(feature = "e2e"))]
    pub fn write(_line: &str) {}

    /// Logs the page loads of every webview (e2e builds only).
    pub fn on_page_load<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
        if !cfg!(feature = "e2e") {
            return builder;
        }
        builder.on_page_load(|webview, payload| {
            write(&format!(
                "page {:?} in {}: {}",
                payload.event(),
                webview.label(),
                payload.url()
            ));
        })
    }

    /// Logs the end of setup, whether the main window exists, and that the
    /// process is still alive a while later (the driver waits 60 s).
    pub fn setup(app: &tauri::App) {
        if !cfg!(feature = "e2e") {
            return;
        }
        use tauri::Manager;
        let window = app.get_webview_window(crate::commands::MAIN_WINDOW);
        write(&format!(
            "setup finished, main window: {}",
            window.is_some()
        ));
        std::thread::spawn(|| {
            for (wait, total) in [(5, 5), (10, 15), (30, 45)] {
                std::thread::sleep(std::time::Duration::from_secs(wait));
                write(&format!("alive after {total} s"));
            }
        });
    }

    pub fn run_event(event: &tauri::RunEvent) {
        if !cfg!(feature = "e2e") {
            return;
        }
        match event {
            tauri::RunEvent::Ready => write("event loop ready"),
            tauri::RunEvent::ExitRequested { code, .. } => {
                write(&format!("exit requested (code {code:?})"));
            }
            tauri::RunEvent::Exit => write("exit"),
            tauri::RunEvent::WindowEvent { label, event, .. } => {
                if matches!(event, tauri::WindowEvent::Destroyed) {
                    write(&format!("window {label} destroyed"));
                }
            }
            _ => {}
        }
    }

    pub fn start() {
        if !cfg!(feature = "e2e") {
            return;
        }
        let args: Vec<_> = std::env::args_os().collect();
        write(&format!("started with {args:?}"));
        for (key, value) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("WEBVIEW2_") {
                write(&format!("env {key:?}={value:?}"));
            }
        }
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            write(&format!("panic: {info}"));
            default_hook(info);
        }));
    }
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

    #[test]
    fn driver_browser_args_keep_the_window_args() {
        assert_eq!(
            browser_args(None, "--remote-debugging-port=0"),
            format!("{WEBVIEW2_DEFAULT_ARGS} --remote-debugging-port=0")
        );
        assert_eq!(
            browser_args(Some("--lang=pt-BR"), "--remote-debugging-port=0"),
            "--lang=pt-BR --remote-debugging-port=0"
        );
    }
}
