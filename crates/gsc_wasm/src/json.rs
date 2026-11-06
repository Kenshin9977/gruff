//! A small JSON writer (no serde: the module stays small) and the position table.

use gsc_check::output::json_str;
use gsc_syntax::{LineIndex, TextSize};

/// Writes JSON in one pass. Containers track whether they need a comma before the next member.
pub struct Json {
    out: String,
    first: Vec<bool>,
}

impl Json {
    pub fn new() -> Json {
        Json {
            out: String::new(),
            first: Vec::new(),
        }
    }

    pub fn finish(self) -> String {
        self.out
    }

    /// A comma before a member, unless it is the container's first.
    pub fn comma(&mut self) {
        if let Some(first) = self.first.last_mut() {
            if !*first {
                self.out.push(',');
            }
            *first = false;
        }
    }

    pub fn object(&mut self, f: impl FnOnce(&mut Json)) {
        self.out.push('{');
        self.first.push(true);
        f(self);
        self.first.pop();
        self.out.push('}');
    }

    pub fn array(&mut self, f: impl FnOnce(&mut Json)) {
        self.out.push('[');
        self.first.push(true);
        f(self);
        self.first.pop();
        self.out.push(']');
    }

    /// `"k":`, the next value written being the member's value.
    pub fn key(&mut self, k: &str) {
        self.comma();
        self.out.push_str(&json_str(k));
        self.out.push(':');
    }

    pub fn str(&mut self, k: &str, v: &str) {
        self.key(k);
        self.out.push_str(&json_str(v));
    }

    pub fn num(&mut self, k: &str, v: u64) {
        self.key(k);
        self.out.push_str(&v.to_string());
    }

    pub fn bool(&mut self, k: &str, v: bool) {
        self.key(k);
        self.out.push_str(if v { "true" } else { "false" });
    }

    pub fn item_str(&mut self, v: &str) {
        self.comma();
        self.out.push_str(&json_str(v));
    }

    pub fn item_object(&mut self, f: impl FnOnce(&mut Json)) {
        self.comma();
        self.object(f);
    }
}

/// Line, column and UTF-16 offset of every byte offset of a text.
pub struct Positions<'a> {
    text: &'a str,
    index: LineIndex,
    /// UTF-16 offset of each byte offset (`text.len() + 1` entries).
    utf16: Vec<u32>,
}

impl<'a> Positions<'a> {
    pub fn new(text: &'a str) -> Positions<'a> {
        let mut utf16 = Vec::with_capacity(text.len() + 1);
        let mut at = 0u32;
        for c in text.chars() {
            for _ in 0..c.len_utf8() {
                utf16.push(at);
            }
            at += c.len_utf16() as u32;
        }
        utf16.push(at);
        Positions {
            text,
            index: LineIndex::new(text),
            utf16,
        }
    }

    pub fn utf16(&self, offset: TextSize) -> u32 {
        let i = usize::from(offset).min(self.utf16.len() - 1);
        self.utf16[i]
    }

    fn position(&self, j: &mut Json, k: &str, offset: TextSize) {
        let lc = self.index.line_col(self.text, offset);
        j.key(k);
        j.object(|j| {
            j.num("offset", u32::from(offset).into());
            j.num("line", lc.line.into());
            j.num("col", lc.col.into());
            j.num("utf16", self.utf16(offset).into());
        });
    }

    /// `"start": {...}, "end": {...}`.
    pub fn range(&self, j: &mut Json, start: TextSize, end: TextSize) {
        self.position(j, "start", start);
        self.position(j, "end", end);
    }
}
