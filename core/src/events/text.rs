//! Text helpers shared by the reducer and the server: everything that ends up
//! in a speech bubble goes through [`clean`] (decision E10 in
//! `docs/FASE2-PLANO.md`).

/// Longest text a bubble may show, in characters.
pub const MAX_TEXT_CHARS: usize = 80;

/// Longest id (session, agent, tool use, pane, task), in characters. Ids are
/// never shown, only matched, but a local client could still send anything.
pub const MAX_ID_CHARS: usize = 128;

/// Removes control characters (including escape sequences' ESC and the C1
/// range), collapses whitespace runs into one space, trims, and cuts to
/// `max` characters, adding `…` when something was cut.
pub fn clean(text: &str, max: usize) -> String {
    let mut out = String::new();
    let mut count = 0;
    let mut pending_space = false;
    let mut cut = false;
    for c in text.chars() {
        if c.is_whitespace() || c.is_control() {
            pending_space = !out.is_empty();
            continue;
        }
        let needed = usize::from(pending_space) + 1;
        if count + needed > max {
            cut = true;
            break;
        }
        if pending_space {
            out.push(' ');
            count += 1;
            pending_space = false;
        }
        out.push(c);
        count += 1;
    }
    if cut && max > 0 {
        // Make room for the ellipsis so the result stays within `max`.
        if count == max {
            out.pop();
        }
        out.push('…');
    }
    out
}

/// Last component of a path, with `/` or `\` as separators, so a bubble
/// never shows directories (which carry the user's name).
pub fn file_name(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['/', '\\']);
    trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed)
}

/// Name of the program a shell command runs, without its arguments: the
/// first word that is not an environment assignment (`TOKEN=... cmd` would
/// otherwise leak the value), reduced to its file name.
pub fn program(command: &str) -> Option<&str> {
    command
        .split_whitespace()
        .find(|word| !word.contains('='))
        .map(file_name)
        .filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_strips_control_characters_and_collapses_spaces() {
        assert_eq!(clean("  lendo\tapi.ts\r\n", 80), "lendo api.ts");
        assert_eq!(clean("\u{1b}[31mred\u{1b}[0m", 80), "[31mred [0m");
        assert_eq!(clean("a\u{9b}b", 80), "a b");
        assert_eq!(clean("", 80), "");
        assert_eq!(clean("\n\n", 80), "");
    }

    #[test]
    fn clean_cuts_on_characters_and_marks_the_cut() {
        assert_eq!(clean("abcdef", 6), "abcdef");
        assert_eq!(clean("abcdefg", 6), "abcde…");
        assert_eq!(clean("ção ção ção", 5), "ção …");
        assert_eq!(
            clean(&"x".repeat(500), MAX_TEXT_CHARS).chars().count(),
            MAX_TEXT_CHARS
        );
        assert_eq!(clean("abc", 0), "");
    }

    #[test]
    fn file_name_drops_directories_on_every_platform() {
        assert_eq!(file_name("/home/user/projeto/src/api.ts"), "api.ts");
        assert_eq!(file_name(r"C:\Users\user\projeto\api.ts"), "api.ts");
        assert_eq!(file_name("api.ts"), "api.ts");
        assert_eq!(file_name("/home/user/projeto/"), "projeto");
        assert_eq!(file_name(""), "");
    }

    #[test]
    fn program_skips_assignments_and_arguments() {
        assert_eq!(program("npm test -- --watch"), Some("npm"));
        assert_eq!(
            program("TOKEN=s3cret API_KEY=x curl -H 'a: b' https://x"),
            Some("curl")
        );
        assert_eq!(program("/usr/local/bin/cargo build"), Some("cargo"));
        assert_eq!(program(r"C:\tools\rg.exe foo"), Some("rg.exe"));
        assert_eq!(program("   "), None);
        assert_eq!(program("A=1"), None);
    }
}
