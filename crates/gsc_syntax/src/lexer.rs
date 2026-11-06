//! Hand-written, lossless lexer. Every byte of the input lands in exactly one token.
//!
//! The lexer is modal: directive lines (`#define` bodies, `#if` lines, `#using` paths) are lexed
//! differently from code. Inactive `#elif`/`#else` branches become one `DISABLED_TEXT` trivia.

use rowan::{TextRange, TextSize};

use crate::{Dialect, SyntaxError, SyntaxKind};
use SyntaxKind::*;

/// A token: its kind and its length in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: SyntaxKind,
    pub len: u32,
}

/// Lex `text` for `dialect`. Returns the tokens (covering the whole text) and the lexical errors.
pub fn tokenize(text: &str, dialect: &Dialect) -> (Vec<Token>, Vec<SyntaxError>) {
    let mut lx = Lexer {
        text,
        b: text.as_bytes(),
        pos: 0,
        dialect,
        tokens: Vec::with_capacity(text.len() / 4),
        errors: Vec::new(),
        line_start: true,
        pp_open: Vec::new(),
        path_next: false,
    };
    lx.run();
    (lx.tokens, lx.errors)
}

struct Lexer<'a> {
    text: &'a str,
    b: &'a [u8],
    pos: usize,
    dialect: &'a Dialect,
    tokens: Vec<Token>,
    errors: Vec<SyntaxError>,
    /// Only whitespace since the last line break.
    line_start: bool,
    /// Start offsets of the open `#if` groups.
    pp_open: Vec<usize>,
    /// The next token is the path of `#using`, `#include`, `#insert` or `#inline`.
    path_next: bool,
}

fn is_word_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn is_hspace(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | 0x0B | 0x0C)
}

fn is_path_char(c: u8) -> bool {
    is_word(c) || matches!(c, b'\\' | b'/' | b'.' | b'-')
}

const DIRECTIVE_WORDS: &[(&str, SyntaxKind)] = &[
    ("using", USING_KW),
    ("include", INCLUDE_KW),
    ("insert", INSERT_KW),
    ("inline", INLINE_KW),
    ("namespace", NAMESPACE_KW),
    ("precache", PRECACHE_KW),
    ("using_animtree", USING_ANIMTREE_KW),
    ("animtree", ANIMTREE_KW),
    ("define", DEFINE_KW),
    ("undef", UNDEF_KW),
    ("if", PP_IF),
    ("ifdef", PP_IFDEF),
    ("ifndef", PP_IFNDEF),
    ("elif", PP_ELIF),
    ("else", PP_ELSE),
    ("endif", PP_ENDIF),
];

impl Lexer<'_> {
    fn peek(&self, n: usize) -> u8 {
        self.b.get(self.pos + n).copied().unwrap_or(0)
    }

    fn push(&mut self, kind: SyntaxKind, start: usize) {
        debug_assert!(self.pos > start);
        self.tokens.push(Token {
            kind,
            len: (self.pos - start) as u32,
        });
        if kind != WHITESPACE {
            self.line_start = false;
        }
    }

    fn error(&mut self, start: usize, end: usize, code: &'static str, message: impl Into<String>) {
        self.errors.push(SyntaxError::new(
            code,
            message,
            TextRange::new(TextSize::from(start as u32), TextSize::from(end as u32)),
        ));
    }

    /// Advance over one character (not one byte), keeping `pos` on a char boundary.
    fn bump_char(&mut self) {
        let c = self.text[self.pos..]
            .chars()
            .next()
            .map_or(1, char::len_utf8);
        self.pos += c;
    }

    fn run(&mut self) {
        if self.text.starts_with('\u{feff}') {
            self.pos = 3;
            self.push(BOM, 0);
        } else if self.text.starts_with("\u{ef}\u{bb}\u{bf}") {
            // A UTF-8 BOM in a file decoded as Windows-1252.
            self.pos = "\u{ef}\u{bb}\u{bf}".len();
            self.push(BOM, 0);
        }
        self.line_start = true;
        while self.pos < self.b.len() {
            self.token();
        }
        if let Some(&start) = self.pp_open.last() {
            self.error(
                start,
                start + 1,
                "E0104",
                "`#if` without a matching `#endif`",
            );
        }
    }

    fn token(&mut self) {
        let start = self.pos;
        let c = self.b[self.pos];
        if self.path_next && !is_hspace(c) {
            self.path_next = false;
            if is_path_char(c) {
                while self.pos < self.b.len() && is_path_char(self.b[self.pos]) {
                    self.pos += 1;
                }
                self.push(PATH, start);
                return;
            }
        }
        match c {
            b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C => self.whitespace(),
            b'/' => match self.peek(1) {
                b'/' => self.line_comment(),
                b'*' => self.block_comment(b'*', BLOCK_COMMENT),
                b'@' => self.block_comment(b'@', DOC_COMMENT),
                b'#' => self.punct(2, DEV_OPEN),
                b'=' => self.punct(2, SLASHEQ),
                _ => self.punct(1, SLASH),
            },
            b'#' => self.hash(),
            b'"' => {
                self.pos += 1;
                self.string_body(start);
                self.push(STRING, start);
            }
            b'&' => match self.peek(1) {
                b'"' => self.prefixed_string(1, ISTRING),
                b'&' => self.punct(2, AMP2),
                b'=' => self.punct(2, AMPEQ),
                _ => self.punct(1, AMP),
            },
            b'%' => match self.peek(1) {
                b'"' => self.prefixed_string(1, PREFIXED_STRING),
                b'\\' if self.peek(2) == b'x' && self.peek(3).is_ascii_hexdigit() => {
                    self.punct(1, PERCENT);
                    self.encoded_name();
                }
                b'=' => self.punct(2, PERCENTEQ),
                _ => self.punct(1, PERCENT),
            },
            b'@' => match self.peek(1) {
                b'"' => self.prefixed_string(1, PREFIXED_STRING),
                c if is_word(c) => {
                    // Decompiler spelling of an unresolved name: `@name`.
                    self.pos += 1;
                    while self.pos < self.b.len() && is_word(self.b[self.pos]) {
                        self.pos += 1;
                    }
                    self.push(IDENT, start);
                }
                _ => self.unknown(),
            },
            b'0'..=b'9' => self.number(),
            b'.' => {
                if self.peek(1).is_ascii_digit() {
                    self.number()
                } else if self.peek(1) == b'.' && self.peek(2) == b'.' {
                    self.punct(3, DOT3)
                } else {
                    self.punct(1, DOT)
                }
            }
            c if is_word_start(c) => self.word(),
            b'=' => match (self.peek(1), self.peek(2)) {
                (b'=', b'=') => self.punct(3, EQ3),
                (b'=', _) => self.punct(2, EQ2),
                _ => self.punct(1, EQ),
            },
            b'!' => match (self.peek(1), self.peek(2)) {
                (b'=', b'=') => self.punct(3, NEQ2),
                (b'=', _) => self.punct(2, NEQ),
                _ => self.punct(1, BANG),
            },
            b'<' => match (self.peek(1), self.peek(2)) {
                (b'<', b'=') => self.punct(3, SHLEQ),
                (b'<', _) => self.punct(2, SHL),
                (b'=', _) => self.punct(2, LTEQ),
                _ => self.punct(1, LT),
            },
            b'>' => match (self.peek(1), self.peek(2)) {
                (b'>', b'=') => self.punct(3, SHREQ),
                (b'>', _) => self.punct(2, SHR),
                (b'=', _) => self.punct(2, GTEQ),
                _ => self.punct(1, GT),
            },
            b'+' => match self.peek(1) {
                b'+' => self.punct(2, PLUS2),
                b'=' => self.punct(2, PLUSEQ),
                _ => self.punct(1, PLUS),
            },
            b'-' => match self.peek(1) {
                b'-' => self.punct(2, MINUS2),
                b'=' => self.punct(2, MINUSEQ),
                b'>' => self.punct(2, ARROW),
                _ => self.punct(1, MINUS),
            },
            b'*' => match self.peek(1) {
                b'=' => self.punct(2, STAREQ),
                _ => self.punct(1, STAR),
            },
            b'|' => match self.peek(1) {
                b'|' => self.punct(2, PIPE2),
                b'=' => self.punct(2, PIPEEQ),
                _ => self.punct(1, PIPE),
            },
            b'^' => match self.peek(1) {
                b'=' => self.punct(2, CARETEQ),
                _ => self.punct(1, CARET),
            },
            b':' => match self.peek(1) {
                b':' => self.punct(2, COLON2),
                _ => self.punct(1, COLON),
            },
            b'?' => match self.peek(1) {
                b'?' => self.punct(2, QUESTION2),
                b'.' if !self.peek(2).is_ascii_digit() => self.punct(2, QUESTION_DOT),
                _ => self.punct(1, QUESTION),
            },
            b'~' => self.punct(1, TILDE),
            b'(' => self.punct(1, L_PAREN),
            b')' => self.punct(1, R_PAREN),
            b'{' => self.punct(1, L_CURLY),
            b'}' => self.punct(1, R_CURLY),
            b'[' => self.punct(1, L_BRACK),
            b']' => self.punct(1, R_BRACK),
            b';' => self.punct(1, SEMICOLON),
            b',' => self.punct(1, COMMA),
            b'$' => self.punct(1, DOLLAR),
            _ => {
                if self.text[self.pos..].starts_with(['\u{a0}', '\u{feff}']) {
                    self.whitespace()
                } else {
                    self.unknown()
                }
            }
        }
    }

    /// An animation name acts printed as escaped bytes (`%\x83\x99-...`): BO7 names it could not
    /// decrypt. The bytes may hold `$`, `;`, `)` or spaces, so the token cannot end at a delimiter.
    /// The third decoded byte is the length of the rest (the 3-byte header plus `len - 1` bytes),
    /// which holds for 726 of the 741 names of the BO7 archive; the token never crosses a line end.
    fn encoded_name(&mut self) {
        let start = self.pos;
        let mut decoded = 0usize;
        let mut total = usize::MAX;
        while let Some(&c) = self.b.get(self.pos) {
            if matches!(c, b'\n' | b'\r') || decoded >= total {
                break;
            }
            let byte = if c == b'\\' && self.peek(1) == b'x' && self.peek(2).is_ascii_hexdigit() {
                let n = if self.peek(3).is_ascii_hexdigit() {
                    2
                } else {
                    1
                };
                let hex = &self.text[self.pos + 2..self.pos + 2 + n];
                self.pos += 2 + n;
                u8::from_str_radix(hex, 16).unwrap_or(0)
            } else if c == b'\\' && !matches!(self.peek(1), 0 | b'\n' | b'\r') {
                let e = self.peek(1);
                self.pos += 1;
                self.bump_char();
                match e {
                    b'a' => 7,
                    b'b' => 8,
                    b't' => 9,
                    b'n' => 10,
                    b'v' => 11,
                    b'f' => 12,
                    b'r' => 13,
                    other => other,
                }
            } else {
                self.bump_char();
                c
            };
            decoded += 1;
            if decoded == 3 {
                total = usize::from(byte) + 2;
            }
        }
        // Without a usable header, stop before trailing blanks.
        if decoded < total {
            while self.pos > start + 1 && is_hspace(self.b[self.pos - 1]) {
                self.pos -= 1;
            }
        }
        self.push(ENCODED_NAME, start);
    }

    fn punct(&mut self, len: usize, kind: SyntaxKind) {
        let start = self.pos;
        self.pos += len;
        self.push(kind, start);
    }

    fn unknown(&mut self) {
        let start = self.pos;
        self.bump_char();
        self.push(ERROR_TOKEN, start);
        let ch = &self.text[start..self.pos];
        self.error(
            start,
            self.pos,
            "E0102",
            format!("unexpected character {ch:?}"),
        );
    }

    fn whitespace(&mut self) {
        let start = self.pos;
        let mut newline = false;
        loop {
            match self.b.get(self.pos) {
                Some(b' ' | b'\t' | 0x0B | 0x0C) => self.pos += 1,
                Some(b'\n' | b'\r') => {
                    newline = true;
                    self.pos += 1;
                }
                Some(_) if self.text[self.pos..].starts_with(['\u{a0}', '\u{feff}']) => {
                    self.bump_char()
                }
                _ => break,
            }
        }
        self.push(WHITESPACE, start);
        if newline {
            self.line_start = true;
            self.path_next = false;
        }
    }

    fn line_comment(&mut self) {
        let start = self.pos;
        while self.pos < self.b.len() && !matches!(self.b[self.pos], b'\n' | b'\r') {
            self.pos += 1;
        }
        self.push(COMMENT, start);
    }

    /// `/* ... */` or `/@ ... @/`.
    fn block_comment(&mut self, mark: u8, kind: SyntaxKind) {
        let start = self.pos;
        self.pos += 2;
        loop {
            match self.b.get(self.pos) {
                None => {
                    self.error(start, start + 2, "E0103", "unterminated block comment");
                    break;
                }
                Some(&c) if c == mark && self.peek(1) == b'/' => {
                    self.pos += 2;
                    break;
                }
                Some(_) => self.pos += 1,
            }
        }
        // Stay on a char boundary even in malformed input.
        while !self.text.is_char_boundary(self.pos) {
            self.pos += 1;
        }
        self.push(kind, start);
    }

    /// The rest of a string after its opening quote, up to and including the closing quote.
    fn string_body(&mut self, start: usize) {
        loop {
            match self.b.get(self.pos) {
                None | Some(b'\n' | b'\r') => {
                    self.error(start, self.pos, "E0101", "unterminated string literal");
                    break;
                }
                Some(b'"') => {
                    self.pos += 1;
                    break;
                }
                Some(b'\\') => {
                    self.pos += 1;
                    if matches!(self.b.get(self.pos), Some(&c) if c != b'\n' && c != b'\r') {
                        self.bump_char();
                    }
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    fn prefixed_string(&mut self, prefix_len: usize, kind: SyntaxKind) {
        let start = self.pos;
        self.pos += prefix_len + 1;
        self.string_body(start);
        self.push(kind, start);
    }

    /// In decompiler output, extend a name over `/`, `\` and (once path-like) `.` separators
    /// glued to word characters: `maps/creek/fx_name`, `vox\a\b.pc.snd`.
    fn decompiled_name_tail(&mut self, mut path: bool) -> bool {
        loop {
            match self.peek(0) {
                b'/' | b'\\' if is_word(self.peek(1)) => path = true,
                b'.' if path && is_word(self.peek(1)) => {}
                _ => return path,
            }
            self.pos += 1;
            while self.pos < self.b.len() && is_word(self.b[self.pos]) {
                self.pos += 1;
            }
        }
    }

    fn number(&mut self) {
        let start = self.pos;
        if self.dialect.decompiled() && self.b[start].is_ascii_digit() {
            // Decompiler names may start with a digit: `051a_name`, `02/name`.
            let mut end = start;
            while end < self.b.len() && self.b[end].is_ascii_digit() {
                end += 1;
            }
            let next = self.b.get(end).copied().unwrap_or(0);
            let next2 = self.b.get(end + 1).copied().unwrap_or(0);
            let hex = self.b[start] == b'0' && matches!(next, b'x' | b'X');
            let exponent = matches!(next, b'e' | b'E')
                && (next2.is_ascii_digit() || matches!(next2, b'+' | b'-'));
            if !hex && !exponent && (is_word_start(next) || (next == b'/' && is_word(next2))) {
                self.pos = end;
                while self.pos < self.b.len() && is_word(self.b[self.pos]) {
                    self.pos += 1;
                }
                let path = self.decompiled_name_tail(false);
                self.push(if path { PATH } else { IDENT }, start);
                return;
            }
        }
        let mut kind = INT_NUMBER;
        if self.b[self.pos] == b'0'
            && matches!(self.peek(1), b'x' | b'X')
            && self.peek(2).is_ascii_hexdigit()
        {
            self.pos += 2;
            while self.pos < self.b.len() && self.b[self.pos].is_ascii_hexdigit() {
                self.pos += 1;
            }
        } else {
            while self.pos < self.b.len() && self.b[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
            if self.peek(0) == b'.' && self.peek(1) != b'.' && !is_word_start(self.peek(1)) {
                kind = FLOAT_NUMBER;
                self.pos += 1;
                while self.pos < self.b.len() && self.b[self.pos].is_ascii_digit() {
                    self.pos += 1;
                }
            }
            if matches!(self.peek(0), b'e' | b'E') {
                let sign = usize::from(matches!(self.peek(1), b'+' | b'-'));
                if self.peek(1 + sign).is_ascii_digit() {
                    kind = FLOAT_NUMBER;
                    self.pos += 1 + sign;
                    while self.pos < self.b.len() && self.b[self.pos].is_ascii_digit() {
                        self.pos += 1;
                    }
                }
            }
        }
        self.push(kind, start);
    }

    fn word(&mut self) {
        let start = self.pos;
        while self.pos < self.b.len() && is_word(self.b[self.pos]) {
            self.pos += 1;
        }
        if self.pos - start == 1
            && matches!(self.b[start], b't' | b's' | b'r' | b'o')
            && self.peek(0) == b'"'
        {
            self.pos += 1;
            self.string_body(start);
            self.push(PREFIXED_STRING, start);
            return;
        }
        let mut kind = IDENT;
        while self.peek(0) == b'\\' && is_word(self.peek(1)) {
            kind = PATH;
            self.pos += 1;
            while self.pos < self.b.len() && is_word(self.b[self.pos]) {
                self.pos += 1;
            }
        }
        if self.dialect.decompiled() && self.decompiled_name_tail(kind == PATH) {
            kind = PATH;
        }
        if kind == IDENT {
            if let Some(k) = self.dialect.keyword(&self.text[start..self.pos]) {
                kind = k;
            }
        }
        self.push(kind, start);
    }

    fn hash(&mut self) {
        let start = self.pos;
        match self.peek(1) {
            b'/' => self.punct(2, DEV_CLOSE),
            b'"' => self.prefixed_string(1, HASH_STRING),
            c if self.dialect.decompiled() && self.bare_hash_ends(c) => {
                // acts prints a zero hash value as a bare `#` (`x ?? #;`, beside `%""`).
                self.punct(1, HASH_IDENT);
            }
            c if is_word_start(c) => {
                let mut end = self.pos + 1;
                while end < self.b.len() && is_word(self.b[end]) {
                    end += 1;
                }
                let word = &self.text[self.pos + 1..end];
                let directive = DIRECTIVE_WORDS
                    .iter()
                    .find(|(w, _)| *w == word)
                    .map(|(_, k)| *k)
                    .filter(|k| self.line_start || *k == ANIMTREE_KW);
                match directive {
                    Some(kind) => {
                        self.pos = end;
                        self.directive(kind, start)
                    }
                    None => {
                        // A hashed name such as `#var_1a2b` or `#tm7/name` (decompiler output).
                        self.pos = end;
                        while self.peek(0) == b'/' && is_word(self.peek(1)) {
                            self.pos += 1;
                            while self.pos < self.b.len() && is_word(self.b[self.pos]) {
                                self.pos += 1;
                            }
                        }
                        self.push(HASH_IDENT, start);
                    }
                }
            }
            _ => self.unknown(),
        }
    }

    /// Can a bare `#` followed by `next` stand alone as a value (`;`, `,`, `)`, `]`, `}` or `:`
    /// next, after blanks)?
    fn bare_hash_ends(&self, next: u8) -> bool {
        let mut i = self.pos + 1;
        if is_hspace(next) {
            while i < self.b.len() && is_hspace(self.b[i]) {
                i += 1;
            }
        }
        matches!(self.b.get(i), Some(b';' | b',' | b')' | b']' | b'}' | b':'))
    }

    fn directive(&mut self, kind: SyntaxKind, start: usize) {
        match kind {
            PP_IF | PP_IFDEF | PP_IFNDEF | PP_ELIF | PP_ELSE | PP_ENDIF => {
                self.directive_line();
                self.push(kind, start);
                self.conditional(kind, start);
            }
            DEFINE_KW => {
                self.push(kind, start);
                self.define();
            }
            USING_KW | INCLUDE_KW | INSERT_KW | INLINE_KW => {
                self.push(kind, start);
                self.path_next = true;
            }
            _ => self.push(kind, start),
        }
    }

    /// Advance to the end of a directive line (honouring `\` continuations), stopping before a
    /// trailing `//` comment and trailing blanks.
    fn directive_line(&mut self) {
        self.pos = self.directive_body(false);
    }

    /// Scan a directive body to the end of its line (honouring `\` continuations), stopping at a
    /// line break or before a `//` comment; strings and `/* */` comments are skipped whole.
    /// Leaves `pos` there and returns the end of the content: past the last non-blank byte, and
    /// past the last continuation too when `continuations_are_content` (a macro body keeps them).
    fn directive_body(&mut self, continuations_are_content: bool) -> usize {
        let mut content_end = self.pos;
        loop {
            match self.b.get(self.pos) {
                None | Some(b'\n' | b'\r') => break,
                Some(b'\\') if self.line_continues() => {
                    self.skip_continuation();
                    if continuations_are_content {
                        content_end = self.pos;
                    }
                }
                Some(b'/') if self.peek(1) == b'/' => break,
                Some(b'/') if self.peek(1) == b'*' => {
                    while self.pos < self.b.len()
                        && !(self.b[self.pos] == b'*' && self.peek(1) == b'/')
                    {
                        self.pos += 1;
                    }
                    self.pos = (self.pos + 2).min(self.b.len());
                    content_end = self.pos;
                }
                Some(b'"') => {
                    let s = self.pos;
                    self.pos += 1;
                    self.string_body(s);
                    content_end = self.pos;
                }
                Some(&c) => {
                    self.bump_char();
                    if !is_hspace(c) {
                        content_end = self.pos;
                    }
                }
            }
        }
        while !self.text.is_char_boundary(content_end) {
            content_end += 1;
        }
        content_end
    }

    fn line_continues(&self) -> bool {
        matches!(self.peek(1), b'\n' | b'\r')
    }

    fn skip_continuation(&mut self) {
        self.pos += 1;
        if self.peek(0) == b'\r' {
            self.pos += 1;
        }
        if self.peek(0) == b'\n' {
            self.pos += 1;
        }
    }

    /// Track `#if` nesting. The first branch of a group is the active one; the text of every later
    /// branch (`#elif`, `#else`) up to the matching `#endif` is one `DISABLED_TEXT` token.
    fn conditional(&mut self, kind: SyntaxKind, start: usize) {
        match kind {
            PP_IF | PP_IFDEF | PP_IFNDEF => self.pp_open.push(start),
            PP_ENDIF => {
                if self.pp_open.pop().is_none() {
                    self.error(
                        start,
                        self.pos,
                        "E0104",
                        "`#endif` without a matching `#if`",
                    );
                }
            }
            _ => {
                if self.pp_open.is_empty() {
                    self.error(start, self.pos, "E0104", "`#elif` or `#else` without `#if`");
                    return;
                }
                let skip_start = self.pos;
                let end = self.find_endif();
                if end > skip_start {
                    self.pos = end;
                    self.push(DISABLED_TEXT, skip_start);
                }
                self.line_start = true;
            }
        }
    }

    /// The offset of the `#` of the `#endif` closing the current group, or the end of the text.
    fn find_endif(&self) -> usize {
        let mut depth = 0u32;
        let mut i = self.pos;
        // Skip the rest of the current line.
        while i < self.b.len() && !matches!(self.b[i], b'\n' | b'\r') {
            i += 1;
        }
        while i < self.b.len() {
            while i < self.b.len() && matches!(self.b[i], b'\n' | b'\r') {
                i += 1;
            }
            let mut j = i;
            while j < self.b.len() && is_hspace(self.b[j]) {
                j += 1;
            }
            if self.b.get(j) == Some(&b'#') {
                let mut k = j + 1;
                while k < self.b.len() && is_word(self.b[k]) {
                    k += 1;
                }
                match &self.text[j + 1..k] {
                    "if" | "ifdef" | "ifndef" => depth += 1,
                    "endif" if depth == 0 => return j,
                    "endif" => depth -= 1,
                    _ => {}
                }
            }
            i = j;
            while i < self.b.len() && !matches!(self.b[i], b'\n' | b'\r') {
                i += 1;
            }
        }
        self.b.len()
    }

    fn hspace(&mut self) -> bool {
        let start = self.pos;
        loop {
            match self.b.get(self.pos) {
                Some(&c) if is_hspace(c) => self.pos += 1,
                Some(b'\\') if self.line_continues() => self.skip_continuation(),
                _ => break,
            }
        }
        if self.pos > start {
            self.push(WHITESPACE, start);
            true
        } else {
            false
        }
    }

    /// `#define NAME`, optional `( params )`, then the body up to the end of the line.
    fn define(&mut self) {
        self.hspace();
        if !is_word_start(self.peek(0)) {
            return;
        }
        let start = self.pos;
        while self.pos < self.b.len() && is_word(self.b[self.pos]) {
            self.pos += 1;
        }
        self.push(IDENT, start);
        if self.peek(0) == b'(' {
            while self.pos < self.b.len() && !matches!(self.b[self.pos], b'\n' | b'\r') {
                if self.hspace() {
                    continue;
                }
                let c = self.b[self.pos];
                if is_word_start(c) {
                    let s = self.pos;
                    while self.pos < self.b.len() && is_word(self.b[self.pos]) {
                        self.pos += 1;
                    }
                    self.push(IDENT, s);
                    continue;
                }
                let before = self.tokens.len();
                self.token();
                if self.tokens.len() > before && self.tokens[before].kind == R_PAREN {
                    break;
                }
            }
        }
        self.hspace();
        let body_start = self.pos;
        let body_end = self.directive_body(true);
        let line_end = self.pos;
        if body_end > body_start {
            self.pos = body_end;
            self.push(MACRO_BODY, body_start);
        }
        if line_end > body_end {
            self.pos = line_end;
            let s = body_end;
            self.push(WHITESPACE, s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The non-whitespace tokens of `text` with their text.
    fn significant<'a>(text: &'a str, tokens: &[Token]) -> Vec<(SyntaxKind, &'a str)> {
        let mut pos = 0;
        tokens
            .iter()
            .map(|t| {
                let s = &text[pos..pos + t.len as usize];
                pos += t.len as usize;
                (t.kind, s)
            })
            .filter(|(k, _)| *k != WHITESPACE)
            .collect()
    }

    fn kinds(text: &str) -> Vec<(SyntaxKind, &str)> {
        let (tokens, _) = tokenize(text, &Dialect::superset());
        significant(text, &tokens)
    }

    #[test]
    fn lexes_hash_literals_and_pointers() {
        assert_eq!(
            kinds(r##"x = #"a" + &"B" + %"c" + &f + ::g + #k;"##),
            vec![
                (IDENT, "x"),
                (EQ, "="),
                (HASH_STRING, "#\"a\""),
                (PLUS, "+"),
                (ISTRING, "&\"B\""),
                (PLUS, "+"),
                (PREFIXED_STRING, "%\"c\""),
                (PLUS, "+"),
                (AMP, "&"),
                (IDENT, "f"),
                (PLUS, "+"),
                (COLON2, "::"),
                (IDENT, "g"),
                (PLUS, "+"),
                (HASH_IDENT, "#k"),
                (SEMICOLON, ";"),
            ]
        );
    }

    #[test]
    fn lexes_directives() {
        let k = kinds("#using a\\b/c;\n#define F(x, y) x + \\\n y // c\n#insert p.gsh;");
        assert_eq!(
            k,
            vec![
                (USING_KW, "#using"),
                (PATH, "a\\b/c"),
                (SEMICOLON, ";"),
                (DEFINE_KW, "#define"),
                (IDENT, "F"),
                (L_PAREN, "("),
                (IDENT, "x"),
                (COMMA, ","),
                (IDENT, "y"),
                (R_PAREN, ")"),
                (MACRO_BODY, "x + \\\n y"),
                (COMMENT, "// c"),
                (INSERT_KW, "#insert"),
                (PATH, "p.gsh"),
                (SEMICOLON, ";"),
            ]
        );
    }

    #[test]
    fn inactive_branches_are_disabled_text() {
        let k = kinds("#if A\nf();\n#else // c\ng(\n#endif\nh();");
        assert_eq!(
            k,
            vec![
                (PP_IF, "#if A"),
                (IDENT, "f"),
                (L_PAREN, "("),
                (R_PAREN, ")"),
                (SEMICOLON, ";"),
                (PP_ELSE, "#else"),
                (DISABLED_TEXT, " // c\ng(\n"),
                (PP_ENDIF, "#endif"),
                (IDENT, "h"),
                (L_PAREN, "("),
                (R_PAREN, ")"),
                (SEMICOLON, ";"),
            ]
        );
    }

    #[test]
    fn dev_blocks_and_comments() {
        let k = kinds("/#println(1);#/ /@ doc @/ /* b */ a/b");
        assert_eq!(
            k,
            vec![
                (DEV_OPEN, "/#"),
                (IDENT, "println"),
                (L_PAREN, "("),
                (INT_NUMBER, "1"),
                (R_PAREN, ")"),
                (SEMICOLON, ";"),
                (DEV_CLOSE, "#/"),
                (DOC_COMMENT, "/@ doc @/"),
                (BLOCK_COMMENT, "/* b */"),
                (IDENT, "a"),
                (SLASH, "/"),
                (IDENT, "b"),
            ]
        );
    }

    #[test]
    fn numbers_and_paths() {
        assert_eq!(
            kinds("1 1.5 .5 0x1F 2e3 a\\b::c x.size"),
            vec![
                (INT_NUMBER, "1"),
                (FLOAT_NUMBER, "1.5"),
                (FLOAT_NUMBER, ".5"),
                (INT_NUMBER, "0x1F"),
                (FLOAT_NUMBER, "2e3"),
                (PATH, "a\\b"),
                (COLON2, "::"),
                (IDENT, "c"),
                (IDENT, "x"),
                (DOT, "."),
                (SIZE_KW, "size"),
            ]
        );
    }

    fn kinds_decompiled(text: &str) -> Vec<(SyntaxKind, &str)> {
        let d = Dialect::superset().with_decompiled(true);
        let (tokens, errors) = tokenize(text, &d);
        assert!(errors.is_empty(), "{errors:?}");
        significant(text, &tokens)
    }

    #[test]
    fn encoded_animation_names_are_one_token() {
        // Header byte 3 is 0x06: 8 bytes in all, holding `$`, `;`, `)` and a space.
        let text = r"x = %\x9f\x01\x06$;) \x80;";
        assert_eq!(
            kinds_decompiled(text),
            vec![
                (IDENT, "x"),
                (EQ, "="),
                (PERCENT, "%"),
                (ENCODED_NAME, r"\x9f\x01\x06$;) \x80"),
                (SEMICOLON, ";"),
            ]
        );
        // An escaped quote counts as one byte: 0x22 is the header length here.
        let (tokens, _) = tokenize(r#"%\x01\x02\"a"#, &Dialect::superset());
        assert_eq!(tokens[1].kind, ENCODED_NAME);
        // A quoted asset name stays a prefixed string.
        assert_eq!(
            kinds_decompiled(r#"f( %"hash_12ab", %"" );"#)[2],
            (PREFIXED_STRING, r#"%"hash_12ab""#)
        );
    }

    #[test]
    fn bare_hash_is_a_zero_hash_in_decompiler_output() {
        assert_eq!(kinds_decompiled("x = y ?? #;")[4], (HASH_IDENT, "#"));
        let (tokens, errors) = tokenize("x = #;", &Dialect::superset());
        assert!(tokens.iter().any(|t| t.kind == ERROR_TOKEN));
        assert_eq!(errors[0].code, "E0102");
    }

    #[test]
    fn lossless_on_garbage() {
        let text = "\u{feff}\"abc\n#if\n`é#/@";
        let (tokens, errors) = tokenize(text, &Dialect::superset());
        let total: u32 = tokens.iter().map(|t| t.len).sum();
        assert_eq!(total as usize, text.len());
        assert!(!errors.is_empty());
    }
}
