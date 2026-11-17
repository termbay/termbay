//! What termbay knows about an integrated shell, from its trusted marks.

use super::osc::{ShellInfo, ShellMark};

/// Where the shell is in its prompt / command cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// No trusted mark yet.
    Starting,
    /// Waiting at the prompt.
    Prompt,
    /// Running a command (or a program that reads the terminal: vim, ssh,
    /// a nested shell...). Only the next trusted `prompt` mark ends it.
    Running,
}

/// State of one integrated shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellState {
    info: Option<ShellInfo>,
    phase: Phase,
    last_exit: Option<i32>,
    submitted: bool,
}

impl Default for ShellState {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellState {
    /// A shell that has not reported anything yet.
    pub fn new() -> Self {
        Self {
            info: None,
            phase: Phase::Starting,
            last_exit: None,
            submitted: false,
        }
    }

    /// Applies one trusted mark. Returns `true` when this is the first
    /// handshake (the integration just became active).
    pub fn apply(&mut self, mark: &ShellMark) -> bool {
        match mark {
            ShellMark::Ready(info) => {
                let first = self.info.is_none();
                self.info = Some(info.clone());
                first
            }
            ShellMark::Prompt { exit_code } => {
                if exit_code.is_some() {
                    self.last_exit = *exit_code;
                }
                self.phase = Phase::Prompt;
                self.submitted = false;
                false
            }
            ShellMark::Exec => {
                self.phase = Phase::Running;
                false
            }
            ShellMark::Completion(_) => false,
        }
    }

    /// The user typed a line break: the line may be running already, even
    /// though the `exec` mark did not arrive yet. Until the next prompt the
    /// shell is not considered at the prompt, so nothing is ever written
    /// to a command that started in the meantime.
    pub fn note_line_submitted(&mut self) {
        self.submitted = true;
    }

    /// Whether the shell completed the handshake, is at its prompt and no
    /// line was submitted since.
    pub fn is_at_prompt(&self) -> bool {
        self.info.is_some() && self.phase == Phase::Prompt && !self.submitted
    }

    /// The handshake, once received.
    pub fn info(&self) -> Option<&ShellInfo> {
        self.info.as_ref()
    }

    /// Current phase.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Exit code of the last command, when known.
    pub fn last_exit(&self) -> Option<i32> {
        self.last_exit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> ShellMark {
        ShellMark::Ready(ShellInfo {
            protocol: 1,
            shell: "fish".into(),
            version: "4.0".into(),
            history_file: None,
        })
    }

    #[test]
    fn prompt_needs_the_handshake() {
        let mut state = ShellState::new();
        assert!(!state.apply(&ShellMark::Prompt { exit_code: None }));
        assert_eq!(state.phase(), Phase::Prompt);
        assert!(!state.is_at_prompt());
        assert!(state.apply(&ready()));
        assert!(state.is_at_prompt());
        assert!(!state.apply(&ready()), "only the first handshake counts");
    }

    #[test]
    fn running_until_the_next_prompt() {
        let mut state = ShellState::new();
        state.apply(&ready());
        state.apply(&ShellMark::Prompt { exit_code: None });
        state.apply(&ShellMark::Exec);
        assert_eq!(state.phase(), Phase::Running);
        assert!(!state.is_at_prompt());
        state.apply(&ShellMark::Prompt { exit_code: Some(2) });
        assert!(state.is_at_prompt());
        assert_eq!(state.last_exit(), Some(2));
        state.apply(&ShellMark::Prompt { exit_code: None });
        assert_eq!(state.last_exit(), Some(2));
    }

    #[test]
    fn a_submitted_line_is_not_the_prompt_anymore() {
        let mut state = ShellState::new();
        state.apply(&ready());
        state.apply(&ShellMark::Prompt { exit_code: None });
        state.note_line_submitted();
        assert!(!state.is_at_prompt());
        state.apply(&ShellMark::Prompt { exit_code: Some(0) });
        assert!(state.is_at_prompt());
    }
}
