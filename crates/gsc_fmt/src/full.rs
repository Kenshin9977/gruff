//! The full style (`style = "full"`, the default): the tree is printed through the document IR of
//! [`crate::doc`], so line breaks are chosen to fit `max_width`.
//!
//! Every token of the tree is emitted in source order through [`Fmt::token`], which also emits
//! the comments and preprocessor lines before it; no token can be dropped or reordered. Comments
//! keep their place: a comment that ended a line still ends one, a comment on its own line stays
//! on its own line. Top-level items and statements that hold `#if` regions or line splices, and
//! the statements between `// gsc-fmt: off` and `// gsc-fmt: on`, are copied as written.

use gsc_syntax::{NodeOrToken, SyntaxKind, SyntaxNode, SyntaxToken, T};
use SyntaxKind::*;

use crate::doc::{self, Container, Doc, Id, Node, PrintOptions, Sep};
use crate::{
    line_ending, trim_hspace_end, BraceStyle, Indent, KeywordSpace, Options, ParenSpacing, Wrap,
};

/// A token of the file: kind and byte range.
#[derive(Clone, Copy, Debug)]
struct Tok {
    kind: SyntaxKind,
    start: u32,
    end: u32,
}

/// Format a parsed file in the full style.
pub(crate) fn format(root: &SyntaxNode, text: &str, opts: &Options) -> Result<String, String> {
    let mut f = Fmt::new(root, text, opts);
    if f.toks.iter().all(|t| t.kind == WHITESPACE) {
        return Ok(String::new());
    }
    f.items(root);
    // Comments and preprocessor lines after the last item.
    f.sep(Sep::HardKeep);
    f.flush(f.toks.len(), false, false);
    if let Some(e) = f.error {
        return Err(e);
    }
    if f.cursor < f.toks.len() && f.toks[f.cursor..].iter().any(|t| t.kind != WHITESPACE) {
        return Err("tokens left after the last item".to_owned());
    }
    let newline = line_ending(text);
    let (indent, indent_width) = match opts.indent {
        Indent::Tabs => ("\t".to_owned(), opts.tab_width as usize),
        Indent::Spaces(n) => (" ".repeat(n as usize), n as usize),
    };
    let po = PrintOptions {
        width: opts.max_width as usize,
        indent,
        indent_width,
        tab_width: opts.tab_width as usize,
        newline,
    };
    let doc = f.doc.finish();
    let mut out = doc::print(&doc, &po);
    let keep = out.trim_end_matches([' ', '\t', '\n', '\r']).len();
    out.truncate(keep);
    if !out.is_empty() {
        out.push_str(newline);
    }
    Ok(out)
}

/// What a `// gsc-fmt: off` or `on` comment (or `// gruff-fmt: off`, `on`) says.
fn directive(comment: &str) -> Option<bool> {
    let body = comment
        .strip_prefix("//")
        .or_else(|| {
            comment
                .strip_prefix("/*")
                .and_then(|c| c.strip_suffix("*/"))
        })?
        .trim();
    // `gsc-fmt:` or, under the tool's name, `gruff-fmt:`.
    let rest = body
        .strip_prefix("gsc-fmt:")
        .or_else(|| body.strip_prefix("gruff-fmt:"))?;
    match rest.trim_start() {
        "off" => Some(false),
        "on" => Some(true),
        _ => None,
    }
}

fn is_comment(kind: SyntaxKind) -> bool {
    matches!(kind, COMMENT | BLOCK_COMMENT | DOC_COMMENT)
}

fn is_pp(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        PP_IF | PP_IFDEF | PP_IFNDEF | PP_ELIF | PP_ELSE | PP_ENDIF | DISABLED_TEXT
    )
}

/// The number of line breaks in `s` (`\r\n` counts once).
fn breaks(s: &str) -> usize {
    let b = s.as_bytes();
    let mut n = 0;
    for (i, &c) in b.iter().enumerate() {
        if c == b'\n' || (c == b'\r' && b.get(i + 1) != Some(&b'\n')) {
            n += 1;
        }
    }
    n
}

/// The blanks after the last line break of `s`.
fn last_line_indent(s: &str) -> &str {
    match s.rfind(['\n', '\r']) {
        Some(i) => &s[i + 1..],
        None => "",
    }
}

/// The gap inside a bracket for `style`; `written` tells whether the source has one (`Preserve`).
fn paren_pad(style: ParenSpacing, written: impl FnOnce() -> bool) -> Sep {
    match style {
        ParenSpacing::Padded => Sep::Space,
        ParenSpacing::Compact => Sep::None,
        ParenSpacing::Preserve if written() => Sep::Space,
        ParenSpacing::Preserve => Sep::None,
    }
}

/// Could `a` and `b` lex differently if written with nothing between them?
fn may_merge(a: &str, b: &str) -> bool {
    let (Some(x), Some(y)) = (a.chars().last(), b.chars().next()) else {
        return false;
    };
    let word =
        |c: char| c.is_alphanumeric() || matches!(c, '_' | '"' | '\'' | '\\') || !c.is_ascii();
    if word(x) && word(y) {
        return true;
    }
    if (x == '.' && y.is_ascii_digit()) || (x.is_ascii_digit() && y == '.') {
        return true;
    }
    if matches!(x, '&' | '#' | '@' | '%' | '$') && (word(y) || y == '"') {
        return true;
    }
    const OPS: &[&str] = &[
        "...", "::", "?.", "??", "->", "/#", "#/", "==", "!=", "<=", ">=", "<<", ">>", "&&", "||",
        "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "//", "/*", "*/", "/@", "@/",
        "..",
    ];
    let mut pair = String::with_capacity(8);
    pair.push(x);
    pair.push(y);
    OPS.iter().any(|op| op.starts_with(&pair))
}

/// Binary operator precedence classes; a chain is a run of the same class.
fn precedence(k: SyntaxKind) -> u8 {
    match k {
        QUESTION2 => 1,
        PIPE2 => 2,
        AMP2 => 3,
        PIPE => 4,
        CARET => 5,
        AMP => 6,
        EQ2 | NEQ | EQ3 | NEQ2 => 7,
        LT | LTEQ | GT | GTEQ => 8,
        SHL | SHR => 9,
        PLUS | MINUS => 10,
        STAR | SLASH | PERCENT => 11,
        _ => 0,
    }
}

/// The direct children of `n` that are not trivia.
fn elements(n: &SyntaxNode) -> Vec<NodeOrToken<SyntaxNode, SyntaxToken>> {
    n.children_with_tokens()
        .filter(|e| match e {
            NodeOrToken::Token(t) => !t.kind().is_trivia(),
            NodeOrToken::Node(_) => true,
        })
        .collect()
}

/// The pending separator: a placeholder node, its kind and the container it sits in.
#[derive(Clone, Copy)]
struct Pending {
    id: Id,
    sep: Sep,
    container: Id,
}

struct Fmt<'a> {
    text: &'a str,
    toks: Vec<Tok>,
    /// `bad[i]`: tokens before index `i` that force a verbatim copy (preprocessor lines, line
    /// splices, error nodes).
    bad_prefix: Vec<u32>,
    /// `// gsc-fmt: off|on` comments: token index and whether it turns formatting on.
    directives: Vec<(usize, bool)>,
    /// The next token to emit.
    cursor: usize,
    opts: &'a Options,
    doc: Doc<'a>,
    pending: Option<Pending>,
    /// What the last emitted comment requires before the next lexeme.
    carry: Sep,
    /// The end offset of the last emitted lexeme, or `None` at the start of the file.
    prev_end: Option<usize>,
    /// The last emitted code token.
    last_code: Option<SyntaxToken>,
    /// Has a lexeme other than the BOM been emitted?
    started: bool,
    error: Option<String>,
}

impl<'a> Fmt<'a> {
    fn new(root: &SyntaxNode, text: &'a str, opts: &'a Options) -> Fmt<'a> {
        let mut toks = Vec::new();
        let mut bad_at = Vec::new();
        for el in root.descendants_with_tokens() {
            match el {
                NodeOrToken::Token(t) => {
                    let r = t.text_range();
                    let k = t.kind();
                    if is_pp(k) || (k == WHITESPACE && t.text().contains('\\')) {
                        bad_at.push(toks.len());
                    }
                    toks.push(Tok {
                        kind: k,
                        start: r.start().into(),
                        end: r.end().into(),
                    });
                }
                NodeOrToken::Node(n) if n.kind() == ERROR => bad_at.push(toks.len()),
                NodeOrToken::Node(_) => {}
            }
        }
        let mut bad_prefix = vec![0u32; toks.len() + 1];
        let mut j = 0;
        for i in 0..toks.len() {
            let mut c = bad_prefix[i];
            while j < bad_at.len() && bad_at[j] == i {
                c += 1;
                j += 1;
            }
            bad_prefix[i + 1] = c;
        }
        // An error node at the very end.
        if j < bad_at.len() {
            bad_prefix[toks.len()] += (bad_at.len() - j) as u32;
        }
        let directives = toks
            .iter()
            .enumerate()
            .filter(|(_, t)| is_comment(t.kind))
            .filter_map(|(i, t)| {
                let s = &text[t.start as usize..t.end as usize];
                directive(s.trim_end()).map(|on| (i, on))
            })
            .collect();
        Fmt {
            text,
            toks,
            bad_prefix,
            directives,
            cursor: 0,
            opts,
            doc: Doc::new(),
            pending: None,
            carry: Sep::None,
            prev_end: None,
            last_code: None,
            started: false,
            error: None,
        }
    }

    fn fail(&mut self, msg: String) {
        if self.error.is_none() {
            self.error = Some(msg);
        }
    }

    fn tok_text(&self, i: usize) -> &'a str {
        let t = self.toks[i];
        &self.text[t.start as usize..t.end as usize]
    }

    /// The index of the token starting at `offset`.
    fn index_at(&self, offset: u32) -> usize {
        self.toks.partition_point(|t| t.start < offset)
    }

    /// The token range of a node, trivia excluded: (first, last) indices.
    fn span(&self, n: &SyntaxNode) -> Option<(usize, usize)> {
        let r = n.text_range();
        let mut first = self.index_at(r.start().into());
        let mut last = self.index_at(r.end().into());
        while first < self.toks.len() && self.toks[first].kind.is_trivia() {
            first += 1;
        }
        while last > first && self.toks[last - 1].kind.is_trivia() {
            last -= 1;
        }
        (first < last && (self.toks[first].start as usize) < usize::from(r.end()))
            .then_some((first, last - 1))
    }

    fn has_bad(&self, first: usize, last: usize) -> bool {
        self.bad_prefix[last + 1] > self.bad_prefix[first]
    }

    // ---- separators and tokens ------------------------------------------------------------

    /// Request a separator before the next lexeme.
    fn sep(&mut self, sep: Sep) {
        let container = self.doc.current();
        match self.pending {
            // Same container, or a container opened since (a node printer asking for a gap
            // before its first token): the caller's position wins.
            Some(p) if p.container == container || self.doc.is_open(p.container) => {
                self.pending = Some(Pending {
                    sep: p.sep.max(sep),
                    ..p
                });
            }
            old => {
                let sep = old.map_or(sep, |p| p.sep.max(sep));
                let id = self.doc.push(Node::Concat(Vec::new()));
                self.pending = Some(Pending { id, sep, container });
            }
        }
    }

    /// Cancel the pending separator.
    fn unsep(&mut self) {
        self.pending = None;
    }

    fn sep_node(&self, sep: Sep, nl: usize) -> Option<Node<'a>> {
        Some(match sep {
            Sep::None => return None,
            Sep::SoftLine => Node::SoftLine,
            Sep::Space => Node::Space,
            Sep::Line => Node::Line,
            Sep::Hard => Node::Hard { blank: 0 },
            Sep::HardKeep => Node::Hard {
                blank: self.kept_blanks(nl),
            },
        })
    }

    /// The blank lines kept of a gap holding `nl` line breaks.
    fn kept_blanks(&self, nl: usize) -> u8 {
        nl.saturating_sub(1).min(self.opts.max_blank_lines as usize) as u8
    }

    /// A line break that keeps the indentation written before the next text (verbatim text): the
    /// gap's blank lines when `keep`, then the blanks of its last line if it has a line break.
    fn push_literal(&self, parts: &mut Vec<Node<'a>>, keep: bool, nl: usize, gap: &'a str) {
        let blank = if keep { self.kept_blanks(nl) } else { 0 };
        parts.push(Node::Literal { blank });
        let ind = last_line_indent(gap);
        if !ind.is_empty() && nl > 0 {
            parts.push(Node::Raw(ind));
        }
    }

    /// Emit the trivia before token `end` (comments, preprocessor lines) and the pending
    /// separator. With `final_`, also the separator before the token itself; otherwise what the
    /// last comment requires is carried to the next lexeme. With `literal`, a line break before
    /// the token keeps the token's original indentation (verbatim regions).
    fn flush(&mut self, end: usize, final_: bool, literal: bool) {
        let pending = self.pending.take();
        let sep = pending.map_or(Sep::None, |p| p.sep).max(self.carry);
        self.carry = Sep::None;
        let mut parts: Vec<Node<'a>> = Vec::new();
        let mut sep_used = false;
        let mut need = Sep::None;
        let mut i = self.cursor;
        let mut last_lexeme = None;
        while i < end {
            let t = self.toks[i];
            if t.kind == WHITESPACE {
                i += 1;
                continue;
            }
            let gap_start = self.prev_end.unwrap_or(0);
            let gap = &self.text[gap_start.min(t.start as usize)..t.start as usize];
            let nl = breaks(gap);
            let text = self.tok_text(i);
            let at_start = !self.started;
            match t.kind {
                BOM => parts.push(Node::Text(text)),
                k if is_comment(k) => {
                    let text = if k == COMMENT {
                        trim_hspace_end(text)
                    } else {
                        text
                    };
                    let own_line = nl > 0 || at_start;
                    if !own_line {
                        // On the line of the previous lexeme: keep the gap (column alignment of
                        // trailing comments), or no gap.
                        // A trailing comment does not count when measuring the line.
                        if !gap.is_empty() {
                            parts.push(Node::Suffix(gap));
                        }
                    } else if !at_start {
                        let s = if sep_used {
                            if sep == Sep::HardKeep {
                                Sep::HardKeep
                            } else {
                                Sep::Hard
                            }
                        } else {
                            sep.max(Sep::Hard)
                        };
                        let blank_ok = s == Sep::HardKeep;
                        parts.push(Node::Hard {
                            blank: if blank_ok {
                                nl.saturating_sub(1).min(self.opts.max_blank_lines as usize) as u8
                            } else {
                                0
                            },
                        });
                    }
                    if own_line {
                        sep_used = true;
                    }
                    parts.push(if text.contains(['\n', '\r']) {
                        Node::Raw(text)
                    } else if !own_line {
                        Node::Suffix(text)
                    } else {
                        Node::Text(text)
                    });
                    let next_ws = self
                        .toks
                        .get(i + 1)
                        .map(|n| (n.kind, &self.text[n.start as usize..n.end as usize]));
                    need = match next_ws {
                        _ if k == COMMENT => Sep::Hard,
                        None => Sep::Hard,
                        Some((WHITESPACE, w)) if w.contains(['\n', '\r']) => Sep::Hard,
                        Some((WHITESPACE, _)) => Sep::Space,
                        Some(_) => Sep::None,
                    };
                }
                _ => {
                    // A preprocessor line or inactive text: on its own line, as written.
                    let after_disabled = i > 0
                        && self.toks[i - 1].kind == DISABLED_TEXT
                        && self.toks[i - 1].end == t.start;
                    if t.kind != DISABLED_TEXT && !after_disabled && !at_start {
                        self.push_literal(&mut parts, sep == Sep::HardKeep || sep_used, nl, gap);
                    }
                    sep_used = true;
                    parts.push(Node::Raw(text));
                    need = Sep::Hard;
                }
            }
            self.prev_end = Some(t.end as usize);
            if t.kind != BOM {
                self.started = true;
            }
            last_lexeme = Some(i);
            i += 1;
        }
        let had_lexemes = last_lexeme.is_some();
        if final_ {
            let target_start = self
                .toks
                .get(end)
                .map_or(self.text.len(), |t| t.start as usize);
            let gap_start = self.prev_end.unwrap_or(0).min(target_start);
            let gap = &self.text[gap_start..target_start];
            let nl = breaks(gap);
            let at_start = !self.started;
            let s = if at_start {
                Sep::None
            } else if !had_lexemes {
                sep
            } else if !sep_used {
                // Only comments on the previous lexeme's line.
                sep.max(need)
            } else if need >= Sep::Hard {
                if sep == Sep::HardKeep {
                    Sep::HardKeep
                } else {
                    Sep::Hard
                }
            } else {
                need
            };
            let mut s = s;
            // A keyword used as a name must stay on the line of the token before it (the parser
            // reads it as a keyword after a line break).
            if matches!(s, Sep::Line | Sep::SoftLine)
                && end < self.toks.len()
                && self.toks[end].kind == IDENT
                && is_keyword_text(self.tok_text(end))
            {
                s = if s == Sep::Line {
                    Sep::Space
                } else {
                    Sep::None
                };
            }
            if s == Sep::None && !gap.is_empty() && end < self.toks.len() && self.prev_end.is_some()
            {
                let prev = &self.text[..gap_start];
                if may_merge(prev, self.tok_text(end)) {
                    s = Sep::Space;
                }
            }
            if literal && s >= Sep::Hard && nl > 0 {
                self.push_literal(&mut parts, s == Sep::HardKeep, nl, gap);
            } else if let Some(n) = self.sep_node(s, nl) {
                parts.push(n);
            }
            self.cursor = end;
        } else {
            // Leave the whitespace before `end` for the next flush.
            self.cursor = last_lexeme.map_or(self.cursor, |l| l + 1);
            if had_lexemes {
                self.carry = need;
            } else if let Some(p) = pending {
                // Nothing emitted: keep the request for the next lexeme.
                self.pending = Some(p);
                self.carry = Sep::None;
                return;
            }
        }
        if parts.is_empty() {
            return;
        }
        let ids: Vec<Id> = parts.into_iter().map(|n| self.doc.detached(n)).collect();
        match pending {
            Some(p) => self.doc.set(p.id, Node::Concat(ids)),
            None => {
                let c = self.doc.push(Node::Concat(Vec::new()));
                self.doc.set(c, Node::Concat(ids));
            }
        }
    }

    /// Emit the comments before `t` now (inside the current container), if there are any.
    fn comments_before(&mut self, t: &SyntaxToken) {
        let end = self.index_at(t.text_range().start().into());
        let any = self.toks[self.cursor..end.min(self.toks.len())]
            .iter()
            .any(|t| t.kind != WHITESPACE);
        if any {
            self.flush(end, false, false);
        } else {
            self.unsep();
        }
    }

    /// Emit a code token.
    fn token(&mut self, t: &SyntaxToken) {
        let start: u32 = t.text_range().start().into();
        let idx = self.index_at(start);
        if idx >= self.toks.len() || self.toks[idx].start != start || idx < self.cursor {
            self.fail(format!("token {:?} at {start} out of order", t.kind()));
            return;
        }
        if let Some(k) = self.toks[self.cursor..idx]
            .iter()
            .find(|k| !k.kind.is_trivia())
        {
            self.fail(format!("token {:?} at {} skipped", k.kind, k.start));
        }
        self.flush(idx, true, false);
        let text = self.tok_text(idx);
        self.doc.push(if text.contains(['\n', '\r']) {
            Node::Raw(text)
        } else {
            Node::Text(text)
        });
        self.cursor = idx + 1;
        self.prev_end = Some(self.toks[idx].end as usize);
        self.started = true;
        self.last_code = Some(t.clone());
    }

    /// Copy tokens `first..=last` (and everything between them) as written.
    fn raw(&mut self, first: usize, last: usize) {
        if first < self.cursor {
            self.fail(format!("verbatim region at {} out of order", first));
            return;
        }
        self.flush(first, true, true);
        let (s, e) = (
            self.toks[first].start as usize,
            self.toks[last].end as usize,
        );
        self.doc.push(Node::Raw(&self.text[s..e]));
        self.cursor = last + 1;
        self.prev_end = Some(e);
        self.started = true;
        self.last_code = None;
    }

    fn raw_node(&mut self, n: &SyntaxNode) {
        if let Some((a, b)) = self.span(n) {
            self.raw(a, b);
        }
    }

    /// Was there whitespace or a comment right before token `t` in the source?
    fn space_before(&self, t: &SyntaxToken) -> bool {
        let i = self.index_at(t.text_range().start().into());
        i > 0 && self.toks[i - 1].kind.is_trivia()
    }

    fn space_after(&self, t: &SyntaxToken) -> bool {
        let i = self.index_at(t.text_range().start().into());
        self.toks.get(i + 1).is_some_and(|k| k.kind.is_trivia())
    }

    // ---- spacing rules --------------------------------------------------------------------

    fn pad_open(&self, style: ParenSpacing, open: &SyntaxToken) -> Sep {
        paren_pad(style, || self.space_after(open))
    }

    fn pad_close(&self, style: ParenSpacing, close: &SyntaxToken) -> Sep {
        paren_pad(style, || self.space_before(close))
    }

    /// The gap between a control keyword and its `(`.
    fn keyword_gap(&self, paren: &SyntaxToken) -> Sep {
        match self.opts.keyword_space {
            KeywordSpace::Space => Sep::Space,
            KeywordSpace::None => Sep::None,
            KeywordSpace::Preserve if self.space_before(paren) => Sep::Space,
            KeywordSpace::Preserve => Sep::None,
        }
    }

    fn brace_sep(&self) -> Sep {
        match self.opts.brace_style {
            BraceStyle::NextLine => Sep::Hard,
            BraceStyle::SameLine => Sep::Space,
        }
    }

    /// The gap between two code tokens in a construct without a layout of its own.
    fn gap(&self, a: &SyntaxToken, b: &SyntaxToken) -> Sep {
        let (ka, kb) = (a.kind(), b.kind());
        let pa = a.parent().map(|p| p.kind());
        let pb = b.parent().map(|p| p.kind());
        let preserve = if self.space_before(b) {
            Sep::Space
        } else {
            Sep::None
        };
        if kb == T![,] || kb == T![;] {
            return Sep::None;
        }
        if ka == T![,] {
            return Sep::Space;
        }
        let is_op = |t: &SyntaxToken, p: Option<SyntaxKind>| match p {
            Some(BIN_EXPR | ASSIGN_EXPR) => t.kind().is_punct(),
            Some(TERNARY_EXPR) => matches!(t.kind(), QUESTION | COLON),
            Some(DEFAULT_VALUE | FILE_CONSTANT | CLASS_VAR | CONST_STMT) => t.kind() == EQ,
            Some(CLASS_BASE) => t.kind() == COLON,
            _ => false,
        };
        if is_op(a, pa) || is_op(b, pb) {
            return Sep::Space;
        }
        if matches!(ka, DOT | QUESTION_DOT | COLON2 | ARROW)
            || matches!(kb, DOT | QUESTION_DOT | COLON2 | ARROW)
        {
            return Sep::None;
        }
        if kb == COLON
            && matches!(
                pb,
                Some(SWITCH_CLAUSE | LABEL_STMT | STRUCT_FIELD | ARRAY_ELEMENT)
            )
        {
            return Sep::None;
        }
        if ka == COLON && matches!(pa, Some(STRUCT_FIELD | ARRAY_ELEMENT | SWITCH_CLAUSE)) {
            return Sep::Space;
        }
        // Prefix operators and the sigils of `&f`, `%anim`, `$name`.
        let first_of_parent = |t: &SyntaxToken| {
            t.parent()
                .and_then(|p| gsc_syntax::first_significant_token(&p))
                .is_some_and(|f| f == *t)
        };
        if matches!(
            pa,
            Some(PREFIX_EXPR | FUNCTION_REF | ANIM_REF | DOLLAR_NAME)
        ) && first_of_parent(a)
            && ka.is_punct()
        {
            return Sep::None;
        }
        if pb == Some(POSTFIX_EXPR) && matches!(kb, PLUS2 | MINUS2) {
            return Sep::None;
        }
        if kb == L_PAREN && matches!(pb, Some(ARG_LIST | PARAM_LIST)) {
            return Sep::None;
        }
        // Anything else (two names, a keyword and a value...) keeps whether there was a gap:
        // tokens written together stay together (`%anim@1` in decompiler output).
        preserve
    }

    /// Request the generic gap before token `b`.
    fn gap_to(&mut self, b: &SyntaxToken) {
        if let Some(a) = self.last_code.clone() {
            let g = self.gap(&a, b);
            self.sep(g);
        }
    }

    // ---- generic printing -----------------------------------------------------------------

    /// Print the children of `n` with the generic gaps.
    fn generic(&mut self, n: &SyntaxNode) {
        // The gap before the first element is the caller's.
        for (i, el) in elements(n).into_iter().enumerate() {
            match el {
                NodeOrToken::Token(t) => {
                    if i > 0 {
                        self.gap_to(&t);
                    }
                    self.token(&t);
                }
                NodeOrToken::Node(c) => {
                    if i > 0 {
                        if let Some(f) = gsc_syntax::first_significant_token(&c) {
                            self.gap_to(&f);
                        }
                    }
                    self.node(&c);
                }
            }
        }
    }

    fn node(&mut self, n: &SyntaxNode) {
        match n.kind() {
            FUNCTION_DEF | CONSTRUCTOR | DESTRUCTOR => self.function(n),
            CLASS_DEF => self.class(n),
            BLOCK => self.block(n),
            IF_STMT => self.if_stmt(n),
            WHILE_STMT | FOREACH_STMT => self.loop_stmt(n),
            FOR_STMT => self.for_stmt(n),
            DO_WHILE_STMT => self.do_while(n),
            SWITCH_STMT => self.switch(n),
            SWITCH_CLAUSE => self.clause(n),
            DEV_BLOCK_STMT | DEV_BLOCK_ITEM => self.dev_block(n),
            ARG_LIST | PARAM_LIST | VECTOR_EXPR => self.args(n),
            ARRAY_EXPR | STRUCT_EXPR => self.literal_list(n),
            BIN_EXPR => self.binary(n),
            ASSIGN_EXPR => self.assign(n),
            TERNARY_EXPR => self.ternary(n),
            CALL_EXPR | POINTER_CALL_EXPR | METHOD_CALL_EXPR => self.call(n),
            PAREN_EXPR => self.paren(n),
            INDEX_EXPR => self.index(n),
            USING_DIRECTIVE
            | INCLUDE_DIRECTIVE
            | INSERT_DIRECTIVE
            | INLINE_DIRECTIVE
            | NAMESPACE_DIRECTIVE
            | PRECACHE_DIRECTIVE
            | USING_ANIMTREE_DIRECTIVE
            | PREPROC_DEFINE
            | PREPROC_UNDEF
            | ERROR => self.raw_node(n),
            _ => self.generic(n),
        }
    }

    // ---- lists of items and statements -----------------------------------------------------

    fn items(&mut self, root: &SyntaxNode) {
        let members: Vec<SyntaxNode> = root.children().collect();
        self.members(&members, Sep::HardKeep, Indented::No);
    }

    /// Print statements or declarations, one per line. Members that hold preprocessor lines,
    /// and members inside a `gsc-fmt: off` region, are copied as written.
    fn members(&mut self, members: &[SyntaxNode], first: Sep, indented: Indented) {
        let mut off = false;
        let mut i = 0;
        while i < members.len() {
            let m = &members[i];
            let Some((a, b)) = self.span(m) else {
                i += 1;
                continue;
            };
            // Directives in the trivia before this member.
            for &(d, on) in &self.directives {
                if d >= self.cursor && d < a {
                    off = !on;
                }
            }
            let sep = if i == 0 { first } else { Sep::HardKeep };
            let verbatim = off || self.has_bad(a, b);
            if verbatim {
                // Extend over the following members that are verbatim too.
                let mut last = b;
                let mut j = i + 1;
                while j < members.len() {
                    let Some((a2, b2)) = self.span(&members[j]) else {
                        j += 1;
                        continue;
                    };
                    let mut off2 = off;
                    for &(d, on) in &self.directives {
                        if d > last && d < a2 {
                            off2 = !on;
                        }
                    }
                    if off2 || self.has_bad(a2, b2) {
                        off = off2;
                        last = b2;
                        j += 1;
                    } else {
                        break;
                    }
                }
                self.sep(sep);
                self.raw(a, last);
                i = j;
                continue;
            }
            match indented {
                Indented::No => {
                    self.sep(sep);
                    self.node(m);
                }
                Indented::UnlessBlock if m.kind() == BLOCK => {
                    self.sep(sep);
                    self.node(m);
                }
                Indented::UnlessBlock => {
                    self.doc.open(Container::Indent);
                    self.sep(sep);
                    self.node(m);
                    self.doc.close();
                }
            }
            i += 1;
        }
    }

    /// Statements between braces (or `/#` `#/`), indented one level.
    fn braced_body(&mut self, members: &[SyntaxNode], close: Option<&SyntaxToken>, indent: bool) {
        if indent {
            self.doc.open(Container::Indent);
        }
        self.members(members, Sep::Hard, Indented::No);
        if let Some(c) = close {
            self.sep(if members.is_empty() {
                Sep::Hard
            } else {
                Sep::HardKeep
            });
            self.comments_before(c);
        }
        if indent {
            self.doc.close();
        }
        if let Some(c) = close {
            self.sep(Sep::Hard);
            self.token(c);
        }
    }

    // ---- declarations and statements ------------------------------------------------------

    fn function(&mut self, n: &SyntaxNode) {
        let mut first = true;
        for el in elements(n) {
            match el {
                NodeOrToken::Node(c) if c.kind() == BLOCK => {
                    self.sep(self.brace_sep());
                    self.block(&c);
                }
                NodeOrToken::Node(c) if c.kind() == PARAM_LIST => {
                    self.sep(Sep::None);
                    self.args(&c);
                }
                NodeOrToken::Node(c) => {
                    if !first {
                        self.sep(Sep::Space);
                    }
                    self.node(&c);
                }
                NodeOrToken::Token(t) => {
                    if !first {
                        self.sep(Sep::Space);
                    }
                    self.token(&t);
                }
            }
            first = false;
        }
    }

    fn class(&mut self, n: &SyntaxNode) {
        let mut members = Vec::new();
        let mut open = None;
        let mut close = None;
        for el in elements(n) {
            match el {
                NodeOrToken::Token(t) if t.kind() == L_CURLY => open = Some(t),
                NodeOrToken::Token(t) if t.kind() == R_CURLY => close = Some(t),
                NodeOrToken::Token(t) if open.is_none() => {
                    self.gap_to(&t);
                    self.token(&t);
                }
                NodeOrToken::Node(c) if open.is_none() => {
                    self.sep(Sep::Space);
                    self.generic(&c);
                }
                NodeOrToken::Node(c) => members.push(c),
                NodeOrToken::Token(t) => {
                    self.fail(format!("unexpected {:?} in a class", t.kind()));
                }
            }
        }
        if let Some(o) = open {
            self.sep(self.brace_sep());
            self.token(&o);
            self.braced_body(&members, close.as_ref(), true);
        }
    }

    fn block(&mut self, n: &SyntaxNode) {
        let mut open = None;
        let mut close = None;
        let mut members = Vec::new();
        for el in elements(n) {
            match el {
                NodeOrToken::Token(t) if t.kind() == L_CURLY && open.is_none() => open = Some(t),
                NodeOrToken::Token(t) if t.kind() == R_CURLY => close = Some(t),
                NodeOrToken::Node(c) => members.push(c),
                NodeOrToken::Token(t) => self.fail(format!("unexpected {:?} in a block", t.kind())),
            }
        }
        if let Some(o) = open {
            self.token(&o);
        }
        self.braced_body(&members, close.as_ref(), true);
    }

    /// The body of a control statement: a block after the brace separator, or a statement on
    /// the next line, indented.
    fn body(&mut self, s: &SyntaxNode) {
        if s.kind() == BLOCK {
            self.sep(self.brace_sep());
            self.block(s);
        } else {
            self.doc.open(Container::Indent);
            self.sep(Sep::Hard);
            self.node(s);
            self.doc.close();
        }
    }

    /// `kw ( inside ) body`: the parenthesised header of `if`, `while`, `foreach`, `switch`.
    /// Returns the elements after the `)`.
    fn header(&mut self, n: &SyntaxNode) -> Vec<NodeOrToken<SyntaxNode, SyntaxToken>> {
        let els = elements(n);
        let mut it = els.into_iter().peekable();
        // The keyword.
        if let Some(NodeOrToken::Token(kw)) = it.peek().cloned() {
            self.token(&kw);
            it.next();
        }
        let Some(NodeOrToken::Token(open)) = it.peek().cloned().filter(|e| e.kind() == L_PAREN)
        else {
            return it.collect();
        };
        it.next();
        self.sep(self.keyword_gap(&open));
        self.token(&open);
        let ps = self.opts.paren_spacing;
        let mut first = true;
        while let Some(el) = it.peek().cloned() {
            if let NodeOrToken::Token(t) = &el {
                if t.kind() == R_PAREN && t.parent().as_ref() == Some(n) {
                    if first {
                        self.sep(Sep::None);
                    } else {
                        self.sep(self.pad_close(ps, t));
                    }
                    self.token(t);
                    it.next();
                    break;
                }
            }
            it.next();
            if first {
                self.sep(self.pad_open(ps, &open));
            }
            match &el {
                NodeOrToken::Token(t) => {
                    if !first {
                        self.gap_to(t);
                    }
                    self.token(t);
                }
                NodeOrToken::Node(c) => {
                    if !first {
                        if let Some(f) = gsc_syntax::first_significant_token(c) {
                            self.gap_to(&f);
                        }
                    }
                    self.expr(c);
                }
            }
            first = false;
        }
        it.collect()
    }

    /// An expression: its own groups decide its line breaks.
    fn expr(&mut self, n: &SyntaxNode) {
        self.node(n);
    }

    fn if_stmt(&mut self, n: &SyntaxNode) {
        let rest = self.header(n);
        let mut then_block = false;
        for el in rest {
            match el {
                NodeOrToken::Node(c) if c.kind() == ELSE_BRANCH => {
                    self.sep(
                        if then_block && self.opts.brace_style == BraceStyle::SameLine {
                            Sep::Space
                        } else {
                            Sep::Hard
                        },
                    );
                    self.else_branch(&c);
                }
                NodeOrToken::Node(c) => {
                    then_block = c.kind() == BLOCK;
                    self.body(&c);
                }
                NodeOrToken::Token(t) => {
                    self.gap_to(&t);
                    self.token(&t);
                }
            }
        }
    }

    fn else_branch(&mut self, n: &SyntaxNode) {
        for el in elements(n) {
            match el {
                NodeOrToken::Token(t) => {
                    self.gap_to(&t);
                    self.token(&t);
                }
                NodeOrToken::Node(c) if c.kind() == IF_STMT => {
                    self.sep(Sep::Space);
                    self.if_stmt(&c);
                }
                NodeOrToken::Node(c) => self.body(&c),
            }
        }
    }

    fn loop_stmt(&mut self, n: &SyntaxNode) {
        for el in self.header(n) {
            match el {
                NodeOrToken::Node(c) => self.body(&c),
                NodeOrToken::Token(t) => {
                    self.gap_to(&t);
                    self.token(&t);
                }
            }
        }
    }

    fn for_stmt(&mut self, n: &SyntaxNode) {
        let ps = self.opts.paren_spacing;
        // 0: before `(`, 1: in the header, 2: after `)`.
        let mut state = 0;
        let mut open: Option<SyntaxToken> = None;
        let mut last: Option<SyntaxKind> = None;
        for el in elements(n) {
            match (&el, state) {
                (NodeOrToken::Token(t), 0) if t.kind() == L_PAREN => {
                    self.sep(self.keyword_gap(t));
                    self.token(t);
                    open = Some(t.clone());
                    state = 1;
                    last = Some(L_PAREN);
                }
                (NodeOrToken::Token(t), 1) if t.kind() == SEMICOLON => {
                    self.sep(Sep::None);
                    self.token(t);
                    last = Some(SEMICOLON);
                }
                (NodeOrToken::Token(t), 1) if t.kind() == R_PAREN => {
                    let clause = !matches!(last, Some(L_PAREN | SEMICOLON));
                    self.sep(if clause {
                        self.pad_close(ps, t)
                    } else {
                        Sep::None
                    });
                    self.token(t);
                    state = 2;
                }
                (NodeOrToken::Node(c), 1) => {
                    let sep = match (&open, last) {
                        (Some(o), Some(L_PAREN)) => self.pad_open(ps, o),
                        _ => Sep::Space,
                    };
                    self.sep(sep);
                    self.expr(c);
                    last = Some(c.kind());
                }
                (NodeOrToken::Node(c), 2) => self.body(c),
                (NodeOrToken::Node(c), _) => self.generic(c),
                (NodeOrToken::Token(t), _) => {
                    self.gap_to(t);
                    self.token(t);
                }
            }
        }
    }

    fn do_while(&mut self, n: &SyntaxNode) {
        let ps = self.opts.paren_spacing;
        let mut body_done = false;
        let mut body_block = false;
        let mut open: Option<SyntaxToken> = None;
        for el in elements(n) {
            match &el {
                NodeOrToken::Token(t) if t.kind() == DO_KW => self.token(t),
                NodeOrToken::Node(c) if !body_done => {
                    body_done = true;
                    body_block = c.kind() == BLOCK;
                    self.body(c);
                }
                NodeOrToken::Token(t) if t.kind() == WHILE_KW => {
                    self.sep(
                        if body_block && self.opts.brace_style == BraceStyle::SameLine {
                            Sep::Space
                        } else {
                            Sep::Hard
                        },
                    );
                    self.token(t);
                }
                NodeOrToken::Token(t) if t.kind() == L_PAREN && open.is_none() => {
                    self.sep(self.keyword_gap(t));
                    self.token(t);
                    open = Some(t.clone());
                }
                NodeOrToken::Token(t) if t.kind() == R_PAREN && t.parent().as_ref() == Some(n) => {
                    self.sep(self.pad_close(ps, t));
                    self.token(t);
                }
                NodeOrToken::Node(c) => {
                    match &open {
                        Some(o) => self.sep(self.pad_open(ps, o)),
                        None => self.sep(Sep::Space),
                    }
                    self.expr(c);
                }
                NodeOrToken::Token(t) => {
                    self.gap_to(t);
                    self.token(t);
                }
            }
        }
    }

    fn switch(&mut self, n: &SyntaxNode) {
        let rest = self.header(n);
        let mut clauses = Vec::new();
        let mut close = None;
        for el in rest {
            match el {
                NodeOrToken::Token(t) if t.kind() == L_CURLY => {
                    self.sep(self.brace_sep());
                    self.token(&t);
                }
                NodeOrToken::Token(t) if t.kind() == R_CURLY => close = Some(t),
                NodeOrToken::Node(c) => clauses.push(c),
                NodeOrToken::Token(t) => {
                    self.gap_to(&t);
                    self.token(&t);
                }
            }
        }
        self.braced_body(&clauses, close.as_ref(), true);
    }

    fn clause(&mut self, n: &SyntaxNode) {
        let mut stmts = Vec::new();
        let mut label_done = false;
        for el in elements(n) {
            match el {
                NodeOrToken::Token(t) if !label_done => {
                    self.gap_to(&t);
                    self.token(&t);
                    if t.kind() == COLON && t.parent().as_ref() == Some(n) {
                        label_done = true;
                    }
                }
                NodeOrToken::Node(c) if !label_done => {
                    self.sep(Sep::Space);
                    self.expr(&c);
                }
                NodeOrToken::Node(c) => stmts.push(c),
                NodeOrToken::Token(t) => self.fail(format!("unexpected {:?} in a case", t.kind())),
            }
        }
        self.members(&stmts, Sep::Hard, Indented::UnlessBlock);
    }

    fn dev_block(&mut self, n: &SyntaxNode) {
        let mut open = None;
        let mut close = None;
        let mut members = Vec::new();
        for el in elements(n) {
            match el {
                NodeOrToken::Token(t) if t.kind() == DEV_OPEN && open.is_none() => open = Some(t),
                NodeOrToken::Token(t) if t.kind() == DEV_CLOSE => close = Some(t),
                NodeOrToken::Node(c) => members.push(c),
                NodeOrToken::Token(t) => {
                    self.fail(format!("unexpected {:?} in a dev block", t.kind()))
                }
            }
        }
        let Some(o) = open else {
            return;
        };
        self.token(&o);
        let one_line = !n.text().contains_char('\n') && !n.text().contains_char('\r');
        if one_line && members.len() <= 1 && members.iter().all(|m| m.kind() != BLOCK) {
            // `/# stmt #/` on one line if it fits; broken it is the multi-line form.
            self.doc.open(Container::Group);
            self.doc.open(Container::Indent);
            for m in &members {
                self.sep(Sep::Line);
                self.node(m);
            }
            self.doc.close();
            if let Some(c) = close {
                self.sep(Sep::Line);
                self.token(&c);
            }
            self.doc.close();
            return;
        }
        // At file scope the declarations stay at column 0; the block may be left open.
        let item = n.kind() == DEV_BLOCK_ITEM;
        self.braced_body(&members, close.as_ref(), !item);
    }

    // ---- expressions ----------------------------------------------------------------------

    /// `( a, b )`: arguments, parameters, vector components.
    fn args(&mut self, n: &SyntaxNode) {
        let els = elements(n);
        let Some(NodeOrToken::Token(open)) = els.first().cloned() else {
            return self.generic(n);
        };
        if open.kind() != L_PAREN {
            return self.generic(n);
        }
        let close = match els.last() {
            Some(NodeOrToken::Token(t)) if t.kind() == R_PAREN && els.len() > 1 => Some(t.clone()),
            _ => None,
        };
        let inner = &els[1..els.len() - usize::from(close.is_some())];
        self.token(&open);
        if inner.is_empty() {
            if let Some(c) = &close {
                self.sep(Sep::None);
                self.token(c);
            }
            return;
        }
        let ps = self.opts.paren_spacing;
        let items = list_items(inner);
        let pad = self.pad_open(ps, &open);
        match self.opts.wrap {
            Wrap::Fill => {
                // The gap after `(` goes inside the indent, so an own-line comment there is
                // indented like the arguments.
                self.doc.open(Container::Indent);
                self.sep(pad);
                self.doc.open(Container::Fill);
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        self.sep(Sep::Line);
                    }
                    self.doc.open(Container::Concat);
                    self.item(item);
                    self.doc.close();
                }
                self.doc.close();
                self.doc.close();
                if let Some(c) = &close {
                    self.sep(self.pad_close(ps, c));
                    self.token(c);
                }
            }
            Wrap::OnePerLine => {
                self.doc.open(Container::Group);
                self.doc.open(Container::Indent);
                self.sep(if pad == Sep::Space {
                    Sep::Line
                } else {
                    Sep::SoftLine
                });
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        self.sep(Sep::Line);
                    }
                    self.item(item);
                }
                self.doc.close();
                if let Some(c) = &close {
                    let p = self.pad_close(ps, c);
                    self.sep(if p == Sep::Space {
                        Sep::Line
                    } else {
                        Sep::SoftLine
                    });
                    self.token(c);
                }
                self.doc.close();
            }
        }
    }

    /// One list item: an element and its comma.
    fn item(&mut self, item: &[NodeOrToken<SyntaxNode, SyntaxToken>]) {
        for el in item {
            match el {
                NodeOrToken::Token(t) if t.kind() == T![,] => {
                    self.sep(Sep::None);
                    self.token(t);
                }
                NodeOrToken::Token(t) => self.token(t),
                NodeOrToken::Node(c) => self.expr(c),
            }
        }
    }

    /// `[ a, b ]` and `{ k: v }`: flat, or one element per line.
    fn literal_list(&mut self, n: &SyntaxNode) {
        let els = elements(n);
        if els.len() < 2 {
            return self.generic(n);
        }
        let (NodeOrToken::Token(open), NodeOrToken::Token(close)) =
            (els[0].clone(), els[els.len() - 1].clone())
        else {
            return self.generic(n);
        };
        self.token(&open);
        let inner = &els[1..els.len() - 1];
        if inner.is_empty() {
            self.sep(Sep::None);
            self.token(&close);
            return;
        }
        let ps = self.opts.paren_spacing;
        let line = |p: Sep| {
            if p == Sep::Space {
                Sep::Line
            } else {
                Sep::SoftLine
            }
        };
        // Lists of plain values fill lines; other lists go one element per line.
        let scalar = self.opts.wrap == Wrap::Fill
            && inner.iter().all(|e| match e {
                NodeOrToken::Node(c) => is_scalar(c),
                NodeOrToken::Token(t) => t.kind() == T![,],
            });
        let items = list_items(inner);
        self.doc.open(Container::Group);
        self.doc.open(Container::Indent);
        self.sep(line(self.pad_open(ps, &open)));
        if scalar {
            self.doc.open(Container::Fill);
        }
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                self.sep(Sep::Line);
            }
            if scalar {
                self.doc.open(Container::Concat);
            }
            self.item(item);
            if scalar {
                self.doc.close();
            }
        }
        if scalar {
            self.doc.close();
        }
        self.doc.close();
        self.sep(line(self.pad_close(ps, &close)));
        self.token(&close);
        self.doc.close();
    }

    /// The operands and operators of a chain of binary operators of one precedence class.
    fn chain(&self, n: &SyntaxNode, prec: u8, out: &mut Vec<NodeOrToken<SyntaxNode, SyntaxToken>>) {
        for el in elements(n) {
            match &el {
                NodeOrToken::Node(c)
                    if c.kind() == BIN_EXPR
                        && out.is_empty()
                        && bin_op(c).is_some_and(|o| precedence(o.kind()) == prec) =>
                {
                    self.chain(c, prec, out);
                }
                _ => out.push(el),
            }
        }
    }

    fn binary(&mut self, n: &SyntaxNode) {
        let Some(op) = bin_op(n) else {
            return self.generic(n);
        };
        let prec = precedence(op.kind());
        let mut parts = Vec::new();
        self.chain(n, prec, &mut parts);
        // parts: operand (op operand)*
        let logical = matches!(op.kind(), AMP2 | PIPE2);
        if logical {
            self.doc.open(Container::Group);
            let mut it = parts.iter();
            if let Some(first) = it.next() {
                self.operand(first);
            }
            self.doc.open(Container::Indent);
            let mut after_op = false;
            for el in it {
                match el {
                    NodeOrToken::Token(t) if !after_op => {
                        self.sep(Sep::Space);
                        self.token(t);
                        after_op = true;
                    }
                    _ => {
                        self.sep(Sep::Line);
                        self.operand(el);
                        after_op = false;
                    }
                }
            }
            self.doc.close();
            self.doc.close();
        } else {
            self.doc.open(Container::Indent);
            self.doc.open(Container::Fill);
            self.doc.open(Container::Concat);
            let mut it = parts.iter();
            if let Some(first) = it.next() {
                self.operand(first);
            }
            let mut after_op = false;
            for el in it {
                match el {
                    NodeOrToken::Token(t) if !after_op => {
                        self.sep(Sep::Space);
                        self.token(t);
                        after_op = true;
                    }
                    _ => {
                        self.doc.close();
                        self.sep(Sep::Line);
                        self.doc.open(Container::Concat);
                        self.operand(el);
                        after_op = false;
                    }
                }
            }
            self.doc.close();
            self.doc.close();
            self.doc.close();
        }
    }

    /// `lhs op rhs`. A value without break points of its own may move to the next line.
    fn assign(&mut self, n: &SyntaxNode) {
        let mut after_op = false;
        for (i, el) in elements(n).into_iter().enumerate() {
            match &el {
                NodeOrToken::Token(t) if !after_op && i > 0 => {
                    self.sep(Sep::Space);
                    self.token(t);
                    after_op = true;
                }
                NodeOrToken::Node(c) if after_op && !has_break_points(c) => {
                    self.doc.open(Container::Group);
                    self.doc.open(Container::Indent);
                    self.sep(Sep::Line);
                    self.expr(c);
                    self.doc.close();
                    self.doc.close();
                }
                NodeOrToken::Node(c) => {
                    if after_op {
                        self.sep(Sep::Space);
                    }
                    self.expr(c);
                }
                NodeOrToken::Token(t) => {
                    self.gap_to(t);
                    self.token(t);
                }
            }
        }
    }

    fn operand(&mut self, el: &NodeOrToken<SyntaxNode, SyntaxToken>) {
        match el {
            NodeOrToken::Node(c) => self.expr(c),
            NodeOrToken::Token(t) => self.token(t),
        }
    }

    fn ternary(&mut self, n: &SyntaxNode) {
        self.doc.open(Container::Group);
        let mut indented = false;
        for el in elements(n) {
            match el {
                NodeOrToken::Token(t) if matches!(t.kind(), QUESTION | COLON) => {
                    if !indented {
                        self.doc.open(Container::Indent);
                        indented = true;
                    }
                    self.sep(Sep::Line);
                    self.token(&t);
                    self.sep(Sep::Space);
                }
                NodeOrToken::Token(t) => {
                    self.gap_to(&t);
                    self.token(&t);
                }
                NodeOrToken::Node(c) => self.expr(&c),
            }
        }
        if indented {
            self.doc.close();
        }
        self.doc.close();
    }

    fn call(&mut self, n: &SyntaxNode) {
        let ps = self.opts.paren_spacing;
        let mut prev: Option<SyntaxKind> = None;
        let mut brackets = 0usize;
        for el in elements(n) {
            let k = el.kind();
            let sep = match (prev, k) {
                (None, _) => Sep::None,
                (_, ARG_LIST) => Sep::None,
                (Some(L_BRACK), L_BRACK) | (Some(R_BRACK), R_BRACK) => Sep::None,
                (Some(L_BRACK), _) if brackets == 2 => match ps {
                    ParenSpacing::Compact => Sep::None,
                    ParenSpacing::Padded => Sep::Space,
                    ParenSpacing::Preserve => self.preserve_before(&el),
                },
                (_, R_BRACK) if brackets == 2 => match ps {
                    ParenSpacing::Compact => Sep::None,
                    ParenSpacing::Padded => Sep::Space,
                    ParenSpacing::Preserve => self.preserve_before(&el),
                },
                (Some(R_BRACK), ARROW) | (Some(ARROW), _) => Sep::None,
                _ => Sep::Space,
            };
            self.sep(sep);
            match &el {
                NodeOrToken::Token(t) => {
                    if t.kind() == L_BRACK {
                        brackets += 1;
                    }
                    self.token(t);
                    if t.kind() == R_BRACK {
                        brackets = brackets.saturating_sub(1);
                    }
                }
                NodeOrToken::Node(c) => self.expr(c),
            }
            prev = Some(k);
        }
    }

    fn preserve_before(&self, el: &NodeOrToken<SyntaxNode, SyntaxToken>) -> Sep {
        let t = match el {
            NodeOrToken::Token(t) => Some(t.clone()),
            NodeOrToken::Node(c) => gsc_syntax::first_significant_token(c),
        };
        match t {
            Some(t) if self.space_before(&t) => Sep::Space,
            _ => Sep::None,
        }
    }

    fn paren(&mut self, n: &SyntaxNode) {
        self.wrapped(n, self.opts.paren_spacing, L_PAREN, R_PAREN);
    }

    fn index(&mut self, n: &SyntaxNode) {
        self.wrapped(n, self.opts.bracket_spacing, L_BRACK, R_BRACK);
    }

    /// `base? open inner close` with padding inside the brackets.
    fn wrapped(
        &mut self,
        n: &SyntaxNode,
        style: ParenSpacing,
        open_k: SyntaxKind,
        close_k: SyntaxKind,
    ) {
        let mut opened: Option<SyntaxToken> = None;
        let mut started = false;
        for el in elements(n) {
            match &el {
                NodeOrToken::Token(t) if t.kind() == open_k && opened.is_none() => {
                    if started {
                        self.sep(Sep::None);
                    }
                    self.token(t);
                    opened = Some(t.clone());
                }
                NodeOrToken::Token(t) if t.kind() == close_k && opened.is_some() => {
                    self.sep(self.pad_close(style, t));
                    self.token(t);
                }
                NodeOrToken::Node(c) => {
                    if let Some(o) = &opened {
                        self.sep(self.pad_open(style, o));
                    }
                    self.expr(c);
                }
                NodeOrToken::Token(t) => {
                    self.gap_to(t);
                    self.token(t);
                }
            }
            started = true;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Indented {
    No,
    /// Each statement indented one level, except blocks (`case` bodies).
    UnlessBlock,
}

/// The operator token of a binary expression.
fn bin_op(n: &SyntaxNode) -> Option<SyntaxToken> {
    n.children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind().is_punct())
}

/// Is `s` spelled like a keyword that is not also a name (`function`, `class`; not `self`)?
fn is_keyword_text(s: &str) -> bool {
    SyntaxKind::KEYWORDS
        .iter()
        .any(|(k, kind)| !kind.is_name_like_keyword() && k.eq_ignore_ascii_case(s))
}

/// The elements of a list grouped with the comma after each.
fn list_items(
    inner: &[NodeOrToken<SyntaxNode, SyntaxToken>],
) -> Vec<Vec<NodeOrToken<SyntaxNode, SyntaxToken>>> {
    let mut items: Vec<Vec<NodeOrToken<SyntaxNode, SyntaxToken>>> = Vec::new();
    for el in inner {
        match items.last_mut() {
            Some(last) if el.kind() == T![,] && last.last().is_some_and(|l| l.kind() != T![,]) => {
                last.push(el.clone())
            }
            _ => items.push(vec![el.clone()]),
        }
    }
    items
}

/// A literal, a name or a negated number: an array element that fills lines.
fn is_scalar(n: &SyntaxNode) -> bool {
    match n.kind() {
        LITERAL | NAME_REF => true,
        PREFIX_EXPR => n.children().all(|c| c.kind() == LITERAL),
        _ => false,
    }
}

/// Does an expression hold a list or operator chain that can break across lines?
fn has_break_points(n: &SyntaxNode) -> bool {
    n.descendants().any(|d| {
        matches!(
            d.kind(),
            ARG_LIST | ARRAY_EXPR | STRUCT_EXPR | BIN_EXPR | TERNARY_EXPR | VECTOR_EXPR
        )
    })
}
