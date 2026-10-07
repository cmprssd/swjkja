//! The original's text primitives, byte for byte: `COM_CompressShader`, `COM_ParseExt`,
//! `SkipBracedSection`, `SkipRestOfLine` and C's `atof`/`atoi`. Shader files are parsed
//! as bytes; they are not guaranteed to be UTF-8.

/// Longest token the original keeps (`MAX_TOKEN_CHARS - 1`); the rest is dropped.
const MAX_TOKEN: usize = 1023;

/// `COM_CompressShader`: drops `//`, `#` and `/* */` comments (even in the middle of
/// a word), collapses runs of spaces and tabs to one space and runs of line breaks to
/// one `\n`, and copies quoted strings unchanged.
pub fn compress(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let (mut newline, mut space) = (false, false);
    let mut i = 0;
    while i < input.len() {
        let c = input[i];
        let next = input.get(i + 1).copied();
        if (c == b'/' && next == Some(b'/')) || c == b'#' {
            while i < input.len() && input[i] != b'\n' {
                i += 1;
            }
        } else if c == b'/' && next == Some(b'*') {
            while i < input.len() && !(input[i] == b'*' && input.get(i + 1) == Some(&b'/')) {
                i += 1;
            }
            if i < input.len() {
                i += 2;
            }
        } else if c == b'\n' || c == b'\r' {
            newline = true;
            i += 1;
        } else if c == b' ' || c == b'\t' {
            space = true;
            i += 1;
        } else {
            if newline {
                out.push(b'\n');
                newline = false;
                space = false;
            }
            if space {
                out.push(b' ');
                space = false;
            }
            if c == b'"' {
                out.push(c);
                i += 1;
                while i < input.len() && input[i] != b'"' {
                    out.push(input[i]);
                    i += 1;
                }
                if i < input.len() {
                    out.push(b'"');
                    i += 1;
                }
            } else {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// A file's contents as the original's C string functions see them: up to the first
/// NUL byte.
pub fn until_nul(data: &[u8]) -> &[u8] {
    &data[..data.iter().position(|&b| b == 0).unwrap_or(data.len())]
}

/// A read position in a text, advanced like the original's `const char **`.
#[derive(Debug, Clone, Copy)]
pub struct Cursor<'a> {
    text: &'a [u8],
    pub pos: usize,
}

impl<'a> Cursor<'a> {
    /// The text must not contain NUL bytes: the original works on C strings, so
    /// callers cut a file at its first NUL ([`until_nul`]) before reading it.
    pub fn new(text: &'a [u8]) -> Self {
        Self::at(text, 0)
    }

    pub fn at(text: &'a [u8], pos: usize) -> Self {
        Self { text, pos }
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.text.len()
    }

    /// `COM_ParseExt`. Returns an empty token at the end of the text, and also at a
    /// line break when `line_breaks` is false; in that case the position is left on
    /// the next line's first token. Words end only at whitespace (bytes up to 0x20);
    /// quoted strings may contain anything but `"`.
    pub fn token(&mut self, line_breaks: bool) -> &'a [u8] {
        let t = self.text;
        let mut i = self.pos;
        let mut saw_newline = false;
        loop {
            while i < t.len() && t[i] <= b' ' {
                saw_newline |= t[i] == b'\n';
                i += 1;
            }
            if i >= t.len() {
                self.pos = t.len();
                return b"";
            }
            if saw_newline && !line_breaks {
                self.pos = i;
                return b"";
            }
            if t[i] == b'/' && t.get(i + 1) == Some(&b'/') {
                while i < t.len() && t[i] != b'\n' {
                    i += 1;
                }
            } else if t[i] == b'/' && t.get(i + 1) == Some(&b'*') {
                i += 2;
                while i < t.len() && !(t[i] == b'*' && t.get(i + 1) == Some(&b'/')) {
                    i += 1;
                }
                if i < t.len() {
                    i += 2;
                }
            } else {
                break;
            }
        }
        if t[i] == b'"' {
            let start = i + 1;
            let end = t[start..]
                .iter()
                .position(|&b| b == b'"')
                .map_or(t.len(), |n| start + n);
            self.pos = (end + 1).min(t.len());
            return &t[start..end.min(start + MAX_TOKEN)];
        }
        let start = i;
        while i < t.len() && t[i] > b' ' {
            i += 1;
        }
        self.pos = i;
        &t[start..i.min(start + MAX_TOKEN)]
    }

    /// `SkipRestOfLine`: moves past the next `\n`.
    pub fn skip_rest_of_line(&mut self) {
        let rest = &self.text[self.pos.min(self.text.len())..];
        self.pos += rest
            .iter()
            .position(|&b| b == b'\n')
            .map_or(rest.len(), |n| n + 1);
    }

    /// `SkipBracedSection`: reads tokens, counting lone `{` and `}`, until the depth
    /// returns to zero or the text ends. True if it returned to zero.
    pub fn skip_braced_section(&mut self, mut depth: i32) -> bool {
        loop {
            match self.token(true) {
                b"{" => depth += 1,
                b"}" => depth -= 1,
                _ => {}
            }
            if depth == 0 || self.at_end() {
                return depth == 0;
            }
        }
    }
}

/// C's `atof`: the longest numeric prefix (after leading whitespace), 0 if none.
pub fn atof(token: &[u8]) -> f32 {
    let s = std::str::from_utf8(token).unwrap_or("").trim_start();
    let b = s.as_bytes();
    let mut end = 0;
    if matches!(b.first(), Some(b'+' | b'-')) {
        end = 1;
    }
    let digits = |from: usize| from + b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    let int_end = digits(end);
    let mut num_end = int_end;
    if b.get(int_end) == Some(&b'.') {
        num_end = digits(int_end + 1);
    }
    if num_end == end || (num_end == end + 1 && b.get(end) == Some(&b'.')) {
        return 0.0;
    }
    // An exponent counts only if digits follow it.
    if matches!(b.get(num_end), Some(b'e' | b'E')) {
        let mut e = num_end + 1;
        if matches!(b.get(e), Some(b'+' | b'-')) {
            e += 1;
        }
        let e_end = digits(e);
        if e_end > e {
            num_end = e_end;
        }
    }
    s[..num_end].parse::<f64>().map_or(0.0, |v| v as f32)
}

/// C's `atoi`: the longest integer prefix, 0 if none.
pub fn atoi(token: &[u8]) -> i32 {
    let s = std::str::from_utf8(token).unwrap_or("").trim_start();
    let b = s.as_bytes();
    let sign = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let end = sign + b[sign..].iter().take_while(|c| c.is_ascii_digit()).count();
    s[..end].parse::<i64>().map_or(0, |v| v as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(text: &[u8], line_breaks: bool) -> Vec<String> {
        let mut c = Cursor::new(text);
        let mut out = Vec::new();
        while !c.at_end() {
            out.push(String::from_utf8_lossy(c.token(line_breaks)).into_owned());
        }
        out
    }

    #[test]
    fn compress_drops_comments_and_collapses_whitespace() {
        assert_eq!(
            compress(b"a  \t b // c\r\n\n\nd /* x\ny */ e #f g\n\"q  // r\" h//i"),
            b"a b\nd e\n\"q  // r\" h"
        );
    }

    #[test]
    fn tokens_stop_at_line_breaks_only_when_asked() {
        assert_eq!(tokens(b"map a\nb", true), ["map", "a", "b"]);
        // Without line breaks: an empty token at the newline, then the next line.
        assert_eq!(tokens(b"map a\nb", false), ["map", "a", "", "b"]);
        // Braces are only tokens when separated by whitespace.
        assert_eq!(
            tokens(b"{map} \"x y\" // c\n/* d */e", true),
            ["{map}", "x y", "e"]
        );
    }

    #[test]
    fn skip_braced_section_counts_lone_braces() {
        let mut c = Cursor::new(b"{ a { b } {x} } tail");
        assert!(c.skip_braced_section(0));
        assert_eq!(c.token(true), b"tail");
        let mut c = Cursor::new(b"{ a { b }");
        assert!(!c.skip_braced_section(0));
    }

    #[test]
    fn atof_and_atoi_read_prefixes_like_c() {
        assert_eq!(atof(b"1.5x"), 1.5);
        assert_eq!(atof(b" -2"), -2.0);
        assert_eq!(atof(b".25"), 0.25);
        assert_eq!(atof(b"3."), 3.0);
        assert_eq!(atof(b"1e3"), 1000.0);
        assert_eq!(atof(b"1e"), 1.0);
        assert_eq!(atof(b"abc"), 0.0);
        assert_eq!(atof(b""), 0.0);
        assert_eq!(atof(b"."), 0.0);
        assert_eq!(atoi(b"42abc"), 42);
        assert_eq!(atoi(b"-7"), -7);
        assert_eq!(atoi(b"x"), 0);
    }
}
