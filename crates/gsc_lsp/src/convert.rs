//! Conversions between LSP positions and URIs and the byte offsets and paths of the core.
//!
//! The core speaks byte offsets into a file's decoded text. LSP speaks `(line, character)` where
//! `character` counts UTF-16 code units by default, or bytes when the client accepts the `utf-8`
//! position encoding.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use gsc_syntax::{TextRange, TextSize};
use lsp_types::{Position, Range, Uri};

/// How `Position::character` counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf16,
}

/// Line starts of a text, for offset and position conversions.
#[derive(Clone, Debug)]
pub struct LineIndex {
    starts: Vec<u32>,
    len: u32,
    /// Is the text pure ASCII (columns are then byte counts in both encodings)?
    ascii: bool,
}

impl LineIndex {
    pub fn new(text: &str) -> LineIndex {
        let mut starts = vec![0];
        let b = text.as_bytes();
        for (i, &c) in b.iter().enumerate() {
            if c == b'\n' || (c == b'\r' && b.get(i + 1) != Some(&b'\n')) {
                starts.push(i as u32 + 1);
            }
        }
        LineIndex {
            starts,
            len: text.len() as u32,
            ascii: text.is_ascii(),
        }
    }

    /// The position of byte `offset` (clamped to the text, snapped back to a char boundary).
    pub fn position(&self, text: &str, offset: TextSize, enc: Encoding) -> Position {
        let offset = u32::from(offset).min(self.len);
        let line = match self.starts.binary_search(&offset) {
            Ok(l) => l,
            Err(l) => l - 1,
        };
        let start = self.starts[line];
        let character = if self.ascii || enc == Encoding::Utf8 {
            offset - start
        } else {
            let mut end = offset as usize;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text[start as usize..end]
                .chars()
                .map(|c| c.len_utf16() as u32)
                .sum()
        };
        Position {
            line: line as u32,
            character,
        }
    }

    pub fn range(&self, text: &str, r: TextRange, enc: Encoding) -> Range {
        Range {
            start: self.position(text, r.start(), enc),
            end: self.position(text, r.end(), enc),
        }
    }

    /// The byte offset of `pos`; positions past the end of a line clamp to its end, lines past
    /// the end of the text to the end of the text.
    pub fn offset(&self, text: &str, pos: Position, enc: Encoding) -> TextSize {
        let Some(&start) = self.starts.get(pos.line as usize) else {
            return TextSize::from(self.len);
        };
        let end = self
            .starts
            .get(pos.line as usize + 1)
            .copied()
            .unwrap_or(self.len);
        let line = &text[start as usize..end as usize];
        let line = line.trim_end_matches(['\n', '\r']);
        let col = if self.ascii || enc == Encoding::Utf8 {
            let mut c = (pos.character as usize).min(line.len());
            while !line.is_char_boundary(c) {
                c -= 1;
            }
            c
        } else {
            let mut units = 0u32;
            let mut bytes = line.len();
            for (i, ch) in line.char_indices() {
                if units >= pos.character {
                    bytes = i;
                    break;
                }
                units += ch.len_utf16() as u32;
            }
            bytes
        };
        TextSize::from(start + col as u32)
    }

    pub fn text_range(&self, text: &str, r: Range, enc: Encoding) -> TextRange {
        let s = self.offset(text, r.start, enc);
        let e = self.offset(text, r.end, enc);
        TextRange::new(s.min(e), e.max(s))
    }
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn percent_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        let keep = b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~');
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The local path of a `file:` URI.
pub fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    let s = uri.as_str();
    let rest = s.strip_prefix("file://")?;
    // Drop an authority (`file://host/share` is a UNC path on Windows).
    let (host, path) = match rest.find('/') {
        Some(0) => ("", rest),
        Some(i) => (&rest[..i], &rest[i..]),
        None => return None,
    };
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let path = percent_decode(path);
    if cfg!(windows) {
        let p = path.trim_start_matches('/');
        let p = p.replace('/', "\\");
        if host.is_empty() || host == "localhost" {
            Some(PathBuf::from(p))
        } else {
            Some(PathBuf::from(format!("\\\\{host}\\{p}")))
        }
    } else if host.is_empty() || host == "localhost" {
        Some(PathBuf::from(path))
    } else {
        None
    }
}

/// The `file:` URI of a path (made absolute).
pub fn path_to_uri(path: &Path) -> Uri {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let s = abs.to_string_lossy().replace('\\', "/");
    let s = s.strip_prefix("//?/").unwrap_or(&s).to_owned();
    let text = if let Some(unc) = s.strip_prefix("//") {
        let (host, rest) = unc.split_once('/').unwrap_or((unc, ""));
        format!("file://{host}/{}", percent_encode_path(rest))
    } else if s.starts_with('/') {
        format!("file://{}", percent_encode_path(&s))
    } else {
        // `C:/x` -> `file:///c%3A/x`, the form VS Code sends.
        let mut chars = s.chars();
        match (chars.next(), chars.next()) {
            (Some(d), Some(':')) if d.is_ascii_alphabetic() => format!(
                "file:///{}%3A{}",
                d.to_ascii_lowercase(),
                percent_encode_path(&s[2..])
            ),
            _ => format!("file:///{}", percent_encode_path(&s)),
        }
    };
    Uri::from_str(&text).unwrap_or_else(|_| Uri::from_str("file:///").expect("a valid URI"))
}

/// A key that identifies a path whatever its spelling: absolute, `/` separators, and lower case
/// on Windows (drive letters and file names are case-insensitive there).
pub fn path_key(path: &Path) -> String {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let s = abs.to_string_lossy().replace('\\', "/");
    let s = s.strip_prefix("//?/").unwrap_or(&s).to_owned();
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_round_trip() {
        let text = "ab\r\nc\u{e9}d\n\u{1F600}x\n";
        let idx = LineIndex::new(text);
        for enc in [Encoding::Utf8, Encoding::Utf16] {
            for (i, _) in text.char_indices() {
                if text[..i].ends_with('\r') {
                    continue; // between `\r` and `\n`: no position names it
                }
                let off = TextSize::from(i as u32);
                let p = idx.position(text, off, enc);
                assert_eq!(idx.offset(text, p, enc), off, "{enc:?} {i} {p:?}");
            }
        }
        let p = idx.position(text, TextSize::from(13), Encoding::Utf16);
        assert_eq!((p.line, p.character), (2, 2));
        let p = idx.position(text, TextSize::from(13), Encoding::Utf8);
        assert_eq!((p.line, p.character), (2, 4));
        // Past the end of a line clamps to it.
        let o = idx.offset(
            text,
            Position {
                line: 0,
                character: 99,
            },
            Encoding::Utf16,
        );
        assert_eq!(o, TextSize::from(2));
    }

    #[test]
    fn uris_round_trip() {
        let p = if cfg!(windows) {
            PathBuf::from("C:\\Users\\a b\\x#y.gsc")
        } else {
            PathBuf::from("/home/a b/x#y.gsc")
        };
        let u = path_to_uri(&p);
        assert!(u.as_str().starts_with("file:///"), "{}", u.as_str());
        let back = uri_to_path(&u).unwrap();
        assert_eq!(path_key(&back), path_key(&p));
        if cfg!(windows) {
            let u = Uri::from_str("file:///c%3A/Users/x/a.gsc").unwrap();
            assert_eq!(
                uri_to_path(&u).unwrap(),
                PathBuf::from("c:\\Users\\x\\a.gsc")
            );
            assert_eq!(path_key(&uri_to_path(&u).unwrap()), "c:/users/x/a.gsc");
        }
    }
}
