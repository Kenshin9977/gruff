//! The facts of one file, read from its syntax tree alone: definitions, namespace, imports and
//! references. Nothing here looks at another file; names that need the workspace (bare calls,
//! `#using` targets) are kept as written and resolved by [`crate::resolve`].

use gsc_syntax::ast::{
    AnyParam, AstNode, CallExpr, ClassDef, Expr, FieldExpr, FunctionDef, FunctionRef, Literal,
    MethodCallExpr, NamespaceDirective, ParamList, PointerCallExpr, PreprocDefine, QualifiedRef,
};
use gsc_syntax::{SyntaxKind, SyntaxNode, SyntaxToken, TextRange, TextSize, WalkEvent};

use crate::profile::{Config, Resolution};
use crate::vfs::normalize_target;

/// What a definition is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DefKind {
    Function,
    /// `class::method`, constructors and destructors included.
    Method,
    Class,
    /// `#define`.
    Define,
    /// A file constant (`const X = 1;` at file scope).
    Const,
}

impl DefKind {
    pub fn label(self) -> &'static str {
        match self {
            DefKind::Function => "function",
            DefKind::Method => "method",
            DefKind::Class => "class",
            DefKind::Define => "define",
            DefKind::Const => "const",
        }
    }

    /// Callable kinds.
    pub fn is_callable(self) -> bool {
        matches!(self, DefKind::Function | DefKind::Method)
    }
}

/// A definition.
#[derive(Clone, Debug)]
pub struct Def {
    pub kind: DefKind,
    /// The namespace in effect (identifier case).
    pub ns: String,
    /// Identifier case for functions, methods (`class::method`) and classes; macros and constants
    /// keep their spelling.
    pub name: String,
    /// The whole definition.
    pub range: TextRange,
    /// The name (the whole definition when it has none).
    pub name_range: TextRange,
    /// The first significant token, where the definition "is" for line numbers.
    pub focus: TextSize,
    /// Parameters as written (`a, &b, c=1, ...`); bases for a class; macro parameters.
    pub params: String,
    /// `autoexec`, `private`, `codecall`, `event_handler[x]`, `dev`, comma-separated.
    pub flags: String,
    /// The summary of the doc comment; `= value` for constants and bodies of undocumented macros.
    pub doc: String,
}

impl Def {
    /// The name without its class (`m` for `class::m`).
    pub fn short_name(&self) -> &str {
        match self.kind {
            DefKind::Method => self.name.rsplit("::").next().unwrap_or(&self.name),
            _ => &self.name,
        }
    }

    /// The class of a method.
    pub fn class(&self) -> Option<&str> {
        match self.kind {
            DefKind::Method => self.name.split_once("::").map(|(c, _)| c),
            _ => None,
        }
    }

    /// `ns::name`.
    pub fn qualified(&self) -> String {
        format!("{}::{}", self.ns, self.name)
    }
}

/// `#using`, `#include`, `#insert` or `#inline`.
#[derive(Clone, Debug)]
pub struct Import {
    /// `using`, `include`, `insert` or `inline`.
    pub directive: &'static str,
    /// The target with `/` separators (lower case in case-insensitive games).
    pub target: String,
    pub focus: TextSize,
    /// The path token.
    pub range: TextRange,
}

impl Import {
    /// Does the directive bring functions into scope (`#using`, `#include`), as opposed to
    /// pasting text (`#insert`, `#inline`)?
    pub fn is_scope(&self) -> bool {
        matches!(self.directive, "using" | "include")
    }
}

/// A call target as written.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    /// `foo()`: resolved against the file, its namespace, its imports and the builtins.
    Bare(String),
    /// `ns::foo()`, `path\file::foo()`; `None` for `::foo`.
    Qualified(Option<String>, String),
    /// A keyword the language treats as a builtin call (`isdefined`, `assert`...).
    Keyword(String),
    /// `[[ expr ]]()`: the expression text.
    Dynamic(String),
    /// `[[ obj ]]->name()`.
    Method(String),
}

/// A reference: a call, a thread, a method call or a function pointer.
#[derive(Clone, Debug)]
pub struct Ref {
    /// The enclosing function or method (index into the file's definitions).
    pub caller: Option<u32>,
    pub target: Target,
    /// `call`, `method`, `thread`, `childthread` or `pointer`.
    pub kind: &'static str,
    /// `self`, `level`, `player`, `game`, `anim`, `other` or `-`.
    pub receiver: &'static str,
    /// The first significant token of the call.
    pub focus: TextSize,
    /// The name referred to (the whole call for dynamic ones).
    pub range: TextRange,
}

impl Ref {
    /// Called on an object (`ent foo()`): builtin lookups prefer methods.
    pub fn has_receiver(&self) -> bool {
        self.receiver != "-"
    }
}

/// Everything one file says about names.
#[derive(Clone, Debug, Default)]
pub struct FileFacts {
    /// The first `#namespace` (T7+), else the default: file name (T7+) or path (IW).
    pub namespace: String,
    pub defs: Vec<Def>,
    pub imports: Vec<Import>,
    pub refs: Vec<Ref>,
    /// Field names used in the file (identifier case, sorted, unique), for completion.
    pub fields: Vec<String>,
    /// Event sites (`notify`, `waittill`, `endon`, wrappers), in tree order.
    pub events: Vec<crate::events::EventSite>,
    /// Event-like string literals (lower case, sorted, unique) used elsewhere than as an event
    /// name: they may reach a `notify( var )`.
    pub strings: Vec<String>,
    /// The namespace in effect from each offset on: the default at 0, then each `#namespace`.
    pub ns_switches: Vec<(TextSize, String)>,
}

impl FileFacts {
    /// The definition enclosing `offset` (innermost: a method before its class).
    pub fn def_at(&self, offset: TextSize) -> Option<u32> {
        self.defs
            .iter()
            .enumerate()
            .filter(|(_, d)| d.range.contains_inclusive(offset))
            .min_by_key(|(_, d)| d.range.len())
            .map(|(i, _)| i as u32)
    }

    /// The caller's namespace and class for resolution.
    pub fn scope_of(&self, caller: Option<u32>) -> (&str, Option<&str>) {
        match caller.map(|c| &self.defs[c as usize]) {
            Some(d) => (&d.ns, d.class()),
            None => (&self.namespace, None),
        }
    }

    /// The namespace and class a reference resolves in: its caller's, or outside functions
    /// (file-scope macro expansions such as `REGISTER_SYSTEM`) the namespace in effect there.
    pub fn ref_scope(&self, x: &Ref) -> (&str, Option<&str>) {
        if x.caller.is_some() {
            return self.scope_of(x.caller);
        }
        let i = self
            .ns_switches
            .partition_point(|(at, _)| *at <= x.range.start());
        match i.checked_sub(1).and_then(|i| self.ns_switches.get(i)) {
            Some((_, ns)) => (ns, None),
            None => (&self.namespace, None),
        }
    }

    /// `ns::fn` of a caller, `<file>` outside functions.
    pub fn caller_name(&self, caller: Option<u32>) -> String {
        match caller.map(|c| &self.defs[c as usize]) {
            Some(d) => d.qualified(),
            None => "<file>".to_owned(),
        }
    }
}

/// Line starts, for 1-based line numbers in O(log n). `LineIndex` also counts the column, which
/// is linear in the line length and quadratic on the one-line arrays of decompiled files.
pub struct LineStarts(Vec<u32>);

impl LineStarts {
    pub fn new(text: &str) -> LineStarts {
        let mut v = vec![0];
        let b = text.as_bytes();
        for (i, &c) in b.iter().enumerate() {
            if c == b'\n' || (c == b'\r' && b.get(i + 1) != Some(&b'\n')) {
                v.push(i as u32 + 1);
            }
        }
        LineStarts(v)
    }

    pub fn line(&self, offset: TextSize) -> u32 {
        let o = u32::from(offset);
        match self.0.binary_search(&o) {
            Ok(l) => l as u32 + 1,
            Err(l) => l as u32,
        }
    }
}

/// The first non-trivia token anywhere under `node`.
pub use gsc_syntax::first_significant_token as first_significant;

/// Where a node "is": its first significant token, else its start.
pub fn focus_of(node: &SyntaxNode) -> TextSize {
    first_significant(node).map_or(node.text_range().start(), |t| t.text_range().start())
}

/// Is `node` inside a `/# #/` dev block?
pub fn in_dev_block(node: &SyntaxNode) -> bool {
    node.ancestors().any(|a| {
        matches!(
            a.kind(),
            SyntaxKind::DEV_BLOCK_STMT | SyntaxKind::DEV_BLOCK_ITEM
        )
    })
}

/// Collapse whitespace and cut to `max` characters, for a one-line cell.
pub fn squash(s: &str, max: usize) -> String {
    let mut out = String::new();
    for (i, w) in s.split_whitespace().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(w);
    }
    if out.chars().count() > max {
        let mut cut: String = out.chars().take(max.saturating_sub(1)).collect();
        cut.push('~');
        return cut;
    }
    out
}

/// The text of a node without its comments, whitespace collapsed.
pub fn code_text(node: &SyntaxNode, max: usize) -> String {
    let mut s = String::new();
    for t in node
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
    {
        if matches!(
            t.kind(),
            SyntaxKind::COMMENT | SyntaxKind::BLOCK_COMMENT | SyntaxKind::DOC_COMMENT
        ) {
            continue;
        }
        if t.kind().is_trivia() {
            s.push(' ');
        } else {
            s.push_str(t.text());
        }
    }
    squash(&s, max)
}

/// The expression inside any number of parentheses.
pub fn strip_parens(mut e: Expr) -> Expr {
    while let Expr::ParenExpr(p) = &e {
        match p.expr() {
            Some(inner) => e = inner,
            None => break,
        }
    }
    e
}

/// The content of a string-like literal (`"a"`, `#"a"`, `&"a"`, `@"a"`), if it is one.
pub fn string_content(lit: &Literal) -> Option<String> {
    let t = lit.token()?;
    let text = t.text();
    let body = match t.kind() {
        SyntaxKind::STRING => text,
        SyntaxKind::HASH_STRING | SyntaxKind::ISTRING | SyntaxKind::PREFIXED_STRING => {
            &text[text.find('"')?..]
        }
        _ => return None,
    };
    let body = body.strip_prefix('"')?;
    Some(body.strip_suffix('"').unwrap_or(body).to_owned())
}

/// Receiver classes: `self`, `level`, `game`, `anim`, `player` (any expression mentioning a
/// player), `other`; `-` when there is none.
pub fn receiver_kind(e: Option<&Expr>) -> &'static str {
    let Some(e) = e else { return "-" };
    let e = strip_parens(e.clone());
    if let Expr::NameRef(n) = &e {
        match n.text().to_ascii_lowercase().as_str() {
            "self" => return "self",
            "level" => return "level",
            "game" => return "game",
            "anim" => return "anim",
            _ => {}
        }
    }
    if e.syntax()
        .text()
        .to_string()
        .to_ascii_lowercase()
        .contains("player")
    {
        return "player";
    }
    "other"
}

fn modifier_kind(m: Option<SyntaxToken>) -> Option<&'static str> {
    match m?.kind() {
        SyntaxKind::THREAD_KW | SyntaxKind::THREADENDON_KW => Some("thread"),
        SyntaxKind::CHILDTHREAD_KW => Some("childthread"),
        _ => None,
    }
}

/// `thread`, `childthread`, `method` (a receiver, no modifier) or `call`.
pub fn call_kind(modifier: Option<SyntaxToken>, receiver: bool) -> &'static str {
    match modifier_kind(modifier) {
        Some(k) => k,
        None if receiver => "method",
        None => "call",
    }
}

/// The event keywords parsed as calls (`self notify( "x" )`): events, not references.
pub fn is_event_keyword(lower: &str) -> bool {
    matches!(lower, "notify" | "waittill" | "waittillmatch" | "endon")
}

fn directive_target(node: &SyntaxNode) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| {
            matches!(
                t.kind(),
                SyntaxKind::PATH | SyntaxKind::STRING | SyntaxKind::IDENT
            )
        })
}

/// The summary of a `/@ ... @/` block or of `///` lines right before a declaration.
pub fn doc_summary(node: &SyntaxNode) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut prev = node.prev_sibling_or_token();
    let mut newlines = 0;
    while let Some(e) = prev {
        let Some(t) = e.as_token() else { break };
        match t.kind() {
            SyntaxKind::DOC_COMMENT => {
                let body = t.text().trim_start_matches("/@").trim_end_matches("@/");
                return block_summary(body);
            }
            SyntaxKind::COMMENT if t.text().starts_with("///") => {
                let rest = &t.text()[3..];
                if rest.starts_with('/') {
                    break;
                }
                lines.push(rest.trim().to_owned());
                newlines = 0;
            }
            SyntaxKind::WHITESPACE => {
                newlines += t.text().matches('\n').count();
                if newlines > 1 {
                    break;
                }
            }
            _ => break,
        }
        prev = t.prev_sibling_or_token();
    }
    lines.reverse();
    squash(&lines.join(" "), 160)
}

fn block_summary(body: &str) -> String {
    let clean = |l: &str| l.trim().trim_matches('"').trim().to_owned();
    for l in body.lines() {
        let c = clean(l);
        if let Some(rest) = c.strip_prefix("Summary:") {
            return squash(rest.trim().trim_matches('"'), 160);
        }
    }
    body.lines()
        .map(clean)
        .find(|l| !l.is_empty())
        .map_or_else(String::new, |l| squash(&l, 160))
}

/// The full doc comment before a declaration (`/@ @/` body or `///` lines), trimmed, for hover.
pub fn doc_text(node: &SyntaxNode) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut prev = node.prev_sibling_or_token();
    let mut newlines = 0;
    while let Some(e) = prev {
        let Some(t) = e.as_token() else { break };
        match t.kind() {
            SyntaxKind::DOC_COMMENT => {
                let body = t.text().trim_start_matches("/@").trim_end_matches("@/");
                // ScriptDoc writes each line as a quoted string.
                let text: Vec<&str> = body
                    .lines()
                    .map(|l| {
                        let l = l.trim();
                        l.strip_prefix('"')
                            .and_then(|x| x.strip_suffix('"'))
                            .unwrap_or(l)
                    })
                    .collect();
                return Some(text.join("\n").trim().to_owned()).filter(|s| !s.is_empty());
            }
            SyntaxKind::COMMENT if t.text().starts_with("///") => {
                lines.push(t.text()[3..].trim().to_owned());
                newlines = 0;
            }
            SyntaxKind::WHITESPACE => {
                newlines += t.text().matches('\n').count();
                if newlines > 1 {
                    break;
                }
            }
            _ => break,
        }
        prev = t.prev_sibling_or_token();
    }
    lines.reverse();
    let s = lines.join("\n");
    (!s.trim().is_empty()).then_some(s)
}

/// The namespace, class and function in effect during a preorder walk. Callers feed it every
/// `Enter` and `Leave` event; it is shared by fact extraction and by the walks of other crates
/// (the index's events and fields), so that both name the enclosing function the same way.
pub struct Scope<'a> {
    pub cfg: &'a Config,
    /// The namespace in effect.
    pub ns: String,
    /// `name` or `class::name` of the enclosing function.
    pub func: Option<String>,
    pub class: Option<String>,
    first_ns: Option<String>,
}

impl<'a> Scope<'a> {
    /// The scope at the top of a file with key `key`.
    pub fn new(cfg: &'a Config, key: &str) -> Scope<'a> {
        let ns = match cfg.resolution {
            Resolution::Namespace => key.rsplit('/').next().unwrap_or(key),
            Resolution::Include => key,
        };
        Scope {
            ns: cfg.ident(ns),
            cfg,
            func: None,
            class: None,
            first_ns: None,
        }
    }

    pub fn ident(&self, s: &str) -> String {
        self.cfg.ident(s)
    }

    pub fn enter(&mut self, node: &SyntaxNode) {
        use SyntaxKind as K;
        match node.kind() {
            K::NAMESPACE_DIRECTIVE => {
                if self.cfg.resolution == Resolution::Namespace {
                    if let Some(n) = NamespaceDirective::cast(node.clone()).and_then(|d| d.name()) {
                        self.ns = self.ident(&n.text());
                    }
                }
                if self.first_ns.is_none() {
                    self.first_ns = Some(self.ns.clone());
                }
            }
            K::FUNCTION_DEF => {
                if let Some(f) = FunctionDef::cast(node.clone()) {
                    let name = f.name().map(|n| self.ident(&n.text())).unwrap_or_default();
                    self.func = Some(match &self.class {
                        Some(c) => format!("{c}::{name}"),
                        None => name,
                    });
                }
            }
            K::CONSTRUCTOR | K::DESTRUCTOR => {
                let which = if node.kind() == K::CONSTRUCTOR {
                    "constructor"
                } else {
                    "destructor"
                };
                let class = self.class.clone().unwrap_or_default();
                self.func = Some(format!("{class}::{which}"));
            }
            K::CLASS_DEF => {
                if let Some(c) = ClassDef::cast(node.clone()) {
                    self.class = Some(c.name().map(|n| self.ident(&n.text())).unwrap_or_default());
                }
            }
            _ => {}
        }
    }

    pub fn leave(&mut self, node: &SyntaxNode) {
        match node.kind() {
            SyntaxKind::FUNCTION_DEF | SyntaxKind::CONSTRUCTOR | SyntaxKind::DESTRUCTOR => {
                self.func = None
            }
            SyntaxKind::CLASS_DEF => self.class = None,
            _ => {}
        }
    }

    /// `ns::fn` of the enclosing function, or `<file>`.
    pub fn func_name(&self) -> String {
        match &self.func {
            Some(f) => format!("{}::{f}", self.ns),
            None => "<file>".to_owned(),
        }
    }

    /// The file's namespace: the first `#namespace`, else the default.
    pub fn file_namespace(&self) -> String {
        self.first_ns.clone().unwrap_or_else(|| self.ns.clone())
    }
}

fn params_text(scope: &Scope, list: Option<ParamList>) -> String {
    let Some(list) = list else {
        return String::new();
    };
    let mut out = Vec::new();
    for p in list.any_params() {
        match p {
            AnyParam::VarargParam(_) => out.push("...".to_owned()),
            AnyParam::Param(p) => {
                let mut s = String::new();
                if p.amp_token().is_some() {
                    s.push('&');
                }
                if p.star_token().is_some() {
                    s.push('*');
                }
                s.push_str(&p.name().map(|n| scope.ident(&n.text())).unwrap_or_default());
                if let Some(d) = p.default_value().and_then(|d| d.expr()) {
                    s.push('=');
                    s.push_str(&code_text(d.syntax(), 24));
                }
                out.push(s);
            }
        }
    }
    out.join(", ")
}

struct Extractor<'a> {
    scope: Scope<'a>,
    facts: FileFacts,
    /// Index of the definition of the enclosing function.
    caller: Option<u32>,
    fields: Vec<String>,
    /// Starts of the event-name arguments seen, to tell them from other strings.
    event_names: std::collections::HashSet<TextSize>,
    strings: Vec<String>,
}

impl Extractor<'_> {
    fn def(
        &mut self,
        kind: DefKind,
        name: String,
        name_range: Option<TextRange>,
        node: &SyntaxNode,
        params: String,
    ) -> u32 {
        let mut flags = Vec::new();
        if let Some(f) = FunctionDef::cast(node.clone()) {
            if f.is_autoexec() {
                flags.push("autoexec".to_owned());
            }
            if f.is_private() {
                flags.push("private".to_owned());
            }
            if f.codecall_kw_token().is_some() {
                flags.push("codecall".to_owned());
            }
            for h in f.event_handler_modifiers() {
                let n = h.name().map(|n| n.text()).unwrap_or_default();
                flags.push(format!("event_handler[{n}]"));
            }
        }
        if in_dev_block(node) {
            flags.push("dev".to_owned());
        }
        self.facts.defs.push(Def {
            kind,
            ns: self.scope.ns.clone(),
            name,
            range: node.text_range(),
            name_range: name_range.unwrap_or(node.text_range()),
            focus: focus_of(node),
            params,
            flags: flags.join(","),
            doc: doc_summary(node),
        });
        (self.facts.defs.len() - 1) as u32
    }

    fn reference(
        &mut self,
        target: Target,
        kind: &'static str,
        receiver: &'static str,
        node: &SyntaxNode,
        range: TextRange,
    ) {
        self.facts.refs.push(Ref {
            caller: self.caller,
            target,
            kind,
            receiver,
            focus: focus_of(node),
            range,
        });
    }

    fn enter(&mut self, node: &SyntaxNode) {
        use SyntaxKind as K;
        self.scope.enter(node);
        match node.kind() {
            K::NAMESPACE_DIRECTIVE => {
                let at = node.text_range().end();
                self.facts.ns_switches.push((at, self.scope.ns.clone()));
            }
            K::USING_DIRECTIVE
            | K::INCLUDE_DIRECTIVE
            | K::INSERT_DIRECTIVE
            | K::INLINE_DIRECTIVE => {
                let directive = match node.kind() {
                    K::USING_DIRECTIVE => "using",
                    K::INCLUDE_DIRECTIVE => "include",
                    K::INSERT_DIRECTIVE => "insert",
                    _ => "inline",
                };
                if let Some(t) = directive_target(node) {
                    self.facts.imports.push(Import {
                        directive,
                        target: normalize_target(t.text(), self.scope.cfg.case_insensitive),
                        focus: focus_of(node),
                        range: t.text_range(),
                    });
                }
            }
            K::FUNCTION_DEF => {
                let Some(f) = FunctionDef::cast(node.clone()) else {
                    return;
                };
                let params = params_text(&self.scope, f.param_list());
                let kind = if self.scope.class.is_some() {
                    DefKind::Method
                } else {
                    DefKind::Function
                };
                let full = self.scope.func.clone().unwrap_or_default();
                let name_range = f.name().map(|n| n.syntax().text_range());
                self.caller = Some(self.def(kind, full, name_range, node, params));
            }
            K::CONSTRUCTOR | K::DESTRUCTOR => {
                let full = self.scope.func.clone().unwrap_or_default();
                let list = node.children().find_map(ParamList::cast);
                let params = params_text(&self.scope, list);
                self.caller = Some(self.def(DefKind::Method, full, None, node, params));
            }
            K::CLASS_DEF => {
                let Some(c) = ClassDef::cast(node.clone()) else {
                    return;
                };
                let name = self.scope.class.clone().unwrap_or_default();
                let bases = c
                    .class_base()
                    .map(|b| {
                        b.name_refs()
                            .map(|n| self.scope.ident(&n.text()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                let name_range = c.name().map(|n| n.syntax().text_range());
                self.def(DefKind::Class, name, name_range, node, bases);
            }
            K::FILE_CONSTANT => {
                if let Some(c) = gsc_syntax::ast::FileConstant::cast(node.clone()) {
                    let name = c.name().map(|n| n.text()).unwrap_or_default();
                    let value = c
                        .expr()
                        .map(|e| code_text(e.syntax(), 60))
                        .unwrap_or_default();
                    let name_range = c.name().map(|n| n.syntax().text_range());
                    let i = self.def(DefKind::Const, name, name_range, node, String::new());
                    self.facts.defs[i as usize].doc = format!("= {value}");
                }
            }
            K::PREPROC_DEFINE => {
                if let Some(d) = PreprocDefine::cast(node.clone()) {
                    let name = d.name().map(|n| n.text()).unwrap_or_default();
                    let params = d
                        .macro_params()
                        .map(|p| {
                            let t = code_text(p.syntax(), 80);
                            t.trim_start_matches('(')
                                .trim_end_matches(')')
                                .trim()
                                .to_owned()
                        })
                        .unwrap_or_default();
                    let body = d
                        .macro_body_token()
                        .map(|t| squash(&t.text().replace("\\\n", " "), 80))
                        .unwrap_or_default();
                    let name_range = d.name().map(|n| n.syntax().text_range());
                    let i = self.def(DefKind::Define, name, name_range, node, params) as usize;
                    let def = &mut self.facts.defs[i];
                    if def.doc.is_empty() && !body.is_empty() {
                        def.doc = format!("= {body}");
                    }
                }
            }
            K::CALL_EXPR => {
                if let Some(c) = CallExpr::cast(node.clone()) {
                    self.call(&c);
                }
            }
            K::POINTER_CALL_EXPR => {
                if let Some(c) = PointerCallExpr::cast(node.clone()) {
                    let receiver = c.receiver();
                    let (target, range) = match c.target() {
                        Some(t) => (code_text(t.syntax(), 80), t.syntax().text_range()),
                        None => (String::new(), node.text_range()),
                    };
                    self.reference(
                        Target::Dynamic(target),
                        call_kind(c.modifier(), receiver.is_some()),
                        receiver_kind(receiver.as_ref()),
                        node,
                        range,
                    );
                }
            }
            K::METHOD_CALL_EXPR => {
                if let Some(c) = MethodCallExpr::cast(node.clone()) {
                    let receiver = c.receiver();
                    let method = c.method();
                    let name = method
                        .as_ref()
                        .map(|n| self.scope.ident(&n.text()))
                        .unwrap_or_default();
                    let range = method.map_or(node.text_range(), |m| m.syntax().text_range());
                    self.reference(
                        Target::Method(name),
                        call_kind(c.modifier(), receiver.is_some()),
                        receiver_kind(c.object().as_ref()),
                        node,
                        range,
                    );
                }
            }
            K::FUNCTION_REF => {
                if let Some(r) = FunctionRef::cast(node.clone()) {
                    let target = match r.qualified_ref() {
                        Some(q) => self.qualified(&q),
                        None => r.name_ref().map(|n| {
                            (
                                Target::Bare(self.scope.ident(&n.text())),
                                n.syntax().text_range(),
                            )
                        }),
                    };
                    if let Some((target, range)) = target {
                        self.reference(target, "pointer", "-", node, range);
                    }
                }
            }
            K::QUALIFIED_REF => {
                // `::foo` or `ns::foo` used as a value (colon-style function pointers).
                let parent = node.parent();
                let is_callee = parent.as_ref().is_some_and(|p| {
                    p.kind() == K::FUNCTION_REF
                        || CallExpr::cast(p.clone())
                            .and_then(|c| c.callee())
                            .is_some_and(|c| c.syntax() == node)
                });
                if !is_callee {
                    if let Some(q) = QualifiedRef::cast(node.clone()) {
                        if let Some((target, range)) = self.qualified(&q) {
                            self.reference(target, "pointer", "-", node, range);
                        }
                    }
                }
            }
            K::FIELD_EXPR => {
                if let Some(n) = FieldExpr::cast(node.clone()).and_then(|f| f.field()) {
                    self.fields.push(self.scope.ident(&n.text()));
                }
            }
            K::LITERAL => {
                if self.event_names.contains(&node.text_range().start()) {
                    return;
                }
                if let Some(s) = Literal::cast(node.clone()).and_then(|l| string_content(&l)) {
                    if crate::events::event_like_string(&s) {
                        self.strings.push(s.to_ascii_lowercase());
                    }
                }
            }
            _ => {}
        }
    }

    fn event_sites(&mut self, c: &CallExpr, op: &str, class: crate::events::EventClass) {
        let before = self.facts.events.len();
        let cfg = self.scope.cfg;
        crate::events::call_sites(c, op, class, self.caller, cfg, &mut self.facts.events);
        for s in &self.facts.events[before..] {
            self.event_names.insert(s.name_range.start());
        }
    }

    fn leave(&mut self, node: &SyntaxNode) {
        self.scope.leave(node);
        if matches!(
            node.kind(),
            SyntaxKind::FUNCTION_DEF | SyntaxKind::CONSTRUCTOR | SyntaxKind::DESTRUCTOR
        ) {
            self.caller = None;
        }
    }

    fn qualified(&self, q: &QualifiedRef) -> Option<(Target, TextRange)> {
        let name_ref = q.name_ref()?;
        let name = self.scope.ident(&name_ref.text());
        let qual = q
            .qualifier()
            .map(|x| {
                normalize_target(
                    &x.syntax().text().to_string(),
                    self.scope.cfg.case_insensitive,
                )
            })
            .filter(|s| !s.is_empty());
        Some((
            Target::Qualified(qual, name),
            name_ref.syntax().text_range(),
        ))
    }

    fn call(&mut self, c: &CallExpr) {
        let Some(callee) = c.callee() else { return };
        let receiver = c.receiver();
        let (target, range) = match &callee {
            Expr::NameRef(n) => {
                let tok_kind = n.ident_token().map(|t| t.kind());
                let name = self.scope.ident(&n.text());
                let lower = name.to_ascii_lowercase();
                if is_event_keyword(&lower) {
                    let class = match lower.as_str() {
                        "notify" => crate::events::EventClass::Emit,
                        "endon" => crate::events::EventClass::Endon,
                        _ => crate::events::EventClass::Wait,
                    };
                    self.event_sites(c, &lower, class);
                    return;
                }
                if let Some(class) = crate::events::wrapper_class(&lower) {
                    self.event_sites(c, &lower, class);
                }
                let keyword = n
                    .syntax()
                    .children_with_tokens()
                    .filter_map(|e| e.into_token())
                    .any(|t| t.kind().is_name_like_keyword());
                let range = n.syntax().text_range();
                if keyword || tok_kind.is_some_and(|k| k.is_keyword()) {
                    (Target::Keyword(name), range)
                } else {
                    (Target::Bare(name), range)
                }
            }
            Expr::QualifiedRef(q) => match self.qualified(q) {
                Some(t) => {
                    if let Target::Qualified(_, name) = &t.0 {
                        let lower = name.to_ascii_lowercase();
                        if let Some(class) = crate::events::wrapper_class(&lower) {
                            self.event_sites(c, &lower, class);
                        }
                    }
                    t
                }
                None => return,
            },
            other => (
                Target::Dynamic(code_text(other.syntax(), 80)),
                other.syntax().text_range(),
            ),
        };
        self.reference(
            target,
            call_kind(c.modifier(), receiver.is_some()),
            receiver_kind(receiver.as_ref()),
            c.syntax(),
            range,
        );
    }
}

/// Extract the facts of a parsed file whose key is `key`.
pub fn extract(root: &SyntaxNode, key: &str, cfg: &Config) -> FileFacts {
    let scope = Scope::new(cfg, key);
    let facts = FileFacts {
        ns_switches: vec![(TextSize::from(0), scope.ns.clone())],
        ..FileFacts::default()
    };
    let mut x = Extractor {
        scope,
        facts,
        caller: None,
        fields: Vec::new(),
        event_names: Default::default(),
        strings: Vec::new(),
    };
    for ev in root.preorder() {
        match ev {
            WalkEvent::Enter(n) => x.enter(&n),
            WalkEvent::Leave(n) => x.leave(&n),
        }
    }
    let mut facts = x.facts;
    facts.namespace = x.scope.file_namespace();
    x.fields.sort();
    x.fields.dedup();
    facts.fields = x.fields;
    x.strings.sort();
    x.strings.dedup();
    facts.strings = x.strings;
    facts
}

/// The node of a definition in its file's tree.
pub fn def_node(root: &SyntaxNode, def: &Def) -> Option<SyntaxNode> {
    let el = root.covering_element(def.range);
    let start = match el {
        gsc_syntax::NodeOrToken::Node(n) => n,
        gsc_syntax::NodeOrToken::Token(t) => t.parent()?,
    };
    start.ancestors().find(|n| n.text_range() == def.range)
}
