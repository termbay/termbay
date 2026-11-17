//! Autocomplete on the Tauri side (`docs/AUTOCOMPLETE.md`, milestone A).
//!
//! - `pty_complete({ id })` -> one of
//!   - `{ status: "ok", line, cursor, replaceFrom, items: [{ text, insert,
//!     description?, kind: "shell" | "history" }] }`
//!   - `{ status: "unavailable" }`: no integration, not at the prompt (a
//!     command is running or a line was just submitted), vi normal mode,
//!     or the pty is gone. The UI gives the key to the shell.
//!   - `{ status: "timeout" }`: no answer in 1.5 s.
//!   - `{ status: "cancelled" }`: a newer request for the same pty replaced
//!     this one.
//!
//!   `cursor` and `replaceFrom` count Unicode characters (code points) of
//!   `line`. `insert` is the insertion plan for the word `line[replaceFrom
//!   ..cursor]`: a suffix, or N DEL (U+007F) followed by the text; it has no
//!   other control character. History items are always a suffix.
//! - `pty-shell` event, to the main window, once per pty when the shell
//!   completes the handshake: `{ id, shell: "fish" | "zsh", protocol: 1 }`.
//!   Never emitted when `[autocomplete] enabled` is false, because nothing
//!   is injected then.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use termbay_core::complete::{self, CompletionItem, Completions, EditMode, HistoryCache, ItemKind};
use termbay_core::config;
use termbay_core::pty::SpawnOptions;
use termbay_core::shell_integration::{
    self, COMPLETION_TIMEOUT, OscScanner, RequestOutcome, ScriptDir, ShellInfo, ShellKind,
    ShellLink, ShellMark,
};

use crate::bridge::PtyBridge;
use crate::commands::MAIN_WINDOW;

/// Event emitted when a shell's integration becomes active.
pub const PTY_SHELL_EVENT: &str = "pty-shell";

/// A shell finished the handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellReady {
    pub id: u32,
    pub shell: ShellKind,
    pub protocol: u32,
}

/// Payload of [`PTY_SHELL_EVENT`].
#[derive(Debug, Clone, Serialize)]
struct PtyShellPayload {
    id: u32,
    shell: &'static str,
    protocol: u32,
}

impl From<ShellReady> for PtyShellPayload {
    fn from(ready: ShellReady) -> Self {
        Self {
            id: ready.id,
            shell: ready.shell.name(),
            protocol: ready.protocol,
        }
    }
}

/// Answer of `pty_complete`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum CompleteResponse {
    Ok(CompletionsPayload),
    Unavailable,
    Timeout,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionsPayload {
    line: String,
    cursor: usize,
    replace_from: usize,
    items: Vec<ItemPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ItemPayload {
    text: String,
    insert: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    kind: &'static str,
}

impl From<CompletionItem> for ItemPayload {
    fn from(item: CompletionItem) -> Self {
        Self {
            text: item.text,
            insert: item.insert,
            description: item.description,
            kind: match item.kind {
                ItemKind::Shell => "shell",
                ItemKind::History => "history",
            },
        }
    }
}

impl From<Completions> for CompletionsPayload {
    fn from(completions: Completions) -> Self {
        Self {
            line: completions.line,
            cursor: completions.cursor,
            replace_from: completions.replace_from,
            items: completions
                .items
                .into_iter()
                .map(ItemPayload::from)
                .collect(),
        }
    }
}

#[derive(Debug)]
enum Mode {
    /// `[autocomplete] enabled = false` (the default) or no scripts: the
    /// shell starts exactly as without this module.
    Disabled,
    /// Scripts installed: fish and zsh get the integration.
    Scripts(ScriptDir),
    /// Tests: any shell gets a nonce in `TERMBAY_NONCE` and nothing else,
    /// so a small fake shell can speak the protocol.
    #[cfg(test)]
    Fake(ShellKind),
}

/// Shell integration and autocomplete for every pty of the bridge.
#[derive(Debug)]
pub struct ShellSupport {
    mode: Mode,
    history_enabled: bool,
    history: HistoryCache,
    links: Mutex<HashMap<u32, Arc<ShellLink>>>,
    timeout: Duration,
}

impl ShellSupport {
    /// No integration: shells start as they always did.
    pub fn disabled() -> Self {
        Self::with_mode(Mode::Disabled, false)
    }

    /// Reads `[autocomplete]` from the user's configuration and installs the
    /// scripts when it is enabled. Problems are logged and turn the feature
    /// off; they never stop the app from opening.
    pub fn from_user_config() -> Self {
        let loaded = config::load();
        for warning in &loaded.warnings {
            eprintln!("termbay: {warning}");
        }
        let settings = loaded.config.autocomplete;
        if !settings.enabled {
            return Self::disabled();
        }
        match ScriptDir::install_default() {
            Ok(scripts) => Self::with_mode(Mode::Scripts(scripts), settings.history),
            Err(err) => {
                eprintln!(
                    "termbay: autocomplete disabled: could not install the shell scripts: {err}"
                );
                Self::disabled()
            }
        }
    }

    fn with_mode(mode: Mode, history_enabled: bool) -> Self {
        Self {
            mode,
            history_enabled,
            history: HistoryCache::new(),
            links: Mutex::default(),
            timeout: COMPLETION_TIMEOUT,
        }
    }

    /// Integrates the shell of `options` when the feature is on and the
    /// shell is supported. `None` leaves `options` untouched.
    pub fn prepare(&self, options: &mut SpawnOptions) -> Option<Tap> {
        let (kind, nonce) = match &self.mode {
            Mode::Disabled => return None,
            Mode::Scripts(scripts) => {
                let injection = shell_integration::inject(options, scripts)?;
                (injection.kind, injection.nonce)
            }
            #[cfg(test)]
            Mode::Fake(kind) => {
                let nonce = shell_integration::Nonce::generate().ok()?;
                options
                    .env
                    .push((shell_integration::NONCE_ENV.into(), nonce.as_str().into()));
                (*kind, nonce)
            }
        };
        Some(Tap {
            scanner: OscScanner::new(&nonce),
            link: Arc::new(ShellLink::new(kind)),
            out: Vec::new(),
            marks: Vec::new(),
        })
    }

    /// Remembers the link of a new pty (and forgets links of ptys that
    /// ended).
    pub fn register(&self, id: u32, link: Arc<ShellLink>) {
        let mut links = self.lock();
        links.retain(|_, link| !link.is_closed());
        links.insert(id, link);
    }

    /// Forgets a pty.
    /// Whether the integrated shell of `id` is running a command,
    /// according to its trusted marks. `false` without integration.
    pub fn is_running_command(&self, id: u32) -> bool {
        let link = self.lock().get(&id).cloned();
        link.is_some_and(|link| link.is_running_command())
    }

    pub fn forget(&self, id: u32) {
        if let Some(link) = self.lock().remove(&id) {
            link.close();
        }
    }

    /// Input queued for a pty (see [`ShellLink::note_input`]).
    pub fn note_input(&self, id: u32, data: &[u8]) {
        if let Some(link) = self.link(id) {
            link.note_input(data);
        }
    }

    /// Asks the shell of pty `id` for completions. `write` queues the
    /// trigger for the shell.
    pub fn complete<W>(&self, id: u32, write: W) -> CompleteResponse
    where
        W: FnOnce(&[u8]) -> Result<(), String>,
    {
        let Some(link) = self.link(id) else {
            return CompleteResponse::Unavailable;
        };
        let reply = match link.request(write, self.timeout) {
            RequestOutcome::Reply(reply) => reply,
            RequestOutcome::Unavailable => return CompleteResponse::Unavailable,
            RequestOutcome::Timeout => return CompleteResponse::Timeout,
            RequestOutcome::Cancelled => return CompleteResponse::Cancelled,
        };
        if reply.mode == EditMode::Command {
            return CompleteResponse::Unavailable;
        }
        let history = if self.history_enabled {
            link.info()
                .and_then(|info| info.history_file)
                .and_then(|path| self.history.get(link.kind().history_format(), &path))
        } else {
            None
        };
        CompleteResponse::Ok(complete::build(&reply, history.as_deref()).into())
    }

    fn link(&self, id: u32) -> Option<Arc<ShellLink>> {
        self.lock().get(&id).cloned()
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<u32, Arc<ShellLink>>> {
        self.links.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Sits between the pty output and the batcher of an integrated pty.
#[derive(Debug)]
pub struct Tap {
    scanner: OscScanner,
    link: Arc<ShellLink>,
    out: Vec<u8>,
    marks: Vec<ShellMark>,
}

impl Tap {
    /// The shared state of this pty.
    pub fn link(&self) -> &Arc<ShellLink> {
        &self.link
    }

    /// Removes the private sequences from `bytes`, gives the rest to
    /// `forward` and applies the marks. Returns the handshake the first
    /// time it arrives.
    pub fn output(&mut self, bytes: &[u8], forward: impl FnOnce(&[u8])) -> Option<ShellInfo> {
        self.out.clear();
        self.scanner.feed(bytes, &mut self.out, &mut self.marks);
        if !self.out.is_empty() {
            forward(&self.out);
        }
        if self.marks.is_empty() {
            return None;
        }
        self.link.apply(self.marks.drain(..))
    }
}

impl PtyBridge {
    /// Completion request for pty `id` (see the module documentation).
    pub fn complete(&self, id: u32) -> CompleteResponse {
        self.shell
            .complete(id, |bytes| self.write(id, bytes.to_vec()))
    }
}

/// Emits [`PTY_SHELL_EVENT`] to the main window.
pub fn emit_shell<R: Runtime>(app: &AppHandle<R>, ready: ShellReady) {
    let _ = app.emit_to(MAIN_WINDOW, PTY_SHELL_EVENT, PtyShellPayload::from(ready));
}

/// Async and on a blocking thread: it waits for the shell (up to 1.5 s).
#[tauri::command]
pub async fn pty_complete<R: Runtime>(
    app: AppHandle<R>,
    id: u32,
) -> Result<CompleteResponse, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<PtyBridge>().complete(id))
        .await
        .map_err(|err| format!("could not complete in pty {id}: {err}"))
}

#[cfg(all(test, unix))]
pub(crate) mod test_support {
    use termbay_core::pty::{ShellSpec, SpawnOptions};

    use super::*;

    /// A `/bin/sh` script that speaks the protocol like the real scripts:
    /// handshake and prompt at start, then, for each trigger, the answer
    /// to `git ch` (or nothing when `silent`).
    pub fn fake_shell(silent: bool) -> SpawnOptions {
        let answer = if silent {
            ":"
        } else {
            r#"printf '\033]6973;%s;complete;%s;insert;git ch;6;4;checkout;checkout;Switch branches;Checkout;Checkout;\007' "$n" "$req""#
        };
        let script = format!(
            r#"n="$TERMBAY_NONCE"
printf 'fake shell\r\n'
printf '\033]6973;%s;ready;1;zsh;5.9;\007' "$n"
printf '\033]6973;%s;prompt;\007' "$n"
stty -icanon -echo min 4 time 0
req=""
while :; do
  c=$(dd bs=4 count=1 2>/dev/null | od -An -tx1 | tr -d ' \n')
  case "$c" in
    f48fbfb[0-9]) req="$req${{c#f48fbfb}}" ;;
    f48fbfbd) {answer}; req="" ;;
    "") exit 0 ;;
  esac
done
"#
        );
        SpawnOptions {
            shell: Some(ShellSpec::new("/bin/sh").arg("-c").arg(script)),
            ..SpawnOptions::default()
        }
    }

    pub fn fake_support(timeout: Duration) -> ShellSupport {
        let mut support = ShellSupport::with_mode(Mode::Fake(ShellKind::Zsh), true);
        support.timeout = timeout;
        support
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn disabled_changes_nothing() {
        let support = ShellSupport::disabled();
        let mut options = SpawnOptions::default();
        let before = format!("{options:?}");
        assert!(support.prepare(&mut options).is_none());
        assert_eq!(format!("{options:?}"), before);
        assert_eq!(
            support.complete(1, |_| panic!("no write")),
            CompleteResponse::Unavailable
        );
    }

    #[cfg(unix)]
    #[test]
    fn enabled_integrates_only_supported_shells() {
        use termbay_core::pty::ShellSpec;

        let base = std::env::temp_dir().join(format!("termbay-app-si-{}", std::process::id()));
        let scripts = ScriptDir::install(&base).unwrap();
        let support = ShellSupport::with_mode(Mode::Scripts(scripts), true);

        let mut bash = SpawnOptions {
            shell: Some(ShellSpec::new("/bin/bash").arg("-l")),
            ..SpawnOptions::default()
        };
        let before = format!("{bash:?}");
        assert!(support.prepare(&mut bash).is_none());
        assert_eq!(format!("{bash:?}"), before);

        let mut zsh = SpawnOptions {
            shell: Some(ShellSpec::new("/bin/zsh").arg("-l")),
            ..SpawnOptions::default()
        };
        let tap = support.prepare(&mut zsh).expect("zsh is supported");
        assert_eq!(tap.link().kind(), ShellKind::Zsh);
        assert!(zsh.env.iter().any(|(k, _)| k == "TERMBAY_NONCE"));
        assert!(zsh.env.iter().any(|(k, _)| k == "ZDOTDIR"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn responses_follow_the_ipc_contract() {
        let ok = CompleteResponse::Ok(CompletionsPayload {
            line: "git ch".into(),
            cursor: 6,
            replace_from: 4,
            items: vec![
                ItemPayload {
                    text: "checkout".into(),
                    insert: "eckout".into(),
                    description: Some("Switch branches".into()),
                    kind: "shell",
                },
                ItemPayload {
                    text: "git checkout main".into(),
                    insert: "eckout main".into(),
                    description: None,
                    kind: "history",
                },
            ],
        });
        assert_eq!(
            serde_json::to_value(ok).unwrap(),
            json!({
                "status": "ok",
                "line": "git ch",
                "cursor": 6,
                "replaceFrom": 4,
                "items": [
                    { "text": "checkout", "insert": "eckout", "description": "Switch branches", "kind": "shell" },
                    { "text": "git checkout main", "insert": "eckout main", "kind": "history" },
                ],
            })
        );
        for (response, status) in [
            (CompleteResponse::Unavailable, "unavailable"),
            (CompleteResponse::Timeout, "timeout"),
            (CompleteResponse::Cancelled, "cancelled"),
        ] {
            assert_eq!(
                serde_json::to_value(response).unwrap(),
                json!({ "status": status })
            );
        }
        let shell = PtyShellPayload::from(ShellReady {
            id: 3,
            shell: ShellKind::Fish,
            protocol: 1,
        });
        assert_eq!(
            serde_json::to_value(shell).unwrap(),
            json!({ "id": 3, "shell": "fish", "protocol": 1 })
        );
    }

    #[test]
    fn the_tap_strips_private_sequences_and_reports_ready_once() {
        let support = ShellSupport::with_mode(Mode::Fake(ShellKind::Fish), true);
        let mut options = SpawnOptions::default();
        let Some(mut tap) = support.prepare(&mut options) else {
            // No random source on this system (Windows until milestone B).
            return;
        };
        let nonce = options
            .env
            .iter()
            .find(|(k, _)| k == "TERMBAY_NONCE")
            .map(|(_, v)| v.to_string_lossy().into_owned())
            .unwrap();
        let ready =
            format!("a\x1b]6973;{nonce};ready;1;fish;4.0;\x07b\x1b]6973;{nonce};prompt;\x07");
        let mut shown = Vec::new();
        let info = tap.output(ready.as_bytes(), |b| shown.extend_from_slice(b));
        assert_eq!(info.unwrap().shell, "fish");
        assert_eq!(shown, b"ab");
        assert!(tap.link().is_at_prompt());
        assert!(tap.output(ready.as_bytes(), |_| {}).is_none());
        let mut called = false;
        assert!(
            tap.output(b"\x1b]6973;forged;exec\x07", |_| called = true)
                .is_none()
        );
        assert!(!called, "nothing to forward");
        assert!(tap.link().is_at_prompt());
    }
}

/// `pty_complete` and `pty-shell` through the mock runtime, with a fake
/// shell speaking the protocol (Unix: milestone A has no Windows shell).
#[cfg(all(test, unix))]
mod ipc_tests {
    use std::sync::mpsc::{self, Receiver};
    use std::time::Instant;

    use serde_json::{Value, json};
    use tauri::http::HeaderMap;
    use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
    use tauri::test::{
        INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
    };
    use tauri::webview::InvokeRequest;
    use tauri::{App, Listener, WebviewWindow, WebviewWindowBuilder};

    use super::test_support::{fake_shell, fake_support};
    use super::*;
    use crate::bridge::test_support::{TIMEOUT, plain_shell};
    use crate::commands::register;

    struct Harness {
        _app: App<MockRuntime>,
        window: WebviewWindow<MockRuntime>,
        shells: Receiver<Value>,
    }

    fn harness(bridge: PtyBridge) -> Harness {
        let app = register(mock_builder(), bridge)
            .channel_interceptor(|_, _, _, _: &InvokeResponseBody| true)
            .build(mock_context(noop_assets()))
            .unwrap();
        let (tx, shells) = mpsc::channel();
        app.listen_any(PTY_SHELL_EVENT, move |event| {
            let _ = tx.send(serde_json::from_str(event.payload()).unwrap());
        });
        let window = WebviewWindowBuilder::new(&app, MAIN_WINDOW, Default::default())
            .build()
            .unwrap();
        Harness {
            _app: app,
            window,
            shells,
        }
    }

    fn request(cmd: &str, args: Value) -> InvokeRequest {
        InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "tauri://localhost".parse().unwrap(),
            body: InvokeBody::Json(args),
            headers: HeaderMap::new(),
            invoke_key: INVOKE_KEY.to_string(),
        }
    }

    fn invoke(window: &WebviewWindow<MockRuntime>, cmd: &str, args: Value) -> Result<Value, Value> {
        get_ipc_response(window, request(cmd, args))
            .map(|body| body.deserialize::<Value>().unwrap())
    }

    impl Harness {
        fn open(&self) -> u32 {
            let id = invoke(
                &self.window,
                "pty_open",
                json!({ "cols": 80, "rows": 24, "onData": "__CHANNEL__:9" }),
            )
            .expect("pty_open failed");
            id.as_u64().unwrap() as u32
        }

        fn complete(&self, id: u32) -> Value {
            invoke(&self.window, "pty_complete", json!({ "id": id })).expect("pty_complete failed")
        }

        fn wait_for_shell(&self, id: u32) {
            let payload = self
                .shells
                .recv_timeout(TIMEOUT)
                .expect("no pty-shell event");
            assert_eq!(payload, json!({ "id": id, "shell": "zsh", "protocol": 1 }));
        }
    }

    #[test]
    fn ok_with_the_shells_candidates() {
        let h = harness(PtyBridge::with_shell_support(
            fake_shell(false),
            fake_support(TIMEOUT),
        ));
        let id = h.open();
        h.wait_for_shell(id);
        let answer = h.complete(id);
        assert_eq!(
            answer,
            json!({
                "status": "ok",
                "line": "git ch",
                "cursor": 6,
                "replaceFrom": 4,
                "items": [
                    { "text": "checkout", "insert": "eckout", "description": "Switch branches", "kind": "shell" },
                    { "text": "Checkout", "insert": "\u{7f}\u{7f}Checkout", "kind": "shell" },
                ],
            })
        );
        // Each request gets its own answer.
        assert_eq!(h.complete(id)["status"], "ok");
    }

    #[test]
    fn unavailable_without_integration_or_after_enter() {
        let h = harness(PtyBridge::with_shell_support(
            fake_shell(false),
            fake_support(TIMEOUT),
        ));
        assert_eq!(h.complete(999), json!({ "status": "unavailable" }));
        let id = h.open();
        h.wait_for_shell(id);
        // A line break was queued: the line may be running already, so the
        // trigger is not sent until the next prompt mark.
        let mut headers = HeaderMap::new();
        headers.insert(
            crate::commands::PTY_ID_HEADER,
            id.to_string().parse().unwrap(),
        );
        let mut write = request("pty_write", json!(null));
        write.body = InvokeBody::Raw(b"\r".to_vec());
        write.headers = headers;
        get_ipc_response(&h.window, write).unwrap();
        assert_eq!(h.complete(id), json!({ "status": "unavailable" }));

        // Feature off: nothing injected, no pty-shell, always unavailable.
        let h = harness(PtyBridge::with_options(plain_shell()));
        let id = h.open();
        assert!(h.shells.recv_timeout(Duration::from_millis(500)).is_err());
        assert_eq!(h.complete(id), json!({ "status": "unavailable" }));
    }

    #[test]
    fn timeout_when_the_shell_does_not_answer() {
        let h = harness(PtyBridge::with_shell_support(
            fake_shell(true),
            fake_support(Duration::from_millis(300)),
        ));
        let id = h.open();
        h.wait_for_shell(id);
        let start = Instant::now();
        assert_eq!(h.complete(id), json!({ "status": "timeout" }));
        assert!(start.elapsed() >= Duration::from_millis(300));
    }

    #[test]
    fn a_new_request_cancels_the_previous_one() {
        let h = harness(PtyBridge::with_shell_support(
            fake_shell(true),
            fake_support(Duration::from_secs(2)),
        ));
        let id = h.open();
        h.wait_for_shell(id);
        std::thread::scope(|scope| {
            let window = &h.window;
            let first =
                scope.spawn(move || invoke(window, "pty_complete", json!({ "id": id })).unwrap());
            std::thread::sleep(Duration::from_millis(300));
            let second = h.complete(id);
            assert_eq!(first.join().unwrap(), json!({ "status": "cancelled" }));
            assert_eq!(second, json!({ "status": "timeout" }));
        });
    }

    /// Uses the real `tauri.conf.json` and `capabilities/main.json`.
    #[test]
    fn capability_allows_pty_complete_only_in_the_main_window() {
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
        let call = || request("pty_complete", json!({ "id": 1 }));
        let answer = get_ipc_response(&window(MAIN_WINDOW), call()).unwrap();
        assert_eq!(
            answer.deserialize::<Value>().unwrap(),
            json!({ "status": "unavailable" })
        );
        assert!(get_ipc_response(&window("other"), call()).is_err());
    }
}
