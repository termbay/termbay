//! Shell integration: lets termbay know when the shell is at its prompt and
//! ask the shell itself for completions (`docs/AUTOCOMPLETE.md`, D12/D13).
//!
//! Milestone A supports fish and zsh on macOS and Linux (D17). Everything
//! here is used only when `[autocomplete] enabled = true` (D15); otherwise
//! the shell starts with exactly the same arguments and environment as
//! without this module, and no scanner runs.
//!
//! - [`ScriptDir::install`] writes the scripts embedded in termbay to a
//!   per-user folder (`0700`, checked owner), never to the user's dotfiles.
//! - [`inject`] changes the [`SpawnOptions`] of a recognized shell:
//!   - **fish:** `--init-command 'source <dir>/fish/termbay.fish'`, which
//!     runs after the user's `config.fish` and leaks nothing to children;
//!   - **zsh:** `ZDOTDIR=<dir>/zsh`, whose `.zshenv` restores the original
//!     `ZDOTDIR` at once, loads the user's `.zshenv` and then the
//!     integration (interactive shells only). The user's `.zprofile`,
//!     `.zshrc` and `.zlogin` load from their usual place.
//!
//!   Both get a fresh random nonce in `TERMBAY_NONCE`, which the scripts
//!   move to a shell variable and remove from the environment.
//! - [`OscScanner`] removes the private OSC from the output and decodes the
//!   trusted [`ShellMark`]s; [`ShellLink`] keeps the [`ShellState`] and
//!   runs completion requests.

pub mod link;
pub mod osc;
pub mod state;

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

pub use link::{COMPLETION_TIMEOUT, RequestOutcome, ShellLink, trigger_bytes};
pub use osc::{OscScanner, ShellInfo, ShellMark};
pub use state::{Phase, ShellState};

use crate::complete::history::HistoryFormat;
use crate::pty::{ShellSpec, SpawnOptions, default_shell};

/// Version of the protocol spoken by the embedded scripts.
pub const PROTOCOL_VERSION: u32 = 1;

/// Environment variable that carries the nonce to the script.
pub const NONCE_ENV: &str = "TERMBAY_NONCE";

/// Environment variable with the user's own `ZDOTDIR` (zsh), set only when
/// the user had one.
pub const USER_ZDOTDIR_ENV: &str = "TERMBAY_USER_ZDOTDIR";

const FISH_SCRIPT: &str = include_str!("scripts/termbay.fish");
const ZSH_ENV_SCRIPT: &str = include_str!("scripts/zshenv.zsh");
const ZSH_SCRIPT: &str = include_str!("scripts/termbay.zsh");

/// The files written by [`ScriptDir::install`], relative to its root.
const FILES: &[(&str, &str)] = &[
    ("fish/termbay.fish", FISH_SCRIPT),
    ("zsh/.zshenv", ZSH_ENV_SCRIPT),
    ("zsh/termbay.zsh", ZSH_SCRIPT),
];

/// A shell termbay can integrate with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShellKind {
    /// fish 3.x or 4.x.
    Fish,
    /// zsh 5.x.
    Zsh,
}

impl ShellKind {
    /// Name used in the protocol and in the `pty-shell` event.
    pub fn name(self) -> &'static str {
        match self {
            Self::Fish => "fish",
            Self::Zsh => "zsh",
        }
    }

    /// Format of this shell's history file.
    pub fn history_format(self) -> HistoryFormat {
        match self {
            Self::Fish => HistoryFormat::Fish,
            Self::Zsh => HistoryFormat::Zsh,
        }
    }

    /// Recognizes fish and zsh started the way [`default_shell`] starts
    /// them (no arguments, or only `-l`/`--login`/`-i`). Any other
    /// argument may change how the shell starts, so it is left alone.
    pub fn detect(spec: &ShellSpec) -> Option<Self> {
        let name = spec.program.file_name()?.to_str()?;
        let name = name.strip_suffix(".exe").unwrap_or(name);
        let kind = match name {
            "fish" => Self::Fish,
            "zsh" => Self::Zsh,
            _ => return None,
        };
        let plain = spec.args.iter().all(|arg| {
            matches!(
                arg.to_str(),
                Some("-l" | "--login" | "-i" | "--interactive")
            )
        });
        plain.then_some(kind)
    }
}

/// A random value per pty that marks the trusted sequences.
#[derive(Clone, PartialEq, Eq)]
pub struct Nonce(String);

impl std::fmt::Debug for Nonce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never printed: a log line must not be enough to forge marks.
        f.write_str("Nonce(..)")
    }
}

impl Nonce {
    /// 128 random bits from the operating system, as 32 hex digits.
    pub fn generate() -> io::Result<Self> {
        let mut bytes = [0u8; 16];
        fill_random(&mut bytes)?;
        Ok(Self(bytes.iter().map(|b| format!("{b:02x}")).collect()))
    }

    /// A given nonce (tests and fakes).
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The value.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(unix)]
fn fill_random(bytes: &mut [u8]) -> io::Result<()> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")?.read_exact(bytes)
}

#[cfg(not(unix))]
fn fill_random(_bytes: &mut [u8]) -> io::Result<()> {
    // Milestone B (PowerShell) brings Windows support.
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "shell integration is not available on this system yet",
    ))
}

/// What [`inject`] did to a shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Injection {
    /// The shell that was integrated.
    pub kind: ShellKind,
    /// The nonce given to it.
    pub nonce: Nonce,
}

/// The folder with the installed scripts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptDir {
    root: PathBuf,
}

impl ScriptDir {
    /// Writes the scripts under `base/termbay/shell-integration/<hash>/`.
    ///
    /// Folders are created with mode `0700`. Existing folders must belong
    /// to the current user and not be writable by others; files whose
    /// content differs are written again. Fails on systems without
    /// integration support (Windows, until milestone B).
    pub fn install(base: &Path) -> io::Result<Self> {
        if cfg!(not(unix)) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "shell integration is not available on this system yet",
            ));
        }
        std::fs::create_dir_all(base)?;
        let mut dir = base.to_path_buf();
        for part in [
            "termbay",
            "shell-integration",
            &format!("v{PROTOCOL_VERSION}-{}", scripts_hash()),
        ] {
            dir.push(part);
            private_dir(&dir)?;
        }
        for (name, content) in FILES {
            let path = dir.join(name);
            if let Some(parent) = path.parent() {
                private_dir(parent)?;
            }
            write_if_changed(&path, content.as_bytes())?;
        }
        Ok(Self { root: dir })
    }

    /// [`ScriptDir::install`] in the user's data folder
    /// ([`default_data_dir`]).
    pub fn install_default() -> io::Result<Self> {
        let base = default_data_dir().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "could not find the user's data folder",
            )
        })?;
        Self::install(&base)
    }

    /// The folder with the scripts.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn fish_script(&self) -> PathBuf {
        self.root.join("fish").join("termbay.fish")
    }

    fn zsh_dir(&self) -> PathBuf {
        self.root.join("zsh")
    }
}

/// The user's data folder: `~/Library/Application Support` on macOS,
/// `$XDG_DATA_HOME` or `~/.local/share` elsewhere on Unix. `None` on
/// Windows (no integration yet) or without a home folder.
pub fn default_data_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        return None;
    }
    let home = crate::config::home_dir()?;
    if cfg!(target_os = "macos") {
        return Some(home.join("Library").join("Application Support"));
    }
    let xdg = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    Some(xdg.unwrap_or_else(|| home.join(".local").join("share")))
}

/// Integrates a recognized shell: changes `options` and returns the nonce
/// to give to the [`OscScanner`]. Returns `None` and leaves `options`
/// untouched when the shell is not recognized or no nonce can be made.
pub fn inject(options: &mut SpawnOptions, scripts: &ScriptDir) -> Option<Injection> {
    let spec = options.shell.clone().unwrap_or_else(default_shell);
    let kind = ShellKind::detect(&spec)?;
    let nonce = Nonce::generate().ok()?;
    let user_zdotdir = env_value(&options.env, "ZDOTDIR").or_else(|| std::env::var_os("ZDOTDIR"));
    apply(options, spec, kind, &nonce, scripts, user_zdotdir);
    Some(Injection { kind, nonce })
}

/// The pure part of [`inject`].
fn apply(
    options: &mut SpawnOptions,
    mut spec: ShellSpec,
    kind: ShellKind,
    nonce: &Nonce,
    scripts: &ScriptDir,
    user_zdotdir: Option<OsString>,
) {
    match kind {
        ShellKind::Fish => {
            spec.args.push("--init-command".into());
            spec.args.push(
                format!(
                    "source {}",
                    fish_quote(&scripts.fish_script().to_string_lossy())
                )
                .into(),
            );
        }
        ShellKind::Zsh => {
            options
                .env
                .retain(|(key, _)| key != "ZDOTDIR" && key != USER_ZDOTDIR_ENV);
            if let Some(original) = user_zdotdir {
                options.env.push((USER_ZDOTDIR_ENV.into(), original));
            }
            options
                .env
                .push(("ZDOTDIR".into(), scripts.zsh_dir().into_os_string()));
        }
    }
    options.env.retain(|(key, _)| key != NONCE_ENV);
    options.env.push((NONCE_ENV.into(), nonce.as_str().into()));
    options.shell = Some(spec);
}

/// The last value given to `key` in an environment list.
fn env_value(env: &[(OsString, OsString)], key: &str) -> Option<OsString> {
    env.iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
}

/// Quotes a string for fish: single quotes, with `\` and `'` escaped.
fn fish_quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('\'');
    for c in text.chars() {
        if c == '\\' || c == '\'' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('\'');
    out
}

/// FNV-1a of the embedded scripts: a new termbay version with other
/// scripts gets another folder.
fn scripts_hash() -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (name, content) in FILES {
        for &b in name.as_bytes().iter().chain(content.as_bytes()) {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{hash:016x}")
}

/// Creates `dir` with mode 0700, or checks that the existing one is a real
/// folder owned by the current user and not writable by others.
#[cfg(unix)]
fn private_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};

    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
        Err(err) => return Err(err),
    }
    let metadata = std::fs::symlink_metadata(dir)?;
    // SAFETY: geteuid has no preconditions and cannot fail.
    let uid = unsafe { libc::geteuid() };
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o022 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} must be a folder owned by the current user and not writable by others",
                dir.display()
            ),
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn private_dir(_dir: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "shell integration is not available on this system yet",
    ))
}

#[cfg(unix)]
fn write_if_changed(path: &Path, content: &[u8]) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if metadata.is_file() && std::fs::read(path)? == content {
            return Ok(());
        }
        // Wrong content, a symlink or something else: replace it.
        std::fs::remove_file(path)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(content)?;
    file.sync_all()
}

#[cfg(not(unix))]
fn write_if_changed(_path: &Path, _content: &[u8]) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "shell integration is not available on this system yet",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(program: &str, args: &[&str]) -> ShellSpec {
        let mut spec = ShellSpec::new(program);
        for arg in args {
            spec = spec.arg(*arg);
        }
        spec
    }

    #[test]
    fn detects_plain_fish_and_zsh_only() {
        assert_eq!(
            ShellKind::detect(&spec("/usr/bin/fish", &[])),
            Some(ShellKind::Fish)
        );
        assert_eq!(
            ShellKind::detect(&spec("/bin/zsh", &["-l"])),
            Some(ShellKind::Zsh)
        );
        assert_eq!(
            ShellKind::detect(&spec("zsh", &["--login", "-i"])),
            Some(ShellKind::Zsh)
        );
        assert_eq!(ShellKind::detect(&spec("/bin/zsh", &["-f"])), None);
        assert_eq!(ShellKind::detect(&spec("/bin/zsh", &["-c", "ls"])), None);
        assert_eq!(ShellKind::detect(&spec("fish", &["--no-config"])), None);
        assert_eq!(ShellKind::detect(&spec("/bin/bash", &[])), None);
        assert_eq!(ShellKind::detect(&spec("/bin/zsh5", &[])), None);
        assert_eq!(ShellKind::detect(&spec("cmd.exe", &[])), None);
    }

    #[test]
    fn nonce_is_never_printed() {
        assert_eq!(format!("{:?}", Nonce::new("secret")), "Nonce(..)");
    }

    #[cfg(unix)]
    #[test]
    fn nonces_are_random_hex() {
        let a = Nonce::generate().unwrap();
        let b = Nonce::generate().unwrap();
        assert_eq!(a.as_str().len(), 32);
        assert!(a.as_str().bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn fish_quoting() {
        assert_eq!(fish_quote("/a b/c"), "'/a b/c'");
        assert_eq!(fish_quote("it's\\x"), "'it\\'s\\\\x'");
    }

    fn scripts() -> ScriptDir {
        ScriptDir {
            root: PathBuf::from("/data/termbay/si"),
        }
    }

    #[test]
    fn fish_gets_an_init_command_and_the_nonce() {
        let mut options = SpawnOptions::default();
        let nonce = Nonce::new("n1");
        apply(
            &mut options,
            spec("/usr/bin/fish", &["-l"]),
            ShellKind::Fish,
            &nonce,
            &scripts(),
            None,
        );
        let shell = options.shell.unwrap();
        assert_eq!(shell.program, PathBuf::from("/usr/bin/fish"));
        let expected = format!("source '{}'", scripts().fish_script().display());
        assert_eq!(
            shell.args,
            [
                OsString::from("-l"),
                "--init-command".into(),
                expected.into()
            ]
        );
        assert_eq!(
            options.env,
            [(OsString::from(NONCE_ENV), OsString::from("n1"))]
        );
    }

    #[test]
    fn zsh_gets_zdotdir_and_keeps_the_users_one() {
        let mut options = SpawnOptions {
            env: vec![
                ("ZDOTDIR".into(), "/old".into()),
                (NONCE_ENV.into(), "stale".into()),
                ("OTHER".into(), "1".into()),
            ],
            ..SpawnOptions::default()
        };
        let nonce = Nonce::new("n2");
        apply(
            &mut options,
            spec("/bin/zsh", &[]),
            ShellKind::Zsh,
            &nonce,
            &scripts(),
            Some("/home/u/.config/zsh".into()),
        );
        assert_eq!(options.shell.unwrap().args, Vec::<OsString>::new());
        assert_eq!(
            options.env,
            [
                (OsString::from("OTHER"), OsString::from("1")),
                (USER_ZDOTDIR_ENV.into(), "/home/u/.config/zsh".into()),
                ("ZDOTDIR".into(), scripts().zsh_dir().into_os_string()),
                (NONCE_ENV.into(), "n2".into()),
            ]
        );

        // Without a ZDOTDIR of their own, none is passed on.
        let mut options = SpawnOptions::default();
        apply(
            &mut options,
            spec("/bin/zsh", &[]),
            ShellKind::Zsh,
            &nonce,
            &scripts(),
            None,
        );
        assert!(options.env.iter().all(|(k, _)| k != USER_ZDOTDIR_ENV));
    }

    #[test]
    fn unrecognized_shells_are_left_untouched() {
        let mut options = SpawnOptions {
            shell: Some(spec("/bin/bash", &["-l"])),
            env: vec![("A".into(), "1".into())],
            ..SpawnOptions::default()
        };
        let before = format!("{options:?}");
        assert!(inject(&mut options, &scripts()).is_none());
        assert_eq!(format!("{options:?}"), before);
    }

    #[test]
    fn the_hash_depends_on_the_scripts() {
        assert_eq!(scripts_hash().len(), 16);
        assert_eq!(scripts_hash(), scripts_hash());
    }

    #[cfg(unix)]
    #[test]
    fn install_writes_private_files_and_repairs_them() {
        use std::os::unix::fs::PermissionsExt;

        let base = std::env::temp_dir().join(format!("termbay-si-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = ScriptDir::install(&base).unwrap();
        assert!(dir.root().starts_with(&base));
        for (name, content) in FILES {
            let path = dir.root().join(name);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), *content);
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "{name} must be private");
        }
        let mode = std::fs::metadata(dir.root()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);

        // A changed file is written again.
        let zshenv = dir.root().join("zsh/.zshenv");
        std::fs::write(&zshenv, "echo planted").unwrap();
        assert_eq!(ScriptDir::install(&base).unwrap(), dir);
        assert_eq!(std::fs::read_to_string(&zshenv).unwrap(), ZSH_ENV_SCRIPT);

        // A folder others can write to is refused.
        std::fs::set_permissions(dir.root(), std::fs::Permissions::from_mode(0o777)).unwrap();
        let err = ScriptDir::install(&base).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        std::fs::set_permissions(dir.root(), std::fs::Permissions::from_mode(0o700)).unwrap();

        std::fs::remove_dir_all(&base).unwrap();
    }
}
