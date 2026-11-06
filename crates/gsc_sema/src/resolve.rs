//! Name resolution across files, per engine family (the order is on [`Workspace::resolve_bare`]).
//!
//! T7+ resolves through `#namespace` and `#using`, IW and T4-T6 through `#include` and path
//! qualifiers. Inside a class method a bare name first reaches the class's methods; a bare name
//! nothing else reaches may still be a macro or a builtin of the other VM.

use std::collections::HashSet;
use std::sync::Arc;

use crate::data::Kind;
use crate::facts::{Ref, Target};
use crate::par::par_map;
use crate::profile::{BareCall, Resolution};
use crate::vfs::{vm_compatible, Vm};
use crate::workspace::{DefId, FileId, Workspace};

/// How a reference was resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum How {
    /// A function of the same file.
    Local,
    /// A function of the caller's namespace in another file (T7+).
    Namespace,
    /// A function of a `#using`'d file (T7+).
    Using,
    /// A function of an `#include`d file (IW, T4-T6).
    Include,
    /// `ns::f` or `path::f` found.
    Qualified,
    /// A class method.
    Method,
    /// The engine catalogue (or a keyword the language treats as a builtin).
    Builtin,
    /// Only in the other VM's catalogue.
    BuiltinOtherVm,
    /// A `#define`.
    Macro,
    /// `[[ expr ]]()` or an ambiguous `->m()`.
    Dynamic,
    Unresolved,
}

impl How {
    pub fn label(self) -> &'static str {
        match self {
            How::Local => "local",
            How::Namespace => "namespace",
            How::Using => "using",
            How::Include => "include",
            How::Qualified => "qualified",
            How::Method => "method",
            How::Builtin => "builtin",
            How::BuiltinOtherVm => "builtin_othervm",
            How::Macro => "macro",
            How::Dynamic => "dynamic",
            How::Unresolved => "unresolved",
        }
    }

    /// Reaches a script definition (function, method or macro).
    pub fn is_script(self) -> bool {
        matches!(
            self,
            How::Local
                | How::Namespace
                | How::Using
                | How::Include
                | How::Qualified
                | How::Method
                | How::Macro
        )
    }
}

/// A reference after resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub how: How,
    /// `ns::fn` for a script function, the bare name for a builtin, the macro as spelled where
    /// defined, the text as written otherwise.
    pub callee: String,
    /// The definition reached, for script functions, methods and macros.
    pub def: Option<DefId>,
}

fn r(how: How, callee: String, def: Option<DefId>) -> Resolved {
    Resolved { how, callee, def }
}

/// What one file sees, resolved: its imports, its macros and every reference.
#[derive(Debug, Default)]
pub struct FileResolution {
    /// `imports[i]` is the file `facts.imports[i]` names, if found.
    pub imports: Vec<Option<FileId>>,
    /// The files `#using`/`#include` bring into scope, in order.
    pub scope: Vec<FileId>,
    /// Macros (lower case) defined in the file and in the files it `#insert`s, transitively.
    pub macros: HashSet<String>,
    /// `refs[i]` resolves `facts.refs[i]`.
    pub refs: Vec<Resolved>,
}

impl Workspace {
    /// The file an import target or path qualifier names, preferring the importer's root, then
    /// its VM.
    pub fn find_file(&self, from: FileId, target: &str) -> Option<FileId> {
        let f = &self.file(from).meta;
        let t = crate::vfs::strip_script_ext(target);
        let score = |&i: &FileId| {
            let c = &self.file(i).meta;
            (usize::from(c.root != f.root), usize::from(c.vm != f.vm), i)
        };
        let by_key = self.files_with_key(t).iter().copied().min_by_key(score);
        if by_key.is_some() {
            return by_key;
        }
        // `#using script_<hex>` naming a file kept under its clear path, or the reverse.
        let scheme = self.profile_of(from).scheme?;
        crate::hashing::script_keys_of_target(&scheme, target)
            .into_iter()
            .filter_map(|k| self.tables.by_script.get(&k))
            .flatten()
            .copied()
            .min_by_key(score)
    }

    /// The resolution of a file, computed on first use and cached until the next change.
    pub fn resolution(&self, f: FileId) -> Arc<FileResolution> {
        self.cache[f.index()]
            .get_or_init(|| Arc::new(self.compute(f)))
            .clone()
    }

    /// Resolve every file on the workspace's threads (batch tools; queries do it lazily).
    pub fn resolve_all(&self) {
        let files: Vec<FileId> = self.files().collect();
        par_map(&files, self.jobs(), |&f| {
            self.resolution(f);
        });
    }

    fn compute(&self, f: FileId) -> FileResolution {
        let facts = &self.file(f).facts;
        let imports: Vec<Option<FileId>> = facts
            .imports
            .iter()
            .map(|i| self.find_file(f, &i.target))
            .collect();
        let scope = facts
            .imports
            .iter()
            .zip(&imports)
            .filter(|(i, _)| i.is_scope())
            .filter_map(|(_, x)| *x)
            .collect();
        let mut res = FileResolution {
            imports,
            scope,
            macros: self.visible_macros(f),
            refs: Vec::new(),
        };
        res.refs = facts
            .refs
            .iter()
            .map(|x| self.resolve_ref(f, &res, x))
            .collect();
        res
    }

    /// Macros of `f` and of the files it `#insert`s or `#inline`s, transitively.
    fn visible_macros(&self, f: FileId) -> HashSet<String> {
        let mut seen = HashSet::new();
        let mut stack = vec![f];
        let mut vis = HashSet::new();
        while let Some(x) = stack.pop() {
            if !seen.insert(x) {
                continue;
            }
            let facts = &self.file(x).facts;
            vis.extend(
                facts
                    .defs
                    .iter()
                    .filter(|d| d.kind == crate::facts::DefKind::Define)
                    .map(|d| d.name.to_ascii_lowercase()),
            );
            for imp in facts.imports.iter().filter(|i| !i.is_scope()) {
                if let Some(y) = self.find_file(x, &imp.target) {
                    stack.push(y);
                }
            }
        }
        vis
    }

    fn resolve_ref(&self, f: FileId, res: &FileResolution, x: &Ref) -> Resolved {
        let (ns, class) = self.file(f).facts.ref_scope(x);
        self.resolve_ref_kind(f, res, self.resolve_in(f, res, ns, class, &x.target), x)
    }

    /// A threaded call links to script functions only: a bare `thread f()` never reaches a
    /// builtin in `namespace-first` games (measured in the T7, T8 and T9 VMs) nor in
    /// `file-first` (T4, IW3) and `builtin-strict` (T6) ones, where it goes on to the same file
    /// and the `#include`d files instead.
    ///
    /// In `builtin-strict` games the linker looks a call up among the builtin functions and a
    /// method call (`ent foo()`) among the builtin methods (two paths of the T6 import
    /// resolver), so only a builtin of the call's kind comes before the script functions.
    fn resolve_ref_kind(
        &self,
        f: FileId,
        res: &FileResolution,
        out: Resolved,
        x: &Ref,
    ) -> Resolved {
        if let (true, How::Builtin, Target::Bare(n) | Target::Qualified(None, n)) =
            (self.builtin_strict(f), out.how, &x.target)
        {
            if !self.is_builtin_kind(f, n, x.has_receiver()) {
                if let Some(s) = self.script_fn(f, res, n) {
                    return s;
                }
            }
        }
        let Target::Bare(n) = &x.target else {
            return out;
        };
        if !matches!(x.kind, "thread" | "childthread")
            || !matches!(out.how, How::Builtin | How::BuiltinOtherVm)
        {
            return out;
        }
        if self.file_first(f) || self.builtin_strict(f) {
            return self
                .script_fn(f, res, n)
                .unwrap_or_else(|| r(How::Unresolved, out.callee, None));
        }
        if self.namespace_first(f) {
            return r(How::Unresolved, out.callee, None);
        }
        out
    }

    /// Resolve `target` as written in file `f`, inside namespace `ns` and class `class`.
    pub fn resolve_target(
        &self,
        f: FileId,
        ns: &str,
        class: Option<&str>,
        target: &Target,
    ) -> Resolved {
        let res = self.resolution(f);
        self.resolve_in(f, &res, ns, class, target)
    }

    fn resolve_in(
        &self,
        f: FileId,
        res: &FileResolution,
        ns: &str,
        class: Option<&str>,
        target: &Target,
    ) -> Resolved {
        if let (Target::Bare(n), Some(class)) = (target, class) {
            if let Some((callee, def)) = self.class_method(class, n) {
                return r(How::Method, callee, def);
            }
        }
        let out = self.resolve_plain(f, res, ns, target);
        match (target, out.how) {
            (Target::Bare(n), How::Unresolved) => self.fallback(f, res, n, out),
            _ => out,
        }
    }

    /// Is `name` a builtin (or opcode builtin) of `f`'s VM?
    pub fn is_builtin(&self, f: FileId, name: &str) -> bool {
        let p = self.profile_of(f);
        p.builtins
            .as_ref()
            .is_some_and(|c| c.contains(self.file(f).meta.vm, &name.to_ascii_lowercase()))
    }

    /// Is `name` a builtin of `f`'s VM callable as a method (`ent foo()`), or as a function?
    /// An intrinsic is both.
    fn is_builtin_kind(&self, f: FileId, name: &str, method: bool) -> bool {
        let vm = self.file(f).meta.vm.unwrap_or(Vm::Gsc);
        let kind = if method { Kind::Method } else { Kind::Function };
        let lower = name.to_ascii_lowercase();
        self.profile_of(f).builtins.as_ref().is_some_and(|c| {
            c.intrinsic(&lower)
                .is_some_and(|i| i.vm.is_none_or(|v| v == vm))
                || c.lookup(&lower)
                    .iter()
                    .any(|b| b.vm == vm && b.kind == kind)
        })
    }

    /// A function of the file: by name, else by hash in a game that hashes identifiers.
    fn local(&self, f: FileId, n: &str) -> Option<DefId> {
        let data = self.file(f);
        data.local_fns
            .get(n)
            .or_else(|| {
                let k = crate::hashing::fn_key(self.profile_of(f).scheme.as_ref(), n)?;
                data.local_fns_h.get(&k)
            })
            .map(|&index| DefId { file: f, index })
    }

    /// A function `ns::n` in a file whose VM `f` can reach, preferring `f`'s root.
    pub fn namespace_fn(&self, f: FileId, ns: &str, n: &str) -> Option<DefId> {
        let me = &self.file(f).meta;
        let pick = |defs: &[DefId]| {
            defs.iter()
                .copied()
                .filter(|d| vm_compatible(self.file(d.file).meta.vm, me.vm))
                .min_by_key(|d| (usize::from(self.file(d.file).meta.root != me.root), *d))
        };
        if let Some(d) = self
            .tables
            .ns_fns
            .get(&(ns.to_owned(), n.to_owned()))
            .and_then(|v| pick(v))
        {
            return Some(d);
        }
        let s = self.profile_of(f).scheme?;
        let key = crate::hashing::ns_fn_key(&s, ns, n)?;
        self.tables.ns_fns_h.get(&key).and_then(|v| pick(v))
    }

    fn qualified_name(&self, d: DefId) -> String {
        self.def(d).qualified()
    }

    /// `ns::n` as written, or, in a game that hashes identifiers, as the definition spells it
    /// (`namespace_1a2b::function_3c4d` reached by `util::foo` stays one callee for the index).
    fn callee_name(&self, f: FileId, d: DefId, ns: &str, n: &str) -> String {
        if self.profile_of(f).scheme.is_some() {
            self.qualified_name(d)
        } else {
            format!("{ns}::{n}")
        }
    }

    fn resolve_plain(
        &self,
        f: FileId,
        res: &FileResolution,
        ns: &str,
        target: &Target,
    ) -> Resolved {
        let cfg = self.profile_of(f).config;
        match target {
            Target::Keyword(n) => r(How::Builtin, n.clone(), None),
            Target::Dynamic(e) => r(How::Dynamic, format!("[[{e}]]"), None),
            Target::Method(m) => match self.unique_method(m) {
                Some((callee, def)) => r(How::Method, callee, Some(def)),
                None => r(How::Dynamic, format!("->{m}"), None),
            },
            Target::Bare(n) => self.resolve_bare(f, res, ns, n),
            // `::foo` in a `builtin-strict` game (T6): the lookup of a bare call, builtin first
            // (`func_ref_shadows_builtin`: the game stores the builtin's native handler).
            Target::Qualified(None, n) if self.builtin_strict(f) => {
                match self.resolve_bare(f, res, ns, n) {
                    x if x.how == How::Unresolved => r(How::Unresolved, format!("::{n}"), None),
                    x => x,
                }
            }
            // `::foo`: a function of the file; in `file-first` games (T4, IW3) then of the
            // `#include`d files, as a bare call without the builtin step (`func_ref_builtin_only`).
            Target::Qualified(None, n) => match self.local(f, n) {
                Some(d) => r(How::Local, self.qualified_name(d), Some(d)),
                None => match res
                    .scope
                    .iter()
                    .filter(|_| self.file_first(f))
                    .find_map(|&u| self.local(u, n))
                {
                    Some(d) => r(How::Include, self.qualified_name(d), Some(d)),
                    None => r(How::Unresolved, format!("::{n}"), None),
                },
            },
            Target::Qualified(Some(q), n) => {
                if cfg.resolution == Resolution::Namespace && !q.contains('/') {
                    if let Some(d) = self.namespace_fn(f, q, n) {
                        return r(How::Qualified, self.callee_name(f, d, q, n), Some(d));
                    }
                    if q.eq_ignore_ascii_case("sys") && self.is_builtin(f, n) {
                        return r(How::Builtin, n.clone(), None);
                    }
                    // `base_class::method()` from a derived class.
                    if let Some((callee, def)) = self.class_method(q, n) {
                        return r(How::Method, callee, def);
                    }
                    return r(How::Unresolved, format!("{q}::{n}"), None);
                }
                // A path qualifier names a file.
                if let Some(other) = self.find_file(f, q) {
                    if let Some(d) = self.local(other, n) {
                        return r(How::Qualified, self.qualified_name(d), Some(d));
                    }
                }
                r(How::Unresolved, format!("{q}::{n}"), None)
            }
        }
    }

    /// A bare call `n()` in namespace `ns` of file `f`, in the profile's
    /// `bare_call_resolution` order (docs/rules/B004.md):
    ///
    /// - `namespace-first` (T7+): a function of `ns` (same file first), then the builtins;
    ///   never another namespace, even `#using`'d (gruff-data/semantics/t7.toml);
    /// - `builtin-first` (not measured; despite the name the same file comes first): the same
    ///   file, the builtins, then (MWIII) `ns`, the builtins and the `#using`'d files, or (IW)
    ///   the `#include`d files and the builtins; threads and `&f` as plain calls, `::f` only
    ///   the same file;
    /// - `file-first` (T4, IW3, T5, measured): the same file, the builtins, the `#include`d
    ///   files; a threaded call and `::f` skip the builtins;
    /// - `builtin-strict` (T6, measured): the builtins, the same file, the `#include`d files;
    ///   a threaded call skips the builtins, `::f` does not.
    pub fn resolve_bare(&self, f: FileId, res: &FileResolution, ns: &str, n: &str) -> Resolved {
        let cfg = self.profile_of(f).config;
        if self.namespace_first(f) {
            if let Some(d) = self.local(f, n).filter(|&d| self.def(d).ns == ns) {
                return r(How::Local, self.qualified_name(d), Some(d));
            }
            if let Some(d) = self.namespace_fn(f, ns, n) {
                let how = if d.file == f {
                    How::Local
                } else {
                    How::Namespace
                };
                return r(how, self.callee_name(f, d, ns, n), Some(d));
            }
            if self.is_builtin(f, n) {
                return r(How::Builtin, n.to_owned(), None);
            }
            return r(How::Unresolved, n.to_owned(), None);
        }
        if self.builtin_strict(f) {
            if self.is_builtin(f, n) {
                return r(How::Builtin, n.to_owned(), None);
            }
            return self
                .script_fn(f, res, n)
                .unwrap_or_else(|| r(How::Unresolved, n.to_owned(), None));
        }
        if let Some(d) = self.local(f, n) {
            return r(How::Local, self.qualified_name(d), Some(d));
        }
        if cfg.bare_call != BareCall::NamespaceFirst && self.is_builtin(f, n) {
            return r(How::Builtin, n.to_owned(), None);
        }
        let imported = |how| {
            res.scope
                .iter()
                .find_map(|&u| self.local(u, n))
                .map(|d| r(how, self.qualified_name(d), Some(d)))
        };
        match cfg.resolution {
            Resolution::Namespace => {
                if let Some(d) = self.namespace_fn(f, ns, n) {
                    return r(How::Namespace, self.callee_name(f, d, ns, n), Some(d));
                }
                if self.is_builtin(f, n) {
                    return r(How::Builtin, n.to_owned(), None);
                }
                if let Some(x) = imported(How::Using) {
                    return x;
                }
            }
            Resolution::Include => {
                if let Some(x) = imported(How::Include) {
                    return x;
                }
                if self.is_builtin(f, n) {
                    return r(How::Builtin, n.to_owned(), None);
                }
            }
        }
        r(How::Unresolved, n.to_owned(), None)
    }

    /// `bare_call_resolution = "namespace-first"` in a `#namespace` game?
    pub fn namespace_first(&self, f: FileId) -> bool {
        let cfg = self.profile_of(f).config;
        cfg.resolution == Resolution::Namespace && cfg.bare_call == BareCall::NamespaceFirst
    }

    /// `bare_call_resolution = "file-first"` (T4, IW3)?
    pub fn file_first(&self, f: FileId) -> bool {
        self.profile_of(f).config.bare_call == BareCall::FileFirst
    }

    /// `bare_call_resolution = "builtin-strict"` (T6)?
    pub fn builtin_strict(&self, f: FileId) -> bool {
        self.profile_of(f).config.bare_call == BareCall::BuiltinStrict
    }

    /// A function of the file, else of its imports (`#include`d or `#using`'d files): the
    /// script resolver of `file-first` and `builtin-strict` games, builtins aside.
    fn script_fn(&self, f: FileId, res: &FileResolution, n: &str) -> Option<Resolved> {
        if let Some(d) = self.local(f, n) {
            return Some(r(How::Local, self.qualified_name(d), Some(d)));
        }
        let how = match self.profile_of(f).config.resolution {
            Resolution::Include => How::Include,
            Resolution::Namespace => How::Using,
        };
        res.scope
            .iter()
            .find_map(|&u| self.local(u, n))
            .map(|d| r(how, self.qualified_name(d), Some(d)))
    }

    /// The script function a plain bare call `name()` (identifier case) of `f` reaches before
    /// a builtin of that name, if any: in `namespace-first` games one of the file's namespace,
    /// in `file-first` games one of the file itself (an `#include`d function does not shadow a
    /// builtin), in `builtin-strict` games none (the builtin comes first), else any
    /// [`Workspace::visible_function`].
    pub fn builtin_shadow(&self, f: FileId, name: &str) -> Option<DefId> {
        if self.builtin_strict(f) {
            return None;
        }
        if self.file_first(f) {
            return self.local(f, name);
        }
        self.visible_function(f, name)
    }

    /// Functions `n` (identifier case) in a namespace other than `ns` that a bare call of `f`
    /// cannot reach: those of `f`'s `#using`'d files, else any of a compatible VM. Empty
    /// outside `namespace-first` games.
    pub fn other_namespace_fns(&self, f: FileId, ns: &str, n: &str) -> Vec<DefId> {
        if !self.namespace_first(f) {
            return Vec::new();
        }
        let me = &self.file(f).meta;
        let other = |d: &DefId| {
            let def = self.def(*d);
            def.kind == crate::facts::DefKind::Function
                && def.ns != ns
                && vm_compatible(self.file(d.file).meta.vm, me.vm)
        };
        let res = self.resolution(f);
        let using: Vec<DefId> = res
            .scope
            .iter()
            .filter_map(|&u| self.local(u, n))
            .filter(other)
            .collect();
        if !using.is_empty() {
            return using;
        }
        self.callables_matching(f, n)
            .iter()
            .copied()
            .filter(other)
            .collect()
    }

    /// An unresolved bare name: a macro, or a builtin of the other VM.
    fn fallback(&self, f: FileId, res: &FileResolution, n: &str, out: Resolved) -> Resolved {
        let lower = n.to_ascii_lowercase();
        if let Some(all) = self.tables.macros.get(&lower) {
            let spelled = self.def(all[0]).name.clone();
            // The visible definition when there is one, for go-to-definition.
            let def = if res.macros.contains(&lower) {
                all.iter()
                    .copied()
                    .find(|d| d.file == f)
                    .or_else(|| {
                        all.iter()
                            .copied()
                            .find(|d| res.imports.contains(&Some(d.file)))
                    })
                    .unwrap_or(all[0])
            } else {
                all[0]
            };
            return r(How::Macro, spelled, Some(def));
        }
        let p = self.profile_of(f);
        if p.builtins
            .as_ref()
            .is_some_and(|c| !c.lookup(&lower).is_empty())
        {
            return r(How::BuiltinOtherVm, lower, None);
        }
        out
    }

    /// `ns::class::m` when `class` or one of its bases declares the method `m`.
    pub fn class_method(&self, class: &str, m: &str) -> Option<(String, Option<DefId>)> {
        let methods = self.tables.methods.get(m)?;
        self.class_and_bases(class)
            .into_iter()
            .find_map(|(c, decl)| {
                let d = methods
                    .iter()
                    .copied()
                    .find(|&d| self.def(d).class() == Some(&c))?;
                let ns = decl.map_or("", |k| self.def(k).ns.as_str());
                Some((format!("{ns}::{c}::{m}"), Some(d)))
            })
    }

    /// `class` and its bases, depth first, each with its declaration (the last one: the classes
    /// of later files override).
    pub fn class_and_bases(&self, class: &str) -> Vec<(String, Option<DefId>)> {
        let mut out = Vec::new();
        let mut stack = vec![class.to_owned()];
        let mut seen = HashSet::new();
        while let Some(c) = stack.pop() {
            if !seen.insert(c.clone()) {
                continue;
            }
            let decl = self.tables.classes.get(&c).and_then(|v| v.last()).copied();
            if let Some(k) = decl {
                let bases: Vec<&str> = self
                    .def(k)
                    .params
                    .split(", ")
                    .filter(|b| !b.is_empty())
                    .collect();
                stack.extend(bases.into_iter().rev().map(str::to_owned));
            }
            out.push((c, decl));
        }
        out
    }

    /// The one method named `m` (`[[ obj ]]->m()`), when every definition is the same
    /// `ns::class::m`.
    fn unique_method(&self, m: &str) -> Option<(String, DefId)> {
        let defs = self.tables.methods.get(m)?;
        let first = *defs.first()?;
        let name = self.qualified_name(first);
        defs.iter()
            .all(|&d| self.qualified_name(d) == name)
            .then_some((name, first))
    }

    /// The script function a bare call `name()` (identifier case) from file `f` reaches,
    /// builtins aside: the file, its namespace (same VM), and in `builtin-first` games the
    /// files it imports.
    pub fn visible_function(&self, f: FileId, name: &str) -> Option<DefId> {
        let facts = &self.file(f).facts;
        if self.namespace_first(f) {
            let ns = &facts.namespace;
            return self
                .local(f, name)
                .filter(|&d| self.def(d).ns == *ns)
                .or_else(|| self.namespace_fn(f, ns, name));
        }
        if let Some(d) = self.local(f, name) {
            return Some(d);
        }
        if self.profile_of(f).config.resolution == Resolution::Namespace {
            if let Some(d) = self.namespace_fn(f, &facts.namespace, name) {
                return Some(d);
            }
        }
        self.resolution(f)
            .scope
            .iter()
            .find_map(|&u| self.local(u, name))
    }
}
