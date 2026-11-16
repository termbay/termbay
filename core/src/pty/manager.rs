//! Several sessions addressed by id (one per tab or split pane).

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use super::error::PtyError;
use super::session::{PtyEvent, PtySession, PtySize, SpawnOptions};

/// Identifier of a session inside a [`PtyManager`]. Ids start at 1 and are
/// never reused by the same manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PtyId(u64);

impl PtyId {
    /// Builds an id from its numeric form (for example, a number received
    /// from the UI).
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Numeric form of the id.
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for PtyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

type SessionMap = HashMap<PtyId, Arc<PtySession>>;

/// Keeps the live sessions of the app, addressed by [`PtyId`].
///
/// A session is forgotten automatically once its shell exits (right after
/// its [`PtyEvent::Exit`] is delivered), and [`PtyManager::kill`] removes
/// and kills it explicitly. Dropping the manager kills every shell.
///
/// All methods take `&self`, so the manager can be shared between threads
/// (for example, as Tauri state).
#[derive(Default)]
pub struct PtyManager {
    sessions: Arc<Mutex<SessionMap>>,
    next_id: AtomicU64,
}

impl fmt::Debug for PtyManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PtyManager")
            .field("sessions", &self.ids())
            .finish()
    }
}

impl PtyManager {
    /// An empty manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a new session and returns its id.
    ///
    /// `on_event` receives the id with every event, so one callback can
    /// route the events of many sessions. The same rules as in
    /// [`PtySession::spawn`] apply. Do not call [`PtyManager::kill`] or
    /// [`PtyManager::kill_all`] from inside the callback.
    pub fn spawn<F>(&self, options: SpawnOptions, mut on_event: F) -> Result<PtyId, PtyError>
    where
        F: FnMut(PtyId, PtyEvent<'_>) + Send + 'static,
    {
        let id = PtyId(self.next_id.fetch_add(1, Ordering::Relaxed) + 1);
        let sessions: Weak<Mutex<SessionMap>> = Arc::downgrade(&self.sessions);

        let session = PtySession::spawn(options, move |event| {
            let exited = matches!(event, PtyEvent::Exit(_));
            on_event(id, event);
            if exited && let Some(sessions) = sessions.upgrade() {
                lock(&sessions).remove(&id);
            }
        })?;
        let session = Arc::new(session);

        let mut sessions = lock(&self.sessions);
        // If the shell already exited, the exit callback may have run before
        // this insert; do not keep a dead session around in that case.
        if session.is_running() {
            sessions.insert(id, session);
        }
        Ok(id)
    }

    /// The session with this id, if it is still alive.
    pub fn get(&self, id: PtyId) -> Option<Arc<PtySession>> {
        lock(&self.sessions).get(&id).cloned()
    }

    /// Sends bytes to the shell of a session. See [`PtySession::write`].
    pub fn write(&self, id: PtyId, data: &[u8]) -> Result<(), PtyError> {
        self.require(id)?.write(data)
    }

    /// Resizes a session. See [`PtySession::resize`].
    pub fn resize(&self, id: PtyId, size: PtySize) -> Result<(), PtyError> {
        self.require(id)?.resize(size)
    }

    /// Removes a session and kills its shell. See [`PtySession::kill`].
    pub fn kill(&self, id: PtyId) -> Result<(), PtyError> {
        let session = lock(&self.sessions)
            .remove(&id)
            .ok_or(PtyError::NotFound(id))?;
        session.kill()
    }

    /// Kills every session (used when the app closes).
    pub fn kill_all(&self) {
        let sessions: Vec<_> = lock(&self.sessions).drain().map(|(_, s)| s).collect();
        for session in sessions {
            let _ = session.kill();
        }
    }

    /// Ids of the live sessions, in creation order.
    pub fn ids(&self) -> Vec<PtyId> {
        let mut ids: Vec<_> = lock(&self.sessions).keys().copied().collect();
        ids.sort();
        ids
    }

    /// Number of live sessions.
    pub fn len(&self) -> usize {
        lock(&self.sessions).len()
    }

    /// `true` when there is no live session.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn require(&self, id: PtyId) -> Result<Arc<PtySession>, PtyError> {
        self.get(id).ok_or(PtyError::NotFound(id))
    }
}

impl Drop for PtyManager {
    fn drop(&mut self) {
        self.kill_all();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
