//! Tauri side of the pty bridge (decision D6 in `docs/FASE1-PLANO.md`).
//!
//! Commands (names as seen from JavaScript):
//!
//! - `pty_open({ cols, rows, onData, cwd? })` -> id (number). `onData` is
//!   a `Channel` that receives the output as raw bytes (`ArrayBuffer`).
//!   An invalid `cwd` opens in the home folder, without error. Fails with
//!   `"too many terminals are open (limit 64)"` at the limit (D18).
//! - `pty_write(bytes, { headers: { "termbay-pty-id": "<id>" } })`: raw
//!   body, id in a header.
//! - `pty_resize({ id, cols, rows })`
//! - `pty_close({ id })`: closing an unknown id is not an error.
//! - `pty_ack({ id, bytes })`: bytes processed by xterm.js; drives the flow
//!   control (D25). Unknown ids are ignored.
//! - `pty_info({ id })` -> `{ shell, cwd, busy, foreground }`.
//! - `get_config()` -> `{ autocomplete: { enabled, trigger, history } }`;
//!   an invalid file gives the defaults, never an error.
//!
//! When a pty ends (exit or close), `pty-exit` is emitted to the `main`
//! window with `{ id, exitCode }`, after its last output was sent.
//! Errors are readable strings.

use std::sync::Arc;

use serde::Serialize;
use tauri::http::HeaderMap;
use tauri::ipc::{Channel, InvokeBody, InvokeResponseBody, Request};
use tauri::{AppHandle, Emitter, Manager, RunEvent, Runtime, State, Window, WindowEvent};

use termbay_core::config::{self, Config};
use termbay_core::pty::PtyInfo;

use crate::bridge::{PtyBridge, PtyExit};
use crate::office::OfficeHub;

/// Header that carries the pty id of a `pty_write` call.
pub const PTY_ID_HEADER: &str = "termbay-pty-id";
/// Event emitted when a pty ends.
pub const PTY_EXIT_EVENT: &str = "pty-exit";
/// Label of the only window that talks to the ptys.
pub const MAIN_WINDOW: &str = "main";

/// Payload of [`PTY_EXIT_EVENT`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PtyExitPayload {
    id: u32,
    exit_code: Option<u32>,
}

impl From<PtyExit> for PtyExitPayload {
    fn from(exit: PtyExit) -> Self {
        Self {
            id: exit.id,
            exit_code: exit.exit_code,
        }
    }
}

/// [`register_with_office`] with a hub of its own, for tests.
#[cfg(test)]
pub fn register<R: Runtime>(builder: tauri::Builder<R>, bridge: PtyBridge) -> tauri::Builder<R> {
    register_with_office(builder, bridge, Arc::default())
}

/// Registers the bridge state, the office hub the event server publishes to
/// (task B1), the commands and the cleanup on window close.
pub fn register_with_office<R: Runtime>(
    builder: tauri::Builder<R>,
    bridge: PtyBridge,
    office: Arc<OfficeHub>,
) -> tauri::Builder<R> {
    builder
        .manage(bridge)
        .manage(office)
        .invoke_handler(tauri::generate_handler![
            pty_open,
            pty_write,
            pty_resize,
            pty_close,
            pty_ack,
            pty_info,
            get_config,
            crate::complete::pty_complete,
            crate::office::office_subscribe
        ])
        .on_window_event(on_window_event)
}

/// Kills every shell when the main window goes away.
fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    if window.label() == MAIN_WINDOW && matches!(event, WindowEvent::Destroyed) {
        window.state::<PtyBridge>().kill_all();
    }
}

/// Kills every shell when the app exits, so none is left orphaned.
pub fn on_run_event<R: Runtime>(app: &AppHandle<R>, event: &RunEvent) {
    if matches!(event, RunEvent::Exit) {
        app.state::<PtyBridge>().kill_all();
    }
}

/// Async and on a blocking thread: starting a shell can take a moment
/// (ConPTY, login shells) and must not hold the main thread.
#[tauri::command]
async fn pty_open<R: Runtime>(
    app: AppHandle<R>,
    cols: u16,
    rows: u16,
    on_data: Channel<InvokeResponseBody>,
    cwd: Option<String>,
) -> Result<u32, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let emitter = app.clone();
        let shell_emitter = app.clone();
        app.state::<PtyBridge>().open_with_shell(
            cols,
            rows,
            cwd.as_deref(),
            move |chunk| {
                // Fails only if the webview is gone; nothing to do then.
                let _ = on_data.send(InvokeResponseBody::Raw(chunk));
            },
            move |exit| {
                let _ = emitter.emit_to(MAIN_WINDOW, PTY_EXIT_EVENT, PtyExitPayload::from(exit));
            },
            move |ready| crate::complete::emit_shell(&shell_emitter, ready),
        )
    })
    .await
    .map_err(|err| format!("could not open a pty: {err}"))?
}

/// Synchronous on purpose: it runs on the main thread in call order, and
/// the bridge only queues the bytes, so it never blocks.
#[tauri::command]
fn pty_write(bridge: State<'_, PtyBridge>, request: Request<'_>) -> Result<(), String> {
    let (id, data) = parse_write(request.body(), request.headers())?;
    bridge.write(id, data.to_vec())
}

#[tauri::command]
fn pty_resize(bridge: State<'_, PtyBridge>, id: u32, cols: u16, rows: u16) -> Result<(), String> {
    bridge.resize(id, cols, rows)
}

/// Async and on a blocking thread: killing waits for the shell to go away
/// (up to a few hundred milliseconds for a stubborn one).
#[tauri::command]
async fn pty_close<R: Runtime>(app: AppHandle<R>, id: u32) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<PtyBridge>().close(id))
        .await
        .map_err(|err| format!("could not close pty {id}: {err}"))?
}

#[tauri::command]
fn pty_ack(bridge: State<'_, PtyBridge>, id: u32, bytes: u64) -> Result<(), String> {
    bridge.ack(id, bytes);
    Ok(())
}

/// Answer of `pty_info` (section 6.3 of `docs/FASE1-PLANO.md`).
#[derive(Debug, Clone, Serialize)]
struct PtyInfoPayload {
    shell: String,
    cwd: Option<String>,
    busy: Option<bool>,
    foreground: Option<String>,
}

impl From<PtyInfo> for PtyInfoPayload {
    fn from(info: PtyInfo) -> Self {
        Self {
            shell: info.shell,
            cwd: info.cwd,
            busy: info.busy,
            foreground: info.foreground,
        }
    }
}

/// Async and on a blocking thread: asking the system (a process snapshot
/// on Windows) can take a moment.
#[tauri::command]
async fn pty_info<R: Runtime>(app: AppHandle<R>, id: u32) -> Result<PtyInfoPayload, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<PtyBridge>().info(id))
        .await
        .map_err(|err| format!("could not read pty {id}: {err}"))?
        .map(PtyInfoPayload::from)
}

/// The configuration the UI needs (D22). Read on each call; problems are
/// logged and give the defaults.
#[tauri::command]
fn get_config() -> Config {
    load_config(config::load())
}

fn load_config(loaded: config::Loaded) -> Config {
    for warning in &loaded.warnings {
        eprintln!("termbay: {warning}");
    }
    loaded.config
}

/// Reads the pty id header and the raw body of a `pty_write` call.
fn parse_write<'a>(body: &'a InvokeBody, headers: &HeaderMap) -> Result<(u32, &'a [u8]), String> {
    let header = headers
        .get(PTY_ID_HEADER)
        .ok_or_else(|| format!("pty_write: missing `{PTY_ID_HEADER}` header"))?;
    let id = header
        .to_str()
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .ok_or_else(|| format!("pty_write: invalid `{PTY_ID_HEADER}` header"))?;
    match body {
        InvokeBody::Raw(bytes) => Ok((id, bytes)),
        InvokeBody::Json(_) => Err("pty_write: the body must be raw bytes (Uint8Array)".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{self, Receiver};
    use std::time::Instant;

    use serde_json::{Value, json};
    use tauri::http::HeaderValue;
    use tauri::ipc::CallbackFn;
    use tauri::test::{
        INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
    };
    use tauri::webview::InvokeRequest;
    use tauri::{App, Listener, WebviewWindow, WebviewWindowBuilder};

    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    use crate::bridge::test_support::{
        CURSOR_ANSWER, CursorQueries, TIMEOUT, echo_marker, plain_shell,
    };

    #[derive(Debug)]
    enum Seen {
        /// Bytes sent through the channel with this callback id.
        Data(u32, Vec<u8>),
        /// Payload of a `pty-exit` event.
        Exit(Value),
    }

    struct Harness {
        app: App<MockRuntime>,
        window: WebviewWindow<MockRuntime>,
        seen: Receiver<Seen>,
        /// Channel id -> (pty id, cursor query finder), filled by `open`.
        ptys: RefCell<HashMap<u32, (u32, CursorQueries)>>,
    }

    fn harness() -> Harness {
        let (tx, seen) = mpsc::channel();
        let data_tx = tx.clone();
        let builder = register(mock_builder(), PtyBridge::with_options(plain_shell()))
            .channel_interceptor(move |_webview, callback, _index, body| {
                if let InvokeResponseBody::Raw(bytes) = body {
                    let _ = data_tx.send(Seen::Data(callback.0, bytes.clone()));
                }
                true
            });
        let app = builder.build(mock_context(noop_assets())).unwrap();
        app.listen_any(PTY_EXIT_EVENT, move |event| {
            let payload = serde_json::from_str(event.payload()).unwrap();
            let _ = tx.send(Seen::Exit(payload));
        });
        let window = WebviewWindowBuilder::new(&app, MAIN_WINDOW, Default::default())
            .build()
            .unwrap();
        Harness {
            app,
            window,
            seen,
            ptys: RefCell::default(),
        }
    }

    fn request(cmd: &str, body: InvokeBody, headers: HeaderMap) -> InvokeRequest {
        InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: if cfg!(windows) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            }
            .parse()
            .unwrap(),
            body,
            headers,
            invoke_key: INVOKE_KEY.to_string(),
        }
    }

    impl Harness {
        fn invoke(&self, cmd: &str, args: Value) -> Result<Value, Value> {
            get_ipc_response(
                &self.window,
                request(cmd, InvokeBody::Json(args), HeaderMap::new()),
            )
            .map(|body| body.deserialize::<Value>().unwrap())
        }

        fn write(&self, id: Option<&str>, body: InvokeBody) -> Result<Value, Value> {
            let mut headers = HeaderMap::new();
            if let Some(id) = id {
                headers.insert(PTY_ID_HEADER, HeaderValue::from_str(id).unwrap());
            }
            get_ipc_response(&self.window, request("pty_write", body, headers))
                .map(|body| body.deserialize::<Value>().unwrap())
        }

        fn open(&self, channel: u32) -> u32 {
            self.open_in(channel, Value::Null).expect("pty_open failed")
        }

        /// `pty_open` with this `cwd` (`null` leaves it out).
        fn open_in(&self, channel: u32, cwd: Value) -> Result<u32, Value> {
            let mut args =
                json!({ "cols": 80, "rows": 24, "onData": format!("__CHANNEL__:{channel}") });
            if !cwd.is_null() {
                args["cwd"] = cwd;
            }
            let id = self.invoke("pty_open", args)?;
            let id = id.as_u64().expect("the id must be a number") as u32;
            self.ptys
                .borrow_mut()
                .insert(channel, (id, CursorQueries::default()));
            Ok(id)
        }

        /// Answers ConPTY's cursor position queries (Windows), as xterm.js
        /// does in the app.
        fn answer_queries(&self, channel: u32, bytes: &[u8]) {
            let mut ptys = self.ptys.borrow_mut();
            let Some((id, queries)) = ptys.get_mut(&channel) else {
                return;
            };
            for _ in 0..queries.count(bytes) {
                let _ = self
                    .app
                    .state::<PtyBridge>()
                    .write(*id, CURSOR_ANSWER.to_vec());
            }
        }

        /// Collects channel data until `needle` shows up.
        fn expect_output(&self, channel: u32, needle: &str) -> Vec<u8> {
            let deadline = Instant::now() + TIMEOUT;
            let mut output = Vec::new();
            while !String::from_utf8_lossy(&output).contains(needle) {
                let left = deadline.saturating_duration_since(Instant::now());
                match self.seen.recv_timeout(left) {
                    Ok(Seen::Data(id, bytes)) if id == channel => {
                        self.answer_queries(channel, &bytes);
                        output.extend(bytes);
                    }
                    Ok(other) => panic!("unexpected {other:?} before {needle:?}"),
                    Err(_) => panic!(
                        "timed out waiting for {needle:?}; output: {}",
                        String::from_utf8_lossy(&output)
                    ),
                }
            }
            output
        }

        /// Waits for the exit event, checking that only data of `channel`
        /// came before it. Returns the payload.
        fn expect_exit(&self, channel: u32) -> Value {
            loop {
                match self.seen.recv_timeout(TIMEOUT).expect("no pty-exit event") {
                    Seen::Data(id, bytes) => {
                        assert_eq!(id, channel);
                        self.answer_queries(channel, &bytes);
                    }
                    Seen::Exit(payload) => return payload,
                }
            }
        }
    }

    #[test]
    fn open_write_and_exit_round_trip() {
        let h = harness();
        let id = h.open(41);
        h.write(
            Some(&id.to_string()),
            InvokeBody::Raw(echo_marker().to_vec()),
        )
        .unwrap();
        h.expect_output(41, "ABCD");
        h.write(Some(&id.to_string()), InvokeBody::Raw(b"exit 3\r".to_vec()))
            .unwrap();
        assert_eq!(h.expect_exit(41), json!({ "id": id, "exitCode": 3 }));
        // Nothing about this pty arrives after its exit event.
        assert!(
            h.seen
                .recv_timeout(std::time::Duration::from_millis(100))
                .is_err()
        );
        // It is gone now.
        let err = h.write(Some(&id.to_string()), InvokeBody::Raw(b"x".to_vec()));
        assert!(err.unwrap_err().as_str().unwrap().contains("no pty"));
    }

    #[test]
    fn write_rejects_a_bad_header_or_body() {
        let h = harness();
        let id = h.open(1).to_string();
        let message =
            |result: Result<Value, Value>| result.unwrap_err().as_str().unwrap().to_owned();

        assert!(message(h.write(None, InvokeBody::Raw(b"x".to_vec()))).contains("missing"));
        assert!(message(h.write(Some("abc"), InvokeBody::Raw(b"x".to_vec()))).contains("invalid"));
        assert!(message(h.write(Some("-1"), InvokeBody::Raw(b"x".to_vec()))).contains("invalid"));
        assert!(message(h.write(Some(&id), InvokeBody::Json(json!("x")))).contains("raw bytes"));
        assert!(message(h.write(Some("999"), InvokeBody::Raw(b"x".to_vec()))).contains("no pty"));
        assert_eq!(
            h.write(Some(&id), InvokeBody::Raw(Vec::new())),
            Ok(Value::Null)
        );
    }

    #[test]
    fn resize_close_and_ack() {
        let h = harness();
        let id = h.open(2);
        assert_eq!(
            h.invoke("pty_resize", json!({ "id": id, "cols": 120, "rows": 40 })),
            Ok(Value::Null)
        );
        let err = h.invoke("pty_resize", json!({ "id": 999, "cols": 1, "rows": 1 }));
        assert!(err.unwrap_err().as_str().unwrap().contains("no pty"));
        assert_eq!(
            h.invoke("pty_ack", json!({ "id": id, "bytes": 4096 })),
            Ok(Value::Null)
        );
        assert_eq!(
            h.invoke("pty_ack", json!({ "id": 999, "bytes": 1 })),
            Ok(Value::Null)
        );

        assert_eq!(h.invoke("pty_close", json!({ "id": id })), Ok(Value::Null));
        let exit = h.expect_exit(2);
        assert_eq!(exit["id"], json!(id));
        // Idempotent, also for ids that never existed.
        assert_eq!(h.invoke("pty_close", json!({ "id": id })), Ok(Value::Null));
        assert_eq!(h.invoke("pty_close", json!({ "id": 999 })), Ok(Value::Null));
        assert_eq!(h.app.state::<PtyBridge>().len(), 0);
    }

    #[test]
    fn invalid_arguments_are_readable_errors() {
        let h = harness();
        let err = h.invoke(
            "pty_open",
            json!({ "cols": 80, "rows": 24, "onData": "nope" }),
        );
        assert!(err.unwrap_err().is_string());
        let err = h.invoke("pty_resize", json!({ "id": "x", "cols": 1, "rows": 1 }));
        assert!(err.unwrap_err().is_string());
    }

    #[test]
    fn closing_the_main_window_kills_every_shell() {
        // The mock runtime has no event loop running here, so the handlers
        // registered by `register` are called directly.
        let h = harness();
        let a = h.open(3);
        let b = h.open(4);
        let bridge = h.app.state::<PtyBridge>();
        assert_eq!(bridge.len(), 2);
        let window = h.window.as_ref().window();
        on_window_event(&window, &WindowEvent::Focused(false));
        assert_eq!(bridge.len(), 2);
        on_window_event(&window, &WindowEvent::Destroyed);
        assert_eq!(bridge.len(), 0);
        let mut exited = Vec::new();
        while exited.len() < 2 {
            if let Seen::Exit(payload) = h.seen.recv_timeout(TIMEOUT).expect("no pty-exit event") {
                exited.push(payload["id"].as_u64().unwrap() as u32);
            }
        }
        exited.sort_unstable();
        assert_eq!(exited, [a.min(b), a.max(b)]);
    }

    #[test]
    fn other_windows_closing_keeps_the_shells() {
        let h = harness();
        h.open(5);
        let other = WebviewWindowBuilder::new(&h.app, "other", Default::default())
            .build()
            .unwrap();
        on_window_event(&other.as_ref().window(), &WindowEvent::Destroyed);
        assert_eq!(h.app.state::<PtyBridge>().len(), 1);
    }

    #[test]
    fn exiting_the_app_kills_every_shell() {
        let h = harness();
        h.open(6);
        on_run_event(h.app.handle(), &RunEvent::Exit);
        assert_eq!(h.app.state::<PtyBridge>().len(), 0);
        h.expect_exit(6);
    }

    /// Uses the real `tauri.conf.json` and `capabilities/main.json`, so the
    /// ACL generated from them is enforced.
    #[test]
    fn capability_allows_the_pty_commands_only_in_the_main_window() {
        let app = register(mock_builder(), PtyBridge::with_options(plain_shell()))
            .build(crate::context())
            .unwrap();
        let window = |label: &str| {
            app.get_webview_window(label).unwrap_or_else(|| {
                WebviewWindowBuilder::new(&app, label, Default::default())
                    .build()
                    .unwrap()
            })
        };
        let ack = || {
            request(
                "pty_ack",
                InvokeBody::Json(json!({ "id": 1, "bytes": 1 })),
                HeaderMap::new(),
            )
        };
        assert!(get_ipc_response(&window(MAIN_WINDOW), ack()).is_ok());
        assert!(get_ipc_response(&window("other"), ack()).is_err());
        let config = || request("get_config", InvokeBody::Json(json!({})), HeaderMap::new());
        assert!(get_ipc_response(&window(MAIN_WINDOW), config()).is_ok());
        assert!(get_ipc_response(&window("other"), config()).is_err());
        // `pty_info` of an unknown id fails in both, but only the main
        // window reaches the command.
        let info = || {
            request(
                "pty_info",
                InvokeBody::Json(json!({ "id": 999 })),
                HeaderMap::new(),
            )
        };
        let message = |label: &str| {
            get_ipc_response(&window(label), info())
                .unwrap_err()
                .to_string()
        };
        assert!(message(MAIN_WINDOW).contains("no pty"));
        assert!(!message("other").contains("no pty"));
        // `office_subscribe` without its channel fails in the main window
        // (it reached the command) and is refused before that elsewhere.
        let office = || {
            request(
                "office_subscribe",
                InvokeBody::Json(json!({})),
                HeaderMap::new(),
            )
        };
        let office_error = |label: &str| {
            get_ipc_response(&window(label), office())
                .unwrap_err()
                .to_string()
        };
        assert!(
            office_error(MAIN_WINDOW).contains("events"),
            "{}",
            office_error(MAIN_WINDOW)
        );
        assert!(!office_error("other").contains("events"));
        // A command that does not exist is not reachable either.
        let unknown = request("pty_spy", InvokeBody::Json(json!({})), HeaderMap::new());
        assert!(get_ipc_response(&window(MAIN_WINDOW), unknown).is_err());
    }

    /// Prints the shell's directory.
    fn pwd_command() -> &'static [u8] {
        if cfg!(windows) { b"cd\r" } else { b"pwd\r" }
    }

    #[test]
    fn pty_open_uses_a_valid_cwd_and_falls_back_to_home() {
        let h = harness();
        // Not canonicalized on Windows: that gives a `\\?\` path, which
        // cmd does not accept as its directory.
        let temp = std::env::temp_dir();
        #[cfg(unix)]
        let temp = temp.canonicalize().unwrap();
        let dir = temp.join(format!("termbay-cwd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let name = dir.file_name().unwrap().to_str().unwrap().to_owned();
        let file = dir.join("file");
        std::fs::write(&file, b"x").unwrap();
        let home = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap();
        let home_name = std::path::Path::new(&home)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();

        let id = h.open_in(10, json!(dir.to_str().unwrap())).unwrap();
        h.write(
            Some(&id.to_string()),
            InvokeBody::Raw(pwd_command().to_vec()),
        )
        .unwrap();
        h.expect_output(10, &name);
        h.invoke("pty_close", json!({ "id": id })).unwrap();
        h.expect_exit(10);

        let missing = dir.join("missing");
        let huge = format!("{}/{}", dir.display(), "a".repeat(5000));
        let bad = [
            json!("relative"),
            json!(missing.to_str().unwrap()),
            json!(file.to_str().unwrap()),
            json!(""),
            json!(huge),
        ];
        for (channel, cwd) in (11..).zip(bad) {
            let id = h.open_in(channel, cwd.clone()).unwrap_or_else(|err| {
                panic!("pty_open with {cwd} failed: {err}");
            });
            h.write(
                Some(&id.to_string()),
                InvokeBody::Raw(pwd_command().to_vec()),
            )
            .unwrap();
            h.expect_output(channel, &home_name);
            h.invoke("pty_close", json!({ "id": id })).unwrap();
            h.expect_exit(channel);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pty_open_stops_at_the_limit_until_one_closes() {
        let h = harness();
        let ids: Vec<u32> = (0..64).map(|channel| h.open(100 + channel)).collect();
        let err = h.open_in(200, Value::Null).unwrap_err();
        assert_eq!(err, json!("too many terminals are open (limit 64)"));
        h.invoke("pty_close", json!({ "id": ids[0] })).unwrap();
        h.open_in(201, Value::Null).expect("a slot was freed");
        h.app.state::<PtyBridge>().kill_all();
    }

    #[test]
    fn pty_info_reports_the_shell_and_rejects_unknown_ids() {
        let h = harness();
        let id = h.open(20);
        let deadline = Instant::now() + TIMEOUT;
        let info = loop {
            let info = h.invoke("pty_info", json!({ "id": id })).unwrap();
            if info["busy"] == json!(false) {
                break info;
            }
            assert!(Instant::now() < deadline, "never idle: {info}");
            // Answers ConPTY's start-up query so the shell can run.
            while let Ok(Seen::Data(channel, bytes)) =
                h.seen.recv_timeout(std::time::Duration::from_millis(50))
            {
                h.answer_queries(channel, &bytes);
            }
        };
        let shell = if cfg!(windows) { "cmd" } else { "sh" };
        assert_eq!(info["shell"], json!(shell));
        assert_eq!(info["foreground"], Value::Null);
        assert!(info["cwd"].is_string() || cfg!(windows), "{info}");
        let keys: Vec<_> = info.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys.len(), 4, "{info}");
        let err = h.invoke("pty_info", json!({ "id": 999 })).unwrap_err();
        assert!(err.as_str().unwrap().contains("no pty"), "{err}");
    }

    #[test]
    fn get_config_gives_defaults_for_a_missing_or_invalid_file() {
        let dir = std::env::temp_dir().join(format!("termbay-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let defaults = json!({
            "autocomplete": { "enabled": false, "trigger": "tab", "history": true }
        });
        let answer = |path: &std::path::Path| {
            serde_json::to_value(load_config(config::load_from(path))).unwrap()
        };

        assert_eq!(answer(&path), defaults);
        std::fs::write(
            &path,
            "[autocomplete]\nenabled = true\ntrigger = \"ctrl+space\"\nhistory = false\n",
        )
        .unwrap();
        assert_eq!(
            answer(&path),
            json!({
                "autocomplete": { "enabled": true, "trigger": "ctrl+space", "history": false }
            })
        );
        std::fs::write(&path, "[autocomplete\nenabled = = 1").unwrap();
        assert_eq!(answer(&path), defaults);
        let _ = std::fs::remove_dir_all(&dir);

        // Through the IPC too.
        let h = harness();
        let config = h.invoke("get_config", json!({})).unwrap();
        assert!(config["autocomplete"]["trigger"].is_string(), "{config}");
    }

    #[test]
    fn exit_payload_uses_camel_case_and_null_for_signals() {
        let payload = PtyExitPayload::from(PtyExit {
            id: 5,
            exit_code: None,
        });
        assert_eq!(
            serde_json::to_value(payload).unwrap(),
            json!({ "id": 5, "exitCode": null })
        );
    }
}
