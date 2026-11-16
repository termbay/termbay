use std::fmt;
use std::io;

use super::PtyId;

/// Errors returned by the pty API.
#[derive(Debug)]
#[non_exhaustive]
pub enum PtyError {
    /// The operating system could not create a pty (or ConPTY).
    Open(String),
    /// The shell could not be started inside the pty.
    Spawn(String),
    /// Reading, writing or resizing failed.
    Io(io::Error),
    /// The shell has already exited, so the session no longer accepts
    /// input or resizes.
    Exited,
    /// [`PtyManager`](super::PtyManager) has no session with this id
    /// (never existed, or its shell already exited).
    NotFound(PtyId),
}

impl fmt::Display for PtyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(msg) => write!(f, "could not open a pty: {msg}"),
            Self::Spawn(msg) => write!(f, "could not start the shell: {msg}"),
            Self::Io(err) => write!(f, "pty i/o error: {err}"),
            Self::Exited => f.write_str("the shell has already exited"),
            Self::NotFound(id) => write!(f, "no pty session with id {id}"),
        }
    }
}

impl std::error::Error for PtyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for PtyError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}
