//! Integration tests for `termbay_core::pty`, written for macOS, Linux and
//! Windows (ConPTY). Each test drives a real shell through a real pty.
//!
//! Commands are written so that the text the shell echoes back while we
//! "type" never contains the marker we look for; only the command's output
//! does (for example `echo AB""CD` prints `ABCD`).

use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use termbay_core::pty::{
    ExitStatus, PtyError, PtyEvent, PtyId, PtyManager, PtySession, PtySize, ShellSpec,
    SpawnOptions, default_shell,
};

/// Generous, because CI machines (and a user's login shell) can be slow.
const TIMEOUT: Duration = Duration::from_secs(30);

enum Event {
    Output(Vec<u8>),
    Exit(ExitStatus),
}

/// Collects what a session sends to its callback.
struct Recorder {
    rx: Receiver<Event>,
    output: Vec<u8>,
    exit: Option<ExitStatus>,
}

impl Recorder {
    fn output_text(&self) -> String {
        String::from_utf8_lossy(&self.output).into_owned()
    }

    fn pump(&mut self, timeout: Duration) -> bool {
        match self.rx.recv_timeout(timeout) {
            Ok(Event::Output(bytes)) => {
                assert!(self.exit.is_none(), "output delivered after Exit");
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

fn spawn(options: SpawnOptions) -> (PtySession, Recorder) {
    let (tx, rx) = mpsc::channel();
    let session = PtySession::spawn(options, move |event| {
        let event = match event {
            PtyEvent::Output(bytes) => Event::Output(bytes.to_vec()),
            PtyEvent::Exit(status) => Event::Exit(status),
        };
        let _ = tx.send(event);
    })
    .expect("failed to spawn the shell");
    let recorder = Recorder {
        rx,
        output: Vec::new(),
        exit: None,
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
    let deadline = Instant::now() + TIMEOUT;
    while !(outputs.get(&first).is_some_and(|s| s.contains("first_one"))
        && outputs
            .get(&second)
            .is_some_and(|s| s.contains("second_two")))
    {
        assert!(Instant::now() < deadline, "outputs so far: {outputs:?}");
        if let Ok((id, Some(bytes))) = rx.recv_timeout(Duration::from_millis(50)) {
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
