//! `termbay hooks install | uninstall | status` (task D2, decision E7 in
//! `docs/FASE2-PLANO.md`): registers `termbay-hook` in Claude Code's
//! `settings.json`.
//!
//! - By default in the user's settings (`$CLAUDE_CONFIG_DIR/settings.json`,
//!   or `~/.claude/settings.json`), so it works in every project. The hook
//!   does nothing outside termbay (decision E1), so this is harmless.
//! - `--project` writes `.claude/settings.json` in the current folder.
//! - Shows what will change and asks before writing (`--yes` skips it).
//! - Merges: hooks of the user and of other tools are kept as they are,
//!   key order included. Our own entries are recognized by the program name
//!   (`termbay-hook`), removed and written again, so installing twice does
//!   not duplicate anything and an old path is replaced.
//! - Keeps a copy of the previous file (`settings.json.bak`) and writes the
//!   new one through a temporary file and a rename.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::events::KEPT_EVENTS;

/// Name of the hook binary, next to `termbay` in every package.
pub const HOOK_FILE_NAME: &str = if cfg!(windows) {
    "termbay-hook.exe"
} else {
    "termbay-hook"
};
/// Seconds Claude Code waits for the hook before going on. The hook gives up
/// after 200 ms by itself; this only guards against something unexpected.
pub const HOOK_TIMEOUT_SECS: u64 = 5;

/// Which settings file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The user's settings: every project.
    User,
    /// `.claude/settings.json` in the current folder.
    Project,
}

/// What to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Install,
    Uninstall,
    Status,
}

/// A settings file that cannot be changed safely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsError {
    /// Not valid JSON. Nothing is written.
    InvalidJson(String),
    /// Valid JSON, but `what` is not an object.
    NotAnObject(&'static str),
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson(err) => write!(f, "it is not valid JSON ({err})"),
            Self::NotAnObject(what) => write!(f, "{what} is not a JSON object"),
        }
    }
}

impl std::error::Error for SettingsError {}

/// Path of the settings file for `scope`. `None` when the home folder is
/// unknown (and no `CLAUDE_CONFIG_DIR` is set).
pub fn settings_path(
    scope: Scope,
    cwd: &Path,
    env: impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    match scope {
        Scope::Project => Some(cwd.join(".claude").join("settings.json")),
        Scope::User => {
            if let Some(dir) = env("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
                return Some(PathBuf::from(dir).join("settings.json"));
            }
            let home = env(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .filter(|h| !h.is_empty())?;
            Some(PathBuf::from(home).join(".claude").join("settings.json"))
        }
    }
}

/// The command written for `event`: the quoted absolute path and the event
/// name. Forward slashes on Windows too, which both `cmd` and the POSIX
/// shells Claude Code may use there read the same way.
pub fn hook_command(hook: &Path, event: &str) -> String {
    let path = hook.to_string_lossy();
    let path = if cfg!(windows) {
        path.replace('\\', "/")
    } else {
        path.into_owned()
    };
    format!("\"{path}\" {event}")
}

/// The program a hook command runs: the quoted part, or the first word.
fn command_program(command: &str) -> &str {
    let command = command.trim_start();
    match command.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or(rest),
        None => command.split_whitespace().next().unwrap_or(""),
    }
}

/// Whether a hook command runs `termbay-hook` (any path, with or without
/// `.exe`): that is how our own entries are told apart from everyone else's.
pub fn is_termbay_command(command: &str) -> bool {
    let program = command_program(command);
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    name == "termbay-hook" || name.eq_ignore_ascii_case("termbay-hook.exe")
}

/// The result of planning a change to a settings file.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    /// The settings after the change.
    pub settings: Value,
    /// Our hook commands removed (old installs, or all of them on uninstall).
    pub removed: usize,
    /// Events that got our hook.
    pub added: usize,
    /// Whether anything differs from the current file.
    pub changed: bool,
}

/// Parses the current file (`None` when it does not exist).
fn parse(current: Option<&str>) -> Result<Value, SettingsError> {
    let Some(text) = current.filter(|t| !t.trim().is_empty()) else {
        return Ok(Value::Object(Map::new()));
    };
    let value: Value =
        serde_json::from_str(text).map_err(|e| SettingsError::InvalidJson(e.to_string()))?;
    if !value.is_object() {
        return Err(SettingsError::NotAnObject("the file"));
    }
    if value.get("hooks").is_some_and(|h| !h.is_object()) {
        return Err(SettingsError::NotAnObject("`hooks`"));
    }
    Ok(value)
}

/// Removes every command of ours from `settings`, and the groups, events and
/// `hooks` object that become empty because of it. Returns how many.
fn remove_ours(settings: &mut Value) -> usize {
    let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) else {
        return 0;
    };
    let mut removed = 0;
    let mut emptied_events = Vec::new();
    for (event, groups) in hooks.iter_mut() {
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };
        let before = removed;
        groups.retain_mut(|group| {
            let Some(commands) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            let count = commands.len();
            commands.retain(|hook| {
                !hook
                    .get("command")
                    .and_then(Value::as_str)
                    .is_some_and(is_termbay_command)
            });
            let gone = count - commands.len();
            removed += gone;
            // Drop a group only if it became empty because of us.
            !(gone > 0 && commands.is_empty())
        });
        if removed > before && groups.is_empty() {
            emptied_events.push(event.clone());
        }
    }
    for event in emptied_events {
        hooks.shift_remove(&event);
    }
    if removed > 0 && hooks.is_empty() {
        settings.as_object_mut().map(|s| s.shift_remove("hooks"));
    }
    removed
}

/// Plans `install`: our entries are replaced by fresh ones pointing at `hook`.
pub fn plan_install(current: Option<&str>, hook: &Path) -> Result<Change, SettingsError> {
    let original = parse(current)?;
    let mut settings = original.clone();
    let removed = remove_ours(&mut settings);
    let root = settings
        .as_object_mut()
        .ok_or(SettingsError::NotAnObject("the file"))?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let hooks = hooks
        .as_object_mut()
        .ok_or(SettingsError::NotAnObject("`hooks`"))?;
    for event in KEPT_EVENTS {
        let groups = hooks
            .entry(*event)
            .or_insert_with(|| Value::Array(Vec::new()));
        let Some(groups) = groups.as_array_mut() else {
            return Err(SettingsError::NotAnObject("an event under `hooks`"));
        };
        groups.push(json!({
            "matcher": "",
            "hooks": [{ "type": "command", "command": hook_command(hook, event), "timeout": HOOK_TIMEOUT_SECS }]
        }));
    }
    let changed = settings != original;
    Ok(Change {
        settings,
        removed,
        added: KEPT_EVENTS.len(),
        changed,
    })
}

/// Plans `uninstall`: only our entries go.
pub fn plan_uninstall(current: Option<&str>) -> Result<Change, SettingsError> {
    let original = parse(current)?;
    let mut settings = original.clone();
    let removed = remove_ours(&mut settings);
    Ok(Change {
        changed: settings != original,
        settings,
        removed,
        added: 0,
    })
}

/// What `status` reports for one settings file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Status {
    /// Events with a command of ours.
    pub events: Vec<String>,
    /// The programs our commands run, without repeats.
    pub programs: Vec<PathBuf>,
}

/// Reads which events run `termbay-hook` and from where.
pub fn status(current: Option<&str>) -> Result<Status, SettingsError> {
    let settings = parse(current)?;
    let mut status = Status::default();
    let Some(hooks) = settings.get("hooks").and_then(Value::as_object) else {
        return Ok(status);
    };
    for (event, groups) in hooks {
        let commands = groups
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|g| g.get("hooks").and_then(Value::as_array))
            .flatten()
            .filter_map(|h| h.get("command").and_then(Value::as_str))
            .filter(|c| is_termbay_command(c));
        let mut found = false;
        for command in commands {
            found = true;
            let program = PathBuf::from(command_program(command));
            if !status.programs.contains(&program) {
                status.programs.push(program);
            }
        }
        if found {
            status.events.push(event.clone());
        }
    }
    Ok(status)
}

/// JSON as written to disk: two-space indentation and a final newline.
pub fn to_text(settings: &Value) -> String {
    let mut text = serde_json::to_string_pretty(settings).unwrap_or_else(|_| "{}".into());
    text.push('\n');
    text
}

/// Writes `text` to `path`: copies the current file to `<path>.bak` first
/// (when there is one), creates the folder if needed, and replaces the file
/// through a temporary file in the same folder. Returns the backup path.
pub fn write_settings(path: &Path, text: &str) -> io::Result<Option<PathBuf>> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let backup = if path.exists() {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(".bak");
        let backup = path.with_file_name(name);
        fs::copy(path, &backup)?;
        Some(backup)
    } else {
        None
    };
    let mut temp_name = path.file_name().unwrap_or_default().to_os_string();
    temp_name.push(format!(".termbay-{}.tmp", std::process::id()));
    let temp = path.with_file_name(temp_name);
    let written = fs::write(&temp, text).and_then(|()| fs::rename(&temp, path));
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written.map(|()| backup)
}

/// Everything `run` needs from the outside, so it can be tested.
pub struct Context<'a> {
    pub cwd: PathBuf,
    pub env: &'a dyn Fn(&str) -> Option<OsString>,
    /// Where `termbay-hook` is: next to the running `termbay`.
    pub hook: PathBuf,
    pub input: &'a mut dyn BufRead,
    pub output: &'a mut dyn Write,
    /// Whether a person can answer the question (stdin is a terminal).
    pub interactive: bool,
}

/// Runs `termbay hooks <action>` and returns the process exit code.
pub fn run(action: Action, scope: Scope, yes: bool, ctx: &mut Context<'_>) -> i32 {
    match run_inner(action, scope, yes, ctx) {
        Ok(code) => code,
        Err(message) => {
            let _ = writeln!(ctx.output, "termbay: {message}");
            1
        }
    }
}

fn run_inner(
    action: Action,
    scope: Scope,
    yes: bool,
    ctx: &mut Context<'_>,
) -> Result<i32, String> {
    let path = settings_path(scope, &ctx.cwd, ctx.env)
        .ok_or("cannot find the home folder; set CLAUDE_CONFIG_DIR or use --project")?;
    let current = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(err) => return Err(format!("cannot read {}: {err}", path.display())),
    };
    let unsafe_file = |err: SettingsError| format!("{} was not changed: {err}", path.display());
    let out = |ctx: &mut Context<'_>, text: String| {
        let _ = writeln!(ctx.output, "{text}");
    };

    let change = match action {
        Action::Status => {
            let status = status(current.as_deref()).map_err(unsafe_file)?;
            report_status(ctx, &path, &status);
            return Ok(0);
        }
        Action::Install => {
            if !ctx.hook.is_file() {
                return Err(format!(
                    "{HOOK_FILE_NAME} was not found next to termbay ({}); reinstall termbay",
                    ctx.hook.display()
                ));
            }
            plan_install(current.as_deref(), &ctx.hook).map_err(unsafe_file)?
        }
        Action::Uninstall => plan_uninstall(current.as_deref()).map_err(unsafe_file)?,
    };

    if !change.changed {
        let what = if action == Action::Install {
            "already installed"
        } else {
            "not installed"
        };
        out(
            ctx,
            format!(
                "termbay-hook is {what} in {}. Nothing to do.",
                path.display()
            ),
        );
        return Ok(0);
    }

    out(ctx, format!("File: {}", path.display()));
    if action == Action::Install {
        out(
            ctx,
            format!(
                "Adds {} for {} events: {}",
                ctx.hook.display(),
                change.added,
                KEPT_EVENTS.join(", ")
            ),
        );
        out(
            ctx,
            "The hook does nothing when claude runs outside termbay.".into(),
        );
    }
    if change.removed > 0 {
        out(
            ctx,
            format!(
                "Removes {} termbay-hook entr{}.",
                change.removed,
                if change.removed == 1 { "y" } else { "ies" }
            ),
        );
    }
    out(ctx, "Other hooks and settings stay as they are.".into());
    if current.is_some() {
        out(
            ctx,
            format!(
                "A copy of the current file is kept as {}.bak.",
                path.display()
            ),
        );
    }

    if !yes {
        if !ctx.interactive {
            return Err("not changed: run again with --yes to confirm without a question".into());
        }
        let _ = write!(ctx.output, "Continue? [y/N] ");
        let _ = ctx.output.flush();
        let mut answer = String::new();
        let _ = ctx.input.read_line(&mut answer);
        if !matches!(
            answer.trim().to_ascii_lowercase().as_str(),
            "y" | "yes" | "s" | "sim"
        ) {
            out(ctx, "Nothing was changed.".into());
            return Ok(1);
        }
    }

    write_settings(&path, &to_text(&change.settings))
        .map_err(|err| format!("could not write {}: {err}", path.display()))?;
    let done = if action == Action::Install {
        "Installed"
    } else {
        "Uninstalled"
    };
    out(
        ctx,
        format!("{done}. Restart running claude sessions to pick it up."),
    );
    Ok(0)
}

fn report_status(ctx: &mut Context<'_>, path: &Path, status: &Status) {
    let mut line = |text: String| {
        let _ = writeln!(ctx.output, "{text}");
    };
    line(format!("File: {}", path.display()));
    if status.events.is_empty() {
        line("termbay-hook is not installed here. Run `termbay hooks install`.".into());
        return;
    }
    line(format!(
        "termbay-hook runs on {} of {} events.",
        status.events.len(),
        KEPT_EVENTS.len()
    ));
    for program in &status.programs {
        let state = if !program.is_file() {
            "missing: run `termbay hooks install` again"
        } else if *program != ctx.hook {
            "a different termbay: run `termbay hooks install` to point at this one"
        } else {
            "ok"
        };
        line(format!("  {} ({state})", program.display()));
    }
    if status.events.len() < KEPT_EVENTS.len() {
        line("Some events are missing: run `termbay hooks install` again.".into());
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn hook() -> PathBuf {
        PathBuf::from(if cfg!(windows) {
            r"C:\Program Files\termbay\termbay-hook.exe"
        } else {
            "/opt/termbay/termbay-hook"
        })
    }

    fn ours(text: &Value) -> usize {
        status(Some(&text.to_string())).unwrap().events.len()
    }

    #[test]
    fn settings_paths() {
        let cwd = Path::new("/work/projeto");
        let env = |vars: &'static [(&'static str, &'static str)]| {
            move |key: &str| {
                vars.iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| OsString::from(v))
            }
        };
        assert_eq!(
            settings_path(Scope::Project, cwd, env(&[])),
            Some(cwd.join(".claude").join("settings.json"))
        );
        assert_eq!(
            settings_path(
                Scope::User,
                cwd,
                env(&[
                    ("CLAUDE_CONFIG_DIR", "/cfg"),
                    ("HOME", "/h"),
                    ("USERPROFILE", "/h")
                ])
            ),
            Some(PathBuf::from("/cfg").join("settings.json"))
        );
        assert_eq!(
            settings_path(
                Scope::User,
                cwd,
                env(&[("HOME", "/h"), ("USERPROFILE", "/h")])
            ),
            Some(PathBuf::from("/h").join(".claude").join("settings.json"))
        );
        assert_eq!(settings_path(Scope::User, cwd, env(&[])), None);
    }

    #[test]
    fn commands_are_recognized_by_program_name() {
        assert!(is_termbay_command(&hook_command(&hook(), "PreToolUse")));
        assert!(is_termbay_command("\"/old/place/termbay-hook\" Stop"));
        assert!(is_termbay_command("termbay-hook PreToolUse"));
        assert!(is_termbay_command(
            r#""C:/Program Files/termbay/termbay-hook.exe" Stop"#
        ));
        assert!(!is_termbay_command("/usr/local/bin/my-hook.sh"));
        assert!(!is_termbay_command("echo termbay-hook"));
        assert!(!is_termbay_command("termbay-hooks-backup PreToolUse"));
        assert!(!is_termbay_command(""));
    }

    #[test]
    fn install_into_a_missing_or_empty_file() {
        for current in [None, Some(""), Some("{}"), Some("  \n")] {
            let change = plan_install(current, &hook()).unwrap();
            assert!(change.changed);
            assert_eq!(change.removed, 0);
            assert_eq!(ours(&change.settings), KEPT_EVENTS.len());
            let group = &change.settings["hooks"]["PreToolUse"][0];
            assert_eq!(group["matcher"], "");
            assert_eq!(group["hooks"][0]["type"], "command");
            assert_eq!(group["hooks"][0]["timeout"], HOOK_TIMEOUT_SECS);
            assert!(
                group["hooks"][0]["command"]
                    .as_str()
                    .unwrap()
                    .ends_with("\" PreToolUse")
            );
        }
        assert!(
            plan_install(None, &hook())
                .unwrap()
                .settings
                .get("MessageDisplay")
                .is_none()
        );
    }

    #[test]
    fn install_keeps_everything_else_in_order() {
        let current = r#"{
  "model": "opus",
  "permissions": {"allow": ["Bash(npm test)"]},
  "hooks": {
    "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "/usr/local/bin/guard.sh"}]}],
    "FileChanged": [{"matcher": "notes.txt", "hooks": [{"type": "command", "command": "echo changed"}]}]
  },
  "zeta": 1
}"#;
        let change = plan_install(Some(current), &hook()).unwrap();
        let s = &change.settings;
        let keys: Vec<&String> = s.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["model", "permissions", "hooks", "zeta"]);
        assert_eq!(s["permissions"]["allow"][0], "Bash(npm test)");
        assert_eq!(
            s["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "/usr/local/bin/guard.sh"
        );
        assert!(is_termbay_command(
            s["hooks"]["PreToolUse"][1]["hooks"][0]["command"]
                .as_str()
                .unwrap()
        ));
        assert_eq!(
            s["hooks"]["FileChanged"][0]["hooks"][0]["command"],
            "echo changed"
        );
    }

    #[test]
    fn installing_twice_changes_nothing_and_an_old_path_is_replaced() {
        let once = to_text(&plan_install(None, &hook()).unwrap().settings);
        let twice = plan_install(Some(&once), &hook()).unwrap();
        assert!(!twice.changed);
        assert_eq!(twice.removed, KEPT_EVENTS.len());
        assert_eq!(ours(&twice.settings), KEPT_EVENTS.len());

        let moved = plan_install(Some(&once), Path::new("/new/place/termbay-hook")).unwrap();
        assert!(moved.changed);
        let status = status(Some(&moved.settings.to_string())).unwrap();
        assert_eq!(status.programs, [PathBuf::from("/new/place/termbay-hook")]);
    }

    #[test]
    fn a_group_shared_with_another_hook_keeps_the_other_hook() {
        let current = r#"{"hooks": {"Stop": [{"matcher": "", "hooks": [
            {"type": "command", "command": "\"/old/termbay-hook\" Stop"},
            {"type": "command", "command": "notify-send done"}
        ]}]}}"#;
        let change = plan_uninstall(Some(current)).unwrap();
        assert_eq!(change.removed, 1);
        assert_eq!(
            change.settings["hooks"]["Stop"][0]["hooks"][0]["command"],
            "notify-send done"
        );
        assert_eq!(
            change.settings["hooks"]["Stop"][0]["hooks"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn uninstall_removes_only_ours_and_what_it_emptied() {
        let installed =
            plan_install(Some(r#"{"model": "opus", "hooks": {"Stop": []}}"#), &hook()).unwrap();
        let change = plan_uninstall(Some(&installed.settings.to_string())).unwrap();
        assert_eq!(change.removed, KEPT_EVENTS.len());
        // An event list emptied by removing our entry goes away, and so does
        // `hooks` once nothing is left in it. Everything else stays.
        assert_eq!(change.settings, json!({"model": "opus"}));

        let untouched = plan_uninstall(Some(r#"{"hooks": {"Stop": []}}"#)).unwrap();
        assert!(!untouched.changed);
        assert_eq!(untouched.settings, json!({"hooks": {"Stop": []}}));
        assert!(!plan_uninstall(None).unwrap().changed);
    }

    #[test]
    fn unsafe_files_are_refused() {
        assert!(matches!(
            plan_install(Some("{ not json"), &hook()),
            Err(SettingsError::InvalidJson(_))
        ));
        assert_eq!(
            plan_install(Some("[1, 2]"), &hook()),
            Err(SettingsError::NotAnObject("the file"))
        );
        assert_eq!(
            plan_install(Some(r#"{"hooks": []}"#), &hook()),
            Err(SettingsError::NotAnObject("`hooks`"))
        );
        assert_eq!(
            plan_install(Some(r#"{"hooks": {"Stop": "x"}}"#), &hook()),
            Err(SettingsError::NotAnObject("an event under `hooks`"))
        );
        assert!(matches!(
            plan_uninstall(Some("nope")),
            Err(SettingsError::InvalidJson(_))
        ));
        assert!(matches!(
            status(Some("nope")),
            Err(SettingsError::InvalidJson(_))
        ));
    }

    /// A temporary folder with a fake `termbay-hook`, removed at the end.
    struct Sandbox {
        dir: PathBuf,
    }

    impl Sandbox {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("termbay-hooks-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("bin")).unwrap();
            fs::write(dir.join("bin").join(HOOK_FILE_NAME), b"").unwrap();
            Self { dir }
        }

        fn settings(&self) -> PathBuf {
            self.dir.join("config").join("settings.json")
        }

        fn run(&self, action: Action, yes: bool, interactive: bool, answer: &str) -> (i32, String) {
            let config = self.dir.join("config").into_os_string();
            let env = move |key: &str| (key == "CLAUDE_CONFIG_DIR").then(|| config.clone());
            let mut input = Cursor::new(answer.as_bytes().to_vec());
            let mut output = Vec::new();
            let mut ctx = Context {
                cwd: self.dir.clone(),
                env: &env,
                hook: self.dir.join("bin").join(HOOK_FILE_NAME),
                input: &mut input,
                output: &mut output,
                interactive,
            };
            let code = run(action, Scope::User, yes, &mut ctx);
            (code, String::from_utf8(output).unwrap())
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn run_install_status_uninstall_on_disk() {
        let sandbox = Sandbox::new("cycle");
        let (code, text) = sandbox.run(Action::Status, false, false, "");
        assert_eq!(code, 0);
        assert!(text.contains("not installed"), "{text}");

        // Without a terminal and without --yes nothing is written.
        let (code, text) = sandbox.run(Action::Install, false, false, "");
        assert_eq!(code, 1, "{text}");
        assert!(text.contains("--yes"), "{text}");
        assert!(!sandbox.settings().exists());

        // Answering no writes nothing either.
        let (code, text) = sandbox.run(Action::Install, false, true, "n\n");
        assert_eq!(code, 1);
        assert!(text.contains("Nothing was changed"), "{text}");
        assert!(!sandbox.settings().exists());

        let (code, text) = sandbox.run(Action::Install, false, true, "y\n");
        assert_eq!(code, 0, "{text}");
        assert!(text.contains("Installed"), "{text}");
        let written = fs::read_to_string(sandbox.settings()).unwrap();
        assert!(written.ends_with('\n'));
        assert_eq!(
            status(Some(&written)).unwrap().events.len(),
            KEPT_EVENTS.len()
        );

        let (code, text) = sandbox.run(Action::Status, false, false, "");
        assert_eq!(code, 0);
        assert!(text.contains("(ok)"), "{text}");

        let (code, text) = sandbox.run(Action::Install, true, false, "");
        assert_eq!(code, 0);
        assert!(text.contains("already installed"), "{text}");

        let (code, _) = sandbox.run(Action::Uninstall, true, false, "");
        assert_eq!(code, 0);
        assert_eq!(fs::read_to_string(sandbox.settings()).unwrap(), "{}\n");
        let backup =
            fs::read_to_string(sandbox.dir.join("config").join("settings.json.bak")).unwrap();
        assert_eq!(backup, written, "the previous file is kept");
    }

    #[test]
    fn run_refuses_a_broken_file_and_a_missing_hook() {
        let sandbox = Sandbox::new("broken");
        fs::create_dir_all(sandbox.settings().parent().unwrap()).unwrap();
        fs::write(sandbox.settings(), "{ broken").unwrap();
        let (code, text) = sandbox.run(Action::Install, true, false, "");
        assert_eq!(code, 1);
        assert!(text.contains("was not changed"), "{text}");
        assert_eq!(fs::read_to_string(sandbox.settings()).unwrap(), "{ broken");

        fs::remove_file(sandbox.dir.join("bin").join(HOOK_FILE_NAME)).unwrap();
        fs::write(sandbox.settings(), "{}").unwrap();
        let (code, text) = sandbox.run(Action::Install, true, false, "");
        assert_eq!(code, 1);
        assert!(text.contains("not found next to termbay"), "{text}");
    }
}
