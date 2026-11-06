//! `#define` macro expansion in a side table. The lossless tree is never changed.
//!
//! [`macro_expansions`] finds every use of a macro in a file and returns its expansion as tokens,
//! so that callers (index, check) can see the calls and events a macro body holds. Macros defined
//! in headers come in through a [`MacroTable`] built from the header trees
//! ([`macro_expansions_with`]); [`header_paths`] lists the headers a file pulls in.
//!
//! The expansion follows the C preprocessor where GSC code uses it: object-like and function-like
//! macros, arguments expanded before substitution, rescanning, and no recursion into a macro
//! being expanded. `#` (stringify) and `##` (paste) are left as written: the shipped code has none.

use std::collections::HashMap;

use rowan::{NodeOrToken, TextRange, WalkEvent};

use crate::{Dialect, Parse, SyntaxKind, SyntaxNode};
use SyntaxKind::*;

/// A token of an expansion: its kind and text. Expanded tokens have no position of their own;
/// they all belong to the use they expand ([`MacroExpansion::use_range`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpandedToken {
    pub kind: SyntaxKind,
    pub text: String,
}

/// One `#define`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacroDef {
    pub name: String,
    /// The parameters of a function-like macro; `None` for an object-like one.
    pub params: Option<Vec<String>>,
    /// The body, without trivia (line continuations removed).
    pub body: Vec<ExpandedToken>,
    /// The range of the `#define` in the file that holds it.
    pub range: TextRange,
}

impl MacroDef {
    /// Is this a function-like macro (`#define F( a, b ) ...`)?
    pub fn is_function_like(&self) -> bool {
        self.params.is_some()
    }
}

/// The macros visible to a file: those of its headers, then its own.
#[derive(Clone, Debug, Default)]
pub struct MacroTable {
    defs: HashMap<String, MacroDef>,
}

impl MacroTable {
    pub fn new() -> MacroTable {
        MacroTable::default()
    }

    /// The `#define`s of a tree (a header or a script), in source order; a later `#define` of the
    /// same name wins and `#undef` removes one.
    pub fn from_tree(root: &SyntaxNode, dialect: &Dialect) -> MacroTable {
        let mut t = MacroTable::new();
        t.add_tree(root, dialect);
        t
    }

    /// Add the `#define`s of a tree (see [`MacroTable::from_tree`]).
    pub fn add_tree(&mut self, root: &SyntaxNode, dialect: &Dialect) {
        for item in flatten(root, dialect) {
            match item {
                Item::Define(d) => self.insert(d),
                Item::Undef(name) => {
                    self.defs.remove(&name);
                }
                Item::Tok(..) => {}
            }
        }
    }

    /// Add (or replace) one definition.
    pub fn insert(&mut self, def: MacroDef) {
        self.defs.insert(def.name.clone(), def);
    }

    /// Add every definition of `other`, replacing those of the same name.
    pub fn extend(&mut self, other: &MacroTable) {
        for d in other.defs.values() {
            self.insert(d.clone());
        }
    }

    pub fn get(&self, name: &str) -> Option<&MacroDef> {
        self.defs.get(name)
    }

    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    /// The definitions, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = &MacroDef> {
        self.defs.values()
    }
}

/// One use of a macro and what it expands to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacroExpansion {
    /// The macro name as written at the use.
    pub name: String,
    /// The use: the name, plus its argument list for a function-like macro.
    pub use_range: TextRange,
    /// The fully expanded tokens (macros inside the body and the arguments expanded too).
    pub tokens: Vec<ExpandedToken>,
    /// False when the expansion stopped early (an argument list without its `)`, or nesting
    /// deeper than 64 levels).
    pub complete: bool,
    /// Is the use at file scope (outside any function, class or constant), where the expansion
    /// is items (`REGISTER_SYSTEM( ... )` expanding to a function)?
    pub at_file_scope: bool,
}

impl MacroExpansion {
    /// The expansion as text, tokens separated by one space.
    pub fn text(&self) -> String {
        let mut s = String::new();
        for (i, t) in self.tokens.iter().enumerate() {
            if i > 0 {
                s.push(' ');
            }
            s.push_str(&t.text);
        }
        s
    }

    /// Parse the expansion. At file scope it parses as items (a file of its own). Elsewhere it
    /// parses as the statements of a function body: the tree is `SOURCE_FILE > FUNCTION_DEF` of
    /// a synthetic `__macro_expansion__()` whose block holds the expansion followed by `;`, so
    /// an expression body (`GetDvarInt( "x", 0 )`) parses as an expression statement. Offsets
    /// are those of the parsed text, not of the file: map everything found inside to
    /// [`MacroExpansion::use_range`].
    pub fn parse(&self, dialect: &Dialect) -> Parse {
        if self.at_file_scope {
            return crate::parse(&self.text(), dialect);
        }
        let text = format!("__macro_expansion__()\n{{\n{}\n;\n}}\n", self.text());
        crate::parse(&text, dialect)
    }
}

/// Every macro use in `root`, expanded with the macros the file itself defines.
pub fn macro_expansions(root: &SyntaxNode, dialect: &Dialect) -> Vec<MacroExpansion> {
    macro_expansions_with(root, &MacroTable::new(), dialect)
}

/// Every macro use in `root`, expanded with `headers` (the macros of the headers the file pulls
/// in, see [`header_paths`]) and then the file's own `#define`s, in source order.
pub fn macro_expansions_with(
    root: &SyntaxNode,
    headers: &MacroTable,
    dialect: &Dialect,
) -> Vec<MacroExpansion> {
    let mut table = headers.clone();
    let mut out = Vec::new();
    // Runs of code tokens between directives.
    let mut toks: Vec<ExpandedToken> = Vec::new();
    let mut ranges: Vec<(TextRange, bool)> = Vec::new();
    let mut flush = |table: &MacroTable, toks: &mut Vec<ExpandedToken>, ranges: &mut Vec<_>| {
        expand_run(table, toks, ranges, &mut out);
        toks.clear();
        ranges.clear();
    };
    for item in flatten(root, dialect) {
        match item {
            Item::Tok(t, r, top) => {
                toks.push(t);
                ranges.push((r, top));
            }
            Item::Define(d) => {
                flush(&table, &mut toks, &mut ranges);
                table.insert(d);
            }
            Item::Undef(name) => {
                flush(&table, &mut toks, &mut ranges);
                table.defs.remove(&name);
            }
        }
    }
    flush(&table, &mut toks, &mut ranges);
    out
}

/// The macro uses of one run of code tokens.
fn expand_run(
    table: &MacroTable,
    toks: &[ExpandedToken],
    ranges: &[(TextRange, bool)],
    out: &mut Vec<MacroExpansion>,
) {
    if table.is_empty() {
        return;
    }
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        let Some(def) = (t.kind == IDENT).then(|| table.get(&t.text)).flatten() else {
            i += 1;
            continue;
        };
        let mut ex = Expander {
            table,
            complete: true,
        };
        let (tokens, used) = if def.is_function_like() {
            match ex.call(def, toks, i, &mut Vec::new(), 0) {
                Some(r) => r,
                None => {
                    // A function-like name without arguments is not a use.
                    i += 1;
                    continue;
                }
            }
        } else {
            let mut hide = vec![def.name.clone()];
            (ex.expand(&def.body, &mut hide, 1), 1)
        };
        out.push(MacroExpansion {
            name: t.text.clone(),
            use_range: ranges[i].0.cover(ranges[i + used - 1].0),
            tokens,
            complete: ex.complete,
            at_file_scope: ranges[i].1,
        });
        i += used;
    }
}

/// The header paths a file pulls in with `#insert` or `#inline`, as written
/// (`scripts\shared\shared.gsh`).
pub fn header_paths(root: &SyntaxNode) -> Vec<String> {
    root.descendants()
        .filter(|n| matches!(n.kind(), INSERT_DIRECTIVE | INLINE_DIRECTIVE))
        .filter_map(|n| {
            n.children_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .find(|t| matches!(t.kind(), PATH | IDENT | STRING))
                .map(|t| t.text().trim_matches('"').to_owned())
        })
        .collect()
}

/// A set of headers (`.gsh` files) by path, to find the macros a file sees through `#insert` and
/// `#inline`, transitively. Paths compare case-insensitively with `/` and `\` alike, and a
/// directive path matches a header whose path ends with it (`scripts\shared\shared.gsh` finds
/// `C:/mods/scripts/shared/shared.gsh`).
#[derive(Clone, Debug, Default)]
pub struct HeaderSet {
    /// Normalised path, the header's own macros, the headers it pulls in.
    entries: Vec<(String, MacroTable, Vec<String>)>,
}

fn normalize_path(p: &str) -> String {
    p.replace('\\', "/").to_ascii_lowercase()
}

impl HeaderSet {
    pub fn new() -> HeaderSet {
        HeaderSet::default()
    }

    /// Add the header at `path` whose tree is `root`.
    pub fn add(&mut self, path: &str, root: &SyntaxNode, dialect: &Dialect) {
        self.entries.push((
            normalize_path(path),
            MacroTable::from_tree(root, dialect),
            header_paths(root),
        ));
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn find(&self, wanted: &str) -> Option<usize> {
        let w = normalize_path(wanted);
        let suffix = format!("/{w}");
        self.entries
            .iter()
            .position(|(p, _, _)| *p == w || p.ends_with(&suffix))
    }

    /// Is there a header for the directive path `wanted`?
    pub fn contains(&self, wanted: &str) -> bool {
        self.find(wanted).is_some()
    }

    /// The macros of the headers `wanted` (directive paths, see [`header_paths`]) and of the
    /// headers they pull in. Paths without a header are skipped.
    pub fn macros_for(&self, wanted: &[String]) -> MacroTable {
        let mut table = MacroTable::new();
        let mut seen = vec![false; self.entries.len()];
        for w in wanted {
            self.visit(w, &mut seen, &mut table);
        }
        table
    }

    /// A header's own headers first, then its macros (which replace theirs), as the
    /// preprocessor reads them.
    fn visit(&self, wanted: &str, seen: &mut [bool], table: &mut MacroTable) {
        let Some(i) = self.find(wanted) else { return };
        if std::mem::replace(&mut seen[i], true) {
            return;
        }
        let (_, t, more) = &self.entries[i];
        for m in more {
            self.visit(m, seen, table);
        }
        table.extend(t);
    }
}

const MAX_DEPTH: usize = 64;

enum Item {
    /// A code token, its range, and whether it is at file scope.
    Tok(ExpandedToken, TextRange, bool),
    Define(MacroDef),
    Undef(String),
}

/// The non-trivia tokens of the code (directive lines left out) and the `#define`/`#undef`
/// directives, in source order.
fn flatten(root: &SyntaxNode, dialect: &Dialect) -> Vec<Item> {
    let mut out = Vec::new();
    let mut walk = root.preorder_with_tokens();
    // Depth of function, class and constant nodes around the current token.
    let mut scoped = 0usize;
    let is_scope = |k: SyntaxKind| matches!(k, FUNCTION_DEF | CLASS_DEF | FILE_CONSTANT);
    while let Some(ev) = walk.next() {
        let e = match ev {
            WalkEvent::Enter(e) => e,
            WalkEvent::Leave(NodeOrToken::Node(n)) => {
                scoped -= usize::from(is_scope(n.kind()));
                continue;
            }
            WalkEvent::Leave(_) => continue,
        };
        match e {
            NodeOrToken::Node(n) => match n.kind() {
                PREPROC_DEFINE => {
                    if let Some(d) = definition(&n, dialect) {
                        out.push(Item::Define(d));
                    }
                    walk.skip_subtree();
                }
                PREPROC_UNDEF => {
                    if let Some(name) = n
                        .descendants_with_tokens()
                        .filter_map(NodeOrToken::into_token)
                        .find(|t| t.kind() == IDENT)
                    {
                        out.push(Item::Undef(name.text().to_owned()));
                    }
                    walk.skip_subtree();
                }
                USING_DIRECTIVE
                | INCLUDE_DIRECTIVE
                | INSERT_DIRECTIVE
                | INLINE_DIRECTIVE
                | NAMESPACE_DIRECTIVE
                | PRECACHE_DIRECTIVE
                | USING_ANIMTREE_DIRECTIVE => {
                    walk.skip_subtree();
                }
                k => scoped += usize::from(is_scope(k)),
            },
            NodeOrToken::Token(t) => {
                if !t.kind().is_trivia() && !t.kind().is_directive() {
                    out.push(Item::Tok(
                        ExpandedToken {
                            kind: t.kind(),
                            text: t.text().to_owned(),
                        },
                        t.text_range(),
                        scoped == 0,
                    ));
                }
            }
        }
    }
    out
}

fn definition(n: &SyntaxNode, dialect: &Dialect) -> Option<MacroDef> {
    let name = n
        .children()
        .find(|c| c.kind() == NAME)
        .map(|c| c.text().to_string())?;
    let params = n.children().find(|c| c.kind() == MACRO_PARAMS).map(|p| {
        p.children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .filter(|t| matches!(t.kind(), IDENT | DOT3))
            .map(|t| t.text().to_owned())
            .collect()
    });
    let body = n
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|t| t.kind() == MACRO_BODY)
        .map(|t| lex_body(t.text(), dialect))
        .unwrap_or_default();
    Some(MacroDef {
        name,
        params,
        body,
        range: n.text_range(),
    })
}

/// Lex a macro body, line continuations removed, trivia dropped.
fn lex_body(text: &str, dialect: &Dialect) -> Vec<ExpandedToken> {
    let text = text
        .replace("\\\r\n", " ")
        .replace("\\\n", " ")
        .replace("\\\r", " ");
    let (tokens, _) = crate::lexer::tokenize(&text, dialect);
    let mut pos = 0usize;
    let mut out = Vec::new();
    for t in tokens {
        let s = &text[pos..pos + t.len as usize];
        pos += t.len as usize;
        if !t.kind.is_trivia() {
            out.push(ExpandedToken {
                kind: t.kind,
                text: s.to_owned(),
            });
        }
    }
    out
}

struct Expander<'a> {
    table: &'a MacroTable,
    complete: bool,
}

impl Expander<'_> {
    /// Expand `tokens`, skipping the macros in `hide` (those being expanded).
    fn expand(
        &mut self,
        tokens: &[ExpandedToken],
        hide: &mut Vec<String>,
        depth: usize,
    ) -> Vec<ExpandedToken> {
        if depth > MAX_DEPTH {
            self.complete = false;
            return tokens.to_vec();
        }
        let mut out = Vec::with_capacity(tokens.len());
        let mut i = 0;
        while i < tokens.len() {
            let t = &tokens[i];
            let def = (t.kind == IDENT)
                .then(|| self.table.get(&t.text))
                .flatten()
                .filter(|d| !hide.contains(&d.name));
            match def {
                Some(def) if def.is_function_like() => {
                    match self.call(def, tokens, i, hide, depth) {
                        Some((exp, used)) => {
                            out.extend(exp);
                            i += used;
                        }
                        None => {
                            out.push(t.clone());
                            i += 1;
                        }
                    }
                }
                Some(def) => {
                    hide.push(def.name.clone());
                    out.extend(self.expand(&def.body, hide, depth + 1));
                    hide.pop();
                    i += 1;
                }
                None => {
                    out.push(t.clone());
                    i += 1;
                }
            }
        }
        out
    }

    /// Expand the call of function-like `def` whose name is `tokens[at]`. Returns the expansion
    /// and the number of tokens used (name to `)`), or `None` when no `(` follows the name.
    fn call(
        &mut self,
        def: &MacroDef,
        tokens: &[ExpandedToken],
        at: usize,
        hide: &mut Vec<String>,
        depth: usize,
    ) -> Option<(Vec<ExpandedToken>, usize)> {
        if tokens.get(at + 1).map(|t| t.kind) != Some(L_PAREN) {
            return None;
        }
        // Split the arguments at depth-0 commas.
        let mut args: Vec<Vec<ExpandedToken>> = vec![Vec::new()];
        let mut level = 0usize;
        let mut j = at + 2;
        let mut closed = false;
        while j < tokens.len() {
            let t = &tokens[j];
            match t.kind {
                L_PAREN | L_BRACK | L_CURLY => level += 1,
                R_PAREN if level == 0 => {
                    closed = true;
                    break;
                }
                R_PAREN | R_BRACK | R_CURLY => level = level.saturating_sub(1),
                COMMA if level == 0 => {
                    args.push(Vec::new());
                    j += 1;
                    continue;
                }
                _ => {}
            }
            args.last_mut().expect("never empty").push(t.clone());
            j += 1;
        }
        if !closed {
            self.complete = false;
        }
        let used = (j + 1).min(tokens.len()) - at;
        let params = def.params.as_deref().unwrap_or_default();
        if params.is_empty() && args.len() == 1 && args[0].is_empty() {
            args.clear();
        }
        let expanded_args: Vec<Vec<ExpandedToken>> = args
            .iter()
            .map(|a| self.expand(a, hide, depth + 1))
            .collect();
        let mut substituted = Vec::with_capacity(def.body.len());
        for t in &def.body {
            let param = (t.kind == IDENT || t.text == "__VA_ARGS__")
                .then(|| {
                    if t.text == "__VA_ARGS__" {
                        params.iter().position(|p| p == "...")
                    } else {
                        params.iter().position(|p| *p == t.text)
                    }
                })
                .flatten();
            match param {
                Some(k) if params[k] == "..." => {
                    // The variadic tail: the remaining arguments with their commas.
                    for (n, a) in expanded_args.iter().enumerate().skip(k) {
                        if n > k {
                            substituted.push(ExpandedToken {
                                kind: COMMA,
                                text: ",".to_owned(),
                            });
                        }
                        substituted.extend(a.iter().cloned());
                    }
                }
                Some(k) => {
                    if let Some(a) = expanded_args.get(k) {
                        substituted.extend(a.iter().cloned());
                    }
                }
                None => substituted.push(t.clone()),
            }
        }
        hide.push(def.name.clone());
        let out = self.expand(&substituted, hide, depth + 1);
        hide.pop();
        Some((out, used))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expansions(text: &str) -> Vec<(String, String)> {
        let d = Dialect::superset();
        let p = crate::parse(text, &d);
        macro_expansions(&p.syntax(), &d)
            .into_iter()
            .map(|e| (text[e.use_range].to_owned(), e.text()))
            .collect()
    }

    #[test]
    fn object_and_function_like() {
        let text = "#define WAIT_FRAME wait 0.05;\n#define NOTIFY_ALL(e, x) level notify(e, x); \\\n  self notify(e)\n#define TWICE(a) a + a\nf()\n{\n    WAIT_FRAME\n    NOTIFY_ALL(\"done\", g(1, 2));\n    x = TWICE(TWICE(1));\n}\n";
        assert_eq!(
            expansions(text),
            vec![
                ("WAIT_FRAME".to_owned(), "wait 0.05 ;".to_owned()),
                (
                    "NOTIFY_ALL(\"done\", g(1, 2))".to_owned(),
                    "level notify ( \"done\" , g ( 1 , 2 ) ) ; self notify ( \"done\" )".to_owned()
                ),
                ("TWICE(TWICE(1))".to_owned(), "1 + 1 + 1 + 1".to_owned()),
            ]
        );
    }

    #[test]
    fn no_recursion_and_order() {
        let text = "f()\n{\n    A;\n}\n#define A B\n#define B A + 1\ng()\n{\n    A;\n}\n#undef A\nh()\n{\n    A;\n}\n";
        // Before its #define and after #undef, A is not a macro; B inside A does not re-expand A.
        assert_eq!(expansions(text), vec![("A".to_owned(), "A + 1".to_owned())]);
    }

    #[test]
    fn headers_and_parse() {
        let d = Dialect::superset();
        let header = crate::parse("#define LOG(m) println(m); level notify(\"logged\")\n", &d);
        let table = MacroTable::from_tree(&header.syntax(), &d);
        assert_eq!(table.len(), 1);
        assert!(table.get("LOG").unwrap().is_function_like());
        let file = crate::parse(
            "#insert scripts\\shared\\log.gsh;\nf()\n{\n    LOG(\"hi\");\n}\n",
            &d,
        );
        assert_eq!(header_paths(&file.syntax()), ["scripts\\shared\\log.gsh"]);
        let ex = macro_expansions_with(&file.syntax(), &table, &d);
        assert_eq!(ex.len(), 1);
        assert!(ex[0].complete);
        let parsed = ex[0].parse(&d);
        assert!(parsed.errors().is_empty(), "{:?}", parsed.errors());
        let calls = parsed
            .syntax()
            .descendants()
            .filter(|n| n.kind() == CALL_EXPR)
            .count();
        assert_eq!(calls, 2);
        // The lossless tree is untouched.
        assert!(file.text().contains("LOG(\"hi\")"));
    }

    #[test]
    fn unclosed_call_is_incomplete() {
        let d = Dialect::superset();
        let p = crate::parse("#define F(a) a\nf()\n{\n    x = F(1;\n}\n", &d);
        let ex = macro_expansions(&p.syntax(), &d);
        assert_eq!(ex.len(), 1);
        assert!(!ex[0].complete);
    }
}
