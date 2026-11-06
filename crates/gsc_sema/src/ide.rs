//! Editor queries in editor terms (offsets, ranges, symbols), without LSP types: the LSP and MCP
//! servers wrap these. Offsets are byte offsets into the file's decoded text.

use std::collections::{BTreeSet, HashMap, HashSet};

use gsc_syntax::ast::{self, AstNode};
use gsc_syntax::{SyntaxKind, SyntaxNode, SyntaxToken, TextRange, TextSize};

use crate::data::Kind;
use crate::facts::{def_node, doc_text, DefKind, Target};
use crate::par::par_map;
use crate::profile::{Config, EventArgs, Resolution};
use crate::resolve::How;
use crate::vfs::{vm_compatible, Vm};
use crate::workspace::{DefId, FileId, Workspace};

/// What a symbol is, for icons and filtering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Macro,
    Const,
    Local,
    Param,
    Builtin,
    Field,
    File,
    Namespace,
    /// An event name (a `notify`, `waittill` or `endon` string).
    Event,
}

impl From<DefKind> for SymbolKind {
    fn from(k: DefKind) -> SymbolKind {
        match k {
            DefKind::Function => SymbolKind::Function,
            DefKind::Method => SymbolKind::Method,
            DefKind::Class => SymbolKind::Class,
            DefKind::Define => SymbolKind::Macro,
            DefKind::Const => SymbolKind::Const,
        }
    }
}

/// A place to navigate to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavTarget {
    pub file: FileId,
    /// The whole declaration.
    pub full_range: TextRange,
    /// Its name.
    pub focus_range: TextRange,
    pub name: String,
    pub kind: SymbolKind,
    /// The namespace of a definition.
    pub container: Option<String>,
}

/// A symbol whose references can be searched.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Symbol {
    Def(DefId),
    /// A parameter or local variable: its name (identifier case) in the function at `scope`.
    Local {
        file: FileId,
        scope: TextRange,
        name: String,
        decl: TextRange,
        param: bool,
    },
    /// A builtin, by lower-case name.
    Builtin(String),
    /// A field name (identifier case), on any object.
    Field(String),
    /// An event name (lower case): every notify, waittill and endon site of it.
    Event(String),
    File(FileId),
    /// A class member (`var x;`), named bare inside the class's methods and those of its
    /// subclasses: the class that declares it and the name (identifier case).
    Member {
        class: String,
        name: String,
    },
}

/// A reference found by [`Workspace::references`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileRange {
    pub file: FileId,
    pub range: TextRange,
}

impl FileRange {
    fn key(&self) -> (FileId, TextSize, TextSize) {
        (self.file, self.range.start(), self.range.end())
    }
}

impl PartialOrd for FileRange {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FileRange {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.key().cmp(&other.key())
    }
}

/// Hover content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hover {
    /// What the hover is about.
    pub range: TextRange,
    /// One line of code: `function ns::f( a, b )`, `builtin function spawn`...
    pub signature: String,
    /// The doc comment, as written.
    pub doc: Option<String>,
    /// Extra facts, one per line (arity, VM, where it is defined).
    pub detail: Vec<String>,
}

impl Hover {
    /// The hover as Markdown: the signature in a `gsc` block, then the details and the doc.
    pub fn markdown(&self) -> String {
        let mut s = format!("```gsc\n{}\n```", self.signature);
        if !self.detail.is_empty() {
            s.push_str("\n\n");
            s.push_str(&self.detail.join("  \n"));
        }
        if let Some(d) = &self.doc {
            s.push_str("\n\n---\n\n");
            s.push_str(d);
        }
        s
    }
}

/// An outline entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentSymbol {
    pub name: String,
    /// Parameters, bases or macro body.
    pub detail: String,
    pub kind: SymbolKind,
    pub range: TextRange,
    pub focus_range: TextRange,
    pub children: Vec<DocumentSymbol>,
}

/// A completion proposal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    pub kind: SymbolKind,
    /// Signature, arity or origin.
    pub detail: Option<String>,
}

/// A caller of a function, with the call sites inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncomingCall {
    /// `None` for calls outside any function.
    pub from: Option<DefId>,
    pub file: FileId,
    pub ranges: Vec<TextRange>,
}

/// What a function calls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Callee {
    Def(DefId),
    Builtin(String),
    /// Not resolved, or dynamic: the text as written.
    Other(String),
}

/// A callee of a function, with the call sites.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutgoingCall {
    pub to: Callee,
    pub ranges: Vec<TextRange>,
}

/// What the cursor is on.
pub(crate) enum Hit {
    Ref(usize),
    Def(u32),
    Import(usize),
    Local(Symbol),
    Field(String, TextRange),
    Macro(DefId, TextRange),
    /// A class named by `new C()` or a base list.
    Class(DefId, TextRange),
    /// A class member: declaring class, name, the range at the cursor.
    Member(String, String, TextRange),
}

fn is_name_token(t: &SyntaxToken) -> bool {
    matches!(
        t.kind(),
        SyntaxKind::IDENT | SyntaxKind::PATH | SyntaxKind::STRING
    ) || t.kind().is_name_like_keyword()
}

/// The name-like token at `offset`, preferring the one starting there.
pub(crate) fn token_at(root: &SyntaxNode, offset: TextSize) -> Option<SyntaxToken> {
    let at = root.token_at_offset(offset);
    let (l, r) = (at.clone().left_biased(), at.right_biased());
    r.clone()
        .filter(is_name_token)
        .or(l.filter(is_name_token))
        .or(r)
}

pub(crate) fn enclosing_function(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.ancestors().find(|a| {
        matches!(
            a.kind(),
            SyntaxKind::FUNCTION_DEF | SyntaxKind::CONSTRUCTOR | SyntaxKind::DESTRUCTOR
        )
    })
}

/// A local of a function: identifier-case name, declaration range, parameter or not.
type LocalDecl = (String, TextRange, bool);

/// Parameters and local variables of a function, first declaration of each name.
fn locals(func: &SyntaxNode, cfg: &Config) -> Vec<LocalDecl> {
    let ident = |s: &str| cfg.ident(s);
    let mut out: Vec<LocalDecl> = Vec::new();
    let mut seen = HashSet::new();
    let mut add = |name: String, range: TextRange, param: bool| {
        if seen.insert(name.clone()) {
            out.push((name, range, param));
        }
    };
    for n in func.descendants() {
        match n.kind() {
            SyntaxKind::PARAM => {
                if let Some(name) = ast::Param::cast(n).and_then(|p| p.name()) {
                    add(ident(&name.text()), name.syntax().text_range(), true);
                }
            }
            SyntaxKind::FOREACH_BINDINGS => {
                if let Some(b) = ast::ForeachBindings::cast(n) {
                    for name in b.names() {
                        add(ident(&name.text()), name.syntax().text_range(), false);
                    }
                }
            }
            SyntaxKind::CONST_STMT => {
                if let Some(name) = ast::ConstStmt::cast(n).and_then(|c| c.name()) {
                    add(ident(&name.text()), name.syntax().text_range(), false);
                }
            }
            SyntaxKind::ASSIGN_EXPR => {
                if let Some(ast::Expr::NameRef(r)) = n.first_child().and_then(ast::Expr::cast) {
                    if r.ident_token()
                        .is_some_and(|t| t.kind() == SyntaxKind::IDENT)
                    {
                        add(ident(&r.text()), r.syntax().text_range(), false);
                    }
                }
            }
            // `self waittill( "e", a, b )` binds `a` and `b`; BO4 and later bind nothing (every
            // argument is an event).
            SyntaxKind::CALL_EXPR if cfg.waittill_args != EventArgs::Events => {
                let Some(call) = ast::CallExpr::cast(n) else {
                    continue;
                };
                let is_waittill = matches!(call.callee(), Some(ast::Expr::NameRef(r))
                    if r.text().eq_ignore_ascii_case("waittill"));
                if !is_waittill {
                    continue;
                }
                for e in call.arg_list().into_iter().flat_map(|a| a.exprs()).skip(1) {
                    if let ast::Expr::NameRef(r) = e {
                        add(ident(&r.text()), r.syntax().text_range(), false);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Case-insensitive subsequence match; `None` when `query` does not match `name`. Lower is better.
fn fuzzy_rank(query: &str, name: &str) -> Option<u8> {
    if query.is_empty() {
        return Some(3);
    }
    let (q, n) = (query.to_ascii_lowercase(), name.to_ascii_lowercase());
    if n == q {
        return Some(0);
    }
    if n.starts_with(&q) {
        return Some(1);
    }
    if n.contains(&q) {
        return Some(2);
    }
    let mut it = n.chars();
    q.chars().all(|c| it.any(|x| x == c)).then_some(3)
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// What precedes the identifier being typed.
enum CompletionContext {
    /// After `.`.
    Field,
    /// After `q::` (`None` for a bare `::`).
    Qualified(Option<String>),
    /// After `->`.
    Method,
    General,
}

fn completion_context(text: &str, offset: usize) -> (String, CompletionContext) {
    let b = text.as_bytes();
    let mut start = offset.min(b.len());
    while start > 0 && is_ident_byte(b[start - 1]) {
        start -= 1;
    }
    let prefix = text[start..offset.min(b.len())].to_owned();
    let mut i = start;
    while i > 0 && (b[i - 1] == b' ' || b[i - 1] == b'\t') {
        i -= 1;
    }
    let before = &text[..i];
    let ctx = if before.ends_with("::") {
        let mut j = i - 2;
        while j > 0 && (b[j - 1] == b' ' || b[j - 1] == b'\t') {
            j -= 1;
        }
        let end = j;
        while j > 0 && (is_ident_byte(b[j - 1]) || b[j - 1] == b'\\' || b[j - 1] == b'/') {
            j -= 1;
        }
        let q = text[j..end].to_owned();
        CompletionContext::Qualified((!q.is_empty()).then_some(q))
    } else if before.ends_with("->") {
        CompletionContext::Method
    } else if before.ends_with('.') && !before.ends_with("..") {
        let digit_before = before.len() >= 2 && b[before.len() - 2].is_ascii_digit();
        if digit_before {
            CompletionContext::General
        } else {
            CompletionContext::Field
        }
    } else {
        CompletionContext::General
    };
    (prefix, ctx)
}

impl Workspace {
    fn nav(&self, d: DefId) -> NavTarget {
        let def = self.def(d);
        NavTarget {
            file: d.file,
            full_range: def.range,
            focus_range: def.name_range,
            name: def.name.clone(),
            kind: def.kind.into(),
            container: Some(def.ns.clone()),
        }
    }

    pub(crate) fn hit(&self, file: FileId, offset: TextSize) -> Option<Hit> {
        let f = self.file(file);
        let facts = &f.facts;
        let root = f.syntax();
        let tok = token_at(&root, offset)?;
        let tr = tok.text_range();
        let smallest = facts
            .refs
            .iter()
            .enumerate()
            .filter(|(_, r)| r.range.contains_range(tr) && !matches!(r.target, Target::Dynamic(_)))
            .min_by_key(|(_, r)| r.range.len());
        if let Some((i, _)) = smallest {
            return Some(Hit::Ref(i));
        }
        if let Some(i) = facts
            .defs
            .iter()
            .position(|d| d.name_range != d.range && d.name_range.contains_range(tr))
        {
            return Some(Hit::Def(i as u32));
        }
        if let Some(i) = facts
            .imports
            .iter()
            .position(|i| i.range.contains_range(tr))
        {
            return Some(Hit::Import(i));
        }
        if tok.kind() != SyntaxKind::IDENT {
            return None;
        }
        let cfg = self.profile_of(file).config;
        let name = cfg.ident(tok.text());
        let parent = tok.parent()?;
        if let Some(fe) = parent.parent().and_then(ast::FieldExpr::cast) {
            if fe.field().is_some_and(|n| n.syntax() == &parent) {
                return Some(Hit::Field(name, tr));
            }
        }
        if let Some(h) = self.class_hit(file, &parent, &name, tr) {
            return Some(h);
        }
        if let Some(func) = enclosing_function(&parent) {
            let found = locals(&func, &cfg).into_iter().find(|(n, _, _)| *n == name);
            if let Some((name, decl, param)) = found {
                return Some(Hit::Local(Symbol::Local {
                    file,
                    scope: func.text_range(),
                    name,
                    decl,
                    param,
                }));
            }
        }
        let m = self.macros_spelled(tok.text()).next().or_else(|| {
            self.tables
                .macros
                .get(&name.to_ascii_lowercase())?
                .first()
                .copied()
        })?;
        Some(Hit::Macro(m, tr))
    }

    /// The symbol at `offset`, for [`Workspace::references`].
    pub fn symbol_at(&self, file: FileId, offset: TextSize) -> Option<Symbol> {
        if let Some(s) = self.event_symbol_at(file, offset) {
            return Some(s);
        }
        match self.hit(file, offset)? {
            Hit::Ref(i) => {
                let r = &self.resolution(file).refs[i];
                match (r.def, r.how) {
                    (Some(d), _) => Some(Symbol::Def(d)),
                    (None, How::Builtin | How::BuiltinOtherVm) => {
                        Some(Symbol::Builtin(r.callee.to_ascii_lowercase()))
                    }
                    _ => None,
                }
            }
            Hit::Def(i) => Some(Symbol::Def(DefId { file, index: i })),
            Hit::Import(i) => self.resolution(file).imports[i].map(Symbol::File),
            Hit::Local(s) => Some(s),
            Hit::Field(n, _) => Some(Symbol::Field(n)),
            Hit::Macro(d, _) | Hit::Class(d, _) => Some(Symbol::Def(d)),
            Hit::Member(class, name, _) => Some(Symbol::Member { class, name }),
        }
    }

    /// Where the name at `offset` is defined. Builtins and fields have no location.
    pub fn definition(&self, file: FileId, offset: TextSize) -> Vec<NavTarget> {
        match self.symbol_at(file, offset) {
            Some(Symbol::Event(name)) => self.event_definition(file, offset, &name),
            Some(Symbol::Def(d)) => vec![self.nav(d)],
            Some(Symbol::Local {
                file,
                name,
                decl,
                param,
                ..
            }) => vec![NavTarget {
                file,
                full_range: decl,
                focus_range: decl,
                name,
                kind: if param {
                    SymbolKind::Param
                } else {
                    SymbolKind::Local
                },
                container: None,
            }],
            Some(Symbol::File(f)) => {
                let empty = TextRange::empty(TextSize::from(0));
                vec![NavTarget {
                    file: f,
                    full_range: empty,
                    focus_range: empty,
                    name: self.file(f).meta.rel.clone(),
                    kind: SymbolKind::File,
                    container: None,
                }]
            }
            Some(Symbol::Member { class, name }) => {
                self.member_decl(&class, &name).into_iter().collect()
            }
            Some(Symbol::Field(name)) => self.field_writes(&name, false),
            _ => Vec::new(),
        }
    }

    /// Every use of `symbol` in the workspace (and its declaration when `include_declaration`),
    /// sorted by file and offset.
    pub fn references(&self, symbol: &Symbol, include_declaration: bool) -> Vec<FileRange> {
        let files: Vec<FileId> = self.files().collect();
        self.references_in(symbol, include_declaration, &files)
    }

    /// [`Workspace::references`] in `files` only (the declaration wherever it is).
    pub fn references_in(
        &self,
        symbol: &Symbol,
        include_declaration: bool,
        files: &[FileId],
    ) -> Vec<FileRange> {
        let in_files = |f: FileId| files.contains(&f);
        let mut out: Vec<FileRange> = match symbol {
            Symbol::Def(d) => {
                let def = self.def(*d);
                let macro_name = (def.kind == DefKind::Define).then(|| def.name.clone());
                let mut v: Vec<FileRange> = par_map(files, self.jobs(), |&f| {
                    let res = self.resolution(f);
                    let facts = &self.file(f).facts;
                    res.refs
                        .iter()
                        .zip(&facts.refs)
                        .filter(|(r, _)| match &macro_name {
                            Some(m) => r.how == How::Macro && r.callee == *m,
                            None => r.def == Some(*d),
                        })
                        .map(|(_, x)| FileRange {
                            file: f,
                            range: x.range,
                        })
                        .collect::<Vec<_>>()
                })
                .into_iter()
                .flatten()
                .collect();
                if def.kind == DefKind::Class {
                    v.extend(self.class_uses(&def.name, files));
                }
                if include_declaration && in_files(d.file) {
                    v.push(FileRange {
                        file: d.file,
                        range: def.name_range,
                    });
                }
                v
            }
            Symbol::Event(name) => {
                let mut v = self.event_references(name);
                if files.len() < self.len() {
                    v.retain(|r| in_files(r.file));
                }
                v
            }
            Symbol::Member { class, name } => {
                self.member_references(class, name, include_declaration, files)
            }
            Symbol::Builtin(name) => par_map(files, self.jobs(), |&f| {
                let res = self.resolution(f);
                let facts = &self.file(f).facts;
                res.refs
                    .iter()
                    .zip(&facts.refs)
                    .filter(|(r, _)| {
                        matches!(r.how, How::Builtin | How::BuiltinOtherVm)
                            && r.callee.eq_ignore_ascii_case(name)
                    })
                    .map(|(_, x)| FileRange {
                        file: f,
                        range: x.range,
                    })
                    .collect::<Vec<_>>()
            })
            .into_iter()
            .flatten()
            .collect(),
            Symbol::Local {
                file,
                scope,
                name,
                decl,
                ..
            } => {
                if !in_files(*file) {
                    return Vec::new();
                }
                let f = self.file(*file);
                let cfg = self.profile_of(*file).config;
                let root = f.syntax();
                let mut v: Vec<FileRange> = root
                    .descendants_with_tokens()
                    .filter_map(|e| e.into_token())
                    .filter(|t| {
                        t.kind() == SyntaxKind::IDENT && scope.contains_range(t.text_range())
                    })
                    .filter(|t| cfg.ident(t.text()) == *name)
                    .filter(|t| {
                        // Not a field name, a callee, or the name of another declaration.
                        let p = t.parent();
                        let field = p.as_ref().and_then(|p| {
                            let fe = ast::FieldExpr::cast(p.parent()?)?;
                            Some(fe.field()?.syntax() == p)
                        });
                        let callee = f.facts.refs.iter().any(|r| r.range == t.text_range());
                        field != Some(true) && !callee
                    })
                    .map(|t| FileRange {
                        file: *file,
                        range: t.text_range(),
                    })
                    .collect();
                if !include_declaration {
                    v.retain(|r| r.range != *decl);
                }
                v
            }
            Symbol::Field(name) => par_map(files, self.jobs(), |&f| {
                let data = self.file(f);
                if !data.facts.fields.contains(name) {
                    return Vec::new();
                }
                let cfg = self.profile_of(f).config;
                data.syntax()
                    .descendants()
                    .filter_map(ast::FieldExpr::cast)
                    .filter_map(|fe| fe.field())
                    .filter(|n| cfg.ident(&n.text()) == *name)
                    .map(|n| FileRange {
                        file: f,
                        range: n.syntax().text_range(),
                    })
                    .collect()
            })
            .into_iter()
            .flatten()
            .collect(),
            Symbol::File(target) => par_map(files, self.jobs(), |&f| {
                let res = self.resolution(f);
                let facts = &self.file(f).facts;
                res.imports
                    .iter()
                    .zip(&facts.imports)
                    .filter(|(x, _)| **x == Some(*target))
                    .map(|(_, i)| FileRange {
                        file: f,
                        range: i.range,
                    })
                    .collect::<Vec<_>>()
            })
            .into_iter()
            .flatten()
            .collect(),
        };
        out.sort();
        out.dedup();
        out
    }

    fn def_hover(&self, d: DefId, range: TextRange) -> Hover {
        let def = self.def(d);
        let f = self.file(d.file);
        let node = def_node(&f.syntax(), def);
        let flags: Vec<&str> = def
            .flags
            .split(',')
            .filter(|x| matches!(*x, "autoexec" | "private"))
            .collect();
        let prefix = if flags.is_empty() {
            String::new()
        } else {
            format!("{} ", flags.join(" "))
        };
        let shown = |q: String| self.display_qualified(d.file, &q);
        let signature = match def.kind {
            DefKind::Function | DefKind::Method => {
                format!(
                    "{prefix}function {}({})",
                    shown(def.qualified()),
                    pad(&def.params)
                )
            }
            DefKind::Class if def.params.is_empty() => format!("class {}", shown(def.qualified())),
            DefKind::Class => format!("class {} : {}", shown(def.qualified()), def.params),
            DefKind::Define if def.params.is_empty() => format!("#define {}", def.name),
            DefKind::Define => format!("#define {}({})", def.name, def.params),
            DefKind::Const => format!("const {} {}", def.name, def.doc),
        };
        let mut detail = Vec::new();
        if def.kind == DefKind::Define && def.doc.starts_with("= ") {
            detail.push(format!("`{}`", def.doc));
        }
        let line = crate::facts::LineStarts::new(f.text()).line(def.focus);
        detail.push(format!("defined in `{}:{line}`", f.meta.rel));
        if matches!(def.kind, DefKind::Function | DefKind::Method) {
            let short = def.short_name();
            if let Some(h) = self.hash_line(d.file, gsc_hash::Role::Function, short) {
                let named = self.recover(d.file, gsc_hash::Role::Function, short);
                detail.push(match named {
                    Some((_, src)) => format!("{h}, name from the {}", src.label()),
                    None => h,
                });
            }
        }
        Hover {
            range,
            signature,
            doc: node.as_ref().and_then(doc_text),
            detail,
        }
    }

    fn builtin_hover(&self, file: FileId, name: &str, range: TextRange) -> Option<Hover> {
        let cat = self.profile_of(file).builtins.as_ref()?;
        let lower = name.to_ascii_lowercase();
        let entries = cat.lookup(&lower);
        let intrinsic = cat.intrinsic(&lower);
        if entries.is_empty() && intrinsic.is_none() {
            return None;
        }
        let kinds: BTreeSet<&str> = entries.iter().map(|b| b.kind.label()).collect();
        let kind = if kinds.is_empty() {
            "function".to_owned()
        } else {
            kinds.into_iter().collect::<Vec<_>>().join("/")
        };
        let mut detail = Vec::new();
        for b in entries {
            let mut line = format!("{} {}", b.vm.label(), b.kind.label());
            match b.describe_arity() {
                Some(a) => line.push_str(&format!(": takes {a}")),
                None => line.push_str(": arity not recorded"),
            }
            if b.dev_only {
                line.push_str(" (dev only)");
            }
            detail.push(line);
        }
        if let Some(i) = intrinsic {
            match i.describe_arity() {
                Some(a) => detail.push(format!("compiled to an opcode: takes {a}")),
                None => detail.push("compiled to an opcode".to_owned()),
            }
        }
        if self.is_builtin(file, &lower) {
            detail.push(format!("engine builtin ({})", cat.game));
        } else {
            let vm = self.file(file).meta.vm.unwrap_or(Vm::Gsc);
            detail.push(format!("not available in {} scripts", vm.label()));
        }
        Some(Hover {
            range,
            signature: format!(
                "builtin {kind} {}",
                self.display_name(file, gsc_hash::Role::Function, &lower)
            ),
            doc: None,
            detail,
        })
    }

    /// Signature, doc comment and facts of the name at `offset`.
    pub fn hover(&self, file: FileId, offset: TextSize) -> Option<Hover> {
        if let Some(h) = self.event_hover(file, offset) {
            return Some(h);
        }
        match self.hit(file, offset)? {
            Hit::Ref(i) => {
                let x = &self.file(file).facts.refs[i];
                let r = &self.resolution(file).refs[i];
                match (r.def, r.how) {
                    (Some(d), _) => Some(self.use_hover(file, d, x.range)),
                    (None, How::Builtin | How::BuiltinOtherVm) => {
                        self.builtin_hover(file, &r.callee, x.range).or(Some(Hover {
                            range: x.range,
                            signature: format!(
                                "builtin {}",
                                self.display_qualified(file, &r.callee)
                            ),
                            doc: None,
                            detail: Vec::new(),
                        }))
                    }
                    (None, how) => Some(Hover {
                        range: x.range,
                        signature: self.display_qualified(file, &r.callee),
                        doc: None,
                        detail: vec![how.label().to_owned()],
                    }),
                }
            }
            Hit::Def(i) => {
                let d = DefId { file, index: i };
                Some(self.def_hover(d, self.def(d).name_range))
            }
            Hit::Import(i) => {
                let imp = &self.file(file).facts.imports[i];
                let found = self.resolution(file).imports[i];
                Some(Hover {
                    range: imp.range,
                    signature: format!("#{} {}", imp.directive, imp.target),
                    doc: None,
                    detail: vec![match found {
                        Some(f) => {
                            let m = &self.file(f).meta;
                            format!("`{}` (root `{}`)", m.rel, self.root(m.root).label)
                        }
                        None => "not found in the workspace".to_owned(),
                    }],
                })
            }
            Hit::Local(Symbol::Local {
                name, decl, param, ..
            }) => Some(Hover {
                range: decl,
                signature: format!("{} {name}", if param { "param" } else { "local" }),
                doc: None,
                detail: Vec::new(),
            }),
            Hit::Local(_) => None,
            Hit::Field(name, range) => {
                let engine = self
                    .profile_of(file)
                    .fields
                    .as_ref()
                    .is_some_and(|c| c.contains(&name));
                Some(Hover {
                    range,
                    signature: format!("field {name}"),
                    doc: None,
                    detail: vec![if engine {
                        "engine field".to_owned()
                    } else {
                        "script field".to_owned()
                    }],
                })
            }
            Hit::Macro(d, range) => Some(self.use_hover(file, d, range)),
            Hit::Class(d, range) => Some(self.def_hover(d, range)),
            Hit::Member(class, name, range) => {
                let mut detail = vec![format!("member of class `{class}`")];
                if let Some(n) = self.member_decl(&class, &name) {
                    let f = self.file(n.file);
                    let line = crate::facts::LineStarts::new(f.text()).line(n.focus_range.start());
                    detail.push(format!("declared in `{}:{line}`", f.meta.rel));
                }
                Some(Hover {
                    range,
                    signature: format!("var {class}::{name}"),
                    doc: None,
                    detail,
                })
            }
        }
    }

    /// The hover of a definition at one of its uses: a macro also shows what it expands to here.
    fn use_hover(&self, file: FileId, d: DefId, range: TextRange) -> Hover {
        let mut h = self.def_hover(d, range);
        if self.def(d).kind == DefKind::Define {
            if let Some(x) = self.macro_expansion_at(file, range.start()) {
                let code = crate::nav::render_tokens(&x.tokens, 240);
                h.detail.push(format!("expands to:\n```gsc\n{code}\n```"));
            }
        }
        h
    }

    /// The outline of a file: definitions in order, methods under their class.
    pub fn document_symbols(&self, file: FileId) -> Vec<DocumentSymbol> {
        let defs = &self.file(file).facts.defs;
        let mut out: Vec<DocumentSymbol> = Vec::new();
        for d in defs {
            let sym = DocumentSymbol {
                name: self.display_name(file, gsc_hash::Role::Function, d.short_name()),
                detail: d.params.clone(),
                kind: d.kind.into(),
                range: d.range,
                focus_range: d.name_range,
                children: Vec::new(),
            };
            match out.last_mut() {
                Some(class)
                    if class.kind == SymbolKind::Class && class.range.contains_range(d.range) =>
                {
                    class.children.push(sym)
                }
                _ => out.push(sym),
            }
        }
        out
    }

    /// Definitions whose name matches `query` (case-insensitive: exact, prefix, substring, then
    /// subsequence), best first, at most `limit`. A query with `::` matches `ns::name`.
    pub fn workspace_symbols(&self, query: &str, limit: usize) -> Vec<NavTarget> {
        let qualified = query.contains("::");
        let mut hits: Vec<(u8, usize, DefId)> = self
            .all_defs()
            .filter_map(|d| {
                let def = self.def(d);
                let name = if qualified {
                    def.qualified()
                } else {
                    def.short_name().to_owned()
                };
                fuzzy_rank(query, &name).map(|rank| (rank, name.len(), d))
            })
            .collect();
        hits.sort();
        hits.truncate(limit);
        hits.into_iter().map(|(_, _, d)| self.nav(d)).collect()
    }

    /// Completions at `offset`: fields after `.`, the functions of a namespace (or file) after
    /// `ns::`, methods after `->`; otherwise locals, parameters, the functions in scope, macros and
    /// the builtins of the file's VM. Items match the identifier typed so far (fuzzy).
    pub fn completions(&self, file: FileId, offset: TextSize) -> Vec<CompletionItem> {
        let f = self.file(file);
        let (prefix, ctx) = completion_context(f.text(), usize::from(offset));
        let mut items: Vec<CompletionItem> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let cfg = self.profile_of(file).config;
        let mut push = |label: String, kind: SymbolKind, detail: Option<String>| {
            if fuzzy_rank(&prefix, &label).is_some() && seen.insert(cfg.ident(&label)) {
                items.push(CompletionItem {
                    label,
                    kind,
                    detail,
                });
            }
        };
        let def_item = |d: DefId| {
            let def = self.def(d);
            let detail = match def.kind {
                DefKind::Function | DefKind::Method => format!(
                    "{}({})",
                    self.display_qualified(d.file, &def.qualified()),
                    pad(&def.params)
                ),
                _ => def.doc.clone(),
            };
            // A hashed name the packages know completes as the name: the compiler hashes it
            // back to the same value.
            let label = match self.recover(d.file, gsc_hash::Role::Function, def.short_name()) {
                Some((n, _)) => n,
                None => def.short_name().to_owned(),
            };
            (label, def.kind.into(), Some(detail))
        };
        let vm = f.meta.vm;
        let profile = self.profile_of(file);
        match ctx {
            CompletionContext::Field => {
                let mut names: BTreeSet<String> = BTreeSet::new();
                if let Some(c) = &profile.fields {
                    names.extend(c.names.iter().map(|n| cfg.ident(n)));
                }
                for g in self.files() {
                    names.extend(self.file(g).facts.fields.iter().cloned());
                }
                for n in names {
                    push(n, SymbolKind::Field, None);
                }
            }
            CompletionContext::Method => {
                let mut names: Vec<&String> = self.tables.methods.keys().collect();
                names.sort();
                for n in names {
                    let d = self.tables.methods[n][0];
                    let (l, k, det) = def_item(d);
                    push(l, k, det);
                }
            }
            CompletionContext::Qualified(q) => {
                let q = q.map(|q| crate::vfs::normalize_target(&q, cfg.case_insensitive));
                match q {
                    None => {
                        for (i, d) in f.facts.defs.iter().enumerate() {
                            if d.kind == DefKind::Function {
                                let (l, k, det) = def_item(DefId {
                                    file,
                                    index: i as u32,
                                });
                                push(l, k, det);
                            }
                        }
                    }
                    Some(q) if q == "sys" => {
                        Self::builtin_items(profile.builtins.as_ref(), vm, &mut push)
                    }
                    Some(q) if cfg.resolution == Resolution::Namespace && !q.contains('/') => {
                        let scheme = profile.scheme;
                        let qk = scheme.and_then(|s| s.key(gsc_hash::Role::Namespace, &q));
                        let same_ns = |ns: &str| {
                            ns == q
                                || (qk.is_some()
                                    && scheme.and_then(|s| s.key(gsc_hash::Role::Namespace, ns))
                                        == qk)
                        };
                        let mut defs: Vec<DefId> = self
                            .tables
                            .ns_fns
                            .iter()
                            .filter(|((ns, _), _)| same_ns(ns))
                            .filter_map(|(_, v)| self.first_callable(v, vm, file))
                            .collect();
                        if let Some(ms) = self.tables.classes.get(&q).map(|_| &self.tables.methods)
                        {
                            defs.extend(
                                ms.values()
                                    .flatten()
                                    .copied()
                                    .filter(|&d| self.def(d).class() == Some(q.as_str())),
                            );
                        }
                        defs.sort();
                        for d in defs {
                            let (l, k, det) = def_item(d);
                            push(l, k, det);
                        }
                    }
                    Some(q) => {
                        if let Some(other) = self.find_file(file, &q) {
                            for (i, d) in self.file(other).facts.defs.iter().enumerate() {
                                let id = DefId {
                                    file: other,
                                    index: i as u32,
                                };
                                if d.kind == DefKind::Function && self.callable_from(id, file) {
                                    let (l, k, det) = def_item(DefId {
                                        file: other,
                                        index: i as u32,
                                    });
                                    push(l, k, det);
                                }
                            }
                        }
                    }
                }
            }
            CompletionContext::General => {
                // Locals and parameters of the enclosing function.
                let root = f.syntax();
                let at = root.token_at_offset(offset).left_biased();
                let func = at
                    .and_then(|t| t.parent())
                    .and_then(|p| enclosing_function(&p));
                let mut class = None;
                if let Some(func) = &func {
                    for (name, _, param) in locals(func, &cfg) {
                        let kind = if param {
                            SymbolKind::Param
                        } else {
                            SymbolKind::Local
                        };
                        push(name, kind, None);
                    }
                    class = f
                        .facts
                        .def_at(func.text_range().start())
                        .and_then(|i| f.facts.defs[i as usize].class().map(str::to_owned));
                }
                // Methods of the enclosing class and of its bases.
                if let Some(c) = &class {
                    for (c, _) in self.class_and_bases(c) {
                        let mut ms: Vec<DefId> = self
                            .tables
                            .methods
                            .values()
                            .flatten()
                            .copied()
                            .filter(|&d| self.def(d).class() == Some(c.as_str()))
                            .collect();
                        ms.sort();
                        for d in ms {
                            let (l, k, det) = def_item(d);
                            push(l, k, det);
                        }
                    }
                }
                // Functions of the file, of its namespace, of its imports.
                for (i, d) in f.facts.defs.iter().enumerate() {
                    if d.kind == DefKind::Function {
                        let (l, k, det) = def_item(DefId {
                            file,
                            index: i as u32,
                        });
                        push(l, k, det);
                    }
                }
                if cfg.resolution == Resolution::Namespace {
                    let mut ns: Vec<DefId> = self
                        .tables
                        .ns_fns
                        .iter()
                        .filter(|((n, _), _)| *n == f.facts.namespace)
                        .filter_map(|(_, v)| self.first_callable(v, vm, file))
                        .collect();
                    ns.sort();
                    for d in ns {
                        let (l, k, det) = def_item(d);
                        push(l, k, det);
                    }
                }
                let res = self.resolution(file);
                // A bare name never reaches another namespace in a `namespace-first` game.
                let scope: &[FileId] = if self.namespace_first(file) {
                    &[]
                } else {
                    &res.scope
                };
                for &u in scope {
                    for (i, d) in self.file(u).facts.defs.iter().enumerate() {
                        if d.kind == DefKind::Function
                            && self.callable_from(
                                DefId {
                                    file: u,
                                    index: i as u32,
                                },
                                file,
                            )
                        {
                            let (l, k, det) = def_item(DefId {
                                file: u,
                                index: i as u32,
                            });
                            push(l, k, det);
                        }
                    }
                }
                // Macros visible here.
                let mut macros: Vec<DefId> = res
                    .macros
                    .iter()
                    .filter_map(|m| self.tables.macros.get(m)?.first().copied())
                    .collect();
                macros.sort();
                for d in macros {
                    let def = self.def(d);
                    push(def.name.clone(), SymbolKind::Macro, Some(def.doc.clone()));
                }
                Self::builtin_items(profile.builtins.as_ref(), vm, &mut push);
            }
        }
        items
    }

    fn builtin_items(
        cat: Option<&crate::data::Catalogue>,
        vm: Option<Vm>,
        push: &mut impl FnMut(String, SymbolKind, Option<String>),
    ) {
        let Some(cat) = cat else { return };
        let vm = vm.unwrap_or(Vm::Gsc);
        let mut by_name: HashMap<&str, Vec<&crate::data::Builtin>> = HashMap::new();
        for (n, v, _) in cat.names() {
            if *v == vm {
                by_name.entry(n.as_str()).or_default();
            }
        }
        let mut names: Vec<&str> = by_name.keys().copied().collect();
        names.sort();
        for n in names {
            let e: Vec<_> = cat.lookup(n).iter().filter(|b| b.vm == vm).collect();
            let kind = if e.iter().all(|b| b.kind == Kind::Method) {
                "method"
            } else {
                "function"
            };
            let arity = e.iter().find_map(|b| b.describe_arity());
            let detail = match arity {
                Some(a) => format!("builtin {kind}, {a}"),
                None => format!("builtin {kind}"),
            };
            push(n.to_owned(), SymbolKind::Builtin, Some(detail));
        }
        for o in cat.intrinsics() {
            let detail = match o.describe_arity() {
                Some(a) => format!("builtin (opcode), {a}"),
                None => "builtin (opcode)".to_owned(),
            };
            push(o.name.clone(), SymbolKind::Builtin, Some(detail));
        }
    }

    /// The first of `defs` that file `from` (of VM `vm`) can call.
    fn first_callable(&self, defs: &[DefId], vm: Option<Vm>, from: FileId) -> Option<DefId> {
        defs.iter()
            .copied()
            .find(|&d| vm_compatible(self.file(d.file).meta.vm, vm) && self.callable_from(d, from))
    }

    /// Can file `from` call `d`? Private functions are callable from their own file only.
    fn callable_from(&self, d: DefId, from: FileId) -> bool {
        d.file == from || !self.def(d).flags.split(',').any(|f| f == "private")
    }

    /// The function at `offset` for a call hierarchy: the one declared there, or the one called.
    pub fn call_hierarchy_target(&self, file: FileId, offset: TextSize) -> Option<DefId> {
        match self.symbol_at(file, offset)? {
            Symbol::Def(d) if self.def(d).kind.is_callable() => Some(d),
            _ => None,
        }
    }

    /// The call hierarchy item of a function.
    pub fn call_hierarchy_item(&self, d: DefId) -> NavTarget {
        self.nav(d)
    }

    /// Who calls (or takes a pointer to) `d`, grouped by calling function.
    pub fn incoming_calls(&self, d: DefId) -> Vec<IncomingCall> {
        let files: Vec<FileId> = self.files().collect();
        let groups: Vec<Vec<IncomingCall>> = par_map(&files, self.jobs(), |&f| {
            let res = self.resolution(f);
            let facts = &self.file(f).facts;
            let mut by: Vec<IncomingCall> = Vec::new();
            for (r, x) in res.refs.iter().zip(&facts.refs) {
                if r.def != Some(d) {
                    continue;
                }
                let from = x.caller.map(|index| DefId { file: f, index });
                match by.iter_mut().find(|c| c.from == from) {
                    Some(c) => c.ranges.push(x.range),
                    None => by.push(IncomingCall {
                        from,
                        file: f,
                        ranges: vec![x.range],
                    }),
                }
            }
            by
        });
        groups.into_iter().flatten().collect()
    }

    /// What `d` calls, grouped by callee, in order of first call.
    pub fn outgoing_calls(&self, d: DefId) -> Vec<OutgoingCall> {
        let res = self.resolution(d.file);
        let facts = &self.file(d.file).facts;
        let mut out: Vec<OutgoingCall> = Vec::new();
        for (r, x) in res.refs.iter().zip(&facts.refs) {
            if x.caller != Some(d.index) {
                continue;
            }
            let to = match (r.def, r.how) {
                (Some(t), _) => Callee::Def(t),
                (None, How::Builtin | How::BuiltinOtherVm) => Callee::Builtin(r.callee.clone()),
                _ => Callee::Other(r.callee.clone()),
            };
            match out.iter_mut().find(|c| c.to == to) {
                Some(c) => c.ranges.push(x.range),
                None => out.push(OutgoingCall {
                    to,
                    ranges: vec![x.range],
                }),
            }
        }
        out
    }
}

/// `( a, b )` spacing as the T7 sources write it; `()` when empty.
fn pad(params: &str) -> String {
    if params.is_empty() {
        String::new()
    } else {
        format!(" {params} ")
    }
}
