//! Navigation beyond definitions and references: class members and class uses, field writes,
//! implementations (overrides, what a field is set to), type definitions (the class a value is
//! an instance of, the function a pointer names), the class hierarchy, highlights within a file,
//! reference counts, and macro expansions at a use. Editor terms, like [`crate::ide`].
//!
//! The behaviour follows GSCode's (Blakintosh/gscode, GPL-3.0) navigation handlers where it
//! defines one: a class member is a bare name inside the class's methods, a field is "defined"
//! by its writes and "implemented" by its plain `=` writes and the functions they bind.

use std::collections::{HashMap, HashSet};

use gsc_syntax::ast::{self, AstNode};
use gsc_syntax::{
    macro_expansions_with, ExpandedToken, MacroExpansion, MacroTable, SyntaxKind, SyntaxNode,
    TextRange, TextSize,
};

use crate::facts::{def_node, DefKind};
use crate::ide::{enclosing_function, FileRange, Hit, NavTarget, Symbol, SymbolKind};
use crate::par::par_map;
use crate::workspace::{DefId, FileId, Workspace};

/// A highlight in one file: the range and whether it writes the name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Highlight {
    pub range: TextRange,
    pub write: bool,
}

/// Case-insensitive substring test (ASCII), to skip files that cannot mention a name.
fn contains_ci(hay: &str, needle: &str) -> bool {
    let (h, n) = (hay.as_bytes(), needle.as_bytes());
    if n.is_empty() {
        return true;
    }
    let first = n[0].to_ascii_lowercase();
    h.len() >= n.len()
        && (0..=h.len() - n.len()).any(|i| {
            h[i].to_ascii_lowercase() == first && h[i..i + n.len()].eq_ignore_ascii_case(n)
        })
}

/// Is the name at `node` (a `NAME_REF`, or the `FIELD_EXPR` naming a field) the target of an
/// assignment? `plain`: `=` only, not `+=` and the like.
fn assigned(node: &SyntaxNode, plain: bool) -> bool {
    let Some(a) = node.parent().and_then(ast::AssignExpr::cast) else {
        return false;
    };
    if a.lhs().is_none_or(|l| l.syntax() != node) {
        return false;
    }
    !plain || a.op().is_some_and(|t| t.text() == "=")
}

/// Is the identifier token at `range` a write: a declaration (`NAME`), an assignment target, or a
/// `foreach` binding?
fn is_write(root: &SyntaxNode, range: TextRange) -> bool {
    let Some(tok) = root
        .token_at_offset(range.start())
        .find(|t| t.text_range() == range)
    else {
        return false;
    };
    let Some(p) = tok.parent() else { return false };
    match p.kind() {
        SyntaxKind::NAME => true,
        SyntaxKind::NAME_REF => {
            if p.parent()
                .is_some_and(|g| g.kind() == SyntaxKind::FOREACH_BINDINGS)
            {
                return true;
            }
            let target = match p.parent().and_then(ast::FieldExpr::cast) {
                Some(fe) if fe.field().is_some_and(|f| f.syntax() == &p) => fe.syntax().clone(),
                _ => p,
            };
            assigned(&target, false)
        }
        _ => false,
    }
}

/// The parameter names of a function node (identifier case).
fn param_names(func: &SyntaxNode, ident: impl Fn(&str) -> String) -> HashSet<String> {
    func.descendants()
        .filter_map(ast::Param::cast)
        .filter_map(|p| p.name())
        .map(|n| ident(&n.text()))
        .collect()
}

/// Expanded tokens as code: spaces where a reader expects them, cut to `max` characters.
pub fn render_tokens(tokens: &[ExpandedToken], max: usize) -> String {
    let mut s = String::new();
    let mut prev: Option<&ExpandedToken> = None;
    for t in tokens {
        if let Some(p) = prev {
            let (a, b) = (p.text.as_str(), t.text.as_str());
            let operand_end = p.kind == SyntaxKind::IDENT
                || p.kind.is_name_like_keyword()
                || matches!(a, ")" | "]");
            let space = match (a, b) {
                // `f()`, `f( a, b )`: the spacing of the shipped scripts.
                ("(", ")") => false,
                ("(", _) | (_, ")") => true,
                ("[", _) | (_, "]") => false,
                ("." | "::" | "!" | "#", _) => false,
                ("&", _) => t.kind != SyntaxKind::IDENT,
                (_, "," | ";" | "." | "::") => false,
                (_, "(" | "[") => !operand_end,
                _ => true,
            };
            if space {
                s.push(' ');
            }
        }
        s.push_str(&t.text);
        prev = Some(t);
    }
    if s.chars().count() > max {
        let mut cut: String = s.chars().take(max).collect();
        cut.push('\u{2026}');
        return cut;
    }
    s
}

impl Workspace {
    /// What a name inside or about a class is: a class named by `new C()` or a base list, a
    /// member at its `var` declaration, or a member named bare in a method of the class.
    pub(crate) fn class_hit(
        &self,
        file: FileId,
        parent: &SyntaxNode,
        name: &str,
        range: TextRange,
    ) -> Option<Hit> {
        let cfg = self.profile_of(file).config;
        match parent.kind() {
            SyntaxKind::NAME_REF => {
                let gp = parent.parent()?;
                if matches!(gp.kind(), SyntaxKind::NEW_EXPR | SyntaxKind::CLASS_BASE) {
                    let d = *self.tables.classes.get(name)?.last()?;
                    return Some(Hit::Class(d, range));
                }
                if gp.kind() == SyntaxKind::QUALIFIED_REF {
                    return None;
                }
                let func = enclosing_function(parent)?;
                let facts = &self.file(file).facts;
                let class = facts.defs[facts.def_at(func.text_range().start())? as usize]
                    .class()?
                    .to_owned();
                if param_names(&func, |s| cfg.ident(s)).contains(name) {
                    return None;
                }
                let owner = self.member_owner(&class, name)?;
                Some(Hit::Member(owner, name.to_owned(), range))
            }
            SyntaxKind::NAME if parent.parent()?.kind() == SyntaxKind::CLASS_VAR => {
                let class = parent
                    .ancestors()
                    .find_map(ast::ClassDef::cast)?
                    .name()?
                    .text();
                Some(Hit::Member(cfg.ident(&class), name.to_owned(), range))
            }
            _ => None,
        }
    }

    /// The `var` members a class declaration names: identifier-case name and range.
    pub fn class_vars(&self, class: DefId) -> Vec<(String, TextRange)> {
        let data = self.file(class.file);
        let cfg = self.profile_of(class.file).config;
        let Some(node) = def_node(&data.syntax(), self.def(class)) else {
            return Vec::new();
        };
        let Some(c) = ast::ClassDef::cast(node) else {
            return Vec::new();
        };
        c.class_members()
            .filter_map(|m| match m {
                ast::ClassMember::ClassVar(v) => v.name(),
                _ => None,
            })
            .map(|n| (cfg.ident(&n.text()), n.syntax().text_range()))
            .collect()
    }

    /// The class (`class` or one of its bases, nearest first) that declares member `name`.
    pub fn member_owner(&self, class: &str, name: &str) -> Option<String> {
        self.class_and_bases(class)
            .into_iter()
            .find_map(|(c, decl)| {
                let k = decl?;
                self.class_vars(k)
                    .iter()
                    .any(|(n, _)| n == name)
                    .then_some(c)
            })
    }

    /// The declaration of member `name` of `class` (the class that declares it).
    pub fn member_decl(&self, class: &str, name: &str) -> Option<NavTarget> {
        let owner = self.member_owner(class, name)?;
        let k = *self.tables.classes.get(&owner)?.last()?;
        let (_, range) = self.class_vars(k).into_iter().find(|(n, _)| n == name)?;
        let full = self
            .file(k.file)
            .syntax()
            .covering_element(range)
            .ancestors()
            .find(|a| a.kind() == SyntaxKind::CLASS_VAR)
            .map_or(range, |n| n.text_range());
        Some(NavTarget {
            file: k.file,
            full_range: full,
            focus_range: range,
            name: name.to_owned(),
            kind: SymbolKind::Field,
            container: Some(owner),
        })
    }

    /// Classes that inherit, directly or not, from `class` (not `class` itself).
    pub fn descendant_classes(&self, class: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .tables
            .classes
            .keys()
            .filter(|k| *k != class)
            .filter(|k| self.class_and_bases(k).iter().any(|(c, _)| c == class))
            .cloned()
            .collect();
        out.sort();
        out
    }

    /// The direct subclasses of `class`.
    pub fn subclasses(&self, class: &str) -> Vec<DefId> {
        let mut out: Vec<DefId> = self
            .tables
            .classes
            .values()
            .filter_map(|v| v.last().copied())
            .filter(|&d| self.def(d).params.split(", ").any(|b| b == class))
            .collect();
        out.sort();
        out
    }

    /// The declarations of the direct bases of a class.
    pub fn superclasses(&self, class: DefId) -> Vec<DefId> {
        self.def(class)
            .params
            .split(", ")
            .filter(|b| !b.is_empty())
            .filter_map(|b| self.tables.classes.get(b)?.last().copied())
            .collect()
    }

    /// Uses of member `name` declared by `owner`: bare names (and `self.name`) in the methods of
    /// the classes that see that declaration, and the declaration itself.
    pub(crate) fn member_references(
        &self,
        owner: &str,
        name: &str,
        include_declaration: bool,
        files: &[FileId],
    ) -> Vec<FileRange> {
        let mut classes = self.descendant_classes(owner);
        classes.push(owner.to_owned());
        let mut out = Vec::new();
        for c in classes {
            if self.member_owner(&c, name).as_deref() != Some(owner) {
                continue;
            }
            for &k in self.tables.classes.get(&c).into_iter().flatten() {
                if files.contains(&k.file) {
                    self.member_uses_in_class(k, name, include_declaration && c == owner, &mut out);
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    fn member_uses_in_class(&self, k: DefId, name: &str, decl: bool, out: &mut Vec<FileRange>) {
        let data = self.file(k.file);
        let cfg = self.profile_of(k.file).config;
        let Some(node) = def_node(&data.syntax(), self.def(k)) else {
            return;
        };
        let callees: HashSet<TextRange> = data.facts.refs.iter().map(|r| r.range).collect();
        for t in node
            .descendants_with_tokens()
            .filter_map(|e| e.into_token())
            .filter(|t| t.kind() == SyntaxKind::IDENT && cfg.ident(t.text()) == name)
        {
            let Some(p) = t.parent() else { continue };
            let ok = match p.kind() {
                SyntaxKind::NAME => {
                    decl && p
                        .parent()
                        .is_some_and(|g| g.kind() == SyntaxKind::CLASS_VAR)
                }
                SyntaxKind::NAME_REF => {
                    let field = p.parent().and_then(ast::FieldExpr::cast);
                    match field {
                        Some(fe) if fe.field().is_some_and(|f| f.syntax() == &p) => {
                            // `self.name`
                            matches!(fe.base(), Some(ast::Expr::NameRef(b)) if b.text().eq_ignore_ascii_case("self"))
                        }
                        _ => {
                            !callees.contains(&t.text_range())
                                && p.parent()
                                    .is_none_or(|g| g.kind() != SyntaxKind::QUALIFIED_REF)
                                && enclosing_function(&p).is_some_and(|f| {
                                    !param_names(&f, |s| cfg.ident(s)).contains(name)
                                })
                        }
                    }
                }
                _ => false,
            };
            if ok {
                out.push(FileRange {
                    file: k.file,
                    range: t.text_range(),
                });
            }
        }
    }

    /// Where class `name` is used in `files`: `new name()` and base lists.
    pub fn class_uses(&self, name: &str, files: &[FileId]) -> Vec<FileRange> {
        par_map(files, self.jobs(), |&f| {
            let data = self.file(f);
            if !contains_ci(data.text(), name) {
                return Vec::new();
            }
            let cfg = self.profile_of(f).config;
            data.syntax()
                .descendants()
                .filter(|n| matches!(n.kind(), SyntaxKind::NEW_EXPR | SyntaxKind::CLASS_BASE))
                .flat_map(|n| n.children().filter_map(ast::NameRef::cast))
                .filter(|r| cfg.ident(&r.text()) == name)
                .map(|r| FileRange {
                    file: f,
                    range: r.syntax().text_range(),
                })
                .collect::<Vec<_>>()
        })
        .into_iter()
        .flatten()
        .collect()
    }

    /// Every assignment to field `name` in the workspace (`plain`: `=` only), as navigation
    /// targets on the field name, with the assigned value's target when it names a class or a
    /// function.
    fn field_assignments(&self, name: &str, plain: bool) -> Vec<(NavTarget, Option<DefId>)> {
        let files: Vec<FileId> = self.files().collect();
        par_map(&files, self.jobs(), |&f| {
            let data = self.file(f);
            if data
                .facts
                .fields
                .binary_search_by(|x| x.as_str().cmp(name))
                .is_err()
            {
                return Vec::new();
            }
            let cfg = self.profile_of(f).config;
            let mut out = Vec::new();
            for fe in data.syntax().descendants().filter_map(ast::FieldExpr::cast) {
                let Some(field) = fe.field() else { continue };
                if cfg.ident(&field.text()) != name || !assigned(fe.syntax(), plain) {
                    continue;
                }
                let assign = fe.syntax().parent().and_then(ast::AssignExpr::cast);
                let value = assign
                    .as_ref()
                    .and_then(|a| a.rhs())
                    .and_then(|e| self.value_target(f, &e));
                let full = assign.map_or(fe.syntax().text_range(), |a| a.syntax().text_range());
                let container = data
                    .facts
                    .def_at(full.start())
                    .map(|i| data.facts.defs[i as usize].qualified());
                out.push((
                    NavTarget {
                        file: f,
                        full_range: full,
                        focus_range: field.syntax().text_range(),
                        name: name.to_owned(),
                        kind: SymbolKind::Field,
                        container,
                    },
                    value,
                ));
            }
            out
        })
        .into_iter()
        .flatten()
        .collect()
    }

    /// Where field `name` is set (`plain`: by `=` only).
    pub fn field_writes(&self, name: &str, plain: bool) -> Vec<NavTarget> {
        self.field_assignments(name, plain)
            .into_iter()
            .map(|(n, _)| n)
            .collect()
    }

    /// The class a value is an instance of (`new C()`) or the function it points to (`&f`,
    /// `ns::f`).
    pub fn value_target(&self, file: FileId, e: &ast::Expr) -> Option<DefId> {
        let e = crate::facts::strip_parens(e.clone());
        if let ast::Expr::NewExpr(n) = &e {
            let cfg = self.profile_of(file).config;
            let name = cfg.ident(&n.name_ref()?.text());
            return self.tables.classes.get(&name)?.last().copied();
        }
        if !matches!(e, ast::Expr::FunctionRef(_) | ast::Expr::QualifiedRef(_)) {
            return None;
        }
        let range = e.syntax().text_range();
        let facts = &self.file(file).facts;
        let i = facts
            .refs
            .iter()
            .position(|r| r.kind == "pointer" && range.contains_range(r.range))?;
        self.resolution(file).refs[i].def
    }

    /// Classes first, else functions: what a set of values is.
    fn type_targets(&self, values: impl IntoIterator<Item = DefId>) -> Vec<NavTarget> {
        let mut values: Vec<DefId> = values.into_iter().collect();
        values.sort();
        values.dedup();
        let classes: Vec<DefId> = values
            .iter()
            .copied()
            .filter(|&d| self.def(d).kind == DefKind::Class)
            .collect();
        let pick = if classes.is_empty() { values } else { classes };
        pick.into_iter().map(|d| self.nav_of(d)).collect()
    }

    pub(crate) fn nav_of(&self, d: DefId) -> NavTarget {
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

    /// The values assigned to local `name` in the function at `scope`.
    fn local_values(&self, file: FileId, scope: TextRange, name: &str) -> Vec<DefId> {
        let data = self.file(file);
        let cfg = self.profile_of(file).config;
        let root = data.syntax();
        let func = root
            .covering_element(scope)
            .ancestors()
            .find(|a| a.text_range() == scope);
        let Some(func) = func else { return Vec::new() };
        func.descendants()
            .filter_map(ast::AssignExpr::cast)
            .filter(
                |a| matches!(a.lhs(), Some(ast::Expr::NameRef(r)) if cfg.ident(&r.text()) == name),
            )
            .filter_map(|a| a.rhs())
            .filter_map(|e| self.value_target(file, &e))
            .collect()
    }

    /// Go to type definition: the class a local, field or member holds an instance of, else the
    /// function it points to.
    pub fn type_definitions(&self, file: FileId, offset: TextSize) -> Vec<NavTarget> {
        match self.symbol_at(file, offset) {
            Some(Symbol::Local {
                file, scope, name, ..
            }) => self.type_targets(self.local_values(file, scope, &name)),
            Some(Symbol::Field(name)) => self.type_targets(
                self.field_assignments(&name, true)
                    .into_iter()
                    .filter_map(|(_, v)| v),
            ),
            Some(Symbol::Member { class, name }) => {
                let files: Vec<FileId> = self.files().collect();
                let refs = self.member_references(&class, &name, false, &files);
                let values = refs.iter().filter_map(|r| {
                    let root = self.file(r.file).syntax();
                    let tok = root
                        .token_at_offset(r.range.start())
                        .find(|t| t.text_range() == r.range)?;
                    let p = tok.parent()?;
                    let target = match p.parent().and_then(ast::FieldExpr::cast) {
                        Some(fe) => fe.syntax().clone(),
                        None => p,
                    };
                    let a = target.parent().and_then(ast::AssignExpr::cast)?;
                    if a.lhs()?.syntax() != &target {
                        return None;
                    }
                    self.value_target(r.file, &a.rhs()?)
                });
                self.type_targets(values.collect::<Vec<_>>())
            }
            Some(Symbol::Def(d)) if self.def(d).kind == DefKind::Class => vec![self.nav_of(d)],
            _ => Vec::new(),
        }
    }

    /// Go to implementation: the overrides of a method in the subclasses; the subclasses of a
    /// class; the plain `=` writes of a field or member and the functions they bind.
    pub fn implementations(&self, file: FileId, offset: TextSize) -> Vec<NavTarget> {
        match self.symbol_at(file, offset) {
            Some(Symbol::Def(d)) => {
                let def = self.def(d);
                match def.kind {
                    DefKind::Method => {
                        let (Some(class), m) = (def.class(), def.short_name()) else {
                            return Vec::new();
                        };
                        let below: HashSet<String> =
                            self.descendant_classes(class).into_iter().collect();
                        let mut out: Vec<DefId> = self
                            .tables
                            .methods
                            .get(m)
                            .into_iter()
                            .flatten()
                            .copied()
                            .filter(|&x| self.def(x).class().is_some_and(|c| below.contains(c)))
                            .collect();
                        out.sort();
                        out.into_iter().map(|x| self.nav_of(x)).collect()
                    }
                    DefKind::Class => {
                        let below = self.descendant_classes(&def.name);
                        below
                            .iter()
                            .filter_map(|c| self.tables.classes.get(c)?.last().copied())
                            .map(|x| self.nav_of(x))
                            .collect()
                    }
                    _ => Vec::new(),
                }
            }
            Some(Symbol::Field(name)) => {
                let mut out = Vec::new();
                let mut bound = Vec::new();
                for (n, v) in self.field_assignments(&name, true) {
                    out.push(n);
                    bound.extend(v.filter(|&d| self.def(d).kind.is_callable()));
                }
                bound.sort();
                bound.dedup();
                out.extend(bound.into_iter().map(|d| self.nav_of(d)));
                out
            }
            Some(Symbol::Member { class, name }) => {
                let files: Vec<FileId> = self.files().collect();
                self.member_references(&class, &name, false, &files)
                    .into_iter()
                    .filter(|r| {
                        let root = self.file(r.file).syntax();
                        is_write(&root, r.range)
                    })
                    .map(|r| NavTarget {
                        file: r.file,
                        full_range: r.range,
                        focus_range: r.range,
                        name: name.clone(),
                        kind: SymbolKind::Field,
                        container: Some(class.clone()),
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// The class at `offset` for a type hierarchy: a class named there, or the class a value
    /// there holds.
    pub fn class_at(&self, file: FileId, offset: TextSize) -> Option<DefId> {
        match self.symbol_at(file, offset)? {
            Symbol::Def(d) if self.def(d).kind == DefKind::Class => Some(d),
            Symbol::Def(d) if self.def(d).kind == DefKind::Method => {
                let c = self.def(d).class()?;
                self.tables.classes.get(c)?.last().copied()
            }
            _ => {
                let t = self.type_definitions(file, offset);
                let n = t.iter().find(|n| n.kind == SymbolKind::Class)?;
                let facts = &self.file(n.file).facts;
                let i = facts
                    .defs
                    .iter()
                    .position(|d| d.kind == DefKind::Class && d.range == n.full_range)?;
                Some(DefId {
                    file: n.file,
                    index: i as u32,
                })
            }
        }
    }

    /// Every occurrence of the symbol at `offset` in its file, writes marked.
    pub fn highlights(&self, file: FileId, offset: TextSize) -> Vec<Highlight> {
        let Some(sym) = self.symbol_at(file, offset) else {
            return Vec::new();
        };
        let refs = self.references_in(&sym, true, &[file]);
        let root = self.file(file).syntax();
        refs.into_iter()
            .filter(|r| r.file == file)
            .map(|r| Highlight {
                range: r.range,
                write: is_write(&root, r.range),
            })
            .collect()
    }

    /// How many times each definition of `file` is used in the workspace (declarations not
    /// counted): functions and methods by resolved reference, macros by use, classes by `new`
    /// and base lists. In definition order; constants are left out.
    pub fn reference_counts(&self, file: FileId) -> Vec<(DefId, usize)> {
        let defs = &self.file(file).facts.defs;
        let macros: HashMap<String, u32> = defs
            .iter()
            .enumerate()
            .filter(|(_, d)| d.kind == DefKind::Define)
            .map(|(i, d)| (d.name.clone(), i as u32))
            .collect();
        let files: Vec<FileId> = self.files().collect();
        let n = defs.len();
        let partial = par_map(&files, self.jobs(), |&f| {
            let mut counts = vec![0usize; n];
            for r in self.resolution(f).refs.iter() {
                match r.def {
                    Some(d) if d.file == file && r.how != crate::resolve::How::Macro => {
                        counts[d.index as usize] += 1
                    }
                    _ if r.how == crate::resolve::How::Macro => {
                        if let Some(&i) = macros.get(&r.callee) {
                            counts[i as usize] += 1;
                        }
                    }
                    _ => {}
                }
            }
            counts
        });
        let mut counts = vec![0usize; n];
        for p in partial {
            for (c, x) in counts.iter_mut().zip(p) {
                *c += x;
            }
        }
        let mut out = Vec::new();
        for (i, d) in defs.iter().enumerate() {
            let id = DefId {
                file,
                index: i as u32,
            };
            let count = match d.kind {
                DefKind::Const => continue,
                DefKind::Class => self.class_uses(&d.name, &files).len(),
                // Constructors and destructors are not called by name.
                DefKind::Method if d.name_range == d.range => continue,
                _ => counts[i],
            };
            out.push((id, count));
        }
        out
    }

    /// The declaration of class `name` (identifier case): the last one, as resolution reads it.
    pub fn class_named(&self, name: &str) -> Option<DefId> {
        self.tables.classes.get(name)?.last().copied()
    }

    /// Functions (not methods) whose name starts with `prefix`, ignoring case.
    pub fn functions_with_prefix(&self, prefix: &str) -> Vec<DefId> {
        let mut out: Vec<DefId> = self
            .tables
            .callables
            .iter()
            .filter(|(k, _)| {
                k.len() >= prefix.len()
                    && k.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
            })
            .flat_map(|(_, v)| v.iter().copied())
            .filter(|&d| self.def(d).kind == DefKind::Function)
            .collect();
        out.sort();
        out
    }

    /// The strings the scripts use as names (events, notify and waittill names, event-like
    /// literals) and the engine's events, lower case, sorted.
    pub fn known_strings(&self, file: FileId) -> Vec<String> {
        let mut set: std::collections::BTreeSet<String> = Default::default();
        for f in self.files() {
            let facts = &self.file(f).facts;
            set.extend(facts.strings.iter().cloned());
            set.extend(facts.events.iter().filter_map(|s| s.key()));
        }
        if let Some(c) = &self.profile_of(file).events {
            set.extend(c.names().map(str::to_ascii_lowercase));
        }
        set.into_iter().filter(|s| !s.is_empty()).collect()
    }

    /// The macros a file sees from other files (its headers), as a table for expansion.
    pub fn macro_table(&self, file: FileId) -> MacroTable {
        let res = self.resolution(file);
        let mut files: Vec<FileId> = res
            .macros
            .iter()
            .filter_map(|m| self.tables.macros.get(m))
            .flatten()
            .map(|d| d.file)
            .filter(|&f| f != file)
            .collect();
        files.sort();
        files.dedup();
        let mut table = MacroTable::new();
        for f in files {
            let data = self.file(f);
            table.add_tree(&data.syntax(), self.profile_of(f).dialect(data.decompiled));
        }
        table
    }

    /// Every macro use of a file with its expansion.
    pub fn macro_expansions(&self, file: FileId) -> Vec<MacroExpansion> {
        let data = self.file(file);
        let dialect = self.profile_of(file).dialect(data.decompiled);
        macro_expansions_with(&data.syntax(), &self.macro_table(file), dialect)
    }

    /// The macro use at `offset` (its name starting there, else the innermost use around it).
    pub fn macro_expansion_at(&self, file: FileId, offset: TextSize) -> Option<MacroExpansion> {
        let all = self.macro_expansions(file);
        if let Some(i) = all.iter().position(|x| x.use_range.start() == offset) {
            return all.into_iter().nth(i);
        }
        all.into_iter()
            .filter(|x| x.use_range.contains_inclusive(offset))
            .min_by_key(|x| x.use_range.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &[(&str, SyntaxKind)]) -> Vec<ExpandedToken> {
        s.iter()
            .map(|(t, k)| ExpandedToken {
                kind: *k,
                text: (*t).to_owned(),
            })
            .collect()
    }

    #[test]
    fn renders_calls_like_the_scripts() {
        let (id, p) = (SyntaxKind::IDENT, SyntaxKind::L_PAREN);
        let t = toks(&[
            ("isdefined", id),
            ("(", p),
            ("self", id),
            (".", p),
            ("x", id),
            (")", p),
            ("&&", p),
            ("f", id),
            ("(", p),
            (")", p),
        ]);
        assert_eq!(render_tokens(&t, 100), "isdefined( self.x ) && f()");
        assert_eq!(render_tokens(&t, 5), "isdef\u{2026}");
    }

    #[test]
    fn substring_ignores_case() {
        assert!(contains_ci("x = new cFoo();", "cfoo"));
        assert!(!contains_ci("x = 1;", "cfoo"));
        assert!(contains_ci("ab", ""));
    }
}
