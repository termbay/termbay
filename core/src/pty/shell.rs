//! Discovery of the user's default shell.

use std::env;
#[cfg(unix)]
use std::ffi::OsStr;
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
/// - **macOS and Linux:** the login shell of the account (`pw_shell` from
///   the user database, as the macOS Terminal and iTerm2 do), then
///   `$SHELL`, each only when it is an absolute path to an existing file;
///   otherwise the first existing of `/bin/zsh` (macOS only), `/bin/bash`
///   and `/bin/sh`. The account comes first because an app opened from
///   Finder or with `open` inherits a `$SHELL` that may not be the user's
///   choice (for example `/bin/zsh` when the user changed to fish). On macOS the shell gets `-l` so it
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
        pick_unix_shell(
            login_shell(),
            env::var_os("SHELL"),
            cfg!(target_os = "macos"),
            |path| path.is_file(),
        )
    }
}

/// The account's login shell (`pw_shell`), read with `getpwuid_r`.
#[cfg(unix)]
fn login_shell() -> Option<OsString> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStrExt;

    let mut buffer = vec![0 as libc::c_char; 4096];
    loop {
        // SAFETY: `passwd` is plain data; zeroed is a valid initial value.
        let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: every pointer is valid for the call and `buffer.len()` is
        // the real size of `buffer`; getuid cannot fail.
        let code = unsafe {
            libc::getpwuid_r(
                libc::getuid(),
                &mut entry,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut result,
            )
        };
        if code == libc::ERANGE && buffer.len() < 1024 * 1024 {
            buffer.resize(buffer.len() * 2, 0);
            continue;
        }
        if code != 0 || result.is_null() || entry.pw_shell.is_null() {
            return None;
        }
        // SAFETY: on success `pw_shell` points to a NUL-terminated string
        // inside `buffer`, which is still alive.
        let shell = unsafe { CStr::from_ptr(entry.pw_shell) };
        return Some(OsStr::from_bytes(shell.to_bytes()).to_os_string());
    }
}

#[cfg_attr(windows, allow(dead_code))]
fn pick_unix_shell(
    login_shell: Option<OsString>,
    env_shell: Option<OsString>,
    macos: bool,
    exists: impl Fn(&Path) -> bool,
) -> ShellSpec {
    let usable = |value: Option<OsString>| {
        value
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute() && exists(path))
    };
    let chosen = usable(login_shell).or_else(|| usable(env_shell));

    let program = chosen.unwrap_or_else(|| {
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
    #[cfg(unix)]
    use std::collections::HashSet;

    #[cfg(unix)]
    fn existing(paths: &[&str]) -> impl Fn(&Path) -> bool {
        let set: HashSet<PathBuf> = paths.iter().map(PathBuf::from).collect();
        move |p: &Path| set.contains(p)
    }

    // Path::is_absolute is platform specific: "/bin/zsh" is not absolute on Windows.
    #[cfg(unix)]
    #[test]
    fn unix_uses_shell_env_when_it_exists() {
        let shell = pick_unix_shell(
            None,
            Some("/usr/local/bin/fish".into()),
            false,
            existing(&["/usr/local/bin/fish", "/bin/bash"]),
        );
        assert_eq!(shell, ShellSpec::new("/usr/local/bin/fish"));
    }

    #[cfg(unix)]
    #[test]
    fn the_accounts_login_shell_comes_before_shell_env() {
        // The app was opened from Finder with SHELL=/bin/zsh, but the
        // account's shell is fish.
        let shell = pick_unix_shell(
            Some("/opt/homebrew/bin/fish".into()),
            Some("/bin/zsh".into()),
            true,
            existing(&["/opt/homebrew/bin/fish", "/bin/zsh"]),
        );
        assert_eq!(shell, ShellSpec::new("/opt/homebrew/bin/fish").arg("-l"));
    }

    #[cfg(unix)]
    #[test]
    fn an_unusable_login_shell_falls_back_to_shell_env() {
        let exists = existing(&["/bin/zsh", "/bin/bash"]);
        for login in ["/removed/fish", "fish", ""] {
            let shell =
                pick_unix_shell(Some(login.into()), Some("/bin/zsh".into()), false, &exists);
            assert_eq!(shell.program, PathBuf::from("/bin/zsh"), "{login:?}");
        }
        let shell = pick_unix_shell(Some("/removed/fish".into()), None, false, &exists);
        assert_eq!(shell.program, PathBuf::from("/bin/bash"));
    }

    #[cfg(unix)]
    #[test]
    fn the_login_shell_of_this_account_is_readable() {
        // Every account has an entry; its shell may be empty in containers.
        if let Some(shell) = login_shell() {
            assert!(!shell.to_string_lossy().contains('\0'));
        }
    }

    #[cfg(unix)]
    #[test]
    fn macos_starts_a_login_shell() {
        let shell = pick_unix_shell(None, Some("/bin/zsh".into()), true, existing(&["/bin/zsh"]));
        assert_eq!(shell, ShellSpec::new("/bin/zsh").arg("-l"));
    }

    #[cfg(unix)]
    #[test]
    fn unix_ignores_missing_or_relative_shell_env() {
        let exists = existing(&["/bin/bash", "/bin/sh"]);
        assert_eq!(
            pick_unix_shell(None, Some("/nope/zsh".into()), false, &exists).program,
            PathBuf::from("/bin/bash")
        );
        assert_eq!(
            pick_unix_shell(None, Some("zsh".into()), false, &exists).program,
            PathBuf::from("/bin/bash")
        );
        assert_eq!(
            pick_unix_shell(None, Some("".into()), false, &exists).program,
            PathBuf::from("/bin/bash")
        );
        assert_eq!(
            pick_unix_shell(None, None, false, &exists).program,
            PathBuf::from("/bin/bash")
        );
    }

    #[cfg(unix)]
    #[test]
    fn macos_falls_back_to_zsh() {
        let shell = pick_unix_shell(
            None,
            None,
            true,
            existing(&["/bin/zsh", "/bin/bash", "/bin/sh"]),
        );
        assert_eq!(shell, ShellSpec::new("/bin/zsh").arg("-l"));
    }

    #[cfg(unix)]
    #[test]
    fn unix_last_resort_is_sh() {
        let shell = pick_unix_shell(None, None, false, existing(&[]));
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
