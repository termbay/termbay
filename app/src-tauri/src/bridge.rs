//! Logic behind the pty commands, without any Tauri type, so it can be
//! tested directly.
//!
//! Each pty opened through the bridge gets:
//!
//! - a session in the core [`PtyManager`] (reader and waiter threads);
//! - a batcher thread ([`spawn_batcher`]) that groups the output and calls
//!   `on_chunk`, then `on_exit` after the last chunk;
//! - a writer thread that takes input from a queue. `write` only queues, so
//!   the caller (the Tauri main thread) never blocks on a shell that is not
//!   reading its input, and bytes reach the shell in the order they were
//!   queued.

use std::collections::HashMap;
use std::fmt;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use termbay_core::pty::{
    BatchLimits, ExitStatus, PtyError, PtyEvent, PtyId, PtyManager, PtySize, SpawnOptions,
    spawn_batcher,
};

use crate::complete::{ShellReady, ShellSupport};

/// How a pty ended, as reported to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtyExit {
    pub id: u32,
    /// Exit code, or `None` when the shell was killed by a signal.
    pub exit_code: Option<u32>,
}

type InputMap = HashMap<u32, mpsc::Sender<Vec<u8>>>;

/// Owns every pty of the app. Dropping it kills every shell.
pub struct PtyBridge {
    manager: PtyManager,
    inputs: Arc<Mutex<InputMap>>,
    template: SpawnOptions,
    limits: BatchLimits,
    /// Shell integration and autocomplete (`complete.rs`); off by default.
    pub(crate) shell: ShellSupport,
}

impl fmt::Debug for PtyBridge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PtyBridge")
            .field("manager", &self.manager)
            .finish_non_exhaustive()
    }
}

impl Default for PtyBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl PtyBridge {
    /// Opens the user's default shell in their home directory, with the
    /// shell integration when `[autocomplete] enabled = true`.
    pub fn new() -> Self {
        Self::with_shell_support(SpawnOptions::default(), ShellSupport::from_user_config())
    }

    /// Uses `template` for every pty (its size is replaced by the one given
    /// to [`PtyBridge::open`]). Used by tests to run a predictable shell.
    #[cfg(test)]
    pub fn with_options(template: SpawnOptions) -> Self {
        Self::with_shell_support(template, ShellSupport::disabled())
    }

    /// Same, with the given shell integration settings.
    pub fn with_shell_support(template: SpawnOptions, shell: ShellSupport) -> Self {
        Self {
            manager: PtyManager::new(),
            inputs: Arc::default(),
            template,
            limits: BatchLimits::default(),
            shell,
        }
    }

    /// Starts a shell and returns its id.
    ///
    /// `on_chunk` receives the batched output, in order. `on_exit` is called
    /// once, after the last chunk, when the shell exits or is closed. Both
    /// run on the batcher thread of this pty. (The app uses
    /// [`PtyBridge::open_with_shell`]; tests that do not care use this.)
    #[cfg(test)]
    pub fn open<C, X>(&self, cols: u16, rows: u16, on_chunk: C, on_exit: X) -> Result<u32, String>
    where
        C: FnMut(Vec<u8>) + Send + 'static,
        X: FnOnce(PtyExit) + Send + 'static,
    {
        self.open_with_shell(cols, rows, on_chunk, on_exit, |_| {})
    }

    /// [`PtyBridge::open`], plus `on_shell` when the shell's integration
    /// becomes active (only with autocomplete enabled). `on_shell` runs on
    /// the reader thread of this pty.
    pub fn open_with_shell<C, X, S>(
        &self,
        cols: u16,
        rows: u16,
        on_chunk: C,
        on_exit: X,
        mut on_shell: S,
    ) -> Result<u32, String>
    where
        C: FnMut(Vec<u8>) + Send + 'static,
        X: FnOnce(PtyExit) + Send + 'static,
        S: FnMut(ShellReady) + Send + 'static,
    {
        // A hidden terminal can measure 0 x 0; a pty needs at least 1 x 1.
        let size = PtySize {
            cols: cols.max(1),
            rows: rows.max(1),
        };

        let inputs = Arc::downgrade(&self.inputs);
        let batcher = spawn_batcher(
            self.limits,
            on_chunk,
            move |(id, status): (PtyId, ExitStatus)| {
                let Some(id) = ipc_id(id) else { return };
                if let Some(inputs) = inputs.upgrade() {
                    lock(&inputs).remove(&id);
                }
                on_exit(PtyExit {
                    id,
                    exit_code: status.signal().is_none().then(|| status.code()),
                });
            },
        )
        .map_err(|err| format!("could not start the output thread: {err}"))?;

        let mut options = SpawnOptions {
            size,
            ..self.template.clone()
        };
        // Without integration there is no tap: the output goes straight
        // to the batcher, as before.
        let mut tap = self.shell.prepare(&mut options);
        let link = tap.as_ref().map(|tap| Arc::clone(tap.link()));
        let pty_id = self
            .manager
            .spawn(options, move |id, event| match event {
                PtyEvent::Output(bytes) => match tap.as_mut() {
                    None => batcher.output(bytes),
                    Some(tap) => {
                        let ready = tap.output(bytes, |shown| batcher.output(shown));
                        if let (Some(info), Some(id)) = (ready, ipc_id(id)) {
                            on_shell(ShellReady {
                                id,
                                shell: tap.link().kind(),
                                protocol: info.protocol,
                            });
                        }
                    }
                },
                PtyEvent::Exit(status) => {
                    if let Some(tap) = &tap {
                        tap.link().close();
                    }
                    batcher.end((id, status));
                }
            })
            .map_err(|err| err.to_string())?;

        let Some(id) = ipc_id(pty_id) else {
            let _ = self.manager.kill(pty_id);
            return Err("too many ptys were opened".to_owned());
        };
        if let Some(link) = link {
            self.shell.register(id, link);
        }

        // The shell may already be gone (for example, a shell that exits at
        // once); its exit event is on the way and no input is needed.
        let Some(session) = self.manager.get(pty_id) else {
            return Ok(id);
        };

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let weak = Arc::downgrade(&session);
        let spawned = thread::Builder::new()
            .name("termbay-pty-writer".into())
            .spawn(move || {
                // Ends when the queue is closed (pty closed or exited) and
                // drained, or when the shell no longer accepts input.
                for data in rx {
                    let Some(session) = weak.upgrade() else { break };
                    if session.write(&data).is_err() {
                        break;
                    }
                }
            });
        if let Err(err) = spawned {
            let _ = self.manager.kill(pty_id);
            return Err(format!("could not start the input thread: {err}"));
        }

        let mut inputs = lock(&self.inputs);
        // `on_exit` removes the entry after the exit status is set, so if
        // the shell is still running here, the removal comes later.
        if session.is_running() {
            inputs.insert(id, tx);
        }
        Ok(id)
    }

    /// Queues bytes for the shell. Never blocks.
    pub fn write(&self, id: u32, data: Vec<u8>) -> Result<(), String> {
        let inputs = lock(&self.inputs);
        let tx = inputs.get(&id).ok_or_else(|| not_found(id))?;
        if data.is_empty() {
            return Ok(());
        }
        self.shell.note_input(id, &data);
        tx.send(data).map_err(|_| not_found(id))
    }

    /// Resizes a pty. Sizes of 0 are raised to 1.
    pub fn resize(&self, id: u32, cols: u16, rows: u16) -> Result<(), String> {
        let size = PtySize {
            cols: cols.max(1),
            rows: rows.max(1),
        };
        self.manager
            .resize(PtyId::from_raw(id.into()), size)
            .map_err(|err| err.to_string())
    }

    /// Kills a pty. Closing an id that does not exist (or already exited)
    /// is not an error.
    pub fn close(&self, id: u32) -> Result<(), String> {
        lock(&self.inputs).remove(&id);
        self.shell.forget(id);
        match self.manager.kill(PtyId::from_raw(id.into())) {
            Ok(()) | Err(PtyError::NotFound(_)) => Ok(()),
            Err(err) => Err(err.to_string()),
        }
    }

    /// Bytes the UI has drawn. Flow control (pausing the reader when too
    /// much is pending) comes in T10; for now the value is accepted and
    /// ignored, for any id.
    pub fn ack(&self, _id: u32, _bytes: u64) {}

    /// Kills every pty (window closed or app exiting).
    pub fn kill_all(&self) {
        lock(&self.inputs).clear();
        self.manager.kill_all();
    }

    /// Number of live ptys.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.manager.len()
    }
}

impl Drop for PtyBridge {
    fn drop(&mut self) {
        self.kill_all();
    }
}

/// The UI receives ids as JavaScript numbers; `u32` is plenty.
fn ipc_id(id: PtyId) -> Option<u32> {
    u32::try_from(id.as_u64()).ok()
}

fn not_found(id: u32) -> String {
    PtyError::NotFound(PtyId::from_raw(id.into())).to_string()
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::time::{Duration, Instant};

    use termbay_core::pty::{ShellSpec, SpawnOptions};

    /// Generous, because CI machines can be slow.
    pub const TIMEOUT: Duration = Duration::from_secs(30);

    /// A small shell that ignores the user's configuration.
    pub fn plain_shell() -> SpawnOptions {
        #[cfg(windows)]
        let shell = ShellSpec::new("cmd.exe").arg("/D");
        #[cfg(not(windows))]
        let shell = ShellSpec::new("/bin/sh");
        SpawnOptions {
            shell: Some(shell),
            env: vec![
                ("ENV".into(), "/dev/null".into()),
                ("PS1".into(), "$ ".into()),
            ],
            ..SpawnOptions::default()
        }
    }

    /// Typed line that prints `ABCD` without the echoed input containing it.
    pub fn echo_marker() -> &'static [u8] {
        if cfg!(windows) {
            b"echo AB^CD\r"
        } else {
            b"echo AB\"\"CD\r"
        }
    }

    /// Cursor position query (DSR 6). On Windows, ConPTY sends it when it
    /// starts and waits for the answer before running the shell. xterm.js
    /// answers in the app; the bridge must not, so tests answer it.
    pub const CURSOR_QUERY: &[u8] = b"\x1b[6n";
    /// Answer to [`CURSOR_QUERY`]: row 1, column 1.
    pub const CURSOR_ANSWER: &[u8] = b"\x1b[1;1R";

    /// Finds cursor position queries in a stream of chunks, including a
    /// query split between two chunks. (Same as the helper in
    /// `core/tests/pty.rs`; test code is not shared between the crates.)
    #[derive(Debug, Default)]
    pub struct CursorQueries {
        tail: Vec<u8>,
    }

    impl CursorQueries {
        /// Number of queries that end in `chunk`.
        pub fn count(&mut self, chunk: &[u8]) -> usize {
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
            // The unscanned rest is shorter than a query and may be the
            // start of one that ends in the next chunk.
            data.drain(..i);
            self.tail = data;
            count
        }
    }

    pub fn eventually(what: &str, mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + TIMEOUT;
        while !condition() {
            assert!(Instant::now() < deadline, "timed out waiting until {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::Receiver;

    use super::test_support::*;
    use super::*;

    enum Seen {
        Chunk(Vec<u8>),
        Exit(PtyExit),
    }

    fn open(bridge: &PtyBridge) -> (u32, Receiver<Seen>) {
        let (tx, rx) = mpsc::channel();
        let exit_tx = tx.clone();
        let id = bridge
            .open(
                80,
                24,
                move |chunk| {
                    let _ = tx.send(Seen::Chunk(chunk));
                },
                move |exit| {
                    let _ = exit_tx.send(Seen::Exit(exit));
                },
            )
            .expect("open failed");
        (id, rx)
    }

    /// Reads until the exit, returning the output and the exit. Answers
    /// cursor position queries on the way (Windows).
    fn until_exit(bridge: &PtyBridge, id: u32, rx: &Receiver<Seen>) -> (Vec<u8>, PtyExit) {
        let mut output = Vec::new();
        let mut queries = CursorQueries::default();
        loop {
            match rx.recv_timeout(TIMEOUT).expect("no exit event") {
                Seen::Chunk(chunk) => {
                    assert!(chunk.len() <= BatchLimits::default().max_bytes);
                    for _ in 0..queries.count(&chunk) {
                        let _ = bridge.write(id, CURSOR_ANSWER.to_vec());
                    }
                    output.extend(chunk);
                }
                Seen::Exit(exit) => {
                    assert!(rx.recv().is_err(), "something arrived after the exit");
                    return (output, exit);
                }
            }
        }
    }

    #[test]
    fn output_comes_before_the_exit_with_its_code() {
        let bridge = PtyBridge::with_options(plain_shell());
        let (id, rx) = open(&bridge);
        bridge.write(id, echo_marker().to_vec()).unwrap();
        bridge.write(id, b"exit 3\r".to_vec()).unwrap();
        let (output, exit) = until_exit(&bridge, id, &rx);
        assert!(
            String::from_utf8_lossy(&output).contains("ABCD"),
            "output: {}",
            String::from_utf8_lossy(&output)
        );
        assert_eq!(
            exit,
            PtyExit {
                id,
                exit_code: Some(3)
            }
        );
        // The exited pty is forgotten.
        eventually("the pty is forgotten", || bridge.len() == 0);
        assert!(
            bridge
                .write(id, b"x".to_vec())
                .unwrap_err()
                .contains("no pty")
        );
        assert!(bridge.resize(id, 80, 24).is_err());
    }

    #[test]
    fn close_kills_the_shell_and_is_idempotent() {
        let bridge = PtyBridge::with_options(plain_shell());
        let (id, rx) = open(&bridge);
        bridge.resize(id, 100, 30).unwrap();
        bridge.close(id).unwrap();
        let (_, exit) = until_exit(&bridge, id, &rx);
        assert_eq!(exit.id, id);
        assert_eq!(bridge.len(), 0);
        bridge.close(id).unwrap();
        bridge.close(12_345).unwrap();
    }

    #[test]
    fn kill_all_leaves_no_shell_behind() {
        let bridge = PtyBridge::with_options(plain_shell());
        let (a, rx_a) = open(&bridge);
        let (b, rx_b) = open(&bridge);
        assert_ne!(a, b);
        assert_eq!(bridge.len(), 2);
        bridge.kill_all();
        assert_eq!(bridge.len(), 0);
        assert_eq!(until_exit(&bridge, a, &rx_a).1.id, a);
        assert_eq!(until_exit(&bridge, b, &rx_b).1.id, b);
        assert!(bridge.write(a, b"x".to_vec()).is_err());
    }

    #[test]
    fn dropping_the_bridge_kills_every_shell() {
        let bridge = PtyBridge::with_options(plain_shell());
        let (_, rx) = open(&bridge);
        drop(bridge);
        // No bridge is left to answer cursor queries; killing does not
        // need them.
        while !matches!(
            rx.recv_timeout(TIMEOUT).expect("no exit event"),
            Seen::Exit(_)
        ) {}
    }

    #[test]
    fn cursor_queries_are_found_even_when_split() {
        let mut q = CursorQueries::default();
        assert_eq!(q.count(b"abc\x1b[6nxyz"), 1);
        assert_eq!(q.count(b"\x1b["), 0);
        assert_eq!(q.count(b"6"), 0);
        assert_eq!(q.count(b"n\x1b[6n\x1b[6n"), 3);
        assert_eq!(q.count(b"[6n"), 0);
        assert_eq!(q.count(b"\x1b"), 0);
        assert_eq!(q.count(b"[6n"), 1);
    }

    #[test]
    fn zero_sizes_are_raised_to_one() {
        let bridge = PtyBridge::with_options(plain_shell());
        let id = bridge.open(0, 0, |_| {}, |_| {}).unwrap();
        bridge.resize(id, 0, 0).unwrap();
        bridge.close(id).unwrap();
    }

    #[test]
    fn unknown_ids_are_errors_except_for_close_and_ack() {
        let bridge = PtyBridge::with_options(plain_shell());
        assert!(bridge.write(7, b"x".to_vec()).is_err());
        assert!(bridge.resize(7, 80, 24).is_err());
        bridge.close(7).unwrap();
        bridge.ack(7, 10);
    }
}
