//! Completion engine of the autocomplete menu (`docs/AUTOCOMPLETE.md`).
//!
//! The shell answers a completion request with its own candidates (see
//! [`crate::shell_integration`]). [`build`] joins them with matching lines
//! of the shell's history, removes duplicates and control characters,
//! orders them and computes, for each one, the exact text the UI types to
//! insert it: the **insertion plan**.
//!
//! # Insertion plan
//!
//! `insert` is relative to the line the shell saw, with the cursor at
//! `cursor` and the current word starting at `replace_from`:
//!
//! - when the candidate starts with the typed word, `insert` is only the
//!   missing suffix (`git ch` + `checkout` -> `eckout`);
//! - otherwise it is one DEL (U+007F) per character of the typed word,
//!   followed by the whole candidate.
//!
//! Apart from those leading DELs, `insert` never contains a control
//! character, so accepting a suggestion can only insert text: it never
//! presses Enter. History items always are a suffix.
//!
//! Positions (`cursor`, `replace_from`) count Unicode characters (code
//! points), not bytes.

pub mod history;

pub use history::{History, HistoryCache};

/// Most shell candidates kept. The UI filters this list locally while the
/// user types, so it gets everything the shell sent (the widgets stop at
/// the same number).
pub const MAX_SHELL_ITEMS: usize = 500;
/// Most history lines kept.
pub const MAX_HISTORY_ITEMS: usize = 50;

/// The line editor's mode when the request ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditMode {
    /// Typing inserts text (emacs keys, vi insert mode).
    Insert,
    /// vi command mode: typed text would run editor commands, so nothing
    /// may be inserted.
    Command,
}

/// One candidate as the shell sent it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawCandidate {
    /// What to show (unquoted). Empty means "same as `insert`".
    pub text: String,
    /// The whole word as the shell would insert it, quoted as needed.
    pub insert: String,
    /// Description, possibly empty.
    pub description: String,
}

/// The shell's answer to a completion request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionReply {
    /// Request number, as written by the core in the trigger.
    pub request: u64,
    /// Editor mode at the time of the request.
    pub mode: EditMode,
    /// The whole command line.
    pub line: String,
    /// Cursor position in `line`, in characters.
    pub cursor: usize,
    /// Start of the word being completed, in characters.
    pub replace_from: usize,
    /// Candidates, in the shell's order.
    pub items: Vec<RawCandidate>,
}

/// Where an item comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// The shell's own completion system.
    Shell,
    /// A line of the shell's history file.
    History,
}

/// One suggestion, ready for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    /// What to show.
    pub text: String,
    /// Insertion plan (see the module documentation).
    pub insert: String,
    /// Description, when there is one.
    pub description: Option<String>,
    /// Origin.
    pub kind: ItemKind,
}

/// The suggestions for one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completions {
    /// The command line the shell saw (control characters removed).
    pub line: String,
    /// Cursor position, in characters of `line`.
    pub cursor: usize,
    /// Start of the word being completed, in characters of `line`.
    pub replace_from: usize,
    /// Shell items first, then history items.
    pub items: Vec<CompletionItem>,
}

/// Joins the shell's candidates and the history into the final list.
pub fn build(reply: &CompletionReply, history: Option<&History>) -> Completions {
    let raw_line: Vec<char> = reply.line.chars().collect();
    let cursor = reply.cursor.min(raw_line.len());
    let replace_from = reply.replace_from.min(cursor);
    let before: String = raw_line[..cursor].iter().collect();
    let word: String = raw_line[replace_from..cursor].iter().collect();

    // Positions refer to the line as the shell saw it. Removing control
    // characters would shift them, so a line with any (a multi-line
    // command, for example) keeps them out of the UI by blanking them.
    let line: String = raw_line
        .iter()
        .map(|&c| if is_unsafe(c) { ' ' } else { c })
        .collect();

    let mut seen = std::collections::HashSet::new();
    let mut prefix = Vec::new();
    let mut loose = Vec::new();
    for raw in reply.items.iter().take(MAX_SHELL_ITEMS) {
        let candidate = raw.insert.as_str();
        // A word that needs a control character to be typed is never
        // inserted: without it, it would name something else.
        if candidate.is_empty() || candidate.chars().any(is_unsafe) {
            continue;
        }
        if !seen.insert(candidate.to_owned()) {
            continue;
        }
        let shown = if raw.text.is_empty() {
            candidate
        } else {
            raw.text.as_str()
        };
        let item = CompletionItem {
            text: clean_display(shown),
            insert: insertion_plan(&word, candidate),
            description: Some(clean_display(&raw.description)).filter(|d| !d.is_empty()),
            kind: ItemKind::Shell,
        };
        if candidate.starts_with(word.as_str()) {
            prefix.push(item);
        } else {
            loose.push(item);
        }
    }
    let mut items = prefix;
    items.append(&mut loose);

    // Whole lines only make sense with the cursor at the end of the line.
    if let Some(history) = history
        && cursor == raw_line.len()
        && !before.trim().is_empty()
        && !before.chars().any(is_unsafe)
    {
        for entry in history.matching(&before, MAX_HISTORY_ITEMS) {
            items.push(CompletionItem {
                text: entry.to_owned(),
                insert: entry[before.len()..].to_owned(),
                description: None,
                kind: ItemKind::History,
            });
        }
    }

    Completions {
        line,
        cursor,
        replace_from,
        items,
    }
}

/// The text the UI types to turn `word` into `candidate`: the suffix when
/// `candidate` starts with `word`, otherwise one DEL per character of
/// `word` followed by `candidate`.
pub fn insertion_plan(word: &str, candidate: &str) -> String {
    match candidate.strip_prefix(word) {
        Some(suffix) => suffix.to_owned(),
        None => {
            let mut plan = "\u{7f}".repeat(word.chars().count());
            plan.push_str(candidate);
            plan
        }
    }
}

/// Characters that must never reach the command line or the menu as they
/// are: C0 and C1 controls, DEL, and the bidirectional overrides that can
/// make text look different from what it is.
pub fn is_unsafe(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
        )
}

/// Removes unsafe characters. Used for anything shown in the menu; a tab
/// or a line break becomes a space so words stay apart.
pub fn clean_display(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\t' | '\n' | '\r') {
            out.push(' ');
        } else if !is_unsafe(c) {
            out.push(c);
        }
    }
    out.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(text: &str, insert: &str, description: &str) -> RawCandidate {
        RawCandidate {
            text: text.into(),
            insert: insert.into(),
            description: description.into(),
        }
    }

    fn reply(line: &str, cursor: usize, from: usize, items: Vec<RawCandidate>) -> CompletionReply {
        CompletionReply {
            request: 1,
            mode: EditMode::Insert,
            line: line.into(),
            cursor,
            replace_from: from,
            items,
        }
    }

    #[test]
    fn plan_is_the_suffix_or_dels_and_the_text() {
        assert_eq!(insertion_plan("ch", "checkout"), "eckout");
        assert_eq!(insertion_plan("", "main"), "main");
        assert_eq!(insertion_plan("main", "main"), "");
        assert_eq!(
            insertion_plan("\"my fi", "my\\ file"),
            "\u{7f}".repeat(6) + "my\\ file"
        );
        // Characters, not bytes.
        assert_eq!(insertion_plan("ação", "Ação"), "\u{7f}".repeat(4) + "Ação");
    }

    #[test]
    fn shell_items_are_ordered_prefix_first_and_deduplicated() {
        let r = reply(
            "git ch",
            6,
            4,
            vec![
                raw("Checkout", "Checkout", ""),
                raw("checkout", "checkout", "Switch branches"),
                raw("cherry-pick", "cherry-pick", "Apply a commit"),
                raw("checkout", "checkout", "duplicate"),
            ],
        );
        let c = build(&r, None);
        assert_eq!(
            (c.line.as_str(), c.cursor, c.replace_from),
            ("git ch", 6, 4)
        );
        let got: Vec<_> = c
            .items
            .iter()
            .map(|i| (i.text.as_str(), i.insert.as_str(), i.description.as_deref()))
            .collect();
        assert_eq!(
            got,
            [
                ("checkout", "eckout", Some("Switch branches")),
                ("cherry-pick", "erry-pick", Some("Apply a commit")),
                ("Checkout", "\u{7f}\u{7f}Checkout", None),
            ]
        );
        assert!(c.items.iter().all(|i| i.kind == ItemKind::Shell));
    }

    #[test]
    fn control_characters_never_reach_the_ui() {
        let r = reply(
            "ls fix",
            6,
            3,
            vec![
                // Quoted by the shell: safe to insert, shown without controls.
                raw("fix\nnl", "fix\\nnl", "line\nbreak"),
                raw("fix\x1b[31mred", "fix\\e[31mred", "\x1b]0;title\x07"),
                // Needs a raw control character: dropped.
                raw("bad", "fix\rrm -rf ~\r", ""),
                raw("bad", "fix\x1b", ""),
                raw("bad", "fix\u{202e}txt.exe", ""),
                raw("", "", ""),
            ],
        );
        let c = build(&r, None);
        assert_eq!(c.items.len(), 2);
        for item in &c.items {
            for s in [&item.text, &item.insert] {
                assert!(!s.chars().any(is_unsafe), "{s:?}");
            }
            if let Some(d) = &item.description {
                assert!(!d.chars().any(is_unsafe), "{d:?}");
            }
        }
        assert_eq!(c.items[0].text, "fix nl");
        assert_eq!(c.items[0].insert, "\\nnl");
        assert_eq!(c.items[0].description.as_deref(), Some("line break"));
        assert_eq!(c.items[1].text, "fix[31mred");
        assert_eq!(c.items[1].description.as_deref(), Some("]0;title"));
    }

    #[test]
    fn line_controls_are_blanked_without_moving_positions() {
        let r = reply("echo a\nls fi", 12, 10, vec![raw("fix", "fix", "")]);
        let c = build(&r, None);
        assert_eq!(c.line, "echo a ls fi");
        assert_eq!(c.items[0].insert, "x");
    }

    #[test]
    fn positions_are_clamped() {
        let r = reply("ls", 99, 120, vec![raw("lsof", "lsof", "")]);
        let c = build(&r, None);
        assert_eq!((c.cursor, c.replace_from), (2, 2));
        assert_eq!(c.items[0].insert, "lsof");
    }

    #[test]
    fn at_most_500_shell_items() {
        let items = (0..800).map(|i| raw("", &format!("f{i}"), "")).collect();
        let c = build(&reply("ls f", 4, 3, items), None);
        assert_eq!(c.items.len(), MAX_SHELL_ITEMS);
    }

    #[test]
    fn history_items_complete_the_whole_line_as_a_suffix() {
        let history = History::from_commands(
            [
                "git checkout main",
                "git commit",
                "git checkout main",
                "ls",
                "git ch\x1bx",
            ]
            .map(String::from)
            .to_vec(),
        );
        let r = reply("git ch", 6, 4, vec![raw("", "checkout", "")]);
        let c = build(&r, Some(&history));
        let h: Vec<_> = c
            .items
            .iter()
            .filter(|i| i.kind == ItemKind::History)
            .map(|i| (i.text.as_str(), i.insert.as_str()))
            .collect();
        assert_eq!(h, [("git checkout main", "eckout main")]);
        assert_eq!(c.items[0].kind, ItemKind::Shell);

        // Not with the cursor inside the line, nor with nothing typed.
        let r = reply("git ch x", 6, 4, vec![]);
        assert!(build(&r, Some(&history)).items.is_empty());
        let r = reply("  ", 2, 2, vec![]);
        assert!(build(&r, Some(&history)).items.is_empty());
    }

    #[test]
    fn display_cleaning() {
        assert_eq!(clean_display("  a\tb\u{9b}c\u{202e}d\r\n"), "a bcd");
        assert!(is_unsafe('\u{7f}'));
        assert!(is_unsafe('\u{85}'));
        assert!(!is_unsafe('é'));
    }
}
