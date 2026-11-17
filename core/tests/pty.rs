//! Integration tests for `termbay_core::pty`, written for macOS, Linux and
//! Windows (ConPTY). Each test drives a real shell through a real pty.
//!
//! Commands are written so that the text the shell echoes back while we
//! "type" never contains the marker we look for; only the command's output
//! does (for example `echo AB""CD` prints `ABCD`).
//!
//! On Windows, ConPTY asks for the cursor position (`ESC[6n`) when it starts
//! and waits for the answer before running the shell. In the app xterm.js
//! answers; here the test helpers do (see [`CursorQueries`]). The core
//! itself must not answer, or the app would send two answers.

use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Weak};
use std::thread;
use std::time::{Duration, Instant};

use termbay_core::pty::{
    ExitStatus, PtyError, PtyEvent, PtyId, PtyManager, PtySession, PtySize, ReadGate, ShellSpec,
    SpawnOptions, default_shell,
};

/// Generous, because CI machines (and a user's login shell) can be slow.
const TIMEOUT: Duration = Duration::from_secs(30);

enum Event {
    Output(Vec<u8>),
    Exit(ExitStatus),
}

/// Cursor position query a terminal must answer (DSR 6).
const CURSOR_QUERY: &[u8] = b"\x1b[6n";
/// Answer to [`CURSOR_QUERY`]: row 1, column 1.
const CURSOR_ANSWER: &[u8] = b"\x1b[1;1R";

/// Finds cursor position queries in a stream of output chunks, including
/// a query split between two chunks.
#[derive(Default)]
struct CursorQueries {
    /// Last bytes of the previous chunk (fewer than a whole query, so a
    /// query is never counted twice).
    tail: Vec<u8>,
}

impl CursorQueries {
    /// Number of queries that end in `chunk`.
    fn count(&mut self, chunk: &[u8]) -> usize {
        let mut data = std::mem::take(&mut self.tail);
        data.extend_from_slice(chunk);
        let mut count = 0;
        let mut i = 0;
        while i + CURSOR_QUERY.len() <= data.len() {
            if data[i..].starts_with(CURSOR_QUERY) {
                count += 1;
                i += CURSOR_QUERY.len();
            } else {
                i += 1;
            }
        }
        // Fewer than `CURSOR_QUERY.len()` bytes were not scanned yet; they
        // may be the start of a query that ends in the next chunk.
        data.drain(..i);
        self.tail = data;
        count
    }
}

#[test]
fn cursor_queries_are_found_even_when_split() {
    let mut q = CursorQueries::default();
    assert_eq!(q.count(b"abc\x1b[6nxyz"), 1);
    assert_eq!(q.count(b"\x1b["), 0);
    assert_eq!(q.count(b"6"), 0);
    assert_eq!(q.count(b"n\x1b[6n\x1b[6n"), 3);
    assert_eq!(q.count(b"[6n"), 0);
    assert_eq!(q.count(b""), 0);
    assert_eq!(q.count(b"\x1b"), 0);
    assert_eq!(q.count(b"[6n"), 1);
}

/// Collects what a session sends to its callback, answering cursor
/// position queries like a terminal would.
struct Recorder {
    rx: Receiver<Event>,
    output: Vec<u8>,
    exit: Option<ExitStatus>,
    session: Weak<PtySession>,
    queries: CursorQueries,
}

impl Recorder {
    fn output_text(&self) -> String {
        String::from_utf8_lossy(&self.output).into_owned()
    }

    fn pump(&mut self, timeout: Duration) -> bool {
        match self.rx.recv_timeout(timeout) {
            Ok(Event::Output(bytes)) => {
                assert!(self.exit.is_none(), "output delivered after Exit");
                for _ in 0..self.queries.count(&bytes) {
                    if let Some(session) = self.session.upgrade() {
                        let _ = session.write(CURSOR_ANSWER);
                    }
                }
                self.output.extend_from_slice(&bytes);
                true
            }
            Ok(Event::Exit(status)) => {
                assert!(self.exit.is_none(), "Exit delivered twice");
                self.exit = Some(status);
                true
            }
            Err(RecvTimeoutError::Timeout) => true,
            Err(RecvTimeoutError::Disconnected) => false,
        }
    }

    /// Waits until the output contains `needle`.
    fn expect_output(&mut self, needle: &str) {
        let deadline = Instant::now() + TIMEOUT;
        while !self.output_text().contains(needle) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {needle:?}; output so far:\n{}",
                self.output_text()
            );
            if !self.pump(Duration::from_millis(50)) && !self.output_text().contains(needle) {
                panic!(
                    "session ended before {needle:?} appeared; output:\n{}",
                    self.output_text()
                );
            }
        }
    }

    /// Waits for the `Exit` event.
    fn expect_exit(&mut self) -> ExitStatus {
        let deadline = Instant::now() + TIMEOUT;
        while self.exit.is_none() {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for the shell to exit; output:\n{}",
                self.output_text()
            );
            assert!(
                self.pump(Duration::from_millis(50)),
                "callback dropped without Exit"
            );
        }
        self.exit.clone().unwrap()
    }
}

fn spawn(options: SpawnOptions) -> (Arc<PtySession>, Recorder) {
    let (tx, rx) = mpsc::channel();
    let session = PtySession::spawn(options, move |event| {
        let event = match event {
            PtyEvent::Output(bytes) => Event::Output(bytes.to_vec()),
            PtyEvent::Exit(status) => Event::Exit(status),
        };
        let _ = tx.send(event);
    })
    .expect("failed to spawn the shell");
    let session = Arc::new(session);
    let recorder = Recorder {
        rx,
        output: Vec::new(),
        exit: None,
        // Weak, so dropping the session in a test still kills the shell.
        session: Arc::downgrade(&session),
        queries: CursorQueries::default(),
    };
    (session, recorder)
}

/// A small, predictable shell that ignores the user's configuration.
fn plain_shell() -> SpawnOptions {
    #[cfg(windows)]
    let shell = ShellSpec::new("cmd.exe").arg("/D");
    #[cfg(not(windows))]
    let shell = ShellSpec::new("/bin/sh");

    SpawnOptions {
        shell: Some(shell),
        // `ENV` would make an interactive POSIX sh read a startup file.
        env: vec![
            ("ENV".into(), "/dev/null".into()),
            ("PS1".into(), "$ ".into()),
        ],
        ..SpawnOptions::default()
    }
}

/// A shell that can report the terminal size.
fn size_shell() -> SpawnOptions {
    #[cfg(windows)]
    {
        SpawnOptions {
            shell: Some(
                ShellSpec::new("powershell.exe")
                    .arg("-NoLogo")
                    .arg("-NoProfile"),
            ),
            ..SpawnOptions::default()
        }
    }
    #[cfg(not(windows))]
    plain_shell()
}

/// Command that prints the terminal size, and the text it prints for a size.
fn size_command() -> (&'static str, fn(PtySize) -> String) {
    if cfg!(windows) {
        (
            "Write-Output ('SZ' + [Console]::WindowHeight + 'x' + [Console]::WindowWidth)\r",
            |s| format!("SZ{}x{}", s.rows, s.cols),
        )
    } else {
        ("stty size\r", |s| format!("{} {}", s.rows, s.cols))
    }
}

/// `echo` command for a given shell that prints `marker` without the typed
/// line containing it. `marker` must have a `_` in the middle.
fn echo_command(shell: &Path, marker: &str) -> String {
    let (head, tail) = marker.split_once('_').expect("marker needs a `_`");
    let name = shell
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match name.as_str() {
        "pwsh" | "powershell" => format!("Write-Output ('{head}_' + '{tail}')\r"),
        "cmd" => format!("echo {head}_^{tail}\r"),
        _ => format!("echo {head}_\"\"{tail}\r"),
    }
}

fn process_exists(pid: u32) -> bool {
    if cfg!(windows) {
        let out = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
            .expect("tasklist failed");
        String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
    } else {
        // `ps -p` exits with 1 when there is no such process.
        Command::new("ps")
            .args(["-p", &pid.to_string()])
            .output()
            .expect("ps failed")
            .status
            .success()
    }
}

fn eventually(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + TIMEOUT;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting until {what}");
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn default_shell_echoes_a_marker() {
    let shell = default_shell();
    let command = echo_command(&shell.program, "termbay_marker");
    let (session, mut rec) = spawn(SpawnOptions {
        shell: Some(shell),
        ..SpawnOptions::default()
    });
    session.write(command.as_bytes()).unwrap();
    rec.expect_output("termbay_marker");
}

#[test]
fn plain_shell_echoes_a_marker() {
    let options = plain_shell();
    let command = echo_command(&options.shell.as_ref().unwrap().program, "plain_marker");
    let (session, mut rec) = spawn(options);
    session.write(command.as_bytes()).unwrap();
    rec.expect_output("plain_marker");
}

#[test]
fn terminal_environment_is_set() {
    let (session, mut rec) = spawn(plain_shell());
    let command = if cfg!(windows) {
        "echo %TERM%^|%COLORTERM%^|%TERM_PROGRAM%\r"
    } else {
        "echo \"$TERM|$COLORTERM|$TERM_PROGRAM\"\r"
    };
    session.write(command.as_bytes()).unwrap();
    rec.expect_output("xterm-256color|truecolor|termbay");
}

#[test]
fn initial_size_and_resize_reach_the_shell() {
    let initial = PtySize { rows: 30, cols: 90 };
    let resized = PtySize {
        rows: 37,
        cols: 101,
    };
    let (command, expected) = size_command();

    let (session, mut rec) = spawn(SpawnOptions {
        size: initial,
        ..size_shell()
    });
    assert_eq!(session.size().unwrap(), initial);
    session.write(command.as_bytes()).unwrap();
    rec.expect_output(&expected(initial));
    // Wait for the next prompt before resizing. Before each prompt, bash's
    // readline re-applies the window size it just read (`set_winsize` in
    // rltty.c: TIOCGWINSZ, then TIOCSWINSZ with the same value). A resize
    // landing between those two calls is overwritten with the old size;
    // that is the shell's doing, not the pty's (macOS CI flake).
    #[cfg(not(windows))]
    rec.expect_output(&format!("{}\r\n$ ", expected(initial)));

    session.resize(resized).unwrap();
    assert_eq!(session.size().unwrap(), resized);
    session.write(command.as_bytes()).unwrap();
    rec.expect_output(&expected(resized));
}

#[test]
fn exit_code_is_reported() {
    let (session, mut rec) = spawn(plain_shell());
    session.write(b"exit 7\r").unwrap();
    let status = rec.expect_exit();
    assert_eq!(status.code(), 7, "{status}");
    assert!(status.signal().is_none());
    assert!(!status.success());
    assert_eq!(session.exit_status(), Some(status));
    assert!(!session.is_running());
}

/// Ctrl+C (`\x03`) interrupts a program that is waiting, with no flood
/// and no flow control involved. On Windows it fails when the shell
/// inherits "ignore Ctrl+C" from the process that started it.
#[test]
fn ctrl_c_interrupts_a_waiting_program() {
    let (session, mut rec) = spawn(plain_shell());
    let (waiting, marker): (&[u8], &[u8]) = if cfg!(windows) {
        (
            b"powershell -NoProfile -Command \"'WAIT'+'ING'; Start-Sleep 60\"\r",
            b"echo AB^CD\r",
        )
    } else {
        // Printed by the program itself, so it already handles SIGINT when
        // the marker shows up (the shell's own child could still be between
        // fork and exec, ignoring it).
        (
            b"sh -c 'echo WAIT\"\"ING; exec sleep 60'\r",
            b"echo AB\"\"CD\r",
        )
    };
    session.write(waiting).unwrap();
    rec.expect_output("WAITING");
    session.write(b"\x03").unwrap();
    // Typed again each second: a line typed while the program is still
    // exiting may be discarded (PowerShell does), which is not the bug.
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut typed = Instant::now();
    session.write(marker).unwrap();
    while !rec.output_text().contains("ABCD") {
        assert!(
            Instant::now() < deadline,
            "Ctrl+C did not interrupt the program; output:\n{}",
            rec.output_text()
        );
        if typed.elapsed() >= Duration::from_secs(1) {
            session.write(marker).unwrap();
            typed = Instant::now();
        }
        rec.pump(Duration::from_millis(50));
    }
}

#[test]
fn clean_exit_is_success() {
    let (session, mut rec) = spawn(plain_shell());
    session.write(b"exit\r").unwrap();
    assert!(rec.expect_exit().success());
}

#[test]
fn write_and_resize_after_exit_fail() {
    let (session, mut rec) = spawn(plain_shell());
    session.write(b"exit 0\r").unwrap();
    rec.expect_exit();
    assert!(matches!(session.write(b"echo\r"), Err(PtyError::Exited)));
    assert!(matches!(
        session.resize(PtySize::default()),
        Err(PtyError::Exited)
    ));
    // Killing an exited session is a no-op.
    session.kill().unwrap();
}

#[test]
fn kill_leaves_no_process_behind() {
    let (session, mut rec) = spawn(plain_shell());
    let pid = session.process_id().expect("no pid");
    assert!(process_exists(pid), "shell {pid} should be running");

    session.kill().unwrap();
    assert!(session.exit_status().is_some());
    rec.expect_exit();
    assert!(!process_exists(pid), "shell {pid} survived kill()");
}

#[test]
fn dropping_the_session_kills_the_shell() {
    let (session, mut rec) = spawn(plain_shell());
    let pid = session.process_id().expect("no pid");
    drop(session);
    rec.expect_exit();
    assert!(!process_exists(pid), "shell {pid} survived drop");
}

/// A program running in the foreground of the shell must not outlive the
/// session either (the kernel hangs up the terminal's foreground group).
#[cfg(unix)]
#[test]
fn kill_also_ends_the_foreground_program() {
    let (session, mut rec) = spawn(plain_shell());
    session
        .write(b"sh -c 'echo GRAND=$$=; exec sleep 1000'\r")
        .unwrap();
    // The typed line contains `GRAND=$$=`; the output has digits instead.
    let deadline = Instant::now() + TIMEOUT;
    let pid = loop {
        let text = rec.output_text();
        let found = text.match_indices("GRAND=").find_map(|(i, m)| {
            let rest = &text[i + m.len()..];
            let end = rest.find('=')?;
            rest[..end].parse::<u32>().ok()
        });
        if let Some(pid) = found {
            break pid;
        }
        assert!(Instant::now() < deadline, "no pid in output:\n{text}");
        rec.pump(Duration::from_millis(50));
    };
    eventually("sleep is running", || process_exists(pid));

    session.kill().unwrap();
    rec.expect_exit();
    eventually("the foreground program is gone", || !process_exists(pid));
}

/// A shell that ignores SIGHUP is still killed (escalation to SIGKILL).
#[cfg(unix)]
#[test]
fn kill_forces_a_shell_that_ignores_hangup() {
    let (session, mut rec) = spawn(plain_shell());
    let pid = session.process_id().expect("no pid");
    session.write(b"trap '' HUP; echo TR\"\"APPED\r").unwrap();
    rec.expect_output("TRAPPED");

    session.kill().unwrap();
    let status = rec.expect_exit();
    // "Killed" on Linux, "Killed: 9" on macOS: SIGKILL, not SIGHUP.
    assert!(
        status.signal().is_some_and(|s| s.starts_with("Killed")),
        "{status}"
    );
    assert!(!process_exists(pid), "shell {pid} survived kill()");
}

#[test]
fn manager_routes_events_by_id_and_forgets_exited_sessions() {
    let manager = PtyManager::new();
    let (tx, rx) = mpsc::channel::<(PtyId, Option<Vec<u8>>)>();

    let spawn_one = || {
        let tx = tx.clone();
        manager
            .spawn(plain_shell(), move |id, event| {
                let _ = tx.send(match event {
                    PtyEvent::Output(bytes) => (id, Some(bytes.to_vec())),
                    PtyEvent::Exit(_) => (id, None),
                });
            })
            .unwrap()
    };
    let first = spawn_one();
    let second = spawn_one();
    assert_ne!(first, second);
    assert_eq!(manager.ids(), vec![first, second]);

    let shell = plain_shell().shell.unwrap().program;
    manager
        .write(first, echo_command(&shell, "first_one").as_bytes())
        .unwrap();
    manager
        .write(second, echo_command(&shell, "second_two").as_bytes())
        .unwrap();

    let mut outputs: std::collections::HashMap<PtyId, String> = Default::default();
    let mut queries: std::collections::HashMap<PtyId, CursorQueries> = Default::default();
    let deadline = Instant::now() + TIMEOUT;
    while !(outputs.get(&first).is_some_and(|s| s.contains("first_one"))
        && outputs
            .get(&second)
            .is_some_and(|s| s.contains("second_two")))
    {
        assert!(Instant::now() < deadline, "outputs so far: {outputs:?}");
        if let Ok((id, Some(bytes))) = rx.recv_timeout(Duration::from_millis(50)) {
            for _ in 0..queries.entry(id).or_default().count(&bytes) {
                let _ = manager.write(id, CURSOR_ANSWER);
            }
            outputs
                .entry(id)
                .or_default()
                .push_str(&String::from_utf8_lossy(&bytes));
        }
    }
    assert!(!outputs[&first].contains("second_two"));
    assert!(!outputs[&second].contains("first_one"));

    // Explicit kill removes the session right away.
    manager.kill(first).unwrap();
    assert!(matches!(
        manager.write(first, b"x"),
        Err(PtyError::NotFound(id)) if id == first
    ));
    assert!(matches!(manager.kill(first), Err(PtyError::NotFound(_))));

    // A shell that exits on its own is forgotten after its Exit event.
    manager.write(second, b"exit\r").unwrap();
    eventually("the exited session is forgotten", || manager.is_empty());
}

#[test]
fn dropping_the_manager_kills_every_shell() {
    let manager = PtyManager::new();
    let ids: Vec<PtyId> = (0..3)
        .map(|_| manager.spawn(plain_shell(), |_, _| {}).unwrap())
        .collect();
    let pids: Vec<u32> = ids
        .iter()
        .map(|id| manager.get(*id).unwrap().process_id().unwrap())
        .collect();
    drop(manager);
    for pid in pids {
        assert!(!process_exists(pid), "shell {pid} survived the manager");
    }
}

#[test]
fn spawning_a_missing_program_fails() {
    let result = PtySession::spawn(
        SpawnOptions {
            shell: Some(ShellSpec::new("termbay-no-such-shell-for-tests")),
            ..SpawnOptions::default()
        },
        |_| {},
    );
    assert!(matches!(result, Err(PtyError::Spawn(_))), "{result:?}");
}

/// Command that prints forever.
fn flood_command() -> &'static [u8] {
    if cfg!(windows) {
        b"for /L %i in (0,0,1) do @echo yyyyyyyyyyyyyyyy\r"
    } else {
        b"while :; do echo yyyyyyyyyyyyyyyy; done\r"
    }
}

/// A plain shell behind a read gate, after its first marker was printed.
fn gated_shell() -> (Arc<PtySession>, Recorder, Arc<ReadGate>) {
    let gate = Arc::new(ReadGate::new());
    let options = SpawnOptions {
        read_gate: Some(Arc::clone(&gate)),
        ..plain_shell()
    };
    let command = echo_command(&options.shell.as_ref().unwrap().program, "gate_ready");
    let (session, mut rec) = spawn(options);
    session.write(command.as_bytes()).unwrap();
    rec.expect_output("gate_ready");
    (session, rec, gate)
}

/// Collects output for `duration`; returns how many bytes arrived.
fn collect_for(rec: &mut Recorder, duration: Duration) -> usize {
    let before = rec.output.len();
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        rec.pump(Duration::from_millis(10));
    }
    rec.output.len() - before
}

#[test]
fn a_paused_gate_stops_reading_and_resuming_continues() {
    let (session, mut rec, gate) = gated_shell();
    session.write(flood_command()).unwrap();
    rec.expect_output("yyyyyyyyyyyyyyyy\r\n");
    gate.pause();
    // A read that was already waiting finishes once; then nothing.
    collect_for(&mut rec, Duration::from_millis(300));
    assert_eq!(collect_for(&mut rec, Duration::from_millis(500)), 0);
    gate.resume();
    eventually("output flows again", || {
        rec.pump(Duration::from_millis(10));
        collect_for(&mut rec, Duration::from_millis(50)) > 0
    });

    // Closing while paused returns quickly and still delivers the exit.
    gate.pause();
    collect_for(&mut rec, Duration::from_millis(300));
    let started = Instant::now();
    session.kill().unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    rec.expect_exit();
}

#[test]
fn exit_while_paused_drains_and_delivers_the_exit() {
    let (session, mut rec, gate) = gated_shell();
    gate.pause();
    collect_for(&mut rec, Duration::from_millis(200));
    session.write(b"exit 4\r").unwrap();
    // On macOS the kernel does not let a process finish closing its
    // terminal while output is pending, so the shell only goes once the
    // reader resumes, as it does when the UI acks (and `kill` opens the
    // gate first). Elsewhere the exit itself opens the gate.
    let mut status = session.wait_timeout(Duration::from_secs(2));
    if status.is_none() && cfg!(target_os = "macos") {
        gate.resume();
        status = session.wait_timeout(TIMEOUT);
    }
    let status = status.expect("the shell did not exit");
    // DRAIN_TIMEOUT is 500 ms; the rest is slack for slow machines.
    let started = Instant::now();
    assert_eq!(rec.expect_exit(), status);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    assert!(!gate.is_paused(), "the gate must be open after the exit");
    assert_eq!(status.code(), 4);
}

#[test]
fn info_reports_the_shell_idle_busy_and_its_directory() {
    let (session, mut rec) = spawn(plain_shell());
    let command = echo_command(Path::new("sh"), "info_ready");
    let command = if cfg!(windows) {
        echo_command(Path::new("cmd"), "info_ready")
    } else {
        command
    };
    session.write(command.as_bytes()).unwrap();
    rec.expect_output("info_ready");

    let info = session.info();
    assert_eq!(info.shell, if cfg!(windows) { "cmd" } else { "sh" });
    eventually("the shell is idle", || {
        rec.pump(Duration::from_millis(10));
        session.info().busy == Some(false)
    });
    assert_eq!(session.info().foreground, None);

    let dir = std::env::temp_dir().join(format!("termbay-info-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Not on Windows: that gives a `\\?\` path, which cmd rejects.
    #[cfg(unix)]
    let dir = dir.canonicalize().unwrap();
    session
        .write(format!("cd \"{}\"\r", dir.display()).as_bytes())
        .unwrap();
    if cfg!(any(target_os = "linux", target_os = "macos")) {
        let expected = dir.to_str().unwrap().to_owned();
        eventually("the cwd follows cd", || {
            rec.pump(Duration::from_millis(10));
            session.info().cwd.as_deref() == Some(expected.as_str())
        });
    } else {
        assert_eq!(session.info().cwd, None);
    }

    let (busy_command, name): (&[u8], &str) = if cfg!(windows) {
        (b"ping -n 30 127.0.0.1\r", "ping")
    } else {
        (b"sleep 30\r", "sleep")
    };
    session.write(busy_command).unwrap();
    eventually("the foreground program is seen", || {
        rec.pump(Duration::from_millis(10));
        let info = session.info();
        info.busy == Some(true)
            && info
                .foreground
                .is_some_and(|fg| fg.eq_ignore_ascii_case(name))
    });

    session.kill().unwrap();
    rec.expect_exit();
    let info = session.info();
    assert_eq!((info.cwd, info.busy, info.foreground), (None, None, None));
    let _ = std::fs::remove_dir(&dir);
}
