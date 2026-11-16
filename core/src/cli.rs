//! Command line parsing for the `termbay` binary.
//!
//! The binary parses its arguments **before** starting any window, so
//! commands such as `--version` (and, in later phases, `termbay hook`)
//! never open a window or touch the UI toolkit.
//!
//! Phase 1 only knows `termbay` (open the app), `--version` and `--help`.
//! Subcommands (`hook`, `init`, `serve`, `doctor`) get a new [`Command`]
//! variant each when they arrive.

use std::ffi::OsString;
use std::fmt;

/// What the binary should do, decided from the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// No arguments: open the desktop app.
    Gui,
    /// `--version` / `-V`: print [`version_text`] and exit.
    Version,
    /// `--help` / `-h` / `help`: print [`help_text`] and exit.
    Help,
}

/// Error returned by [`parse`] for arguments termbay does not understand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    message: String,
}

impl CliError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Exit code the process should use for this error (usage error).
    pub fn exit_code(&self) -> i32 {
        2
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}\nRun `termbay --help` for usage.", self.message)
    }
}

impl std::error::Error for CliError {}

/// Parses the command line arguments, **without** the program name.
///
/// ```
/// use termbay_core::cli::{parse, Command};
/// assert_eq!(parse(Vec::<String>::new()), Ok(Command::Gui));
/// assert_eq!(parse(["--version"]), Ok(Command::Version));
/// ```
pub fn parse<I, T>(args: I) -> Result<Command, CliError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let args: Vec<OsString> = args
        .into_iter()
        .map(Into::into)
        // Older macOS versions pass a process serial number when the app
        // is opened from Finder. It is not meant for us.
        .filter(|arg| !arg.to_string_lossy().starts_with("-psn_"))
        .collect();

    let Some(first) = args.first() else {
        return Ok(Command::Gui);
    };

    let first = first
        .to_str()
        .ok_or_else(|| CliError::new("argument is not valid UTF-8"))?;

    let command = match first {
        "-V" | "--version" => Command::Version,
        "-h" | "--help" | "help" => Command::Help,
        other if other.starts_with('-') => {
            return Err(CliError::new(format!("unknown option `{other}`")));
        }
        other => return Err(CliError::new(format!("unknown command `{other}`"))),
    };

    if let Some(extra) = args.get(1) {
        return Err(CliError::new(format!(
            "unexpected argument `{}`",
            extra.to_string_lossy()
        )));
    }

    Ok(command)
}

/// Text printed by `termbay --version`, without a trailing newline.
pub fn version_text() -> String {
    format!("termbay {}", crate::VERSION)
}

/// Text printed by `termbay --help`, without a trailing newline.
pub fn help_text() -> String {
    format!(
        "termbay {version}
A lightweight open source terminal with a 2D office for Claude agents.

Usage:
  termbay              Open the terminal window
  termbay --version    Print the version and exit
  termbay --help       Print this help and exit

Options:
  -h, --help       Print help
  -V, --version    Print version",
        version = crate::VERSION
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_arguments_opens_the_app() {
        assert_eq!(parse(Vec::<String>::new()), Ok(Command::Gui));
    }

    #[test]
    fn version_flags() {
        assert_eq!(parse(["--version"]), Ok(Command::Version));
        assert_eq!(parse(["-V"]), Ok(Command::Version));
    }

    #[test]
    fn help_flags() {
        assert_eq!(parse(["--help"]), Ok(Command::Help));
        assert_eq!(parse(["-h"]), Ok(Command::Help));
        assert_eq!(parse(["help"]), Ok(Command::Help));
    }

    #[test]
    fn macos_process_serial_number_is_ignored() {
        assert_eq!(parse(["-psn_0_12345"]), Ok(Command::Gui));
    }

    #[test]
    fn unknown_option_is_an_error() {
        let err = parse(["--nope"]).unwrap_err();
        assert!(err.to_string().contains("unknown option `--nope`"));
        assert!(err.to_string().contains("termbay --help"));
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn unknown_command_is_an_error() {
        let err = parse(["frobnicate"]).unwrap_err();
        assert!(err.to_string().contains("unknown command `frobnicate`"));
    }

    #[test]
    fn extra_arguments_are_an_error() {
        let err = parse(["--version", "extra"]).unwrap_err();
        assert!(err.to_string().contains("unexpected argument `extra`"));
    }

    #[test]
    fn version_text_uses_cargo_version() {
        assert_eq!(
            version_text(),
            format!("termbay {}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn help_mentions_every_option() {
        let help = help_text();
        assert!(help.contains("--version"));
        assert!(help.contains("--help"));
    }
}
