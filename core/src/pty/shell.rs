//! Discovery of the user's default shell.

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// A program and its arguments, used to start the shell of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellSpec {
    /// Path (or name to look up in `PATH`) of the shell executable.
    pub program: PathBuf,
    /// Arguments passed to the shell.
    pub args: Vec<OsString>,
}

impl ShellSpec {
    /// A shell with no arguments.
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    /// Adds one argument (builder style).
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }
}

/// Finds the user's default shell.
///
/// - **macOS and Linux:** `$SHELL` when it is an absolute path to an
///   existing file; otherwise the first existing of `/bin/zsh` (macOS
///   only), `/bin/bash` and `/bin/sh`. On macOS the shell gets `-l` so it
///   runs as a login shell and loads the user's `PATH` even when the app
///   was opened from Finder. Linux terminals traditionally start
///   non-login shells, so no flag is added there.
/// - **Windows:** `pwsh.exe` (PowerShell 7+), then `powershell.exe`
///   (Windows PowerShell), both with `-NoLogo`, looked up in `PATH`;
///   otherwise `%COMSPEC%`, falling back to `cmd.exe`.
pub fn default_shell() -> ShellSpec {
    #[cfg(windows)]
    {
        pick_windows_shell(
            env::var_os("PATH").as_deref(),
            env::var_os("COMSPEC"),
            |path| path.is_file(),
        )
    }
    #[cfg(not(windows))]
    {
        pick_unix_shell(env::var_os("SHELL"), cfg!(target_os = "macos"), |path| {
            path.is_file()
        })
    }
}

#[cfg_attr(windows, allow(dead_code))]
fn pick_unix_shell(
    env_shell: Option<OsString>,
    macos: bool,
    exists: impl Fn(&Path) -> bool,
) -> ShellSpec {
    let from_env = env_shell
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && exists(path));

    let program = from_env.unwrap_or_else(|| {
        let candidates: &[&str] = if macos {
            &["/bin/zsh", "/bin/bash", "/bin/sh"]
        } else {
            &["/bin/bash", "/bin/sh"]
        };
        candidates
            .iter()
            .map(PathBuf::from)
            .find(|path| exists(path))
            .unwrap_or_else(|| PathBuf::from("/bin/sh"))
    });

    let shell = ShellSpec::new(program);
    if macos { shell.arg("-l") } else { shell }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn pick_windows_shell(
    path_var: Option<&std::ffi::OsStr>,
    comspec: Option<OsString>,
    exists: impl Fn(&Path) -> bool,
) -> ShellSpec {
    let dirs: Vec<PathBuf> = path_var
        .map(|p| env::split_paths(p).collect())
        .unwrap_or_default();

    for name in ["pwsh.exe", "powershell.exe"] {
        if let Some(found) = dirs.iter().map(|dir| dir.join(name)).find(|p| exists(p)) {
            return ShellSpec::new(found).arg("-NoLogo");
        }
    }

    let cmd = comspec
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cmd.exe"));
    ShellSpec::new(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn existing(paths: &[&str]) -> impl Fn(&Path) -> bool {
        let set: HashSet<PathBuf> = paths.iter().map(PathBuf::from).collect();
        move |p: &Path| set.contains(p)
    }

    #[test]
    fn unix_uses_shell_env_when_it_exists() {
        let shell = pick_unix_shell(
            Some("/usr/local/bin/fish".into()),
            false,
            existing(&["/usr/local/bin/fish", "/bin/bash"]),
        );
        assert_eq!(shell, ShellSpec::new("/usr/local/bin/fish"));
    }

    #[test]
    fn macos_starts_a_login_shell() {
        let shell = pick_unix_shell(Some("/bin/zsh".into()), true, existing(&["/bin/zsh"]));
        assert_eq!(shell, ShellSpec::new("/bin/zsh").arg("-l"));
    }

    #[test]
    fn unix_ignores_missing_or_relative_shell_env() {
        let exists = existing(&["/bin/bash", "/bin/sh"]);
        assert_eq!(
            pick_unix_shell(Some("/nope/zsh".into()), false, &exists).program,
            PathBuf::from("/bin/bash")
        );
        assert_eq!(
            pick_unix_shell(Some("zsh".into()), false, &exists).program,
            PathBuf::from("/bin/bash")
        );
        assert_eq!(
            pick_unix_shell(Some("".into()), false, &exists).program,
            PathBuf::from("/bin/bash")
        );
        assert_eq!(
            pick_unix_shell(None, false, &exists).program,
            PathBuf::from("/bin/bash")
        );
    }

    #[test]
    fn macos_falls_back_to_zsh() {
        let shell = pick_unix_shell(None, true, existing(&["/bin/zsh", "/bin/bash", "/bin/sh"]));
        assert_eq!(shell, ShellSpec::new("/bin/zsh").arg("-l"));
    }

    #[test]
    fn unix_last_resort_is_sh() {
        let shell = pick_unix_shell(None, false, existing(&[]));
        assert_eq!(shell.program, PathBuf::from("/bin/sh"));
    }

    fn windows_path(dirs: &[&str]) -> OsString {
        env::join_paths(dirs.iter().map(PathBuf::from)).unwrap()
    }

    #[test]
    fn windows_prefers_pwsh() {
        let a = PathBuf::from("a");
        let b = PathBuf::from("b");
        let path = windows_path(&["a", "b"]);
        let pwsh = b.join("pwsh.exe");
        let powershell = a.join("powershell.exe");
        let shell = pick_windows_shell(Some(&path), Some("cmd.exe".into()), |p| {
            p == pwsh || p == powershell
        });
        assert_eq!(shell, ShellSpec::new(b.join("pwsh.exe")).arg("-NoLogo"));
    }

    #[test]
    fn windows_then_windows_powershell() {
        let a = PathBuf::from("a");
        let path = windows_path(&["a"]);
        let powershell = a.join("powershell.exe");
        let shell = pick_windows_shell(Some(&path), None, |p| p == powershell);
        assert_eq!(
            shell,
            ShellSpec::new(a.join("powershell.exe")).arg("-NoLogo")
        );
    }

    #[test]
    fn windows_then_comspec_then_cmd() {
        let path = windows_path(&["a"]);
        let shell = pick_windows_shell(
            Some(&path),
            Some("C:\\Windows\\system32\\cmd.exe".into()),
            |_| false,
        );
        assert_eq!(shell, ShellSpec::new("C:\\Windows\\system32\\cmd.exe"));

        let shell = pick_windows_shell(None, None, |_| false);
        assert_eq!(shell, ShellSpec::new("cmd.exe"));
    }

    #[test]
    fn default_shell_is_not_empty() {
        assert!(!default_shell().program.as_os_str().is_empty());
    }
}
