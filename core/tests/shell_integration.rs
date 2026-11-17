//! Shell integration with real fish and zsh (milestone A, macOS and Linux).
//!
//! Each test starts the shell in a pty with a temporary HOME holding a
//! small user configuration, injects the integration and checks that:
//! the user's configuration still loads, the environment is restored, the
//! trusted marks arrive, completions come from the shell itself, the core
//! refuses to send the trigger while a command runs, and no user file is
//! changed. A shell that is not installed is skipped with a message.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use termbay_core::complete::{self, EditMode, ItemKind};
use termbay_core::pty::{PtyEvent, PtySession, ShellSpec, SpawnOptions};
use termbay_core::shell_integration::{
    OscScanner, Phase, RequestOutcome, ScriptDir, ShellKind, ShellLink, ShellMark, inject,
};

const TIMEOUT: Duration = Duration::from_secs(30);

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Skips the test when the shell is missing, unless the shell is listed in
/// `TERMBAY_REQUIRE_SHELLS` (comma separated; the CI sets `zsh,fish` on
/// macOS and Linux), so a runner without it fails instead of passing
/// without testing anything.
fn shell_or_skip(name: &str) -> Option<PathBuf> {
    let found = find_in_path(name);
    if found.is_none() {
        let required = std::env::var("TERMBAY_REQUIRE_SHELLS").unwrap_or_default();
        assert!(
            !required.split(',').any(|s| s.trim() == name),
            "`{name}` is required by TERMBAY_REQUIRE_SHELLS but is not installed"
        );
        eprintln!("skipping: `{name}` is not installed on this machine");
    }
    found
}

/// A temporary folder removed at the end of the test.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("termbay-it-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        // Canonical, so paths reported by the shell compare equal (macOS
        // has /var -> /private/var).
        Self(path.canonicalize().unwrap())
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Files whose names exercise the escaping: space, `;`, line break, ESC.
fn make_work_dir(root: &Path) -> PathBuf {
    let work = root.join("work");
    std::fs::create_dir_all(work.join("fixdir")).unwrap();
    for name in ["fix one.txt", "fix;semi", "fix\nnl", "fix\x1besc"] {
        std::fs::write(work.join(name), "").unwrap();
    }
    work
}

/// Folders that only match "pro" and "aç" when case is ignored (one with a
/// space, one with an accent), next to the work folder.
fn make_case_dir(root: &Path) {
    for name in ["Projects", "Projetos OpenSources", "Ações"] {
        std::fs::create_dir_all(root.join("case").join(name)).unwrap();
    }
}

/// Terminal queries the shells wait for (fish 4 waits for the primary
/// device attributes), with the answers xterm.js would give.
const QUERIES: &[(&[u8], &[u8])] = &[
    (b"\x1b[0c", b"\x1b[?62;22c"),
    (b"\x1b[c", b"\x1b[?62;22c"),
    (b"\x1b[6n", b"\x1b[1;1R"),
];

struct Running {
    session: Arc<PtySession>,
    link: Arc<ShellLink>,
    /// The session nonce, to forge sequences that are almost right.
    nonce: String,
    visible: Arc<Mutex<Vec<u8>>>,
    marks: Arc<Mutex<Vec<ShellMark>>>,
}

impl Running {
    fn start(mut options: SpawnOptions, scripts: &ScriptDir) -> Self {
        let injection = inject(&mut options, scripts).expect("the shell must be recognized");
        let nonce = injection.nonce.as_str().to_owned();
        let link = Arc::new(ShellLink::new(injection.kind));
        let mut scanner = OscScanner::new(&injection.nonce);
        let visible = Arc::new(Mutex::new(Vec::new()));
        let marks = Arc::new(Mutex::new(Vec::new()));
        let (answers_tx, answers_rx) = mpsc::channel::<&'static [u8]>();

        let cb_link = Arc::clone(&link);
        let cb_visible = Arc::clone(&visible);
        let cb_marks = Arc::clone(&marks);
        let mut tail: Vec<u8> = Vec::new();
        let session = PtySession::spawn(options, move |event| {
            let PtyEvent::Output(bytes) = event else {
                cb_link.close();
                return;
            };
            let mut out = Vec::new();
            let mut found = Vec::new();
            scanner.feed(bytes, &mut out, &mut found);
            // Answer queries, even when split between reads.
            tail.extend_from_slice(&out);
            let mut i = 0;
            while i < tail.len() {
                if let Some((query, answer)) =
                    QUERIES.iter().find(|(q, _)| tail[i..].starts_with(q))
                {
                    let _ = answers_tx.send(answer);
                    i += query.len();
                } else {
                    i += 1;
                }
            }
            let keep = tail.len().min(4);
            tail.drain(..tail.len() - keep);
            cb_visible.lock().unwrap().extend_from_slice(&out);
            cb_marks.lock().unwrap().extend(found.iter().cloned());
            cb_link.apply(found);
        })
        .expect("could not start the shell");
        let session = Arc::new(session);
        let writer = Arc::downgrade(&session);
        thread::spawn(move || {
            for answer in answers_rx {
                let Some(session) = writer.upgrade() else {
                    break;
                };
                let _ = session.write(answer);
            }
        });
        Self {
            session,
            link,
            nonce,
            visible,
            marks,
        }
    }

    fn execs(&self) -> usize {
        self.marks
            .lock()
            .unwrap()
            .iter()
            .filter(|m| **m == ShellMark::Exec)
            .count()
    }

    fn prompts(&self) -> usize {
        self.marks
            .lock()
            .unwrap()
            .iter()
            .filter(|m| matches!(m, ShellMark::Prompt { .. }))
            .count()
    }

    fn wait_until(&self, what: &str, mut condition: impl FnMut(&Self) -> bool) {
        let deadline = Instant::now() + TIMEOUT;
        while !condition(self) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting until {what}; output so far:\n{}",
                String::from_utf8_lossy(&self.visible.lock().unwrap())
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn output_contains(&self, needle: &str) -> bool {
        String::from_utf8_lossy(&self.visible.lock().unwrap()).contains(needle)
    }

    /// Runs a command line and waits for the next prompt.
    fn run(&self, line: &str) {
        let before = self.prompts();
        self.session.write(format!("{line}\r").as_bytes()).unwrap();
        self.wait_until("the next prompt", |s| {
            s.prompts() > before && s.link.is_at_prompt()
        });
    }

    fn complete(&self) -> RequestOutcome {
        let session = Arc::clone(&self.session);
        self.link.request(
            move |bytes| session.write(bytes).map_err(|e| e.to_string()),
            Duration::from_secs(10),
        )
    }
}

/// Snapshot of every file under `root`: path and content.
fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(content) = std::fs::read(&path) {
                files.push((path, content));
            }
        }
    }
    files.sort();
    files
}

/// The widget may switch terminal modes (fish does around key bindings),
/// but must not draw anything nor ring the bell.
fn assert_nothing_drawn(output: &[u8]) {
    let mut rest = Vec::new();
    let mut i = 0;
    while i < output.len() {
        if output[i] == 0x1b && output.get(i + 1) == Some(&b'[') {
            i += 2;
            while i < output.len() && !(0x40..=0x7e).contains(&output[i]) {
                i += 1;
            }
            i += 1;
        } else if output[i] == 0x1b {
            i += 2;
        } else {
            rest.push(output[i]);
            i += 1;
        }
    }
    assert!(
        rest.is_empty(),
        "the widget drew {:?}",
        String::from_utf8_lossy(&rest)
    );
}

/// Checks the answer to `ls fix` + trigger and returns the inserts.
/// Whether the shell reports names with a line break correctly: all but
/// fish 3.3, which has no `complete --escape` and prints them split.
fn reports_line_breaks(shell: &Running) -> bool {
    let Some(info) = shell.link.info() else {
        return true;
    };
    if info.shell != "fish" {
        return true;
    }
    let mut parts = info
        .version
        .split('.')
        .map(|p| p.parse::<u32>().unwrap_or(0));
    let (major, minor) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    (major, minor) >= (3, 4)
}

/// [`check_ls_fix`] on a new request, knowing the shell's limits.
fn check_ls_fix_for(shell: &Running) -> Vec<String> {
    let strict = reports_line_breaks(shell);
    check_ls_fix(shell.complete(), strict)
}

fn check_ls_fix(outcome: RequestOutcome, strict: bool) -> Vec<String> {
    let RequestOutcome::Reply(reply) = outcome else {
        panic!("expected a reply, got {outcome:?}");
    };
    assert_eq!(reply.mode, EditMode::Insert);
    assert_eq!(reply.line, "ls fix");
    assert_eq!(reply.cursor, 6);
    assert_eq!(reply.replace_from, 3);
    let built = complete::build(&reply, None);
    for item in &built.items {
        assert_eq!(item.kind, ItemKind::Shell);
        assert!(!item.text.chars().any(complete::is_unsafe), "{item:?}");
        let after_dels = item.insert.trim_start_matches('\u{7f}');
        assert!(!after_dels.chars().any(complete::is_unsafe), "{item:?}");
        assert!(!strict || item.text.starts_with("fix"), "{item:?}");
    }
    let texts: Vec<&str> = built.items.iter().map(|i| i.text.as_str()).collect();
    let mut names = vec!["fix one.txt", "fix;semi", "fixdir/"];
    if strict {
        names.push("fix nl");
    }
    for expected in names {
        assert!(
            texts.contains(&expected),
            "missing {expected:?} in {texts:?}"
        );
    }
    let raw_inserts: Vec<String> = reply.items.iter().map(|i| i.insert.clone()).collect();
    assert!(
        raw_inserts.contains(&"fix\\ one.txt".to_owned()),
        "{raw_inserts:?}"
    );
    assert!(
        raw_inserts.contains(&"fix\\;semi".to_owned()),
        "{raw_inserts:?}"
    );
    assert!(
        raw_inserts.contains(&"fixdir/".to_owned()),
        "{raw_inserts:?}"
    );
    // Every name arrived, the ones with a line break and an ESC included,
    // quoted by the shell so the insert has no control character.
    if strict {
        assert_eq!(built.items.len(), 5, "{texts:?}");
    }
    built.items.iter().map(|i| i.insert.clone()).collect()
}

/// Asks for completions of `line` (typed at an empty prompt) and returns
/// the built items as (text, insert), then clears the line.
fn items_for(shell: &Running, line: &str) -> Vec<(String, String)> {
    shell.session.write(line.as_bytes()).unwrap();
    let outcome = shell.complete();
    shell.session.write(b"\x15").unwrap();
    let RequestOutcome::Reply(reply) = outcome else {
        panic!("expected a reply for {line:?}, got {outcome:?}");
    };
    assert_eq!(reply.mode, EditMode::Insert);
    assert_eq!(reply.line, line);
    complete::build(&reply, None)
        .items
        .into_iter()
        .map(|item| (item.text, item.insert))
        .collect()
}

/// "pro" finds "Projects" and "Projetos OpenSources", and "aç" finds
/// "Ações": the typed word is replaced (DEL per character, then the
/// escaped name).
fn check_ignores_case(shell: &Running) {
    shell.run("cd ../case");
    let items = items_for(shell, "ls pro");
    let del3 = "\u{7f}".repeat(3);
    for expected in [
        ("Projects/".to_owned(), format!("{del3}Projects/")),
        (
            "Projetos OpenSources/".to_owned(),
            format!("{del3}Projetos\\ OpenSources/"),
        ),
    ] {
        assert!(
            items.contains(&expected),
            "missing {expected:?} in {items:?}"
        );
    }
    let items = items_for(shell, "ls aç");
    let expected = ("Ações/".to_owned(), format!("{}Ações/", "\u{7f}".repeat(2)));
    assert!(
        items.contains(&expected),
        "missing {expected:?} in {items:?}"
    );
    shell.run("cd ../work");
}

/// While a command runs, the trigger is never written, even if a program
/// prints a forged mark without the nonce.
fn check_refused_while_running(shell: &Running) {
    shell
        .session
        .write(b"printf '\\033]6973;0000;prompt;\\007'; sleep 3\r")
        .unwrap();
    shell.wait_until("the command runs", |s| {
        s.link.info().is_some() && !s.link.is_at_prompt()
    });
    thread::sleep(Duration::from_millis(300));
    let outcome = shell.link.request(
        |_| panic!("the trigger must not be written while a command runs"),
        Duration::from_secs(1),
    );
    assert_eq!(outcome, RequestOutcome::Unavailable);
    assert!(
        !shell.output_contains("\x1b]6973"),
        "private sequences never reach the screen"
    );
}

#[test]
fn zsh_integration() {
    let Some(zsh) = shell_or_skip("zsh") else {
        return;
    };
    let tmp = TempDir::new("zsh");
    let home = tmp.0.join("home");
    let zdotdir = home.join(".config").join("zsh");
    std::fs::create_dir_all(&zdotdir).unwrap();
    let histfile = home.join("hist").join("zsh_history");
    std::fs::write(zdotdir.join(".zshenv"), "export FROM_ZSHENV=yes\n").unwrap();
    std::fs::write(
        zdotdir.join(".zshrc"),
        format!(
            "alias hi='echo HELLO_FROM_''RC'\nHISTFILE={}\nPS1='%% '\nautoload -Uz compinit\ncompinit -u -d {}/.zcompdump\n",
            histfile.display(),
            home.display()
        ),
    )
    .unwrap();
    let work = make_work_dir(&tmp.0);
    make_case_dir(&tmp.0);
    let scripts = ScriptDir::install(&tmp.0.join("data")).unwrap();
    let before = snapshot(&home);

    let options = SpawnOptions {
        shell: Some(ShellSpec::new(&zsh).arg("-l")),
        cwd: Some(work.clone()),
        env: vec![
            ("HOME".into(), home.clone().into()),
            ("ZDOTDIR".into(), zdotdir.clone().into()),
            ("LANG".into(), "en_US.UTF-8".into()),
        ],
        ..SpawnOptions::default()
    };
    let shell = Running::start(options, &scripts);
    shell.wait_until("the handshake and the first prompt", |s| {
        s.link.is_at_prompt()
    });
    let info = shell.link.info().unwrap();
    assert_eq!((info.protocol, info.shell.as_str()), (1, "zsh"));
    assert_eq!(info.history_file.as_deref(), Some(histfile.as_path()));
    assert_eq!(shell.link.kind(), ShellKind::Zsh);

    // The user's configuration loaded and the environment is the original.
    shell.run(
        "hi; print -r -- \"N=[${TERMBAY_NONCE-}] U=[${TERMBAY_USER_ZDOTDIR-}] Z=[$ZDOTDIR] E=[$FROM_ZSHENV]\"",
    );
    assert!(shell.output_contains("HELLO_FROM_RC"));
    let expected = format!("N=[] U=[] Z=[{}] E=[yes]", zdotdir.display());
    assert!(shell.output_contains(&expected), "expected {expected}");
    // Only the integration's variables: the test's own environment may have
    // others (TERMBAY_REQUIRE_SHELLS in the CI, TERMBAY_CONFIG).
    shell.run("print -r -- \"C=[$(env | grep -cE '^TERMBAY_(NONCE|USER_ZDOTDIR)=')]\"");
    assert!(
        shell.output_contains("C=[0]"),
        "no TERMBAY_ variable is left"
    );
    // Marks of the standard protocols reach the screen.
    assert!(shell.output_contains("\x1b]133;A\x07"));
    assert!(shell.output_contains("\x1b]133;C\x07"));
    assert!(shell.output_contains("\x1b]133;D;0\x07"));
    assert!(shell.output_contains("\x1b]7;file://"));
    assert!(shell.marks.lock().unwrap().contains(&ShellMark::Exec));

    // Completions come from zsh's own completion system.
    shell.session.write(b"ls fix").unwrap();
    shell.wait_until("the typed text is echoed", |s| s.output_contains("fix"));
    thread::sleep(Duration::from_millis(200));
    let mark = shell.visible.lock().unwrap().len();
    let first = check_ls_fix_for(&shell);
    thread::sleep(Duration::from_millis(200));
    assert_nothing_drawn(&shell.visible.lock().unwrap()[mark..]);
    // The widget does not change the line: a second request sees the same.
    let second = check_ls_fix_for(&shell);
    assert_eq!(first, second);
    shell.session.write(b"\x15").unwrap(); // Ctrl+U: clear the line.
    check_ignores_case(&shell);
    // The matcher-list used for the request is gone afterwards.
    shell.run(
        "print -r -- \"S=[$(zstyle -L ':completion:_termbay_compwidget:*' | wc -l | tr -d ' ')]\"",
    );
    assert!(shell.output_contains("S=[0]"));

    check_refused_while_running(&shell);
    shell.wait_until("the prompt after sleep", |s| s.link.is_at_prompt());
    assert_eq!(shell.link.info().unwrap().shell, "zsh");

    shell.session.write(b"exit\r").unwrap();
    shell.session.wait_timeout(TIMEOUT);
    drop(shell);
    let after = snapshot(&home);
    // The user's files are untouched; only what zsh itself writes (the
    // history and the compinit dump) may be new.
    for (path, content) in &before {
        let now = after.iter().find(|(p, _)| p == path).map(|(_, c)| c);
        assert_eq!(now, Some(content), "{} changed", path.display());
    }
    for (path, content) in &after {
        let text = String::from_utf8_lossy(content);
        // Nothing of the scripts (their `_termbay` names or the private
        // sequence) ended up in a user file.
        assert!(
            !text.contains("_termbay") && !text.contains("\x1b]6973"),
            "{} has something of the integration",
            path.display()
        );
    }
}

/// A user without `compinit` (and without a ZDOTDIR of their own): the
/// widget falls back to zsh's globbing for files and to the command names
/// for the first word, still ignoring case.
#[test]
fn zsh_without_compinit_or_zdotdir() {
    if std::env::var_os("ZDOTDIR").is_some() {
        eprintln!("skipping: ZDOTDIR is set in the test environment");
        return;
    }
    let Some(zsh) = shell_or_skip("zsh") else {
        return;
    };
    let tmp = TempDir::new("zsh-plain");
    let home = tmp.0.join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join(".zshrc"),
        "PS1='%% '\nalias hi='echo HI_''HOME'\n",
    )
    .unwrap();
    // Debian and Ubuntu run `compinit` from /etc/zsh/zshrc unless this is
    // set; a user there without the completion system has it.
    std::fs::write(home.join(".zshenv"), "skip_global_compinit=1\n").unwrap();
    let work = make_work_dir(&tmp.0);
    make_case_dir(&tmp.0);
    let scripts = ScriptDir::install(&tmp.0.join("data")).unwrap();
    let options = SpawnOptions {
        shell: Some(ShellSpec::new(&zsh)),
        cwd: Some(work),
        env: vec![
            ("HOME".into(), home.clone().into()),
            ("LANG".into(), "en_US.UTF-8".into()),
        ],
        ..SpawnOptions::default()
    };
    let shell = Running::start(options, &scripts);
    shell.wait_until("the first prompt", |s| s.link.is_at_prompt());
    shell.run("hi; print -r -- \"Z=[${ZDOTDIR-unset}] C=[${+functions[compdef]}]\"");
    assert!(shell.output_contains("HI_HOME"));
    assert!(
        shell.output_contains("Z=[unset] C=[0]"),
        "no compinit was loaded"
    );

    shell.session.write(b"ls fix").unwrap();
    check_ls_fix_for(&shell);
    shell.session.write(b"\x15").unwrap();
    check_ignores_case(&shell);

    // First word: builtins, aliases and commands of $PATH.
    let items = items_for(&shell, "ech");
    assert!(
        items.contains(&("echo".to_owned(), "o".to_owned())),
        "{items:?}"
    );
    let items = items_for(&shell, "h");
    assert!(
        items.contains(&("hi".to_owned(), "i".to_owned())),
        "{items:?}"
    );
    let items = items_for(&shell, "LS");
    assert!(items.iter().any(|(text, _)| text == "ls"), "{items:?}");
    // After the first word only files.
    let items = items_for(&shell, "ls ech");
    assert!(items.is_empty(), "{items:?}");
    // A path under the home folder keeps the `~`.
    std::fs::create_dir_all(home.join("My Docs")).unwrap();
    let items = items_for(&shell, "ls ~/my");
    assert!(
        items.contains(&(
            "~/My Docs/".to_owned(),
            format!("{}~/My\\ Docs/", "\u{7f}".repeat(4))
        )),
        "{items:?}"
    );
    // The Tab of the user is untouched: still no completion system, and no
    // style of ours left (Debian's /etc/zsh/zshrc sets styles of its own).
    shell.run("print -r -- \"C=[${+functions[compdef]}] M=[$(zstyle -L | grep completion | grep -c termbay)]\"");
    assert!(
        shell.output_contains("C=[0] M=[0]"),
        "{}",
        String::from_utf8_lossy(&shell.visible.lock().unwrap())
            .rsplit("C=[")
            .next()
            .unwrap_or("")
    );
}

#[test]
fn fish_integration() {
    let Some(fish) = shell_or_skip("fish") else {
        return;
    };
    let tmp = TempDir::new("fish");
    let home = tmp.0.join("home");
    let config = home.join(".config").join("fish");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("config.fish"),
        "alias hi 'echo HELLO_FROM_''CONFIG'\nfunction fish_greeting; end\nfunction fish_prompt; echo -n '> '; end\n",
    )
    .unwrap();
    let data = home.join(".local").join("share");
    let work = make_work_dir(&tmp.0);
    make_case_dir(&tmp.0);
    let scripts = ScriptDir::install(&tmp.0.join("data")).unwrap();
    let before = snapshot(&home);

    let options = SpawnOptions {
        shell: Some(ShellSpec::new(&fish).arg("-l")),
        cwd: Some(work.clone()),
        env: vec![
            ("HOME".into(), home.clone().into()),
            ("XDG_CONFIG_HOME".into(), home.join(".config").into()),
            ("XDG_DATA_HOME".into(), data.clone().into()),
            ("LANG".into(), "en_US.UTF-8".into()),
        ],
        ..SpawnOptions::default()
    };
    let shell = Running::start(options, &scripts);
    shell.wait_until("the handshake and the first prompt", |s| {
        s.link.is_at_prompt()
    });
    let info = shell.link.info().unwrap();
    assert_eq!((info.protocol, info.shell.as_str()), (1, "fish"));
    assert_eq!(
        info.history_file.as_deref(),
        Some(data.join("fish").join("fish_history").as_path())
    );

    shell.run(
        "hi; echo \"N=[$TERMBAY_NONCE]\" (count (env | string match -r '^TERMBAY_(NONCE|USER_ZDOTDIR)='))",
    );
    assert!(shell.output_contains("HELLO_FROM_CONFIG"));
    assert!(shell.output_contains("N=[] 0"));
    assert!(shell.output_contains("\x1b]7;file://"));

    shell.session.write(b"ls fix").unwrap();
    shell.wait_until("the typed text is echoed", |s| s.output_contains("fix"));
    thread::sleep(Duration::from_millis(200));
    let mark = shell.visible.lock().unwrap().len();
    let first = check_ls_fix_for(&shell);
    thread::sleep(Duration::from_millis(200));
    assert_nothing_drawn(&shell.visible.lock().unwrap()[mark..]);
    let second = check_ls_fix_for(&shell);
    assert_eq!(first, second);
    shell.session.write(b"\x15").unwrap();
    // fish's `complete --do-complete` already ignores case (and ranks exact
    // prefixes first); nothing in the script is needed for it.
    check_ignores_case(&shell);

    check_refused_while_running(&shell);
    shell.wait_until("the prompt after sleep", |s| {
        s.link.is_at_prompt() && s.link.info().is_some()
    });
    assert_ne!(
        shell.marks.lock().unwrap().last(),
        None,
        "marks keep arriving"
    );

    shell.session.write(b"exit\r").unwrap();
    shell.session.wait_timeout(TIMEOUT);
    drop(shell);
    let after = snapshot(&home);
    for (path, content) in &before {
        let now = after.iter().find(|(p, _)| p == path).map(|(_, c)| c);
        assert_eq!(now, Some(content), "{} changed", path.display());
    }
    for (path, content) in &after {
        let text = String::from_utf8_lossy(content);
        // Nothing of the scripts (their `_termbay` names or the private
        // sequence) ended up in a user file.
        assert!(
            !text.contains("_termbay") && !text.contains("\x1b]6973"),
            "{} has something of the integration",
            path.display()
        );
    }
}

#[test]
fn phase_is_running_during_a_command() {
    let Some(program) = shell_or_skip("fish").or_else(|| shell_or_skip("zsh")) else {
        return;
    };
    let tmp = TempDir::new("phase");
    let home = tmp.0.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let scripts = ScriptDir::install(&tmp.0.join("data")).unwrap();
    let options = SpawnOptions {
        shell: Some(ShellSpec::new(&program)),
        cwd: Some(home.clone()),
        env: vec![
            ("HOME".into(), home.clone().into()),
            ("XDG_CONFIG_HOME".into(), home.join(".config").into()),
            ("XDG_DATA_HOME".into(), home.join(".data").into()),
        ],
        ..SpawnOptions::default()
    };
    let shell = Running::start(options, &scripts);
    shell.wait_until("the first prompt", |s| s.link.is_at_prompt());
    shell.session.write(b"sleep 2\r").unwrap();
    shell.wait_until("the exec mark", |s| {
        s.marks.lock().unwrap().contains(&ShellMark::Exec)
    });
    let mut state = termbay_core::shell_integration::ShellState::new();
    for mark in shell.marks.lock().unwrap().iter() {
        state.apply(mark);
    }
    assert_eq!(state.phase(), Phase::Running);
}

/// In vi insert mode the widget answers normally; in vi normal mode it
/// says so, because typed text would run editor commands there.
fn check_vi_modes(shell: &Running) {
    shell.wait_until("the first prompt", |s| s.link.is_at_prompt());
    shell.session.write(b"ls fix").unwrap();
    let RequestOutcome::Reply(reply) = shell.complete() else {
        panic!("expected an answer in insert mode")
    };
    assert_eq!(reply.mode, EditMode::Insert);
    assert!(!reply.items.is_empty());
    shell.session.write(b"\x1b").unwrap();
    // Let the shell see a lone ESC (zsh waits KEYTIMEOUT for more keys).
    thread::sleep(Duration::from_millis(800));
    let RequestOutcome::Reply(reply) = shell.complete() else {
        panic!("expected an answer in normal mode")
    };
    assert_eq!(reply.mode, EditMode::Command);
    assert!(reply.items.is_empty());
}

#[test]
fn zsh_vi_mode() {
    let Some(zsh) = shell_or_skip("zsh") else {
        return;
    };
    let tmp = TempDir::new("zsh-vi");
    let home = tmp.0.join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join(".zshrc"),
        format!(
            "bindkey -v\nPS1='%% '\nautoload -Uz compinit\ncompinit -u -d {}/.zcompdump\n",
            home.display()
        ),
    )
    .unwrap();
    let work = make_work_dir(&tmp.0);
    make_case_dir(&tmp.0);
    let scripts = ScriptDir::install(&tmp.0.join("data")).unwrap();
    let options = SpawnOptions {
        shell: Some(ShellSpec::new(&zsh)),
        cwd: Some(work),
        env: vec![
            ("HOME".into(), home.clone().into()),
            ("ZDOTDIR".into(), home.clone().into()),
        ],
        ..SpawnOptions::default()
    };
    check_vi_modes(&Running::start(options, &scripts));
}

#[test]
fn fish_vi_mode_in_the_c_locale() {
    let Some(fish) = shell_or_skip("fish") else {
        return;
    };
    let tmp = TempDir::new("fish-vi");
    let home = tmp.0.join("home");
    let config = home.join(".config").join("fish");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("config.fish"),
        "fish_vi_key_bindings\nfunction fish_greeting; end\n",
    )
    .unwrap();
    let work = make_work_dir(&tmp.0);
    make_case_dir(&tmp.0);
    let scripts = ScriptDir::install(&tmp.0.join("data")).unwrap();
    let options = SpawnOptions {
        shell: Some(ShellSpec::new(&fish)),
        cwd: Some(work),
        env: vec![
            ("HOME".into(), home.clone().into()),
            ("XDG_CONFIG_HOME".into(), home.join(".config").into()),
            ("XDG_DATA_HOME".into(), home.join(".data").into()),
            ("LC_ALL".into(), "C".into()),
        ],
        ..SpawnOptions::default()
    };
    check_vi_modes(&Running::start(options, &scripts));
}

// ---------------------------------------------------------------------------
// Security checks of milestone A (AC8, `docs/SEGURANCA.md`).
// ---------------------------------------------------------------------------

/// Output that imitates the integration without the session nonce: OSC 133
/// and OSC 7 (any program may print them), private sequences with a wrong,
/// missing, truncated or extended nonce, a handshake for another shell,
/// answers to the next completion requests, a sequence longer than the
/// scanner's limit, and sequences cut by CAN or by another ESC.
fn forged_output(nonce: &str) -> Vec<u8> {
    let mut nonces = vec![
        "f".repeat(32),
        String::new(),
        "0000".to_owned(),
        nonce[..31].to_owned(),
        nonce[1..].to_owned(),
        format!("{nonce}0"),
        format!(" {nonce}"),
    ];
    if nonce.to_uppercase() != nonce {
        nonces.push(nonce.to_uppercase());
    }
    let mut out = b"FORGED_START\r\n".to_vec();
    out.extend_from_slice(b"\x1b]133;A\x07\x1b]133;B\x07\x1b]133;C\x07\x1b]133;D;0\x1b\\");
    out.extend_from_slice(b"\x1b]7;file://evil/etc\x07");
    out.extend_from_slice(b"\x1b]6973;exec\x07\x1b]6973;prompt;0\x1b\\");
    for (i, n) in nonces.iter().enumerate() {
        let end: &[u8] = if i % 2 == 0 { b"\x07" } else { b"\x1b\\" };
        let mut bodies = vec![
            "ready;1;fish;9.9;/tmp/evil_history".to_owned(),
            "prompt;0".to_owned(),
            "exec".to_owned(),
            "prompt;".to_owned(),
        ];
        for request in 1..=20 {
            bodies.push(format!(
                "complete;{request};insert;ls fix;6;3;PWNED;PWNED\\x0d;evil\\x0arm -rf ~"
            ));
        }
        for body in bodies {
            out.extend_from_slice(format!("\x1b]6973;{n};{body}").as_bytes());
            out.extend_from_slice(end);
        }
        // Cut short by CAN, and by the start of another sequence.
        out.extend_from_slice(format!("\x1b]6973;{n};prompt;0\x18").as_bytes());
        out.extend_from_slice(format!("\x1b]6973;{n};exec\x1b[0m").as_bytes());
    }
    // Longer than the scanner's 1 MiB limit, then a forged mark.
    out.extend_from_slice(b"\x1b]6973;");
    out.extend(std::iter::repeat_n(b'a', 1536 * 1024));
    out.extend_from_slice(b"\x07\x1b]6973;ffffffffffffffffffffffffffffffff;prompt;0\x07");
    out.extend_from_slice(b"\r\nFORGED_END\r\n");
    out
}

/// `cat` of a forged file, then `sleep`: nothing in the file changes the
/// state, requests are refused while the command runs, nothing private
/// reaches the screen, and the next completion is the shell's own.
fn check_forged_output_is_ignored(shell: &Running, dir: &Path) {
    let file = dir.join("forged.txt");
    std::fs::write(&file, forged_output(&shell.nonce)).unwrap();
    let info = shell.link.info().unwrap();
    let marks_before = shell.marks.lock().unwrap().len();
    let shown_before = shell.visible.lock().unwrap().len();
    let shown_since = |s: &Running| s.visible.lock().unwrap()[shown_before..].to_vec();

    shell
        .session
        .write(format!("cat '{}'; sleep 2\r", file.display()).as_bytes())
        .unwrap();
    shell.wait_until("cat printed the whole file", |s| {
        String::from_utf8_lossy(&shown_since(s)).contains("FORGED_END")
    });
    assert!(
        !shell.link.is_at_prompt(),
        "forged prompt marks were trusted"
    );
    let outcome = shell.link.request(
        |_| panic!("the trigger must not be written while a command runs"),
        Duration::from_secs(1),
    );
    assert_eq!(outcome, RequestOutcome::Unavailable);

    shell.wait_until("the prompt after the command", |s| s.link.is_at_prompt());
    let new_marks = shell.marks.lock().unwrap()[marks_before..].to_vec();
    assert_eq!(
        new_marks.iter().filter(|m| **m == ShellMark::Exec).count(),
        1,
        "only the shell's own exec mark: {new_marks:?}"
    );
    assert!(
        new_marks
            .iter()
            .all(|m| matches!(m, ShellMark::Exec | ShellMark::Prompt { .. })),
        "no forged handshake or completion: {new_marks:?}"
    );
    assert_eq!(
        new_marks.last(),
        Some(&ShellMark::Prompt { exit_code: Some(0) })
    );
    assert_eq!(
        shell.link.info().unwrap(),
        info,
        "the handshake is unchanged"
    );

    let shown = shown_since(shell);
    let text = String::from_utf8_lossy(&shown);
    assert!(
        !text.contains("]6973"),
        "private sequences never reach the screen"
    );
    assert!(!text.contains("PWNED") && !text.contains("aaaaaaaa"));
    // Standard marks are not filtered: xterm.js sees them (the menu only
    // closes on them; it never opens without a request).
    assert!(text.contains("\x1b]133;C\x07"));

    // The next completion is the shell's own answer, not a forged one.
    shell.session.write(b"ls fix").unwrap();
    check_ls_fix_for(shell);
    shell.session.write(b"\x15").unwrap();
}

/// A full-screen program runs: requests are refused without writing
/// anything, so the Tab the UI then sends goes to the program as typed.
fn check_full_screen_programs(shell: &Running, dir: &Path) {
    let alt_screen_since = |s: &Running, from: usize| {
        s.visible.lock().unwrap()[from..]
            .windows(8)
            .any(|w| w == b"\x1b[?1049h")
    };
    let refuse = |what: &str| {
        let outcome = shell.link.request(
            |_| panic!("the trigger must not be written while {what} runs"),
            Duration::from_secs(1),
        );
        assert_eq!(outcome, RequestOutcome::Unavailable, "{what}");
    };

    if let Some(less) = find_in_path("less") {
        let pager = dir.join("pager.txt");
        let lines: String = (0..300).map(|i| format!("pager line {i}\n")).collect();
        std::fs::write(&pager, lines).unwrap();
        let from = shell.visible.lock().unwrap().len();
        shell
            .session
            .write(format!("'{}' '{}'\r", less.display(), pager.display()).as_bytes())
            .unwrap();
        shell.wait_until("less is on screen", |s| {
            alt_screen_since(s, from) && s.output_contains("pager line 10")
        });
        refuse("less");
        shell.session.write(b"\tq").unwrap();
        shell.wait_until("the prompt after less", |s| s.link.is_at_prompt());
    } else {
        eprintln!("skipping the less check: `less` is not installed");
    }

    let Some(vim) = find_in_path("vim").or_else(|| find_in_path("vi")) else {
        eprintln!("skipping the vim check: `vim` is not installed");
        return;
    };
    let target = dir.join("typed-in-vim.txt");
    let from = shell.visible.lock().unwrap().len();
    shell
        .session
        .write(
            format!(
                "'{}' -u NONE -i NONE -N -n '{}'\r",
                vim.display(),
                target.display()
            )
            .as_bytes(),
        )
        .unwrap();
    shell.wait_until("vim is on screen", |s| alt_screen_since(s, from));
    thread::sleep(Duration::from_millis(500));
    shell.session.write(b"i").unwrap();
    thread::sleep(Duration::from_millis(300));
    refuse("vim");
    // What the UI sends after `unavailable`: the Tab itself.
    shell.session.write(b"\tX\x1b").unwrap();
    thread::sleep(Duration::from_millis(300));
    shell.session.write(b":wq\r").unwrap();
    shell.wait_until("the prompt after vim", |s| s.link.is_at_prompt());
    let written = std::fs::read(&target).unwrap();
    assert_eq!(written, b"\tX\n", "vim got exactly the Tab, nothing else");
}

fn zsh_session(tmp: &TempDir, zshrc: &str) -> Option<(Running, PathBuf)> {
    let zsh = shell_or_skip("zsh")?;
    let home = tmp.0.join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join(".zshrc"), zshrc).unwrap();
    let work = make_work_dir(&tmp.0);
    let scripts = ScriptDir::install(&tmp.0.join("data")).unwrap();
    let options = SpawnOptions {
        shell: Some(ShellSpec::new(&zsh)),
        cwd: Some(work),
        env: vec![
            ("HOME".into(), home.clone().into()),
            ("ZDOTDIR".into(), home.clone().into()),
            ("LANG".into(), "en_US.UTF-8".into()),
        ],
        ..SpawnOptions::default()
    };
    let shell = Running::start(options, &scripts);
    shell.wait_until("the first prompt", |s| s.link.is_at_prompt());
    Some((shell, home))
}

fn fish_session(tmp: &TempDir, config: &str) -> Option<(Running, PathBuf)> {
    let fish = shell_or_skip("fish")?;
    let home = tmp.0.join("home");
    let dir = home.join(".config").join("fish");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.fish"),
        format!("function fish_greeting; end\n{config}"),
    )
    .unwrap();
    let work = make_work_dir(&tmp.0);
    let scripts = ScriptDir::install(&tmp.0.join("data")).unwrap();
    let options = SpawnOptions {
        shell: Some(ShellSpec::new(&fish)),
        cwd: Some(work),
        env: vec![
            ("HOME".into(), home.clone().into()),
            ("XDG_CONFIG_HOME".into(), home.join(".config").into()),
            ("XDG_DATA_HOME".into(), home.join(".data").into()),
            ("LANG".into(), "en_US.UTF-8".into()),
        ],
        ..SpawnOptions::default()
    };
    let shell = Running::start(options, &scripts);
    shell.wait_until("the first prompt", |s| s.link.is_at_prompt());
    Some((shell, home))
}

#[test]
fn zsh_ignores_forged_output_and_full_screen_programs_keep_tab() {
    let tmp = TempDir::new("zsh-sec");
    let rc = format!(
        "PS1='%% '\nautoload -Uz compinit\ncompinit -u -d {}/.zcompdump\n",
        tmp.0.join("home").display()
    );
    let Some((shell, _home)) = zsh_session(&tmp, &rc) else {
        return;
    };
    check_forged_output_is_ignored(&shell, &tmp.0);
    check_full_screen_programs(&shell, &tmp.0);
}

#[test]
fn fish_ignores_forged_output_and_full_screen_programs_keep_tab() {
    let tmp = TempDir::new("fish-sec");
    let Some((shell, _home)) = fish_session(&tmp, "") else {
        return;
    };
    check_forged_output_is_ignored(&shell, &tmp.0);
    check_full_screen_programs(&shell, &tmp.0);
}

/// zsh: programs started by the user's own startup files (`.zshenv`,
/// `.zprofile`, `.zshrc`, `.zlogin`) never see the nonce nor the shim's
/// variables, because the shim removes them before loading any of them.
#[test]
fn zsh_startup_files_never_see_the_nonce() {
    let Some(zsh) = shell_or_skip("zsh") else {
        return;
    };
    let tmp = TempDir::new("zsh-env");
    let home = tmp.0.join("home");
    let zdotdir = home.join("zdot");
    std::fs::create_dir_all(&zdotdir).unwrap();
    for name in [".zshenv", ".zprofile", ".zshrc", ".zlogin"] {
        std::fs::write(
            zdotdir.join(name),
            format!("env > '{}/env{name}'\n", tmp.0.display()),
        )
        .unwrap();
    }
    let scripts = ScriptDir::install(&tmp.0.join("data")).unwrap();
    let options = SpawnOptions {
        shell: Some(ShellSpec::new(&zsh).arg("-l")),
        cwd: Some(home.clone()),
        env: vec![
            ("HOME".into(), home.clone().into()),
            ("ZDOTDIR".into(), zdotdir.clone().into()),
        ],
        ..SpawnOptions::default()
    };
    let shell = Running::start(options, &scripts);
    shell.wait_until("the first prompt", |s| s.link.is_at_prompt());
    for name in [".zshenv", ".zprofile", ".zshrc", ".zlogin"] {
        let env = std::fs::read_to_string(tmp.0.join(format!("env{name}")))
            .unwrap_or_else(|_| panic!("{name} did not run"));
        assert!(!env.contains(&shell.nonce), "{name} saw the nonce");
        assert!(!env.contains("TERMBAY_NONCE"), "{name}: {env}");
        assert!(!env.contains("TERMBAY_USER_ZDOTDIR"), "{name}: {env}");
        assert!(
            env.contains(&format!("ZDOTDIR={}\n", zdotdir.display())),
            "{name} sees the user's own ZDOTDIR"
        );
    }
}

/// fish: the known window (`docs/AUTOCOMPLETE.md`, section 10). The
/// `--init-command` runs after `config.fish`, so programs started by the
/// user's `config.fish` (and `conf.d`) still inherit `TERMBAY_NONCE`.
/// Commands typed in the session do not. If this test starts failing
/// because the nonce is gone from `config.fish`, the window was closed:
/// update `docs/SEGURANCA.md` and turn the assertion around.
#[test]
fn fish_known_window_config_fish_children_inherit_the_nonce() {
    let tmp = TempDir::new("fish-env");
    let config = format!("env > '{}/env-config'\n", tmp.0.display());
    let Some((shell, _home)) = fish_session(&tmp, &config) else {
        return;
    };
    let env = std::fs::read_to_string(tmp.0.join("env-config")).unwrap();
    assert!(
        env.contains(&format!("TERMBAY_NONCE={}", shell.nonce)),
        "config.fish no longer sees the nonce: the known window is closed"
    );
    shell.run(&format!("env > '{}/env-session'", tmp.0.display()));
    let env = std::fs::read_to_string(tmp.0.join("env-session")).unwrap();
    assert!(!env.contains("TERMBAY_NONCE"), "{env}");
    assert!(!env.contains(&shell.nonce));
}

/// The script folder refuses unsafe places and repairs planted files.
#[test]
fn script_folder_refuses_unsafe_permissions() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let set_mode =
        |path: &Path, mode| std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    let tmp = TempDir::new("si-perms");
    let base = tmp.0.join("data");
    let dir = ScriptDir::install(&base).unwrap();
    let root = dir.root().to_path_buf();

    // Group-writable or world-writable folders, at any level.
    for (folder, mode) in [
        (root.clone(), 0o770),
        (root.clone(), 0o707),
        (base.join("termbay"), 0o777),
        (base.join("termbay").join("shell-integration"), 0o775),
        (root.join("zsh"), 0o722),
    ] {
        set_mode(&folder, mode).unwrap();
        let err = ScriptDir::install(&base).unwrap_err();
        assert_eq!(
            err.kind(),
            std::io::ErrorKind::PermissionDenied,
            "{} with {mode:o}",
            folder.display()
        );
        set_mode(&folder, 0o700).unwrap();
    }
    assert_eq!(ScriptDir::install(&base).unwrap(), dir);

    // A symlink in place of the versioned folder (to a folder someone
    // else may control) is refused, not followed.
    let elsewhere = tmp.0.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    set_mode(&elsewhere, 0o700).unwrap();
    std::fs::rename(&root, tmp.0.join("moved")).unwrap();
    symlink(&elsewhere, &root).unwrap();
    let err = ScriptDir::install(&base).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    std::fs::remove_file(&root).unwrap();
    std::fs::rename(tmp.0.join("moved"), &root).unwrap();

    // A symlink planted in place of a script is replaced by the real file;
    // its target is not written.
    let decoy = tmp.0.join("decoy.zsh");
    std::fs::write(&decoy, "echo decoy\n").unwrap();
    let script = root.join("zsh").join("termbay.zsh");
    let content = std::fs::read(&script).unwrap();
    std::fs::remove_file(&script).unwrap();
    symlink(&decoy, &script).unwrap();
    ScriptDir::install(&base).unwrap();
    let metadata = std::fs::symlink_metadata(&script).unwrap();
    assert!(metadata.is_file(), "the symlink was replaced");
    assert_eq!(metadata.permissions().mode() & 0o077, 0);
    assert_eq!(std::fs::read(&script).unwrap(), content);
    assert_eq!(std::fs::read_to_string(&decoy).unwrap(), "echo decoy\n");
}

/// A script with the right content but writable by others is written
/// again (P3 in `docs/SEGURANCA.md`): with traversable folders (0755),
/// another local user could otherwise change it before a new tab starts.
#[test]
fn script_files_writable_by_others_are_repaired() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = TempDir::new("si-file-mode");
    let base = tmp.0.join("data");
    let dir = ScriptDir::install(&base).unwrap();
    let script = dir.root().join("zsh").join("termbay.zsh");
    for folder in [dir.root().to_path_buf(), dir.root().join("zsh")] {
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o666)).unwrap();
    ScriptDir::install(&base).unwrap();
    let mode = std::fs::metadata(&script).unwrap().permissions().mode();
    assert_eq!(mode & 0o022, 0, "the script is still writable by others");
}

// ---------------------------------------------------------------------------
// Latency of a completion request (AC8, `docs/AUTOCOMPLETE.md`, section 6:
// p95 below 100 ms for `git`, `cd` and `ls`, in fish and zsh).
//
//   cargo test --release -p termbay-core --test shell_integration \
//     completion_latency -- --ignored --nocapture
//
// Measured in the core: from `ShellLink::request` (trigger written to the
// pty) to the decoded answer, plus `complete::build`. No UI, no IPC. With
// `TERMBAY_BENCH_USER_HOME=1` the shells load the real user configuration
// instead of a small one in a temporary HOME.
// ---------------------------------------------------------------------------

const LATENCY_RUNS: usize = 60;
const LATENCY_WARMUP: usize = 3;

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    let rank = ((sorted.len() as f64) * p).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// A folder with a git repository (branches `checkout-a`, `cherry-b`),
/// sub-folders and files, for `git ch`, `cd ` and `ls `.
fn make_latency_dir(root: &Path) -> (PathBuf, bool) {
    let work = root.join("latency");
    for i in 0..20 {
        std::fs::create_dir_all(work.join(format!("dir{i:02}"))).unwrap();
        std::fs::write(work.join(format!("file{i:02}.txt")), "").unwrap();
    }
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(&work)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .is_ok_and(|out| out.status.success())
    };
    let has_git = git(&["init", "-q", "-b", "main"])
        && git(&["commit", "-q", "--allow-empty", "-m", "init"])
        && git(&["branch", "checkout-a"])
        && git(&["branch", "cherry-b"]);
    (work, has_git)
}

fn measure(shell: &Running, line: &str) -> (Vec<Duration>, usize) {
    let mut times = Vec::new();
    let mut items = 0;
    for run in 0..LATENCY_WARMUP + LATENCY_RUNS {
        shell.session.write(line.as_bytes()).unwrap();
        thread::sleep(Duration::from_millis(30));
        let start = Instant::now();
        let outcome = shell.complete();
        let RequestOutcome::Reply(reply) = outcome else {
            panic!("no answer for {line:?}: {outcome:?}");
        };
        let built = complete::build(&reply, None);
        let elapsed = start.elapsed();
        shell.session.write(b"\x15").unwrap();
        thread::sleep(Duration::from_millis(30));
        if run >= LATENCY_WARMUP {
            times.push(elapsed);
            items = built.items.len();
        }
    }
    times.sort();
    (times, items)
}

fn report_latency(shell_name: &str, shell: &Running, has_git: bool) -> Duration {
    let mut worst = Duration::ZERO;
    let mut cases = vec!["cd ", "ls "];
    if has_git {
        cases.insert(0, "git ch");
    }
    for line in cases {
        let (times, items) = measure(shell, line);
        let p95 = percentile(&times, 0.95);
        worst = worst.max(p95);
        println!(
            "{shell_name} {line:8} items {items:3}  p50 {:6.1} ms  p95 {:6.1} ms  max {:6.1} ms  ({} runs)",
            percentile(&times, 0.50).as_secs_f64() * 1e3,
            p95.as_secs_f64() * 1e3,
            times.last().unwrap().as_secs_f64() * 1e3,
            times.len()
        );
    }
    worst
}

#[test]
#[ignore = "benchmark: run with --release -- --ignored --nocapture"]
fn completion_latency() {
    let user_home = std::env::var_os("TERMBAY_BENCH_USER_HOME").is_some();
    let tmp = TempDir::new("latency");
    let (work, has_git) = make_latency_dir(&tmp.0);
    let scripts = ScriptDir::install(&tmp.0.join("data")).unwrap();
    let home = tmp.0.join("home");
    std::fs::create_dir_all(home.join(".config").join("fish")).unwrap();
    std::fs::write(
        home.join(".zshrc"),
        format!(
            "PS1='%% '\nautoload -Uz compinit\ncompinit -u -d {}/.zcompdump\n",
            home.display()
        ),
    )
    .unwrap();
    std::fs::write(
        home.join(".config").join("fish").join("config.fish"),
        "function fish_greeting; end\n",
    )
    .unwrap();
    let env = |shell: &str| -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
        if user_home {
            return Vec::new();
        }
        let mut env = vec![("HOME".into(), home.clone().into())];
        match shell {
            "zsh" => env.push(("ZDOTDIR".into(), home.clone().into())),
            _ => {
                env.push(("XDG_CONFIG_HOME".into(), home.join(".config").into()));
                env.push(("XDG_DATA_HOME".into(), home.join(".data").into()));
            }
        }
        env
    };
    println!(
        "configuration: {}",
        if user_home {
            "the user's own"
        } else {
            "temporary HOME"
        }
    );

    let mut worst = Duration::ZERO;
    for name in ["zsh", "fish"] {
        let Some(program) = shell_or_skip(name) else {
            continue;
        };
        let options = SpawnOptions {
            shell: Some(ShellSpec::new(&program)),
            cwd: Some(work.clone()),
            env: env(name),
            ..SpawnOptions::default()
        };
        let shell = Running::start(options, &scripts);
        shell.wait_until("the first prompt", |s| s.link.is_at_prompt());
        worst = worst.max(report_latency(name, &shell, has_git));
    }
    assert!(
        worst < Duration::from_millis(100),
        "p95 of {:.1} ms (goal: below 100 ms)",
        worst.as_secs_f64() * 1e3
    );
}

/// P1 in `docs/SEGURANCA.md`: a line typed ahead while a command runs
/// (`sleep 0.3` + Enter, then `cat` + Enter) is still in the terminal's
/// input queue when `sleep` ends. zsh sends the prompt mark only from zle
/// with no pending input, so the core never sees "at the prompt" before
/// `cat` starts. Here the input goes straight to the pty, so the core does
/// not even know about the typeahead: the shell alone must get it right.
#[test]
fn typed_ahead_command_never_gets_the_trigger() {
    let tmp = TempDir::new("race");
    let Some((shell, _home)) = zsh_session(&tmp, "PS1='%% '\n") else {
        return;
    };
    for round in 0..5 {
        let from = shell.visible.lock().unwrap().len();
        let execs = shell.execs();
        shell.session.write(b"sleep 0.3\r").unwrap();
        shell.wait_until("sleep runs", |s| s.execs() > execs);
        shell.session.write(b"cat\r").unwrap();
        // A Tab pressed as soon as the core believes the shell is at its
        // prompt (busy wait, as fast as a request can come), or, when no
        // prompt mark comes, 150 ms after `sleep` ended (its precmd printed
        // the OSC 133 D mark). With the fix, the prompt mark only comes
        // after `cat`, so the request is refused.
        let deadline = Instant::now() + TIMEOUT;
        let mut ended: Option<Instant> = None;
        while !shell.link.is_at_prompt() {
            if ended.is_none()
                && String::from_utf8_lossy(&shell.visible.lock().unwrap()[from..])
                    .contains("\x1b]133;D;0")
            {
                ended = Some(Instant::now());
            }
            if ended.is_some_and(|at| at.elapsed() > Duration::from_millis(150)) {
                break;
            }
            assert!(Instant::now() < deadline, "sleep did not end");
            std::hint::spin_loop();
        }
        let session = Arc::clone(&shell.session);
        let outcome = shell.link.request(
            move |bytes| session.write(bytes).map_err(|e| e.to_string()),
            Duration::from_millis(300),
        );
        shell.wait_until("cat runs", |s| s.execs() > execs + 1);
        thread::sleep(Duration::from_millis(200));
        let leaked = shell.visible.lock().unwrap()[from..]
            .windows(3)
            .any(|w| w == b"\xf4\x8f\xbf");
        // End `cat` (a pending partial line needs the CR before Ctrl+D).
        shell.session.write(b"\r\x04").unwrap();
        shell.wait_until("the prompt after cat", |s| s.link.is_at_prompt());
        assert!(!leaked, "round {round}: `cat` received the trigger");
        assert_eq!(outcome, RequestOutcome::Unavailable, "round {round}");
    }
}

/// P1 for fish, through the app's path: every key goes through the bridge,
/// which tells the core about line breaks. fish cannot see pending input,
/// so after typeahead the core waits until the prompt stays quiet
/// (`TYPEAHEAD_SETTLE`); the typed-ahead `cat` starts first.
#[test]
fn fish_typed_ahead_command_never_gets_the_trigger() {
    let tmp = TempDir::new("race-fish");
    let Some((shell, _home)) = fish_session(&tmp, "") else {
        return;
    };
    let type_keys = |s: &Running, keys: &[u8]| {
        s.link.note_input(keys);
        s.session.write(keys).unwrap();
    };
    for round in 0..5 {
        let from = shell.visible.lock().unwrap().len();
        let execs = shell.execs();
        type_keys(&shell, b"sleep 0.3\r");
        shell.wait_until("sleep runs", |s| s.execs() > execs);
        let prompts = shell.prompts();
        type_keys(&shell, b"cat\r");
        // A Tab pressed right after the prompt mark of `sleep`.
        let deadline = Instant::now() + TIMEOUT;
        while shell.prompts() == prompts {
            assert!(Instant::now() < deadline, "sleep did not end");
            std::hint::spin_loop();
        }
        let session = Arc::clone(&shell.session);
        let outcome = shell.link.request(
            move |bytes| session.write(bytes).map_err(|e| e.to_string()),
            Duration::from_millis(600),
        );
        shell.wait_until("cat runs", |s| s.execs() > execs + 1);
        thread::sleep(Duration::from_millis(200));
        let leaked = shell.visible.lock().unwrap()[from..]
            .windows(3)
            .any(|w| w == b"\xf4\x8f\xbf");
        type_keys(&shell, b"\r\x04");
        shell.wait_until("the prompt after cat", |s| s.link.is_at_prompt());
        assert!(
            !leaked,
            "round {round}: `cat` received the trigger ({outcome:?})"
        );
        assert_eq!(outcome, RequestOutcome::Unavailable, "round {round}");
    }
    // Afterwards the prompt settles and completion works again.
    shell.session.write(b"ls fix").unwrap();
    check_ls_fix_for(&shell);
}

/// P2: the fish 3.3 path (no `complete --escape`), forced on this fish.
/// Candidates are escaped by the script with `string escape`.
#[test]
fn fish_without_complete_escape() {
    let Some(fish) = shell_or_skip("fish") else {
        return;
    };
    let tmp = TempDir::new("fish-noescape");
    let home = tmp.0.join("home");
    std::fs::create_dir_all(home.join(".config").join("fish")).unwrap();
    std::fs::write(
        home.join(".config").join("fish").join("config.fish"),
        "function fish_greeting; end\n",
    )
    .unwrap();
    std::fs::create_dir_all(home.join("My Docs")).unwrap();
    let work = make_work_dir(&tmp.0);
    make_case_dir(&tmp.0);
    let scripts = ScriptDir::install(&tmp.0.join("data")).unwrap();
    let options = SpawnOptions {
        shell: Some(ShellSpec::new(&fish)),
        cwd: Some(work),
        env: vec![
            ("HOME".into(), home.clone().into()),
            ("XDG_CONFIG_HOME".into(), home.join(".config").into()),
            ("XDG_DATA_HOME".into(), home.join(".data").into()),
            ("LANG".into(), "en_US.UTF-8".into()),
            ("TERMBAY_TEST_FISH_NO_ESCAPE".into(), "1".into()),
        ],
        ..SpawnOptions::default()
    };
    let shell = Running::start(options, &scripts);
    shell.wait_until("the first prompt", |s| s.link.is_at_prompt());
    shell.session.write(b"ls fix").unwrap();
    check_ls_fix(shell.complete(), false);
    shell.session.write(b"\x15").unwrap();
    check_ignores_case(&shell);
    let items = items_for(&shell, "ls ~/My");
    assert!(
        items.iter().any(|(text, insert)| text == "~/My Docs/"
            && insert == " Docs/".replace(' ', "\\ ").as_str()),
        "{items:?}"
    );
}

/// P4: the script folder is deleted while the app is open. The next spawn
/// writes it again, so zsh still loads the user's .zshrc and integrates.
#[test]
fn deleted_scripts_are_written_again_before_a_spawn() {
    let Some(zsh) = shell_or_skip("zsh") else {
        return;
    };
    let tmp = TempDir::new("si-deleted");
    let home = tmp.0.join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join(".zshrc"), "PS1='%% '\nalias hi='echo HI_''RC'\n").unwrap();
    let base = tmp.0.join("data");
    let scripts = ScriptDir::install(&base).unwrap();
    std::fs::remove_dir_all(base.join("termbay")).unwrap();
    let options = SpawnOptions {
        shell: Some(ShellSpec::new(&zsh)),
        cwd: Some(home.clone()),
        env: vec![
            ("HOME".into(), home.clone().into()),
            ("ZDOTDIR".into(), home.clone().into()),
        ],
        ..SpawnOptions::default()
    };
    let shell = Running::start(options, &scripts);
    shell.wait_until("the first prompt", |s| s.link.is_at_prompt());
    shell.run("hi");
    assert!(
        shell.output_contains("HI_RC"),
        "the user's .zshrc was loaded"
    );
    assert!(scripts.root().join("zsh").join(".zshenv").is_file());
}

/// P4: when the scripts cannot be written again, the shell starts without
/// integration (untouched options), never with a broken ZDOTDIR.
#[test]
fn unrepairable_scripts_leave_the_shell_untouched() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = TempDir::new("si-unrepairable");
    let base = tmp.0.join("data");
    let scripts = ScriptDir::install(&base).unwrap();
    std::fs::remove_dir_all(scripts.root()).unwrap();
    let parent = scripts.root().parent().unwrap().to_path_buf();
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o500)).unwrap();
    let mut options = SpawnOptions {
        shell: Some(ShellSpec::new("/bin/zsh")),
        ..SpawnOptions::default()
    };
    let before = format!("{options:?}");
    let injected = inject(&mut options, &scripts);
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(injected.is_none());
    assert_eq!(format!("{options:?}"), before);
}

/// P3: the data folder that holds `termbay/` must not be writable by others
/// (unless it has the sticky bit, like /tmp). Group write is fine when the
/// group is the user's own (Ubuntu's umask 002).
#[test]
fn a_shared_data_folder_is_refused() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = TempDir::new("si-shared-base");
    let base = tmp.0.join("data");
    std::fs::create_dir_all(&base).unwrap();
    for mode in [0o777, 0o757, 0o703] {
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(mode)).unwrap();
        let err = ScriptDir::install(&base).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied, "{mode:o}");
    }
    std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o1777)).unwrap();
    ScriptDir::install(&base).unwrap();
    // The temporary folder belongs to the user's primary group here.
    std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o775)).unwrap();
    ScriptDir::install(&base).unwrap();
    std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o755)).unwrap();
    ScriptDir::install(&base).unwrap();
}

/// Letters typed ahead without Enter: zle-line-init sees pending input and
/// waits; once zle has read them, the prompt mark comes from
/// zle-line-pre-redraw and completion works on that line.
#[test]
fn zsh_letters_typed_ahead_still_complete() {
    let tmp = TempDir::new("typeahead-letters");
    let Some((shell, _home)) = zsh_session(
        &tmp,
        "PS1='%% '\nautoload -Uz compinit\ncompinit -u -C -d \"$HOME/.zcompdump\"\n",
    ) else {
        return;
    };
    let execs = shell.execs();
    shell.session.write(b"sleep 0.3\r").unwrap();
    shell.wait_until("sleep runs", |s| s.execs() > execs);
    shell.session.write(b"ls fix").unwrap();
    shell.wait_until("the prompt after sleep", |s| s.link.is_at_prompt());
    check_ls_fix_for(&shell);
}
