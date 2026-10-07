//! Splitting command text, as `Cbuf_Execute` and `Cmd_TokenizeString` do.

/// Longest command line `Cbuf_Execute` hands on (`MAX_CMD_LINE - 1` bytes).
const MAX_CMD_LINE: usize = 1023;
/// Most arguments `Cmd_TokenizeString` keeps.
const MAX_TOKENS: usize = 1024;

/// `Cmd_TokenizeString`: arguments separated by whitespace; `"quoted strings"` are one
/// argument (no escapes); `//` ends the line; `/* */` comments are skipped. A quote or
/// a comment also ends a word that runs into it.
pub fn tokenize(line: &str) -> Vec<String> {
    let b = line.as_bytes();
    let mut args = Vec::new();
    let mut i = 0;
    while args.len() < MAX_TOKENS {
        loop {
            while i < b.len() && b[i] <= b' ' {
                i += 1;
            }
            if i >= b.len() || b[i..].starts_with(b"//") {
                return args;
            }
            if b[i..].starts_with(b"/*") {
                match b[i..].windows(2).position(|w| w == b"*/") {
                    Some(n) => i += n + 2,
                    None => return args,
                }
            } else {
                break;
            }
        }
        let start;
        if b[i] == b'"' {
            start = i + 1;
            i = start
                + b[start..]
                    .iter()
                    .position(|&c| c == b'"')
                    .unwrap_or(b.len() - start);
            args.push(String::from_utf8_lossy(&b[start..i]).into_owned());
            if i >= b.len() {
                return args;
            }
            i += 1;
            continue;
        }
        start = i;
        while i < b.len()
            && b[i] > b' '
            && b[i] != b'"'
            && !b[i..].starts_with(b"//")
            && !b[i..].starts_with(b"/*")
        {
            i += 1;
        }
        args.push(String::from_utf8_lossy(&b[start..i]).into_owned());
    }
    args
}

/// `Cmd_ArgsFrom`: the arguments from `n` on, joined with single spaces.
pub fn args_from(args: &[String], n: usize) -> String {
    args.get(n..).unwrap_or_default().join(" ")
}

/// The command buffer: text waiting to be run, one command at a time.
#[derive(Debug, Default)]
pub struct CommandBuffer {
    text: Vec<u8>,
    /// Frames left to wait before running more (`wait`).
    pub wait: u32,
}

/// Comment state `Cbuf_Execute` keeps while it runs; it starts fresh on each call.
#[derive(Debug, Default)]
pub struct SplitState {
    in_slash_comment: bool,
    in_star_comment: bool,
}

impl CommandBuffer {
    /// `Cbuf_AddText`: appends to the end.
    pub fn add(&mut self, text: &str) {
        self.text.extend_from_slice(text.as_bytes());
    }

    /// `Cbuf_InsertText`: puts text (and a line break) before everything waiting, so
    /// an `exec`'d file runs before the commands that followed the `exec`.
    pub fn insert(&mut self, text: &str) {
        let mut t = text.as_bytes().to_vec();
        t.push(b'\n');
        t.extend_from_slice(&self.text);
        self.text = t;
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The next command, split like `Cbuf_Execute`: at `;` or a line break outside
    /// quotes; a `//` comment keeps its `;`s on its line and a `/* */` comment keeps
    /// its `;`s and line breaks. Lines longer than 1023 bytes are cut. `None` when the
    /// buffer is empty or waiting (each call while waiting uses up one frame).
    pub fn next(&mut self, state: &mut SplitState) -> Option<String> {
        if self.text.is_empty() {
            return None;
        }
        if self.wait > 0 {
            self.wait -= 1;
            return None;
        }
        let t = &self.text;
        let mut quotes = 0;
        let mut i = 0;
        while i < t.len() {
            if t[i] == b'"' {
                quotes += 1;
            }
            if quotes % 2 == 0 {
                if i + 1 < t.len() {
                    let pair = &t[i..i + 2];
                    if !state.in_star_comment && pair == b"//" {
                        state.in_slash_comment = true;
                    } else if !state.in_slash_comment && pair == b"/*" {
                        state.in_star_comment = true;
                    } else if state.in_star_comment && pair == b"*/" {
                        state.in_star_comment = false;
                        i += 1;
                        break;
                    }
                }
                if !state.in_slash_comment && !state.in_star_comment && t[i] == b';' {
                    break;
                }
            }
            if !state.in_star_comment && (t[i] == b'\n' || t[i] == b'\r') {
                state.in_slash_comment = false;
                break;
            }
            i += 1;
        }
        let line = String::from_utf8_lossy(&t[..i.min(MAX_CMD_LINE)]).into_owned();
        let consumed = (i + 1).min(t.len());
        self.text.drain(..consumed);
        Some(line)
    }
}

/// `Com_ParseCommandLine`: the command line split into commands at `+` (outside
/// quotes) and at line breaks; the text before the first `+` is the first entry.
pub fn split_command_line(line: &str) -> Vec<String> {
    const MAX_CONSOLE_LINES: usize = 32;
    let mut out = vec![String::new()];
    let mut in_quotes = false;
    for c in line.chars() {
        if c == '"' {
            in_quotes = !in_quotes;
        }
        // Once 32 entries exist the original stops splitting: the rest of the line,
        // separators included, stays in the last entry.
        if ((c == '+' && !in_quotes) || c == '\n' || c == '\r') && out.len() < MAX_CONSOLE_LINES {
            out.push(String::new());
        } else {
            out.last_mut().unwrap().push(c);
        }
    }
    out
}
