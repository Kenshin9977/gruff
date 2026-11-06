//! Source bytes to text and back, and line/column lookup.
//!
//! GSC files are mostly ASCII. Some carry UTF-8 (often with a BOM), and older ones carry
//! Windows-1252 bytes in strings and comments. A file that is valid UTF-8 is used as is. Any other
//! file is decoded byte by byte as Windows-1252, where the five bytes the code page leaves
//! undefined map to the matching C1 control characters (the WHATWG mapping). That decoding is a
//! bijection, so [`SourceText::to_bytes`] gives back the input exactly.

use rowan::{TextRange, TextSize};

/// How the bytes of a file were decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// The file is valid UTF-8 (a BOM, if any, is kept as a trivia token).
    Utf8,
    /// The file is not valid UTF-8 and was decoded as Windows-1252.
    Windows1252,
}

/// The text of a file plus the encoding needed to write it back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceText {
    pub text: String,
    pub encoding: Encoding,
}

/// Windows-1252 bytes 0x80..=0x9F. Undefined bytes map to the C1 control of the same value.
const CP1252_HIGH: [char; 32] = [
    '\u{20AC}', '\u{0081}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{008D}', '\u{017D}', '\u{008F}',
    '\u{0090}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{009D}', '\u{017E}', '\u{0178}',
];

fn cp1252_decode(b: u8) -> char {
    match b {
        0x80..=0x9F => CP1252_HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

fn cp1252_encode(c: char) -> Option<u8> {
    let u = c as u32;
    if u < 0x80 || (0xA0..=0xFF).contains(&u) {
        return Some(u as u8);
    }
    CP1252_HIGH
        .iter()
        .position(|&h| h == c)
        .map(|i| 0x80 + i as u8)
}

impl SourceText {
    /// Decode file bytes: UTF-8 when valid, Windows-1252 otherwise.
    pub fn decode(bytes: &[u8]) -> SourceText {
        match std::str::from_utf8(bytes) {
            Ok(s) => SourceText {
                text: s.to_owned(),
                encoding: Encoding::Utf8,
            },
            Err(_) => SourceText {
                text: bytes.iter().map(|&b| cp1252_decode(b)).collect(),
                encoding: Encoding::Windows1252,
            },
        }
    }

    /// Encode `text` back to bytes in this file's encoding. Characters Windows-1252 cannot hold
    /// (only possible if the text was edited) become `?`.
    pub fn encode(&self, text: &str) -> Vec<u8> {
        match self.encoding {
            Encoding::Utf8 => text.as_bytes().to_vec(),
            Encoding::Windows1252 => text
                .chars()
                .map(|c| cp1252_encode(c).unwrap_or(b'?'))
                .collect(),
        }
    }

    /// The original bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.encode(&self.text)
    }
}

/// Does the text look like decompiler output (gsc-tool or acts header comments)?
pub fn looks_decompiled(text: &str) -> bool {
    let mut end = text.len().min(16 * 1024);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let head = &text[..end];
    head.contains("// Checksum 0x")
        || head.contains("github.com/xensik/gsc-tool")
        || head.contains("// Params ")
}

/// Is this a compiled script rather than source? Compiled scripts sometimes carry a source
/// extension.
pub fn is_binary(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x80GSC") || bytes.iter().take(1024).any(|&b| b == 0)
}

/// Maps text offsets to 1-based line and column numbers. Columns count characters. `\r\n`, `\n`
/// and a lone `\r` all end a line.
#[derive(Clone, Debug)]
pub struct LineIndex {
    line_starts: Vec<TextSize>,
}

/// A 1-based line and column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineCol {
    pub line: u32,
    pub col: u32,
}

impl LineIndex {
    pub fn new(text: &str) -> LineIndex {
        let mut line_starts = vec![TextSize::from(0)];
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\n' => line_starts.push(TextSize::from((i + 1) as u32)),
                b'\r' if bytes.get(i + 1) != Some(&b'\n') => {
                    line_starts.push(TextSize::from((i + 1) as u32))
                }
                _ => {}
            }
            i += 1;
        }
        LineIndex { line_starts }
    }

    /// Line and column of `offset` in `text` (the text this index was built from).
    pub fn line_col(&self, text: &str, offset: TextSize) -> LineCol {
        let line = match self.line_starts.binary_search(&offset) {
            Ok(l) => l,
            Err(l) => l - 1,
        };
        let start = self.line_starts[line];
        let end = offset.min(TextSize::of(text));
        let col = text
            .get(usize::from(start)..usize::from(end))
            .map(|s| s.chars().count())
            .unwrap_or(0);
        LineCol {
            line: line as u32 + 1,
            col: col as u32 + 1,
        }
    }

    /// The range of the 1-based `line`, without its line terminator.
    pub fn line_range(&self, text: &str, line: u32) -> Option<TextRange> {
        let start = *self.line_starts.get(line.checked_sub(1)? as usize)?;
        let next = self
            .line_starts
            .get(line as usize)
            .copied()
            .unwrap_or(TextSize::of(text));
        let mut end = usize::from(next);
        let bytes = text.as_bytes();
        while end > usize::from(start) && matches!(bytes[end - 1], b'\n' | b'\r') {
            end -= 1;
        }
        Some(TextRange::new(start, TextSize::from(end as u32)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cp1252_round_trips_every_byte() {
        let bytes: Vec<u8> = (0..=255u8).collect();
        let src = SourceText::decode(&bytes);
        assert_eq!(src.encoding, Encoding::Windows1252);
        assert_eq!(src.to_bytes(), bytes);
    }

    #[test]
    fn utf8_stays_utf8() {
        let src = SourceText::decode("\u{feff}é".as_bytes());
        assert_eq!(src.encoding, Encoding::Utf8);
        assert_eq!(src.to_bytes(), "\u{feff}é".as_bytes());
    }

    #[test]
    fn line_cols() {
        let text = "ab\r\ncd\ne\rf";
        let idx = LineIndex::new(text);
        let lc = |o: u32| idx.line_col(text, TextSize::from(o));
        assert_eq!(lc(0), LineCol { line: 1, col: 1 });
        assert_eq!(lc(4), LineCol { line: 2, col: 1 });
        assert_eq!(lc(5), LineCol { line: 2, col: 2 });
        assert_eq!(lc(7), LineCol { line: 3, col: 1 });
        assert_eq!(lc(9), LineCol { line: 4, col: 1 });
        assert_eq!(
            idx.line_range(text, 1),
            Some(TextRange::new(0.into(), 2.into()))
        );
    }
}
