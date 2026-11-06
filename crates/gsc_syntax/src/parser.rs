//! Parser core: an event-emitting recursive-descent parser (matklad's resilient LL design).
//!
//! The grammar functions live in `grammar`. They see only non-trivia tokens and emit
//! `Open`/`Close`/`Advance` events; [`build_tree`] replays the events over the full token list
//! (trivia included) into a lossless rowan tree. A fuel counter turns a loop that stops making
//! progress into an early end of input, so the parser always terminates and never panics.

use std::cell::Cell;

use rowan::{GreenNode, GreenNodeBuilder, TextRange, TextSize};

use crate::lexer::Token;
use crate::{Dialect, SyntaxError, SyntaxKind};
use SyntaxKind::*;

/// Lookahead budget between two consumed tokens.
const FUEL: u32 = 16 * 1024;
/// Maximum nesting of statements and expressions before the parser stops descending.
pub(crate) const MAX_DEPTH: u32 = 256;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Event {
    Open { kind: SyntaxKind },
    Close,
    Advance { remap: Option<SyntaxKind> },
}

pub(crate) struct MarkOpened {
    index: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct MarkClosed {
    index: usize,
}

pub(crate) struct Parser<'t> {
    text: &'t str,
    /// Kinds of the non-trivia tokens.
    kinds: Vec<SyntaxKind>,
    /// Start offset of each non-trivia token, plus the end of the text.
    starts: Vec<u32>,
    /// End offset of each non-trivia token.
    ends: Vec<u32>,
    /// Whether trivia holding a line break precedes the token.
    newline_before: Vec<bool>,
    /// Whether trivia of any kind precedes the token.
    space_before: Vec<bool>,
    pos: usize,
    fuel: Cell<u32>,
    stuck: Cell<bool>,
    events: Vec<Event>,
    errors: Vec<SyntaxError>,
    pub(crate) depth: u32,
    pub(crate) dev_depth: u32,
    pub(crate) dialect: &'t Dialect,
}

impl<'t> Parser<'t> {
    pub(crate) fn new(text: &'t str, tokens: &[Token], dialect: &'t Dialect) -> Parser<'t> {
        let mut kinds = Vec::with_capacity(tokens.len() / 2);
        let mut starts = Vec::with_capacity(tokens.len() / 2);
        let mut ends = Vec::with_capacity(tokens.len() / 2);
        let mut newline_before = Vec::with_capacity(tokens.len() / 2);
        let mut space_before = Vec::with_capacity(tokens.len() / 2);
        let mut offset = 0u32;
        let mut nl = true;
        let mut sp = false;
        for t in tokens {
            if t.kind.is_trivia() {
                sp = true;
                let s = &text[offset as usize..(offset + t.len) as usize];
                if s.contains(['\n', '\r']) {
                    nl = true;
                }
            } else {
                kinds.push(t.kind);
                starts.push(offset);
                ends.push(offset + t.len);
                newline_before.push(nl);
                space_before.push(sp);
                nl = false;
                sp = false;
            }
            offset += t.len;
        }
        starts.push(offset);
        Parser {
            text,
            kinds,
            starts,
            ends,
            newline_before,
            space_before,
            pos: 0,
            fuel: Cell::new(FUEL),
            stuck: Cell::new(false),
            events: Vec::new(),
            errors: Vec::new(),
            depth: 0,
            dev_depth: 0,
            dialect,
        }
    }

    pub(crate) fn finish(self) -> (Vec<Event>, Vec<SyntaxError>) {
        (self.events, self.errors)
    }

    // ----- markers -------------------------------------------------------------------------

    pub(crate) fn open(&mut self) -> MarkOpened {
        let m = MarkOpened {
            index: self.events.len(),
        };
        self.events.push(Event::Open { kind: ERROR });
        m
    }

    /// Open a node that starts before the already closed node `m` (Pratt left wrapping).
    pub(crate) fn open_before(&mut self, m: MarkClosed) -> MarkOpened {
        self.events.insert(m.index, Event::Open { kind: ERROR });
        MarkOpened { index: m.index }
    }

    pub(crate) fn close(&mut self, m: MarkOpened, kind: SyntaxKind) -> MarkClosed {
        self.events[m.index] = Event::Open { kind };
        self.events.push(Event::Close);
        MarkClosed { index: m.index }
    }

    /// Drop a node that turned out to be unnecessary; its children stay in the parent.
    pub(crate) fn abandon(&mut self, m: MarkOpened) {
        self.events[m.index] = Event::Open { kind: TOMBSTONE };
        self.events.push(Event::Close);
    }

    // ----- lookahead -----------------------------------------------------------------------

    /// Kind of the `n`-th token ahead. Spends fuel; when the fuel is gone, returns `EOF`.
    pub(crate) fn nth(&self, n: usize) -> SyntaxKind {
        if self.stuck.get() {
            return EOF;
        }
        let fuel = self.fuel.get();
        if fuel == 0 {
            self.stuck.set(true);
            return EOF;
        }
        self.fuel.set(fuel - 1);
        self.kinds.get(self.pos + n).copied().unwrap_or(EOF)
    }

    pub(crate) fn current(&self) -> SyntaxKind {
        self.nth(0)
    }

    pub(crate) fn at(&self, kind: SyntaxKind) -> bool {
        self.nth(0) == kind
    }

    pub(crate) fn at_any(&self, kinds: &[SyntaxKind]) -> bool {
        kinds.contains(&self.nth(0))
    }

    /// Kind of the token at absolute index `i`, without spending fuel.
    pub(crate) fn kind_at(&self, i: usize) -> SyntaxKind {
        self.kinds.get(i).copied().unwrap_or(EOF)
    }

    /// Text of the token at absolute index `i`.
    pub(crate) fn text_at(&self, i: usize) -> &'t str {
        self.token_text(i)
    }

    pub(crate) fn position(&self) -> usize {
        self.pos
    }

    /// Is the `n`-th token ahead the contextual word `word` (an identifier, any case)?
    pub(crate) fn at_contextual(&self, n: usize, word: &str) -> bool {
        self.nth(n) == IDENT && self.token_text(self.pos + n).eq_ignore_ascii_case(word)
    }

    fn token_text(&self, i: usize) -> &'t str {
        if i >= self.kinds.len() {
            return "";
        }
        &self.text[self.starts[i] as usize..self.ends[i] as usize]
    }

    /// Does a line break separate the `n`-th token ahead from the token before it?
    pub(crate) fn newline_before(&self, n: usize) -> bool {
        self.newline_before
            .get(self.pos + n)
            .copied()
            .unwrap_or(true)
    }

    /// Are the `n`-th and `n+1`-th tokens ahead adjacent, with no trivia between them?
    pub(crate) fn joint(&self, n: usize) -> bool {
        !self
            .space_before
            .get(self.pos + n + 1)
            .copied()
            .unwrap_or(true)
    }

    /// Is the `n`-th token ahead glued to the token before it?
    pub(crate) fn joint_before(&self, n: usize) -> bool {
        !self.space_before.get(self.pos + n).copied().unwrap_or(true)
    }

    /// Have all tokens been consumed?
    pub(crate) fn at_real_eof(&self) -> bool {
        self.pos >= self.kinds.len()
    }

    pub(crate) fn unstick(&mut self) {
        self.stuck.set(false);
        self.fuel.set(FUEL);
    }

    // ----- consuming -----------------------------------------------------------------------

    pub(crate) fn bump(&mut self) {
        self.bump_remap_opt(None)
    }

    /// Consume the current token, storing it in the tree with kind `kind` (contextual keywords).
    pub(crate) fn bump_remap(&mut self, kind: SyntaxKind) {
        self.bump_remap_opt(Some(kind))
    }

    fn bump_remap_opt(&mut self, remap: Option<SyntaxKind>) {
        if self.pos >= self.kinds.len() {
            return;
        }
        self.fuel.set(FUEL);
        self.events.push(Event::Advance { remap });
        self.pos += 1;
    }

    pub(crate) fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub(crate) fn expect(&mut self, kind: SyntaxKind) -> bool {
        if self.eat(kind) {
            return true;
        }
        self.error_expected(&describe(kind));
        false
    }

    // ----- errors --------------------------------------------------------------------------

    /// Range of the current token, or an empty range at the end of the text.
    fn current_range(&self) -> TextRange {
        let i = self.pos.min(self.kinds.len());
        let start = TextSize::from(self.starts[i]);
        if i >= self.kinds.len() {
            return TextRange::empty(start);
        }
        TextRange::new(start, TextSize::from(self.ends[i]))
    }

    pub(crate) fn error(&mut self, code: &'static str, message: impl Into<String>) {
        let range = self.current_range();
        // One error per position is enough; later ones are cascades.
        if self.errors.last().is_some_and(|e| e.range == range) {
            return;
        }
        self.errors.push(SyntaxError::new(code, message, range));
    }

    /// Report an error on the token just consumed.
    pub(crate) fn error_at_previous(&mut self, code: &'static str, message: impl Into<String>) {
        let i = self
            .pos
            .saturating_sub(1)
            .min(self.kinds.len().saturating_sub(1));
        if i >= self.kinds.len() {
            return;
        }
        let range = TextRange::new(TextSize::from(self.starts[i]), TextSize::from(self.ends[i]));
        self.errors.push(SyntaxError::new(code, message, range));
    }

    pub(crate) fn error_expected(&mut self, what: &str) {
        let found = self.found();
        self.error("E0201", format!("expected {what}, found {found}"));
    }

    pub(crate) fn found(&self) -> String {
        let i = self.pos;
        if i >= self.kinds.len() {
            return "end of file".to_owned();
        }
        let text = self.token_text(i);
        match self.kinds[i] {
            k if k.text().is_some() => format!("`{text}`"),
            _ => {
                let short: String = text.chars().take(24).collect();
                format!("`{short}`")
            }
        }
    }

    /// Wrap the current token in an error node.
    pub(crate) fn advance_with_error(&mut self, code: &'static str, message: impl Into<String>) {
        let m = self.open();
        self.error(code, message);
        self.bump();
        self.close(m, ERROR);
    }

    /// Wrap tokens in one error node until a token of `recovery` (at least one token).
    pub(crate) fn skip_until(&mut self, recovery: &[SyntaxKind], code: &'static str, msg: String) {
        let m = self.open();
        self.error(code, msg);
        self.bump();
        while !self.at(EOF) && !self.at_any(recovery) {
            self.bump();
        }
        self.close(m, ERROR);
    }
}

/// How an error message names a token kind.
pub(crate) fn describe(kind: SyntaxKind) -> String {
    match kind {
        IDENT => "an identifier".to_owned(),
        PATH => "a script path".to_owned(),
        STRING => "a string".to_owned(),
        k => match k.text() {
            Some(t) => format!("`{t}`"),
            None => format!("{k:?}"),
        },
    }
}

/// Replay `events` over `tokens` (trivia included) into a green tree.
pub(crate) fn build_tree(text: &str, tokens: &[Token], events: Vec<Event>) -> GreenNode {
    let mut builder = GreenNodeBuilder::new();
    let mut offset = 0usize;
    let mut i = 0usize;
    let mut depth = 0usize;
    let mut skip_close: Vec<bool> = Vec::new();

    fn emit(
        builder: &mut GreenNodeBuilder,
        text: &str,
        t: Token,
        offset: &mut usize,
        kind: SyntaxKind,
    ) {
        let end = *offset + t.len as usize;
        builder.token(kind.into(), &text[*offset..end]);
        *offset = end;
    }

    let flush_trivia = |builder: &mut GreenNodeBuilder, i: &mut usize, offset: &mut usize| {
        while *i < tokens.len() && tokens[*i].kind.is_trivia() {
            emit(builder, text, tokens[*i], offset, tokens[*i].kind);
            *i += 1;
        }
    };

    let n = events.len();
    for (e_idx, event) in events.into_iter().enumerate() {
        match event {
            Event::Open { kind: TOMBSTONE } => skip_close.push(true),
            Event::Open { kind } => {
                skip_close.push(false);
                if depth > 0 {
                    flush_trivia(&mut builder, &mut i, &mut offset);
                }
                builder.start_node(kind.into());
                depth += 1;
            }
            Event::Close => {
                if skip_close.pop() == Some(true) {
                    continue;
                }
                depth -= 1;
                if depth == 0 || e_idx + 1 == n {
                    // The root: everything left goes inside it.
                    while i < tokens.len() {
                        emit(&mut builder, text, tokens[i], &mut offset, tokens[i].kind);
                        i += 1;
                    }
                }
                builder.finish_node();
            }
            Event::Advance { remap } => {
                flush_trivia(&mut builder, &mut i, &mut offset);
                if i < tokens.len() {
                    let t = tokens[i];
                    emit(&mut builder, text, t, &mut offset, remap.unwrap_or(t.kind));
                    i += 1;
                }
            }
        }
    }
    builder.finish()
}
