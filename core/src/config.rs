//! User configuration: `~/.termbay/config.toml`, or the file named by
//! `TERMBAY_CONFIG` (decision D8).
//!
//! For now only the `[autocomplete]` section is read (decision D15: off by
//! default). T14 extends this module with the rest of the configuration.
//!
//! A missing file is the default configuration. An unreadable or invalid
//! file never stops termbay from opening: the defaults are used and a
//! warning is returned for the caller to log. Unknown sections and keys are
//! ignored, so older versions keep reading newer files.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Environment variable that points to another configuration file.
pub const CONFIG_ENV: &str = "TERMBAY_CONFIG";

/// Files larger than this are not read (a configuration is a few lines).
const MAX_CONFIG_SIZE: u64 = 1024 * 1024;

/// The whole configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Config {
    /// `[autocomplete]`.
    pub autocomplete: AutocompleteConfig,
}

/// `[autocomplete]`: the Warp-style suggestion menu (`docs/AUTOCOMPLETE.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct AutocompleteConfig {
    /// Injects the shell integration and enables the menu. Off by default
    /// in milestone A (D15). Applies to tabs opened after the change.
    pub enabled: bool,
    /// Key that opens the menu.
    pub trigger: Trigger,
    /// Includes the shell's history file in the suggestions.
    pub history: bool,
}

impl Default for AutocompleteConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            trigger: Trigger::Tab,
            history: true,
        }
    }
}

/// Key that opens the autocomplete menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Trigger {
    /// `trigger = "tab"`.
    #[serde(rename = "tab")]
    Tab,
    /// `trigger = "ctrl+space"`, for people who keep the shell's own Tab.
    #[serde(rename = "ctrl+space")]
    CtrlSpace,
}

/// Result of [`load`]: the configuration to use and what went wrong, if
/// anything.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Loaded {
    /// Configuration to use (defaults where the file was missing or invalid).
    pub config: Config,
    /// File that was looked at, when one could be determined.
    pub path: Option<PathBuf>,
    /// Readable problems, for the log. Never fatal.
    pub warnings: Vec<String>,
}

/// Path of the configuration file: `$TERMBAY_CONFIG` when set and not
/// empty, otherwise `~/.termbay/config.toml`. `None` when the home folder
/// is unknown.
pub fn config_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(CONFIG_ENV).filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(path));
    }
    home_dir().map(|home| home.join(".termbay").join("config.toml"))
}

/// Loads the user's configuration (see [`config_path`]).
pub fn load() -> Loaded {
    match config_path() {
        Some(path) => load_from(&path),
        None => Loaded {
            warnings: vec![
                "could not find the home folder; using the default configuration".into(),
            ],
            ..Loaded::default()
        },
    }
}

/// Loads a configuration file. A missing file is not a problem.
pub fn load_from(path: &Path) -> Loaded {
    let mut loaded = Loaded {
        path: Some(path.to_path_buf()),
        ..Loaded::default()
    };
    match read_limited(path) {
        Ok(text) => {
            let (config, warnings) = parse(&text);
            loaded.config = config;
            loaded.warnings.extend(
                warnings
                    .into_iter()
                    .map(|w| format!("{}: {w}", path.display())),
            );
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => loaded
            .warnings
            .push(format!("{}: could not read: {err}", path.display())),
    }
    loaded
}

/// Parses the text of a configuration file. Each section that cannot be
/// understood falls back to its defaults, with a warning.
pub fn parse(text: &str) -> (Config, Vec<String>) {
    let mut config = Config::default();
    let mut warnings = Vec::new();
    let table = match text.parse::<toml::Table>() {
        Ok(table) => table,
        Err(err) => {
            warnings.push(format!(
                "invalid TOML, using the defaults: {}",
                err.message()
            ));
            return (config, warnings);
        }
    };
    if let Some(section) = table.get("autocomplete") {
        match section.clone().try_into::<AutocompleteConfig>() {
            Ok(autocomplete) => config.autocomplete = autocomplete,
            Err(err) => warnings.push(format!(
                "invalid [autocomplete] section, using its defaults: {}",
                err.message()
            )),
        }
    }
    (config, warnings)
}

fn read_limited(path: &Path) -> io::Result<String> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::other("not a file"));
    }
    if metadata.len() > MAX_CONFIG_SIZE {
        return Err(io::Error::other("larger than 1 MiB"));
    }
    let bytes = fs::read(path)?;
    String::from_utf8(bytes).map_err(|_| io::Error::other("not valid UTF-8"))
}

/// The user's home folder: `$HOME` on Unix, `%USERPROFILE%` on Windows.
pub(crate) fn home_dir() -> Option<PathBuf> {
    let name = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_off_tab_and_history() {
        let config = Config::default();
        assert!(!config.autocomplete.enabled);
        assert_eq!(config.autocomplete.trigger, Trigger::Tab);
        assert!(config.autocomplete.history);
        assert_eq!(parse(""), (Config::default(), Vec::new()));
    }

    #[test]
    fn reads_the_autocomplete_section() {
        let (config, warnings) =
            parse("[autocomplete]\nenabled = true\ntrigger = \"ctrl+space\"\nhistory = false\n");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            config.autocomplete,
            AutocompleteConfig {
                enabled: true,
                trigger: Trigger::CtrlSpace,
                history: false,
            }
        );
    }

    #[test]
    fn missing_keys_keep_their_defaults() {
        let (config, warnings) = parse("[autocomplete]\nenabled = true\n");
        assert!(warnings.is_empty());
        assert!(config.autocomplete.enabled);
        assert!(config.autocomplete.history);
        assert_eq!(config.autocomplete.trigger, Trigger::Tab);
    }

    #[test]
    fn unknown_sections_and_keys_are_ignored() {
        let (config, warnings) = parse(
            "[theme]\nname = \"dark\"\n[autocomplete]\nenabled = true\nfuture = 3\n[[profiles]]\nx = 1\n",
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(config.autocomplete.enabled);
    }

    #[test]
    fn invalid_toml_falls_back_to_defaults_with_a_warning() {
        let (config, warnings) = parse("[autocomplete\nenabled = true");
        assert_eq!(config, Config::default());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("invalid TOML"));
    }

    #[test]
    fn an_invalid_section_falls_back_to_its_defaults() {
        for text in [
            "[autocomplete]\nenabled = \"yes\"\n",
            "[autocomplete]\ntrigger = \"f1\"\n",
            "autocomplete = 3\n",
        ] {
            let (config, warnings) = parse(text);
            assert_eq!(config, Config::default(), "{text}");
            assert_eq!(warnings.len(), 1, "{text}");
            assert!(warnings[0].contains("[autocomplete]"), "{text}");
        }
    }

    #[test]
    fn files_missing_unreadable_or_too_big_never_fail() {
        let dir = std::env::temp_dir().join(format!("termbay-config-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        let missing = load_from(&dir.join("nope.toml"));
        assert_eq!(missing.config, Config::default());
        assert!(missing.warnings.is_empty());

        let file = dir.join("config.toml");
        fs::write(&file, "[autocomplete]\r\nenabled = true\r\n").unwrap();
        let ok = load_from(&file);
        assert!(ok.config.autocomplete.enabled, "CRLF files are fine");
        assert!(ok.warnings.is_empty());

        fs::write(&file, [0xff, 0xfe, b'x']).unwrap();
        let bad = load_from(&file);
        assert_eq!(bad.config, Config::default());
        assert_eq!(bad.warnings.len(), 1);

        let folder = load_from(&dir);
        assert_eq!(folder.config, Config::default());
        assert_eq!(folder.warnings.len(), 1);

        fs::write(&file, vec![b'#'; (MAX_CONFIG_SIZE + 1) as usize]).unwrap();
        let big = load_from(&file);
        assert_eq!(big.config, Config::default());
        assert!(big.warnings[0].contains("1 MiB"));

        fs::remove_dir_all(&dir).unwrap();
    }
}
