//! Runs the real `termbay-hook` binary against a real event server
//! (task A3 in `docs/FASE2-PLANO.md`). In every case it must exit with 0,
//! print nothing on stdout and finish within its budget.

use std::io::Write;
use std::net::TcpListener;
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use termbay_core::events::env::{EVENTS_VAR, HOOK_VARS, PANE_VAR, TOKEN_VAR};
use termbay_core::events::{EventServer, ReducedEvent};

const HOOK: &str = env!("CARGO_BIN_EXE_termbay-hook");
/// The 200 ms budget plus generous room for a slow CI machine to spawn and
/// tear down the process.
const MAX_RUN: Duration = Duration::from_secs(3);

const PAYLOAD: &str = r#"{"session_id":"sess-0001","hook_event_name":"PreToolUse","tool_name":"Read",
"cwd":"/home/user/projeto","transcript_path":"/home/user/.claude/t.jsonl",
"tool_input":{"file_path":"/home/user/projeto/src/api.ts"},"tool_use_id":"toolu_A"}"#;

fn server() -> (EventServer, Receiver<ReducedEvent>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let server = EventServer::start(move |event| {
        let _ = tx.lock().unwrap().send(event);
    })
    .unwrap();
    (server, rx)
}

/// Runs the hook with exactly the given hook variables (others removed) and
/// `stdin`, and checks the contract: exit 0, empty stdout, within `MAX_RUN`.
fn run_hook(vars: &[(&str, String)], stdin: &[u8]) -> Output {
    let mut command = Command::new(HOOK);
    command.arg("PreToolUse");
    for key in HOOK_VARS {
        command.env_remove(key);
    }
    command.envs(vars.iter().map(|(k, v)| (*k, v.as_str())));
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let started = Instant::now();
    let mut child = command.spawn().expect("termbay-hook runs");
    let mut pipe = child.stdin.take().unwrap();
    let input = stdin.to_vec();
    // The hook may stop reading early (too big, or not inside termbay).
    let writer = thread::spawn(move || {
        let _ = pipe.write_all(&input);
    });
    let output = child.wait_with_output().unwrap();
    let _ = writer.join();
    let elapsed = started.elapsed();

    assert!(output.status.success(), "exit status {:?}", output.status);
    assert!(
        output.stdout.is_empty(),
        "stdout: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(elapsed < MAX_RUN, "took {elapsed:?}");
    output
}

fn vars_for(server: &EventServer) -> Vec<(&'static str, String)> {
    vec![
        (EVENTS_VAR, server.addr().to_string()),
        (TOKEN_VAR, server.token().to_owned()),
        (PANE_VAR, "3".to_owned()),
    ]
}

#[test]
fn sends_the_reduced_event_to_the_app() {
    let (server, rx) = server();
    run_hook(&vars_for(&server), PAYLOAD.as_bytes());
    let event = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("event delivered");
    assert_eq!(event.event, "PreToolUse");
    assert_eq!(event.session, "sess-0001");
    assert_eq!(event.pane.as_deref(), Some("3"));
    assert_eq!(event.file.as_deref(), Some("api.ts"));
    assert_eq!(event.tool_use_id.as_deref(), Some("toolu_A"));
}

#[test]
fn does_nothing_outside_termbay() {
    let (server, rx) = server();
    run_hook(&[], PAYLOAD.as_bytes());
    // Only the address, or only the token, is not enough either.
    run_hook(
        &[(EVENTS_VAR, server.addr().to_string())],
        PAYLOAD.as_bytes(),
    );
    run_hook(
        &[(TOKEN_VAR, server.token().to_owned())],
        PAYLOAD.as_bytes(),
    );
    assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
}

#[test]
fn wrong_token_bad_input_and_unused_events_are_quiet() {
    let (server, rx) = server();
    let mut wrong = vars_for(&server);
    wrong[1].1 = "0".repeat(64);
    run_hook(&wrong, PAYLOAD.as_bytes());

    let vars = vars_for(&server);
    run_hook(&vars, b"not json");
    run_hook(&vars, b"");
    run_hook(
        &vars,
        br#"{"session_id":"s","hook_event_name":"MessageDisplay","delta":"x"}"#,
    );
    // Bigger than the 8 MiB limit: dropped without reading it all.
    let huge = format!(
        r#"{{"session_id":"s","hook_event_name":"PreToolUse","pad":"{}"}}"#,
        "x".repeat(9 << 20)
    );
    run_hook(&vars, huge.as_bytes());

    assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
}

#[test]
fn gives_up_quickly_when_the_app_is_gone() {
    let (server, _) = server();
    let vars = vars_for(&server);
    drop(server);
    let started = Instant::now();
    run_hook(&vars, PAYLOAD.as_bytes());
    assert!(started.elapsed() < MAX_RUN);
}

#[test]
fn gives_up_quickly_when_the_app_does_not_answer() {
    // Accepts connections (the OS does) but never reads or answers.
    let silent = TcpListener::bind("127.0.0.1:0").unwrap();
    let vars = vec![
        (EVENTS_VAR, silent.local_addr().unwrap().to_string()),
        (TOKEN_VAR, "ab".repeat(32)),
    ];
    let started = Instant::now();
    run_hook(&vars, PAYLOAD.as_bytes());
    // The 200 ms budget, not the 2 s of the server's own timeout.
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "{:?}",
        started.elapsed()
    );
    drop(silent);
}

#[test]
fn debug_explains_on_stderr_only() {
    let vars = vec![(termbay_hook::DEBUG_VAR, "1".to_owned())];
    let output = run_hook(&vars, PAYLOAD.as_bytes());
    assert!(String::from_utf8_lossy(&output.stderr).contains("NotInTermbay"));
}
