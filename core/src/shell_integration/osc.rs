//! Finds termbay's private OSC sequences in the pty output.
//!
//! [`OscScanner`] is a pure state machine (no clock, no I/O), like the
//! output batcher. It passes every byte through, **in the same order and
//! unchanged**, except the private sequences `ESC ] 6973 ; ... BEL` (or
//! `... ESC \`), which it removes. Standard sequences, OSC 133 and OSC 7
//! included, go on to xterm.js.
//!
//! A private sequence becomes a [`ShellMark`] only when its first field is
//! the session nonce, which the integration script took out of the
//! environment at startup. Sequences forged by other programs (`cat` of a
//! file, a remote shell) are removed and ignored.
//!
//! # Wire format (protocol 1)
//!
//! ```text
//! ESC ] 6973 ; <nonce> ; <kind> [; <field>]... BEL
//!
//! ready;<protocol>;<shell>;<shell version>;<history file>
//! prompt;<exit code of the last command, or empty>
//! exec
//! complete;<request>;<insert|command>;<line>;<cursor>;<from>[;<text>;<insert>;<description>]...
//! ```
//!
//! Fields are escaped in the style of VS Code's OSC 633: `\\` is a
//! backslash and `\xHH` is the byte `HH` (the scripts use it for `;` and
//! for every byte below 0x20). The decoded bytes are read as UTF-8.

use crate::complete::{CompletionReply, EditMode, RawCandidate};

use super::Nonce;

/// Number of termbay's private OSC.
pub const PRIVATE_OSC: &str = "6973";

/// Longest private sequence accepted (1 MiB). Longer ones are dropped.
pub const MAX_SEQUENCE: usize = 1024 * 1024;

const PREFIX: &[u8] = b"\x1b]6973;";
const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;

/// What the shell reports through the private OSC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellMark {
    /// Handshake, sent at the first prompt.
    Ready(ShellInfo),
    /// The shell is about to show its prompt and wait for a command.
    Prompt {
        /// Exit code of the command that just ended, when one ran.
        exit_code: Option<i32>,
    },
    /// A command line was accepted and is about to run.
    Exec,
    /// Answer to a completion request.
    Completion(CompletionReply),
}

/// The handshake of an integrated shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellInfo {
    /// Protocol version of the script.
    pub protocol: u32,
    /// Shell name (`fish`, `zsh`).
    pub shell: String,
    /// Shell version.
    pub version: String,
    /// Absolute path of the shell's history file, when it keeps one.
    pub history_file: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Plain output.
    Ground,
    /// Matched the first `n` bytes of `ESC ] 6973 ;`; they are held back.
    Prefix(usize),
    /// Inside a private sequence.
    Body,
    /// Inside a private sequence, right after an ESC.
    BodyEsc,
    /// Inside a private sequence that grew too long; dropped.
    Discard,
    /// Same, right after an ESC.
    DiscardEsc,
}

/// Removes the private OSC from the output and decodes the marks.
#[derive(Debug)]
pub struct OscScanner {
    nonce: Vec<u8>,
    state: State,
    body: Vec<u8>,
    max: usize,
}

impl OscScanner {
    /// A scanner that trusts sequences carrying `nonce`.
    pub fn new(nonce: &Nonce) -> Self {
        Self::with_limit(nonce, MAX_SEQUENCE)
    }

    /// Same, with another size limit for one sequence (tests).
    pub fn with_limit(nonce: &Nonce, max: usize) -> Self {
        Self {
            nonce: nonce.as_str().as_bytes().to_vec(),
            state: State::Ground,
            body: Vec::new(),
            max: max.max(1),
        }
    }

    /// Bytes held back because they may start a private sequence (at most
    /// the length of `ESC ] 6973 ;`, never visible on their own).
    pub fn held(&self) -> usize {
        match self.state {
            State::Prefix(n) => n,
            _ => 0,
        }
    }

    /// Scans one chunk of output. Bytes to show are appended to `out`,
    /// trusted marks to `marks`. Sequences may be split anywhere between
    /// chunks.
    pub fn feed(&mut self, input: &[u8], out: &mut Vec<u8>, marks: &mut Vec<ShellMark>) {
        let mut i = 0;
        while i < input.len() {
            match self.state {
                State::Ground => match memchr::memchr(ESC, &input[i..]) {
                    None => {
                        out.extend_from_slice(&input[i..]);
                        i = input.len();
                    }
                    Some(offset) => {
                        out.extend_from_slice(&input[i..i + offset]);
                        i += offset + 1;
                        self.state = State::Prefix(1);
                    }
                },
                State::Prefix(n) => {
                    if input[i] == PREFIX[n] {
                        i += 1;
                        if n + 1 == PREFIX.len() {
                            self.body.clear();
                            self.state = State::Body;
                        } else {
                            self.state = State::Prefix(n + 1);
                        }
                    } else {
                        // Not ours: release the held bytes and look at this
                        // byte again from the ground state.
                        out.extend_from_slice(&PREFIX[..n]);
                        self.state = State::Ground;
                    }
                }
                State::Body => {
                    let rest = &input[i..];
                    let end = rest
                        .iter()
                        .position(|&b| matches!(b, BEL | ESC | CAN | SUB));
                    let take = end.unwrap_or(rest.len());
                    if self.body.len() + take > self.max {
                        self.body = Vec::new();
                        self.state = State::Discard;
                        continue;
                    }
                    self.body.extend_from_slice(&rest[..take]);
                    i += take;
                    let Some(_) = end else { continue };
                    let terminator = input[i];
                    i += 1;
                    match terminator {
                        BEL => self.finish(marks),
                        ESC => self.state = State::BodyEsc,
                        // CAN and SUB cancel a sequence.
                        _ => self.abort(),
                    }
                }
                State::BodyEsc => {
                    if input[i] == b'\\' {
                        i += 1;
                        self.finish(marks);
                    } else {
                        // ESC followed by anything else ends the sequence
                        // without dispatching it; the ESC starts a new one.
                        self.abort();
                        self.state = State::Prefix(1);
                    }
                }
                State::Discard => {
                    match input[i..]
                        .iter()
                        .position(|&b| matches!(b, BEL | ESC | CAN | SUB))
                    {
                        None => i = input.len(),
                        Some(offset) => {
                            let terminator = input[i + offset];
                            i += offset + 1;
                            self.state = if terminator == ESC {
                                State::DiscardEsc
                            } else {
                                State::Ground
                            };
                        }
                    }
                }
                State::DiscardEsc => {
                    if input[i] == b'\\' {
                        i += 1;
                        self.state = State::Ground;
                    } else {
                        self.state = State::Prefix(1);
                    }
                }
            }
        }
    }

    fn abort(&mut self) {
        self.body.clear();
        self.state = State::Ground;
    }

    fn finish(&mut self, marks: &mut Vec<ShellMark>) {
        self.state = State::Ground;
        let body = std::mem::take(&mut self.body);
        if let Some(mark) = parse_body(&body, &self.nonce) {
            marks.push(mark);
        }
    }
}

/// Decodes the body of a private sequence (after `6973;`). `None` when the
/// nonce is wrong or the content is not understood.
fn parse_body(body: &[u8], nonce: &[u8]) -> Option<ShellMark> {
    let mut fields = body.split(|&b| b == b';');
    let given = fields.next()?;
    if nonce.is_empty() || !constant_time_eq(given, nonce) {
        return None;
    }
    let kind = fields.next()?;
    let mut fields = fields.map(decode_field);
    match kind {
        b"ready" => {
            let protocol = fields.next()?.parse().ok()?;
            let shell = fields.next()?;
            let version = fields.next().unwrap_or_default();
            let history_file = fields
                .next()
                .map(std::path::PathBuf::from)
                .filter(|path| path.is_absolute());
            Some(ShellMark::Ready(ShellInfo {
                protocol,
                shell,
                version,
                history_file,
            }))
        }
        b"prompt" => Some(ShellMark::Prompt {
            exit_code: fields.next().and_then(|code| code.parse().ok()),
        }),
        b"exec" => Some(ShellMark::Exec),
        b"complete" => {
            let request = fields.next()?.parse().ok()?;
            let mode = match fields.next()?.as_str() {
                "insert" => EditMode::Insert,
                "command" => EditMode::Command,
                _ => return None,
            };
            let line = fields.next()?;
            let cursor = fields.next()?.parse().ok()?;
            let replace_from = fields.next()?.parse().ok()?;
            let rest: Vec<String> = fields.collect();
            let items = rest
                .as_chunks::<3>()
                .0
                .iter()
                .map(|[text, insert, description]| RawCandidate {
                    text: text.clone(),
                    insert: insert.clone(),
                    description: description.clone(),
                })
                .collect();
            Some(ShellMark::Completion(CompletionReply {
                request,
                mode,
                line,
                cursor,
                replace_from,
                items,
            }))
        }
        _ => None,
    }
}

/// Undoes the field escaping: `\\` and `\xHH`. Anything else is kept.
pub fn decode_field(raw: &[u8]) -> String {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'\\' {
            if raw.get(i + 1) == Some(&b'\\') {
                out.push(b'\\');
                i += 2;
                continue;
            }
            if raw.get(i + 1) == Some(&b'x')
                && let (Some(hi), Some(lo)) = (
                    raw.get(i + 2).and_then(|&b| hex(b)),
                    raw.get(i + 3).and_then(|&b| hex(b)),
                )
            {
                out.push(hi << 4 | lo);
                i += 4;
                continue;
            }
        }
        out.push(raw[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Escapes a field the way the scripts do (used by tests and fakes).
pub fn encode_field(text: &str) -> String {
    let mut bytes = Vec::with_capacity(text.len());
    for &b in text.as_bytes() {
        match b {
            b'\\' => bytes.extend_from_slice(b"\\\\"),
            b';' => bytes.extend_from_slice(b"\\x3b"),
            0..0x20 | 0x7f => bytes.extend_from_slice(format!("\\x{b:02x}").as_bytes()),
            _ => bytes.push(b),
        }
    }
    // Only ASCII bytes were replaced, so the result is still UTF-8.
    String::from_utf8(bytes).unwrap_or_default()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONCE: &str = "0123456789abcdef0123456789abcdef";

    fn scanner() -> OscScanner {
        OscScanner::new(&Nonce::new(NONCE))
    }

    #[cfg(windows)]
    const HISTORY: &str = r"C:\Users\u\.zsh_history";
    #[cfg(not(windows))]
    const HISTORY: &str = "/home/u/.zsh_history";

    fn osc(body: &str) -> Vec<u8> {
        format!("\x1b]6973;{body}\x07").into_bytes()
    }

    fn scan_all(scanner: &mut OscScanner, chunks: &[&[u8]]) -> (Vec<u8>, Vec<ShellMark>) {
        let mut out = Vec::new();
        let mut marks = Vec::new();
        for chunk in chunks {
            scanner.feed(chunk, &mut out, &mut marks);
        }
        (out, marks)
    }

    #[test]
    fn plain_output_and_other_sequences_pass_unchanged() {
        let input: &[u8] = b"hello \x1b[31mred\x1b[0m \x1b]133;A\x07\x1b]7;file://h/tmp\x1b\\\x1b]697;x\x07\x1b]69730;x\x07\x1b]6973\x07 \x1b\x1b] end";
        let (out, marks) = scan_all(&mut scanner(), &[input]);
        assert_eq!(out, input);
        assert!(marks.is_empty());
    }

    #[test]
    fn private_sequences_are_removed_and_decoded() {
        let mut input = b"a".to_vec();
        input.extend(osc(&format!(
            "{NONCE};ready;1;zsh;5.9;{}",
            encode_field(HISTORY)
        )));
        input.extend(b"b");
        input.extend(format!("\x1b]6973;{NONCE};prompt;0\x1b\\").as_bytes());
        input.extend(osc(&format!("{NONCE};exec")));
        input.extend(osc(&format!("{NONCE};prompt;")));
        input.extend(osc(&format!("{NONCE};prompt;130")));
        input.extend(b"c");
        let (out, marks) = scan_all(&mut scanner(), &[&input]);
        assert_eq!(out, b"abc");
        assert_eq!(
            marks,
            [
                ShellMark::Ready(ShellInfo {
                    protocol: 1,
                    shell: "zsh".into(),
                    version: "5.9".into(),
                    history_file: Some(HISTORY.into()),
                }),
                ShellMark::Prompt { exit_code: Some(0) },
                ShellMark::Exec,
                ShellMark::Prompt { exit_code: None },
                ShellMark::Prompt {
                    exit_code: Some(130)
                },
            ]
        );
    }

    #[test]
    fn wrong_or_missing_nonce_is_removed_but_ignored() {
        let mut input = Vec::new();
        input.extend(osc("ffffffffffffffffffffffffffffffff;prompt;0"));
        input.extend(osc("prompt;0"));
        input.extend(osc(";prompt;0"));
        input.extend(osc(&format!("{NONCE}0;exec")));
        input.extend(osc(&format!("{};exec", &NONCE[1..])));
        input.extend(osc(&format!("{NONCE};unknown")));
        input.extend(osc(&format!("{NONCE};complete;x;insert;l;0;0")));
        input.extend(b"visible");
        let (out, marks) = scan_all(&mut scanner(), &[&input]);
        assert_eq!(out, b"visible");
        assert!(marks.is_empty(), "{marks:?}");
    }

    #[test]
    fn completion_reply_with_escaped_fields() {
        let fields = [
            "complete",
            "42",
            "insert",
            &encode_field("ls fix"),
            "6",
            "3",
            &encode_field("fix one.txt"),
            &encode_field("fix\\ one.txt"),
            "",
            &encode_field("fix;semi"),
            &encode_field("fix\\;semi"),
            &encode_field("a\x1bb\nc é"),
            "dangling",
        ];
        let input = osc(&format!("{NONCE};{}", fields.join(";")));
        assert!(!input[1..input.len() - 1].contains(&ESC));
        let (out, marks) = scan_all(&mut scanner(), &[&input]);
        assert!(out.is_empty());
        let [ShellMark::Completion(reply)] = marks.as_slice() else {
            panic!("{marks:?}")
        };
        assert_eq!(reply.request, 42);
        assert_eq!(reply.mode, EditMode::Insert);
        assert_eq!(
            (reply.line.as_str(), reply.cursor, reply.replace_from),
            ("ls fix", 6, 3)
        );
        assert_eq!(reply.items.len(), 2);
        assert_eq!(reply.items[0].insert, "fix\\ one.txt");
        assert_eq!(reply.items[1].text, "fix;semi");
        assert_eq!(reply.items[1].description, "a\x1bb\nc é");
    }

    #[test]
    fn decoding_accepts_both_hex_cases_and_keeps_bad_escapes() {
        assert_eq!(decode_field(br"a\x3Bb\x3bc\\d\xC3\xA9"), "a;b;c\\dé");
        assert_eq!(decode_field(br"\x\xZZ\x4"), r"\x\xZZ\x4");
        assert_eq!(decode_field(br"end\"), "end\\");
        assert_eq!(encode_field("a;b\\c\x07é"), r"a\x3bb\\c\x07é");
    }

    #[test]
    fn sequences_split_between_every_pair_of_bytes() {
        let mut input = b"x\x1b[1m".to_vec();
        input.extend(osc(&format!("{NONCE};prompt;7")));
        input.extend(b"\x1b]133;A\x07y\x1b");
        input.extend(format!("]6973;{NONCE};exec\x1b\\z").as_bytes());
        let expected_out = b"x\x1b[1m\x1b]133;A\x07yz".to_vec();
        let expected_marks = vec![ShellMark::Prompt { exit_code: Some(7) }, ShellMark::Exec];
        for split in 0..=input.len() {
            let (out, marks) = scan_all(&mut scanner(), &[&input[..split], &input[split..]]);
            assert_eq!(out, expected_out, "split at {split}");
            assert_eq!(marks, expected_marks, "split at {split}");
        }
        // One byte at a time.
        let chunks: Vec<&[u8]> = input.chunks(1).collect();
        let (out, marks) = scan_all(&mut scanner(), &chunks);
        assert_eq!(out, expected_out);
        assert_eq!(marks, expected_marks);
    }

    /// Small deterministic generator (no `rand` dependency).
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self, bound: u64) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 33) % bound
        }
    }

    #[test]
    fn random_streams_keep_every_other_byte_in_order() {
        let mut rng = Lcg(7);
        for _round in 0..200 {
            // Build a stream of random pieces and the expected result.
            let mut input = Vec::new();
            let mut expected = Vec::new();
            let mut expected_marks = 0;
            for _ in 0..rng.next(30) {
                match rng.next(6) {
                    0 => {
                        let seq = osc(&format!("{NONCE};exec"));
                        input.extend(&seq);
                        expected_marks += 1;
                    }
                    1 => input.extend(osc("forged;exec")),
                    2 => {
                        let seq = b"\x1b]133;D;0\x07\x1b]7;file://h/x\x1b\\";
                        input.extend(seq);
                        expected.extend(seq);
                    }
                    _ => {
                        // Random bytes, without ESC so they cannot form or
                        // cut a sequence by accident.
                        for _ in 0..rng.next(40) {
                            let b = rng.next(256) as u8;
                            if b != ESC {
                                input.push(b);
                                expected.push(b);
                            }
                        }
                    }
                }
            }
            let mut chunks = Vec::new();
            let mut start = 0;
            while start < input.len() {
                let len = 1 + rng.next(17) as usize;
                let end = (start + len).min(input.len());
                chunks.push(&input[start..end]);
                start = end;
            }
            let (out, marks) = scan_all(&mut scanner(), &chunks);
            assert_eq!(out, expected);
            assert_eq!(marks.len(), expected_marks);
        }
    }

    #[test]
    fn huge_sequences_are_dropped_and_output_resumes() {
        let mut s = OscScanner::with_limit(&Nonce::new(NONCE), 64);
        let mut input = format!("\x1b]6973;{NONCE};complete;1;insert;").into_bytes();
        input.extend(vec![b'a'; 1000]);
        input.extend(b"\x07after");
        input.extend(osc(&format!("{NONCE};exec")));
        let (out, marks) = scan_all(&mut s, &[&input[..500], &input[500..]]);
        assert_eq!(out, b"after");
        assert_eq!(marks, [ShellMark::Exec]);

        // Also when the oversized sequence ends with ESC \.
        let mut input = b"\x1b]6973;".to_vec();
        input.extend(vec![b'b'; 100]);
        input.extend(b"\x1b\\ok");
        let (out, marks) = scan_all(&mut s, &[&input]);
        assert_eq!(out, b"ok");
        assert!(marks.is_empty());
    }

    #[test]
    fn cancelled_or_interrupted_sequences() {
        // CAN cancels: nothing dispatched, nothing shown.
        let mut input = format!("\x1b]6973;{NONCE};exec\x18after").into_bytes();
        let (out, marks) = scan_all(&mut scanner(), &[&input]);
        assert_eq!(out, b"after");
        assert!(marks.is_empty());

        // ESC + another byte ends the sequence; the escape is shown.
        input = format!("\x1b]6973;{NONCE};exec\x1b[1mbold").into_bytes();
        let (out, marks) = scan_all(&mut scanner(), &[&input]);
        assert_eq!(out, b"\x1b[1mbold");
        assert!(marks.is_empty());

        // ESC + a new private sequence right away.
        input = format!("\x1b]6973;junk\x1b]6973;{NONCE};exec\x07").into_bytes();
        let (out, marks) = scan_all(&mut scanner(), &[&input]);
        assert!(out.is_empty());
        assert_eq!(marks, [ShellMark::Exec]);
    }

    #[test]
    fn held_bytes_are_only_a_possible_prefix() {
        let mut s = scanner();
        let (out, _) = scan_all(&mut s, &[b"abc\x1b]69"]);
        assert_eq!(out, b"abc");
        assert_eq!(s.held(), 4);
        let (out, _) = scan_all(&mut s, &[b"x"]);
        assert_eq!(out, b"\x1b]69x");
        assert_eq!(s.held(), 0);
    }

    #[test]
    fn relative_history_paths_are_ignored() {
        let input = osc(&format!("{NONCE};ready;1;fish;4.0;relative/path"));
        let (_, marks) = scan_all(&mut scanner(), &[&input]);
        let [ShellMark::Ready(info)] = marks.as_slice() else {
            panic!()
        };
        assert_eq!(info.history_file, None);
        let input = osc(&format!("{NONCE};ready;1;fish;4.0;"));
        let (_, marks) = scan_all(&mut scanner(), &[&input]);
        let [ShellMark::Ready(info)] = marks.as_slice() else {
            panic!()
        };
        assert_eq!(info.history_file, None);
    }
}
