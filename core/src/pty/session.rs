//! A single pty running one shell.

use std::ffi::OsString;
use std::fmt;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, native_pty_system};

use super::error::PtyError;
use super::flow::ReadGate;
use super::procinfo::{self, PtyInfo};
use super::shell::{ShellSpec, default_shell};

/// Size of each read from the pty. Batching for the UI happens one layer
/// up (the Tauri bridge); this only bounds a single callback call.
const READ_BUFFER_SIZE: usize = 64 * 1024;

/// After the shell exits, how long to wait for the remaining output before
/// delivering [`PtyEvent::Exit`]. On Unix a background process started by
/// the shell can keep the pty open forever, so this must be bounded.
const DRAIN_TIMEOUT: Duration = Duration::from_millis(500);

/// How long [`PtySession::kill`] waits after the polite signal (SIGHUP on
/// Unix) before forcing the shell down with SIGKILL.
const KILL_GRACE: Duration = Duration::from_millis(250);

/// How long [`PtySession::kill`] waits for the forced kill to be observed.
const KILL_TIMEOUT: Duration = Duration::from_secs(2);

/// Terminal size in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PtySize {
    /// Number of lines.
    pub rows: u16,
    /// Number of columns.
    pub cols: u16,
}

impl Default for PtySize {
    /// The classic 24 x 80 terminal.
    fn default() -> Self {
        Self { rows: 24, cols: 80 }
    }
}

impl From<PtySize> for portable_pty::PtySize {
    fn from(size: PtySize) -> Self {
        Self {
            rows: size.rows,
            cols: size.cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

impl From<portable_pty::PtySize> for PtySize {
    fn from(size: portable_pty::PtySize) -> Self {
        Self {
            rows: size.rows,
            cols: size.cols,
        }
    }
}

/// How to start a session. `SpawnOptions::default()` opens the user's
/// default shell ([`default_shell`]) in their home directory at 24 x 80.
#[derive(Debug, Clone, Default)]
pub struct SpawnOptions {
    /// Shell to run. `None` means [`default_shell`].
    pub shell: Option<ShellSpec>,
    /// Working directory. `None` (or a path that is not a directory) means
    /// the user's home directory.
    pub cwd: Option<PathBuf>,
    /// Extra environment variables, applied after termbay's own (`TERM`,
    /// `COLORTERM`, `TERM_PROGRAM`, `TERM_PROGRAM_VERSION`), so they can
    /// override them.
    pub env: Vec<(OsString, OsString)>,
    /// Initial size.
    pub size: PtySize,
    /// Event server for `termbay-hook` (decision E1 in `docs/FASE2-PLANO.md`).
    /// [`crate::pty::PtyManager::spawn`] turns it into `TERMBAY_EVENTS`,
    /// `TERMBAY_TOKEN` and `TERMBAY_PANE` (the session id);
    /// [`PtySession::spawn`] alone ignores it, as it has no id.
    pub hook_env: Option<crate::events::HookEnv>,
    /// Flow control: when set, the reader thread waits on this gate before
    /// each read (see [`ReadGate`]). The session opens it for good when the
    /// shell exits or is killed, so the rest of the output is drained.
    /// `None` reads without limit.
    pub read_gate: Option<Arc<ReadGate>>,
}

/// Something that happened in a session, delivered to the callback given
/// to [`PtySession::spawn`].
#[derive(Debug, PartialEq, Eq)]
pub enum PtyEvent<'a> {
    /// Raw bytes written by the shell (and its children) to the terminal.
    /// They are not guaranteed to end on a UTF-8 boundary.
    Output(&'a [u8]),
    /// The shell exited. This is always the last event of a session and is
    /// delivered exactly once, after the remaining output (bounded wait).
    Exit(ExitStatus),
}

/// How the shell process ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitStatus {
    code: u32,
    signal: Option<String>,
}

impl ExitStatus {
    /// Exit code. When the process was killed by a signal (Unix) this is
    /// not meaningful; check [`ExitStatus::signal`] first.
    pub fn code(&self) -> u32 {
        self.code
    }

    /// Name of the signal that killed the process (Unix only), such as
    /// `"Hangup"` or `"Killed"`.
    pub fn signal(&self) -> Option<&str> {
        self.signal.as_deref()
    }

    /// `true` when the process exited normally with code 0.
    pub fn success(&self) -> bool {
        self.signal.is_none() && self.code == 0
    }
}

impl From<portable_pty::ExitStatus> for ExitStatus {
    fn from(status: portable_pty::ExitStatus) -> Self {
        Self {
            code: status.exit_code(),
            signal: status.signal().map(str::to_owned),
        }
    }
}

impl fmt::Display for ExitStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.signal {
            Some(signal) => write!(f, "terminated by signal {signal}"),
            None => write!(f, "exited with code {}", self.code),
        }
    }
}

type Callback = Box<dyn FnMut(PtyEvent<'_>) + Send>;

/// The callback plus a flag that stops delivery once `Exit` was sent.
struct Sink {
    callback: Callback,
    closed: bool,
}

/// State shared between the session handle and its two threads.
struct Shared {
    /// Taken (and dropped) when the shell exits. On Windows dropping it
    /// closes the pseudo console, which is what ends the reader thread.
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    status: Mutex<Option<ExitStatus>>,
    exited: Condvar,
    gate: Option<Arc<ReadGate>>,
}

/// One shell running in its own pty.
///
/// Created by [`PtySession::spawn`]. Two background threads belong to each
/// session: one reads the output and one waits for the shell to exit.
/// Dropping the session kills the shell (see [`PtySession::kill`]).
pub struct PtySession {
    shared: Arc<Shared>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    pid: Option<u32>,
    shell_name: String,
}

impl fmt::Debug for PtySession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PtySession")
            .field("pid", &self.pid)
            .field("exit_status", &self.exit_status())
            .finish()
    }
}

impl PtySession {
    /// Opens a pty, starts the shell in it and returns the session.
    ///
    /// `on_event` is called from a background thread: once per chunk of
    /// output, then once with [`PtyEvent::Exit`]. It must not block for
    /// long, because the shell's output is not read while it runs. It may
    /// call [`PtySession::write`] or [`PtySession::resize`].
    pub fn spawn<F>(options: SpawnOptions, on_event: F) -> Result<Self, PtyError>
    where
        F: FnMut(PtyEvent<'_>) + Send + 'static,
    {
        let pair = native_pty_system()
            .openpty(options.size.into())
            .map_err(|err| PtyError::Open(format!("{err:#}")))?;

        let shell = options.shell.unwrap_or_else(default_shell);
        let shell_name = procinfo::program_name(&shell.program);
        let mut cmd = CommandBuilder::new(&shell.program);
        cmd.args(&shell.args);
        if let Some(cwd) = &options.cwd {
            cmd.cwd(cwd);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "termbay");
        cmd.env("TERM_PROGRAM_VERSION", crate::VERSION);
        // Never inherited from termbay's own environment: a termbay started
        // inside another one must not send this shell's events to the other.
        for key in crate::events::env::HOOK_VARS {
            cmd.env_remove(key);
        }
        for (key, value) in &options.env {
            cmd.env(key, value);
        }

        #[cfg(windows)]
        allow_ctrl_c_in_children();
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|err| PtyError::Spawn(format!("{}: {err:#}", shell.program.display())))?;
        // Our copy of the slave side must be closed, otherwise the reader
        // never sees end-of-file once the shell exits.
        drop(pair.slave);

        let mut killer = child.clone_killer();
        let pid = child.process_id();

        let io_handles = pair
            .master
            .try_clone_reader()
            .and_then(|reader| Ok((reader, pair.master.take_writer()?)));
        let (reader, writer) = match io_handles {
            Ok(handles) => handles,
            Err(err) => {
                let _ = killer.kill();
                let _ = child.wait();
                return Err(PtyError::Open(format!("{err:#}")));
            }
        };

        let shared = Arc::new(Shared {
            master: Mutex::new(Some(pair.master)),
            writer: Mutex::new(Some(writer)),
            status: Mutex::new(None),
            exited: Condvar::new(),
            gate: options.read_gate,
        });
        let sink = Arc::new(Mutex::new(Sink {
            callback: Box::new(on_event),
            closed: false,
        }));

        // Dropped by the reader thread when it finishes; the waiter thread
        // uses the disconnect as the "all output read" signal.
        let (done_tx, done_rx) = mpsc::channel::<()>();

        let session = Self {
            shared: Arc::clone(&shared),
            killer: Mutex::new(killer),
            pid,
            shell_name,
        };

        let reader_sink = Arc::clone(&sink);
        let reader_gate = shared.gate.clone();
        let spawned = thread::Builder::new()
            .name("termbay-pty-reader".into())
            .spawn(move || read_loop(reader, &reader_sink, reader_gate.as_deref(), done_tx));
        if let Err(err) = spawned {
            let _ = lock(&session.killer).kill();
            let status = child.wait().map(ExitStatus::from).ok();
            session.mark_failed(status);
            return Err(PtyError::Io(err));
        }

        let waiter_shared = Arc::clone(&shared);
        let spawned = thread::Builder::new()
            .name("termbay-pty-waiter".into())
            .spawn(move || {
                let status = child.wait().map(ExitStatus::from).unwrap_or(ExitStatus {
                    code: 1,
                    signal: None,
                });
                finish(&waiter_shared, &sink, &done_rx, status);
            });
        if let Err(err) = spawned {
            // Without a waiter nobody reaps the child; kill it directly.
            let _ = lock(&session.killer).kill();
            session.mark_failed(None);
            return Err(PtyError::Io(err));
        }

        Ok(session)
    }

    /// Sends bytes to the shell, exactly as typed (use `\r` for Enter).
    pub fn write(&self, data: &[u8]) -> Result<(), PtyError> {
        let mut writer = lock(&self.shared.writer);
        let writer = writer.as_mut().ok_or(PtyError::Exited)?;
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    /// Changes the terminal size. The shell receives `SIGWINCH` on Unix;
    /// ConPTY handles the equivalent on Windows.
    pub fn resize(&self, size: PtySize) -> Result<(), PtyError> {
        let master = lock(&self.shared.master);
        let master = master.as_ref().ok_or(PtyError::Exited)?;
        master.resize(size.into()).map_err(anyhow_to_io)
    }

    /// Current terminal size as known by the operating system.
    pub fn size(&self) -> Result<PtySize, PtyError> {
        let master = lock(&self.shared.master);
        let master = master.as_ref().ok_or(PtyError::Exited)?;
        master.get_size().map(PtySize::from).map_err(anyhow_to_io)
    }

    /// Process id of the shell, when the platform reports one.
    pub fn process_id(&self) -> Option<u32> {
        self.pid
    }

    /// Exit status, or `None` while the shell is running.
    pub fn exit_status(&self) -> Option<ExitStatus> {
        lock(&self.shared.status).clone()
    }

    /// `true` while the shell has not exited.
    pub fn is_running(&self) -> bool {
        self.exit_status().is_none()
    }

    /// Blocks until the shell exits or `timeout` passes. Returns the exit
    /// status, or `None` on timeout.
    pub fn wait_timeout(&self, timeout: Duration) -> Option<ExitStatus> {
        let status = lock(&self.shared.status);
        let (status, _) = self
            .shared
            .exited
            .wait_timeout_while(status, timeout, |status| status.is_none())
            .unwrap_or_else(PoisonError::into_inner);
        status.clone()
    }

    /// Terminates the shell and returns once it is gone.
    ///
    /// - **Unix:** sends `SIGHUP` (what a terminal does when its window
    ///   closes, so the shell can also hang up its jobs), waits up to
    ///   250 ms, then sends `SIGKILL`.
    /// - **Windows:** terminates the shell process; closing the pseudo
    ///   console afterwards ends the programs attached to it.
    ///
    /// Does nothing if the shell already exited. The [`PtyEvent::Exit`]
    /// event is still delivered to the callback.
    pub fn kill(&self) -> Result<(), PtyError> {
        if !self.is_running() {
            return Ok(());
        }
        self.open_gate();
        // The result is ignored on purpose: the process may exit on its own
        // between the check above and this call.
        let _ = lock(&self.killer).kill();
        if self.wait_timeout(KILL_GRACE).is_some() {
            return Ok(());
        }

        #[cfg(unix)]
        self.force_kill();

        match self.wait_timeout(KILL_TIMEOUT) {
            Some(_) => Ok(()),
            None => Err(PtyError::Io(io::Error::new(
                io::ErrorKind::TimedOut,
                "the shell did not exit after being killed",
            ))),
        }
    }

    /// What the system says about the shell and its foreground program
    /// (`pty_info`). Computed on each call; nothing runs in between.
    ///
    /// - `cwd`: the shell process's directory on macOS and Linux; `None`
    ///   on Windows.
    /// - `busy` / `foreground`: on Unix, the foreground process group of
    ///   the pty compared with the shell; on Windows, the shell's direct
    ///   children (console hosts ignored).
    ///
    /// After the shell exited, only `shell` is known.
    pub fn info(&self) -> PtyInfo {
        let mut info = PtyInfo {
            shell: self.shell_name.clone(),
            cwd: None,
            busy: None,
            foreground: None,
        };
        let Some(pid) = self.pid.filter(|_| self.is_running()) else {
            return info;
        };
        info.cwd = procinfo::process_cwd(pid).and_then(procinfo::path_to_string);
        let foreground = self.foreground(pid);
        info.busy = foreground.as_ref().map(Option::is_some);
        info.foreground = foreground.flatten().filter(|name| !name.is_empty());
        info
    }

    /// `None`: unknown; `Some(None)`: the shell is in the foreground;
    /// `Some(Some(name))`: another program is (its name may be empty when
    /// unknown, which is reported as `busy` without a name).
    #[cfg(unix)]
    fn foreground(&self, pid: u32) -> Option<Option<String>> {
        let leader = {
            let master = lock(&self.shared.master);
            master.as_ref()?.process_group_leader()?
        };
        let leader = u32::try_from(leader).ok()?;
        if leader == pid {
            return Some(None);
        }
        Some(Some(procinfo::process_name(leader).unwrap_or_default()))
    }

    #[cfg(windows)]
    fn foreground(&self, pid: u32) -> Option<Option<String>> {
        let children = procinfo::child_processes(pid)?;
        Some(children.into_iter().last())
    }

    fn open_gate(&self) {
        if let Some(gate) = &self.shared.gate {
            gate.open();
        }
    }

    /// Used when `spawn` fails half-way: records a status so that dropping
    /// the half-built session does not wait for an exit nobody will report.
    fn mark_failed(&self, status: Option<ExitStatus>) {
        *lock(&self.shared.status) = Some(status.unwrap_or(ExitStatus {
            code: 1,
            signal: None,
        }));
    }

    #[cfg(unix)]
    fn force_kill(&self) {
        let Some(pid) = self.pid.and_then(|pid| libc::pid_t::try_from(pid).ok()) else {
            return;
        };
        // Holding the status lock keeps the window in which the pid could
        // already be reaped (and reused) as small as possible.
        let status = lock(&self.shared.status);
        if status.is_none() {
            // SAFETY: `kill` has no memory safety requirements; `pid` is our
            // own child, which has not been reported as exited yet.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}

fn read_loop(
    mut reader: Box<dyn Read + Send>,
    sink: &Mutex<Sink>,
    gate: Option<&ReadGate>,
    done: mpsc::Sender<()>,
) {
    let mut buf = vec![0u8; READ_BUFFER_SIZE];
    loop {
        // Outside the sink lock, so `finish` can always take it.
        if let Some(gate) = gate {
            gate.wait();
        }
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let mut sink = lock(sink);
                if sink.closed {
                    break;
                }
                (sink.callback)(PtyEvent::Output(&buf[..n]));
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            // EIO on Linux once the shell is gone, broken pipe on Windows.
            Err(_) => break,
        }
    }
    drop(done);
}

/// Runs on the waiter thread after the shell exited.
fn finish(shared: &Shared, sink: &Mutex<Sink>, done: &mpsc::Receiver<()>, status: ExitStatus) {
    *lock(&shared.status) = Some(status.clone());
    shared.exited.notify_all();
    // A paused reader must drain what is left, not wait forever.
    if let Some(gate) = &shared.gate {
        gate.open();
    }

    // Release our side of the pty. On Windows this closes the pseudo
    // console so the reader sees end-of-file; on Unix the reader keeps its
    // own descriptor and drains what is left.
    drop(lock(&shared.writer).take());
    drop(lock(&shared.master).take());

    // Either the reader finished (sender dropped) or we stop waiting.
    let _ = done.recv_timeout(DRAIN_TIMEOUT);

    let mut sink = lock(sink);
    sink.closed = true;
    // Replace the callback so whatever it captured (channels, handles) is
    // released right after the last event.
    let mut callback = std::mem::replace(&mut sink.callback, Box::new(|_| {}));
    callback(PtyEvent::Exit(status));
}

/// Makes the shells started from now on handle Ctrl+C.
///
/// On Windows, "ignore Ctrl+C" is a process attribute inherited by child
/// processes (`SetConsoleCtrlHandler(NULL, TRUE)`, or a parent started with
/// `CREATE_NEW_PROCESS_GROUP`, as some launchers and CI runners do). A shell
/// that inherits it, and every program it starts, never sees the
/// `CTRL_C_EVENT` that ConPTY raises when the user types Ctrl+C (`\x03`),
/// so nothing can be interrupted. Clearing the attribute in our own process
/// before each spawn gives the shell the default behaviour, as in a fresh
/// console window. termbay has no console of its own (it is a GUI app), so
/// this changes nothing else for it; under a console (tests, the CLI),
/// Ctrl+C there ends the process, which is the default anyway.
#[cfg(windows)]
fn allow_ctrl_c_in_children() {
    use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
    // SAFETY: no handler routine is passed; with `NULL` and `FALSE` the call
    // only clears the "ignore Ctrl+C" flag of the current process. A failure
    // leaves things as they were, so the result is ignored.
    unsafe {
        SetConsoleCtrlHandler(None, 0);
    }
}

fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn anyhow_to_io(err: impl fmt::Display) -> PtyError {
    PtyError::Io(io::Error::other(format!("{err:#}")))
}
