//! Read-only access to the shell's own history file.
//!
//! termbay never writes history and never keeps it anywhere but in memory.
//! Only the end of the file is read (at most [`MAX_READ`] bytes, the last
//! [`MAX_ENTRIES`] commands), and a file is read again only when its size
//! or modification time changed.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

/// Most commands kept per file.
pub const MAX_ENTRIES: usize = 10_000;
/// Most bytes read from the end of a history file.
pub const MAX_READ: u64 = 2 * 1024 * 1024;

/// Format of a history file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HistoryFormat {
    /// `~/.local/share/fish/fish_history` (YAML-like, `- cmd: ...`).
    Fish,
    /// `$HISTFILE` of zsh, plain or `EXTENDED_HISTORY`, metafied.
    Zsh,
}

impl HistoryFormat {
    /// Extracts the commands of a history file, oldest first.
    pub fn parse(self, bytes: &[u8]) -> Vec<String> {
        match self {
            Self::Fish => parse_fish(bytes),
            Self::Zsh => parse_zsh(bytes),
        }
    }
}

/// Distinct commands with how often and how recently they were used.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    /// Most relevant first: more uses, then more recent.
    entries: Vec<String>,
}

impl History {
    /// Builds the index from commands, oldest first. Only the last
    /// [`MAX_ENTRIES`] count; commands with control characters (multi-line
    /// commands, for example) are left out, since they could not be
    /// inserted as they are.
    pub fn from_commands(commands: Vec<String>) -> Self {
        let skip = commands.len().saturating_sub(MAX_ENTRIES);
        let mut stats: HashMap<String, (u32, usize)> = HashMap::new();
        for (index, command) in commands.into_iter().skip(skip).enumerate() {
            if command.trim().is_empty() || command.chars().any(super::is_unsafe) {
                continue;
            }
            let entry = stats.entry(command).or_insert((0, 0));
            entry.0 += 1;
            entry.1 = index;
        }
        let mut ranked: Vec<(String, (u32, usize))> = stats.into_iter().collect();
        ranked.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(b.1.1.cmp(&a.1.1)));
        Self {
            entries: ranked.into_iter().map(|(command, _)| command).collect(),
        }
    }

    /// Commands that start with `prefix` and are longer than it, most
    /// relevant first.
    pub fn matching(&self, prefix: &str, limit: usize) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|entry| entry.len() > prefix.len() && entry.starts_with(prefix))
            .take(limit)
            .map(String::as_str)
            .collect()
    }

    /// Number of distinct commands.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there is no command.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug)]
struct Cached {
    modified: Option<SystemTime>,
    len: u64,
    history: Arc<History>,
}

/// History files already read, shared by every tab of the same shell.
#[derive(Debug, Default)]
pub struct HistoryCache {
    files: Mutex<HashMap<(HistoryFormat, PathBuf), Cached>>,
}

impl HistoryCache {
    /// Creates an empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// The history in `path`, read again only if the file changed. `None`
    /// when the file cannot be read (missing, not a regular file...).
    pub fn get(&self, format: HistoryFormat, path: &Path) -> Option<Arc<History>> {
        let metadata = std::fs::metadata(path).ok().filter(|m| m.is_file())?;
        let modified = metadata.modified().ok();
        let len = metadata.len();
        let key = (format, path.to_path_buf());
        {
            let files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(cached) = files.get(&key)
                && cached.modified == modified
                && cached.len == len
            {
                return Some(Arc::clone(&cached.history));
            }
        }
        // Read without holding the lock; a concurrent read of the same file
        // only costs a second read.
        let bytes = read_tail(path, MAX_READ).ok()?;
        let history = Arc::new(History::from_commands(format.parse(&bytes)));
        let mut files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        files.insert(
            key,
            Cached {
                modified,
                len,
                history: Arc::clone(&history),
            },
        );
        Some(history)
    }
}

/// Reads at most `max` bytes from the end of the file. When the file is
/// longer, the first (partial) line is dropped.
fn read_tail(path: &Path, max: u64) -> io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(max);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity((len - start) as usize);
    file.take(max).read_to_end(&mut bytes)?;
    if start > 0 {
        match memchr::memchr(b'\n', &bytes) {
            Some(newline) => {
                bytes.drain(..=newline);
            }
            None => bytes.clear(),
        }
    }
    Ok(bytes)
}

/// fish: `- cmd: <command>` lines, with `\\` for a backslash and `\n` for a
/// line break. The other lines (`when:`, `paths:`) are ignored.
pub fn parse_fish(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    text.lines()
        .filter_map(|line| line.strip_prefix("- cmd: "))
        .map(|escaped| {
            let mut out = String::with_capacity(escaped.len());
            let mut chars = escaped.chars();
            while let Some(c) = chars.next() {
                if c != '\\' {
                    out.push(c);
                    continue;
                }
                match chars.next() {
                    Some('\\') => out.push('\\'),
                    Some('n') => out.push('\n'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push('\\'),
                }
            }
            out
        })
        .collect()
}

/// zsh: one command per line, optionally prefixed by `: <start>:<elapsed>;`
/// (`EXTENDED_HISTORY`). A line ending in a backslash continues on the next
/// one. Bytes are "metafied": 0x83 followed by `b ^ 0x20` stands for `b`.
pub fn parse_zsh(bytes: &[u8]) -> Vec<String> {
    const META: u8 = 0x83;
    let mut plain = Vec::with_capacity(bytes.len());
    let mut iter = bytes.iter();
    while let Some(&b) = iter.next() {
        if b == META {
            if let Some(&next) = iter.next() {
                plain.push(next ^ 0x20);
            }
        } else {
            plain.push(b);
        }
    }

    let mut commands = Vec::new();
    let mut current: Vec<u8> = Vec::new();
    let mut continuing = false;
    for line in plain.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if continuing {
            current.push(b'\n');
            current.extend_from_slice(line);
        } else {
            current = strip_extended(line).to_vec();
        }
        // An odd number of trailing backslashes escapes the line break.
        let trailing = current.iter().rev().take_while(|&&b| b == b'\\').count();
        if trailing % 2 == 1 {
            current.pop();
            continuing = true;
            continue;
        }
        continuing = false;
        if !current.is_empty() {
            commands.push(String::from_utf8_lossy(&current).into_owned());
        }
        current.clear();
    }
    if !current.is_empty() {
        commands.push(String::from_utf8_lossy(&current).into_owned());
    }
    commands
}

/// Removes `: 1700000000:0;` from the start of a zsh history line.
fn strip_extended(line: &[u8]) -> &[u8] {
    let Some(rest) = line.strip_prefix(b": ") else {
        return line;
    };
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || rest.get(digits) != Some(&b':') {
        return line;
    }
    let rest = &rest[digits + 1..];
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || rest.get(digits) != Some(&b';') {
        return line;
    }
    &rest[digits + 1..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fish_history() {
        let file = b"- cmd: git status\n  when: 1700000000\n- cmd: echo a\\\\b\n  when: 1700000001\n  paths:\n    - foo\n- cmd: printf x\\ny\n- cmd: caf\xc3\xa9\n";
        assert_eq!(
            parse_fish(file),
            ["git status", "echo a\\b", "printf x\ny", "café"]
        );
    }

    #[test]
    fn zsh_plain_extended_multiline_and_metafied() {
        let mut file =
            b": 1700000000:0;git status\nls -la\n: 1700000002:3;echo one\\\ntwo\n".to_vec();
        // A lone byte 0x83 is stored as 83 a3 (not valid UTF-8 on its own).
        file.extend_from_slice(b"echo \x83\xa3x\n");
        file.extend_from_slice(b"echo back\\\\\n: notextended\n");
        assert_eq!(
            parse_zsh(&file),
            [
                "git status",
                "ls -la",
                "echo one\ntwo",
                "echo \u{fffd}x",
                "echo back\\\\",
                ": notextended",
            ]
        );
        // Metafied UTF-8: € is e2 82 ac, and 0x82 is stored as 83 a2.
        assert_eq!(parse_zsh(b"\xe2\x83\xa2\xac\n"), ["€"]);
    }

    #[test]
    fn ranking_by_use_then_recency() {
        let history = History::from_commands(
            [
                "ls",
                "git push",
                "git pull",
                "git push",
                "git status",
                "git pull",
                "git push",
            ]
            .map(String::from)
            .to_vec(),
        );
        assert_eq!(
            history.matching("git ", 10),
            ["git push", "git pull", "git status"]
        );
        assert_eq!(history.matching("git ", 1), ["git push"]);
        assert_eq!(history.matching("git push", 10), Vec::<&str>::new());
        assert_eq!(history.len(), 4);
    }

    #[test]
    fn multi_line_and_control_commands_are_left_out() {
        let history = History::from_commands(
            ["echo one\ntwo", "echo \x1b[2J", "echo ok", "   "]
                .map(String::from)
                .to_vec(),
        );
        assert_eq!(history.matching("echo", 10), ["echo ok"]);
    }

    #[test]
    fn only_the_last_entries_count() {
        let commands: Vec<String> = (0..MAX_ENTRIES + 5).map(|i| format!("cmd {i}")).collect();
        let history = History::from_commands(commands);
        assert_eq!(history.len(), MAX_ENTRIES);
        assert!(
            history
                .matching("cmd 0", 10_000)
                .iter()
                .all(|c| *c != "cmd 0")
        );
    }

    #[test]
    fn cache_reads_the_tail_and_rereads_on_change() {
        let dir = std::env::temp_dir().join(format!("termbay-history-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("zsh_history");
        std::fs::write(&path, "git status\n").unwrap();

        let cache = HistoryCache::new();
        let first = cache.get(HistoryFormat::Zsh, &path).unwrap();
        assert_eq!(first.matching("git", 5), ["git status"]);
        let again = cache.get(HistoryFormat::Zsh, &path).unwrap();
        assert!(
            Arc::ptr_eq(&first, &again),
            "unchanged files are not read again"
        );

        std::fs::write(&path, "git status\ngit stash\n").unwrap();
        let changed = cache.get(HistoryFormat::Zsh, &path).unwrap();
        assert_eq!(changed.matching("git st", 5).len(), 2);

        assert!(
            cache
                .get(HistoryFormat::Zsh, &dir.join("missing"))
                .is_none()
        );
        assert!(cache.get(HistoryFormat::Zsh, &dir).is_none());

        // Only the end of a big file is read; the cut line is dropped.
        let big = dir.join("big");
        let mut text = String::from("first-line-that-is-cut\n");
        text.push_str(&"x".repeat(100));
        text.push_str("\nlast\n");
        std::fs::write(&big, &text).unwrap();
        let tail = read_tail(&big, 60).unwrap();
        assert_eq!(tail, b"last\n");

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
