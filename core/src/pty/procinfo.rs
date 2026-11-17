//! What the operating system says about the processes of a pty: the
//! shell's working directory and the program in the foreground. Used by
//! [`PtySession::info`](super::PtySession::info) (`pty_info`, section 6.3
//! of `docs/FASE1-PLANO.md`).
//!
//! One function per system, with a fallback everywhere: an answer that
//! cannot be known is `None`, never an error.

use std::path::{Path, PathBuf};

/// Longest program name reported, in characters.
pub const MAX_NAME_CHARS: usize = 64;

/// What [`PtySession::info`](super::PtySession::info) reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyInfo {
    /// Name of the shell program, without folder or extension: `"zsh"`,
    /// `"fish"`, `"bash"`, `"pwsh"`, `"cmd"`.
    pub shell: String,
    /// Current directory of the shell process (not of the foreground
    /// program), or `None` when it cannot be known (always on Windows for
    /// now) or is not valid UTF-8.
    pub cwd: Option<String>,
    /// `Some(true)`: a program runs in place of the shell; `Some(false)`:
    /// the shell is idle; `None`: unknown.
    pub busy: Option<bool>,
    /// Name of the foreground program when `busy` is `Some(true)` and it is
    /// known, otherwise `None`.
    pub foreground: Option<String>,
}

/// Program name without folder or extension, cleaned with [`clean_name`].
pub fn program_name(program: &Path) -> String {
    let stem = program
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    clean_name(&stem)
}

/// Removes control characters and keeps at most [`MAX_NAME_CHARS`].
pub fn clean_name(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_CHARS)
        .collect()
}

/// A path as UTF-8, or `None` when it is not valid UTF-8.
pub fn path_to_string(path: PathBuf) -> Option<String> {
    path.into_os_string().into_string().ok()
}

/// Current directory of a process.
#[cfg(target_os = "linux")]
pub fn process_cwd(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

/// Current directory of a process.
#[cfg(target_os = "macos")]
pub fn process_cwd(pid: u32) -> Option<PathBuf> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStrExt;

    let pid = libc::c_int::try_from(pid).ok()?;
    let mut info = std::mem::MaybeUninit::<libc::proc_vnodepathinfo>::zeroed();
    let size = libc::c_int::try_from(size_of::<libc::proc_vnodepathinfo>()).ok()?;
    // SAFETY: the buffer is a zeroed `proc_vnodepathinfo` of exactly
    // `size` bytes, which is what PROC_PIDVNODEPATHINFO writes.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written != size {
        return None;
    }
    // SAFETY: fully written by the call above (checked by its size).
    let info = unsafe { info.assume_init() };
    // SAFETY: `vip_path` is a contiguous `[c_char; MAXPATHLEN]` split in
    // rows only for old compilers; it is read as one byte slice.
    let bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(
            info.pvi_cdir.vip_path.as_ptr().cast::<u8>(),
            size_of_val(&info.pvi_cdir.vip_path),
        )
    };
    let path = CStr::from_bytes_until_nul(bytes).ok()?.to_bytes();
    if path.is_empty() {
        return None;
    }
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(path)))
}

/// Current directory of a process: unknown on this system.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn process_cwd(_pid: u32) -> Option<PathBuf> {
    None
}

/// Name of a process.
#[cfg(target_os = "linux")]
pub fn process_name(pid: u32) -> Option<String> {
    let comm = std::fs::read(format!("/proc/{pid}/comm")).ok()?;
    let name = clean_name(String::from_utf8_lossy(&comm).trim_end_matches('\n'));
    (!name.is_empty()).then_some(name)
}

/// Name of a process.
#[cfg(target_os = "macos")]
pub fn process_name(pid: u32) -> Option<String> {
    let pid = libc::c_int::try_from(pid).ok()?;
    let mut buf = [0u8; 256];
    let len = u32::try_from(buf.len()).ok()?;
    // SAFETY: `buf` is valid for `len` bytes; `proc_name` writes at most
    // that many and returns how many it wrote.
    let written = unsafe { libc::proc_name(pid, buf.as_mut_ptr().cast(), len) };
    let written = usize::try_from(written).ok().filter(|&n| n > 0)?;
    let name = clean_name(&String::from_utf8_lossy(&buf[..written.min(buf.len())]));
    (!name.is_empty()).then_some(name)
}

/// Name of a process: unknown on this system.
#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
pub fn process_name(_pid: u32) -> Option<String> {
    None
}

/// Programs that ConPTY attaches to every console; never the foreground.
#[cfg(windows)]
const CONSOLE_HOSTS: &[&str] = &["conhost.exe", "openconsole.exe"];

/// Direct children of `pid`, by name, ignoring the console hosts. `None`
/// when the system could not be asked. The last child listed (usually the
/// newest) comes last.
#[cfg(windows)]
pub fn child_processes(pid: u32) -> Option<Vec<String>> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    // SAFETY: plain Win32 call; the handle is checked and closed below.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut children = Vec::new();
    // SAFETY: an all-zero PROCESSENTRY32W is valid; `dwSize` is set as the
    // API requires before the first call.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = u32::try_from(size_of::<PROCESSENTRY32W>()).unwrap_or(0);
    // SAFETY: `snapshot` is a valid snapshot handle and `entry` a properly
    // sized PROCESSENTRY32W.
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while more {
        if entry.th32ParentProcessID == pid && entry.th32ProcessID != pid {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let exe = String::from_utf16_lossy(&entry.szExeFile[..len]);
            if !CONSOLE_HOSTS.contains(&exe.to_ascii_lowercase().as_str()) {
                children.push(program_name(Path::new(&exe)));
            }
        }
        // SAFETY: same handle and entry as above.
        more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    // SAFETY: `snapshot` is valid and closed once.
    unsafe {
        CloseHandle(snapshot);
    }
    Some(children)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_lose_folder_extension_and_control_characters() {
        assert_eq!(program_name(Path::new("/bin/zsh")), "zsh");
        assert_eq!(program_name(Path::new("pwsh.exe")), "pwsh");
        assert_eq!(program_name(Path::new("")), "");
        assert_eq!(clean_name("vi\x1b[31mm\u{7}"), "vi[31mm");
        assert_eq!(clean_name(&"x".repeat(100)).chars().count(), MAX_NAME_CHARS);
        assert_eq!(clean_name(&"é".repeat(100)), "é".repeat(MAX_NAME_CHARS));
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_utf8_is_none() {
        use std::os::unix::ffi::OsStrExt;
        let path = PathBuf::from(std::ffi::OsStr::from_bytes(b"/tmp/\xff\xfe"));
        assert_eq!(path_to_string(path), None);
        assert_eq!(
            path_to_string(PathBuf::from("/tmp/ação")),
            Some("/tmp/ação".into())
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn this_process_has_a_cwd_and_a_name() {
        let pid = std::process::id();
        let cwd = process_cwd(pid).expect("no cwd");
        assert_eq!(
            cwd.canonicalize().unwrap(),
            std::env::current_dir().unwrap().canonicalize().unwrap()
        );
        assert!(process_name(pid).is_some());
        assert_eq!(process_cwd(u32::MAX), None);
    }
}
