//! The minimal style (`style = "minimal"`): a gofmt-style whitespace normaliser.
//!
//! It keeps the author's line breaks and only rewrites horizontal whitespace between code tokens
//! and the indentation of statement lines. Comments, strings, dev blocks, directives, `#if`
//! regions and continuation lines stay as written.

use gsc_syntax::{NodeOrToken, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken, T};
use SyntaxKind::*;

use crate::{line_ending, trim_hspace_end, Indent, Options, ParenSpacing};

/// Format a parsed file in the minimal style.
pub(crate) fn format(root: &SyntaxNode, text: &str, opts: &Options) -> String {
    Printer::new(root, text, opts).print()
}

/// What to put between two code tokens on the same line.
enum Gap {
    /// Exactly one space.
    Space,
    /// At least one space: keep the original blanks (column alignment), or add one.
    SomeSpace,
    Nothing,
    Keep,
}

/// Node kinds whose spacing and indentation stay as written.
fn is_verbatim_node(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        DEV_BLOCK_STMT
            | DEV_BLOCK_ITEM
            | USING_DIRECTIVE
            | INCLUDE_DIRECTIVE
            | INSERT_DIRECTIVE
            | INLINE_DIRECTIVE
            | NAMESPACE_DIRECTIVE
            | PRECACHE_DIRECTIVE
            | USING_ANIMTREE_DIRECTIVE
            | PREPROC_DEFINE
            | PREPROC_UNDEF
            | MACRO_CALL_ITEM
            | MACRO_STMT
            | ERROR
    )
}

/// Statement-like nodes: a line that starts one of these is re-indented.
fn is_statement_like(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        FUNCTION_DEF
            | CLASS_DEF
            | CLASS_VAR
            | CONSTRUCTOR
            | DESTRUCTOR
            | FILE_CONSTANT
            | BLOCK
            | EXPR_STMT
            | IF_STMT
            | ELSE_BRANCH
            | WHILE_STMT
            | DO_WHILE_STMT
            | FOR_STMT
            | FOREACH_STMT
            | SWITCH_STMT
            | SWITCH_CLAUSE
            | BREAK_STMT
            | CONTINUE_STMT
            | RETURN_STMT
            | WAIT_STMT
            | WAIT_REAL_TIME_STMT
            | WAIT_FRAME_STMT
            | WAITTILL_FRAME_END_STMT
            | CONST_STMT
            | PROFILER_STMT
            | BREAKPOINT_STMT
            | GOTO_STMT
            | LABEL_STMT
            | EMPTY_STMT
    )
}

fn is_control(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        IF_STMT | WHILE_STMT | DO_WHILE_STMT | FOR_STMT | FOREACH_STMT | SWITCH_STMT
    )
}

/// Is this `(` or `)` one whose inside follows [`ParenSpacing`]?
fn is_spaced_paren(tok: &SyntaxToken) -> bool {
    matches!(tok.kind(), L_PAREN | R_PAREN)
        && tok
            .parent()
            .is_some_and(|p| matches!(p.kind(), ARG_LIST | PARAM_LIST) || is_control(p.kind()))
}

/// Is this token a binary, assignment or ternary operator?
fn is_binary_op(tok: &SyntaxToken) -> bool {
    let Some(p) = tok.parent() else {
        return false;
    };
    match p.kind() {
        BIN_EXPR | ASSIGN_EXPR => tok.kind().is_punct(),
        TERNARY_EXPR => matches!(tok.kind(), QUESTION | COLON),
        DEFAULT_VALUE => tok.kind() == EQ,
        _ => false,
    }
}

fn is_layout(kind: SyntaxKind) -> bool {
    kind == WHITESPACE || kind.is_trivia()
}

/// The first token of `node` that is not whitespace or trivia.
fn first_code_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    let end = node.text_range().end();
    let mut t = node.first_token()?;
    while is_layout(t.kind()) {
        t = t.next_token()?;
        if t.text_range().start() >= end {
            return None;
        }
    }
    Some(t)
}

/// The largest element that starts with `tok`: `tok` itself, or its highest ancestor whose first
/// code token is `tok`.
fn unit_of(tok: &SyntaxToken) -> SyntaxElement {
    let mut el = SyntaxElement::Token(tok.clone());
    let mut node = tok.parent();
    while let Some(n) = node {
        if n.kind() == SOURCE_FILE || first_code_token(&n).as_ref() != Some(tok) {
            break;
        }
        node = n.parent();
        el = SyntaxElement::Node(n);
    }
    el
}

fn is_brace(el: &SyntaxElement) -> bool {
    matches!(el, NodeOrToken::Token(t) if matches!(t.kind(), L_CURLY | R_CURLY))
}

/// The indentation level of a statement-like element.
fn level(el: &SyntaxElement) -> usize {
    let Some(p) = el.parent() else {
        return 0;
    };
    let pk = p.kind();
    let parent_level = || level(&SyntaxElement::Node(p.clone()));
    let kind = el.kind();
    let is_token = el.as_token().is_some();
    match pk {
        SOURCE_FILE => 0,
        BLOCK | CLASS_DEF | SWITCH_STMT if is_brace(el) => parent_level(),
        BLOCK | CLASS_DEF | SWITCH_STMT => parent_level() + 1,
        SWITCH_CLAUSE if kind == BLOCK => parent_level(),
        SWITCH_CLAUSE => parent_level() + 1,
        IF_STMT | WHILE_STMT | DO_WHILE_STMT | FOR_STMT | FOREACH_STMT
            if kind == BLOCK || kind == ELSE_BRANCH || is_token =>
        {
            parent_level()
        }
        IF_STMT | WHILE_STMT | DO_WHILE_STMT | FOR_STMT | FOREACH_STMT => parent_level() + 1,
        ELSE_BRANCH if kind == BLOCK || kind == IF_STMT || is_token => parent_level(),
        ELSE_BRANCH => parent_level() + 1,
        _ => parent_level(),
    }
}

/// The indentation level of the line that starts with `tok`, or `None` if the line keeps its
/// indentation (a continuation line, a comment, verbatim text).
fn line_level(tok: &SyntaxToken) -> Option<usize> {
    let el = unit_of(tok);
    let parent = el.parent()?;
    let ok = match &el {
        NodeOrToken::Node(n) => {
            let body = n.kind() == BLOCK
                && matches!(parent.kind(), FUNCTION_DEF | CONSTRUCTOR | DESTRUCTOR);
            body || is_statement_like(n.kind())
                && (matches!(
                    parent.kind(),
                    SOURCE_FILE | BLOCK | CLASS_DEF | SWITCH_STMT | SWITCH_CLAUSE | ELSE_BRANCH
                ) || is_control(parent.kind()))
        }
        NodeOrToken::Token(t) => {
            (matches!(t.kind(), L_CURLY | R_CURLY)
                && matches!(parent.kind(), BLOCK | CLASS_DEF | SWITCH_STMT))
                || (t.kind() == WHILE_KW && parent.kind() == DO_WHILE_STMT)
        }
    };
    ok.then(|| level(&el))
}

struct Printer<'a> {
    tokens: Vec<SyntaxToken>,
    /// Tokens whose surrounding whitespace is left alone: dev blocks, directives, `#if` regions,
    /// macro invocations.
    verbatim: Vec<bool>,
    text: &'a str,
    opts: &'a Options,
    out: String,
}

impl<'a> Printer<'a> {
    fn new(root: &SyntaxNode, text: &'a str, opts: &'a Options) -> Printer<'a> {
        let tokens: Vec<SyntaxToken> = root
            .descendants_with_tokens()
            .filter_map(|e| e.into_token())
            .collect();
        let mut verbatim = Vec::with_capacity(tokens.len());
        let mut pp_depth = 0usize;
        for t in &tokens {
            let in_pp = match t.kind() {
                PP_IF | PP_IFDEF | PP_IFNDEF => {
                    pp_depth += 1;
                    true
                }
                PP_ENDIF => {
                    pp_depth = pp_depth.saturating_sub(1);
                    true
                }
                _ => pp_depth > 0,
            };
            let v = in_pp
                || t.kind().is_directive()
                || t.parent_ancestors().any(|a| is_verbatim_node(a.kind()));
            verbatim.push(v);
        }
        Printer {
            tokens,
            verbatim,
            text,
            opts,
            out: String::with_capacity(text.len() + text.len() / 8),
        }
    }

    fn print(mut self) -> String {
        let n = self.tokens.len();
        if self.tokens.iter().all(|t| t.kind() == WHITESPACE) {
            // An empty or blank file: nothing to keep.
            return String::new();
        }
        for i in 0..n {
            let kind = self.tokens[i].kind();
            let prev = i.checked_sub(1);
            if kind == WHITESPACE {
                let ws = self.tokens[i].text().to_owned();
                let splice = ws.contains('\\')
                    || prev.is_some_and(|p| self.tokens[p].text().ends_with('\\'));
                if splice {
                    // A line splice: leave it exactly as written.
                    self.out.push_str(&ws);
                } else if i + 1 == n {
                    // Trailing whitespace of the file: replaced by the final newline below.
                } else if ws.contains(['\n', '\r']) {
                    self.line_break(&ws, i + 1);
                } else {
                    match prev {
                        Some(p) if self.tokens[p].kind() != BOM => self.gap(p, i + 1, &ws),
                        // The indentation of the first line.
                        _ => self.indent(&ws, i + 1),
                    }
                }
                continue;
            }
            if let Some(p) = prev.filter(|&p| self.tokens[p].kind() != WHITESPACE) {
                self.gap(p, i, "");
            }
            let tok = &self.tokens[i];
            if kind == COMMENT {
                self.out.push_str(trim_hspace_end(tok.text()));
            } else {
                self.out.push_str(tok.text());
            }
        }
        let last = &self.tokens[n - 1];
        let spliced_end = last.kind() == WHITESPACE
            && (last.text().contains('\\') || n >= 2 && self.tokens[n - 2].text().ends_with('\\'));
        if !spliced_end {
            self.out.push_str(line_ending(self.text));
        }
        self.out
    }

    /// Whitespace holding line breaks: keep the breaks, drop the blanks before each, and indent
    /// the next line.
    fn line_break(&mut self, ws: &str, next: usize) {
        let last_break = ws.rfind(['\n', '\r']).expect("a line break");
        let (breaks, indent) = ws.split_at(last_break + 1);
        self.out
            .extend(breaks.chars().filter(|c| matches!(c, '\n' | '\r')));
        self.indent(indent, next);
    }

    /// Indent the line that starts with token `next`, whose original indentation is `orig`.
    fn indent(&mut self, orig: &str, next: usize) {
        let tok = &self.tokens[next];
        let level = if self.verbatim[next] || tok.kind().is_trivia() {
            None
        } else {
            line_level(tok)
        };
        match level {
            Some(level) => match self.opts.indent {
                Indent::Tabs => self.out.extend(std::iter::repeat_n('\t', level)),
                Indent::Spaces(w) => self
                    .out
                    .extend(std::iter::repeat_n(' ', level * w as usize)),
            },
            None => self.out.push_str(orig),
        }
    }

    /// The horizontal gap `orig` between tokens `a` and `b` on one line.
    fn gap(&mut self, a: usize, b: usize, orig: &str) {
        let (ta, tb) = (&self.tokens[a], &self.tokens[b]);
        let keep =
            ta.kind().is_trivia() || tb.kind().is_trivia() || self.verbatim[a] || self.verbatim[b];
        let gap = if keep {
            Gap::Keep
        } else {
            decide(ta, tb, self.opts)
        };
        match gap {
            Gap::Space => self.out.push(' '),
            Gap::SomeSpace if orig.is_empty() => self.out.push(' '),
            Gap::SomeSpace => self.out.push_str(orig),
            Gap::Nothing => {}
            Gap::Keep => self.out.push_str(orig),
        }
    }
}

/// The gap between code tokens `a` and `b` on one line.
fn decide(a: &SyntaxToken, b: &SyntaxToken, opts: &Options) -> Gap {
    let (ka, kb) = (a.kind(), b.kind());
    let closes = |k: SyntaxKind| matches!(k, R_PAREN | R_BRACK | R_CURLY);
    let in_for_header = |t: &SyntaxToken| {
        matches!(t.kind(), L_PAREN | SEMICOLON) && t.parent().is_some_and(|p| p.kind() == FOR_STMT)
    };
    if kb == T![;] && in_for_header(b) && in_for_header(a) {
        // An empty clause: `for(;;)`, `for( ; ; )` stay as written.
        return Gap::Keep;
    }
    if kb == T![,] || kb == T![;] {
        return Gap::Nothing;
    }
    if ka == T![,] {
        return if closes(kb) {
            Gap::Keep
        } else {
            Gap::SomeSpace
        };
    }
    if ka == T![;] && a.parent().is_some_and(|p| p.kind() == FOR_STMT) {
        // `for( i = 0; i < n; i++ )`, but `for(;;)` stays as written.
        return if closes(kb) { Gap::Keep } else { Gap::Space };
    }
    let a_open = ka == L_PAREN && is_spaced_paren(a);
    let b_close = kb == R_PAREN && is_spaced_paren(b);
    if a_open && kb == R_PAREN && a.parent() == b.parent() {
        return Gap::Nothing;
    }
    if a_open || b_close {
        return match opts.paren_spacing {
            ParenSpacing::Padded => Gap::Space,
            ParenSpacing::Compact => Gap::Nothing,
            ParenSpacing::Preserve => Gap::Keep,
        };
    }
    if kb == L_PAREN
        && b.parent()
            .is_some_and(|p| matches!(p.kind(), ARG_LIST | PARAM_LIST))
    {
        return Gap::Nothing;
    }
    if is_binary_op(a) || is_binary_op(b) {
        return Gap::SomeSpace;
    }
    Gap::Keep
}
