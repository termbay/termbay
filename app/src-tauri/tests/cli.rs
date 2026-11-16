//! Runs the real `termbay` binary. None of these commands may open a window.

use std::io::Read;
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

struct Output {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl std::fmt::Debug for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Output")
            .field("status", &self.status)
            .field("stdout", &String::from_utf8_lossy(&self.stdout))
            .field("stderr", &String::from_utf8_lossy(&self.stderr))
            .finish()
    }
}

/// Runs termbay and fails (instead of hanging) if it does not exit on its
/// own, which is what would happen if a window had been opened.
fn termbay(args: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_termbay"))
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run the termbay binary");

    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().expect("wait failed") {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("termbay {args:?} did not exit; did it open a window?");
        }
        thread::sleep(Duration::from_millis(20));
    };

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_end(&mut stderr)
        .unwrap();
    Output {
        status,
        stdout,
        stderr,
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is not UTF-8")
}

#[test]
fn version_matches_cargo_toml() {
    let output = termbay(&["--version"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        stdout(&output).trim_end(),
        format!("termbay {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn short_version_flag() {
    let output = termbay(&["-V"]);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout(&output).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_lists_the_options() {
    let output = termbay(&["--help"]);
    assert!(output.status.success(), "{output:?}");
    let text = stdout(&output);
    assert!(text.contains("Usage:"));
    assert!(text.contains("--version"));
    assert!(text.contains("--help"));
}

#[test]
fn unknown_option_fails_with_usage_error() {
    let output = termbay(&["--definitely-not-an-option"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--definitely-not-an-option"));
    assert!(output.stdout.is_empty());
}
