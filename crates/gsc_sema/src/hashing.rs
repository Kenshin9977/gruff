//! Names through hashes, for the games that hash identifiers (BO3, BO4 to BO7, MWIII).
//!
//! A decompiler names what it cannot name by hash (`function_1a2b3c4d`, `#"hash_..."`); the same
//! symbol may be written by name in another file. Definitions and references are indexed by
//! [`gsc_hash::Scheme::key`] as well as by name, so both spellings meet. Names come back from the
//! hash packages ([`load_packages`]) and from the clear names of the workspace itself. See
//! `docs/hash-packages.md`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gsc_hash::{Dictionary, LoadStats, Package, Role, Scheme, MASK60};
use gsc_syntax::SyntaxKind;

use crate::facts::{Def, Target};
use crate::vfs::FileMeta;
use crate::workspace::{FileId, Workspace};

/// The hash key of a function name.
pub(crate) fn fn_key(scheme: Option<&Scheme>, name: &str) -> Option<u64> {
    scheme?.key(Role::Function, name)
}

/// `(namespace key, short-name key)` of a definition.
pub(crate) fn def_keys(s: &Scheme, d: &Def) -> (u64, u64) {
    (
        s.key(Role::Namespace, &d.ns).unwrap_or(0),
        s.key(Role::Function, d.short_name()).unwrap_or(0),
    )
}

/// The key of `ns::n` in the hashed function table.
pub(crate) fn ns_fn_key(s: &Scheme, ns: &str, n: &str) -> Option<(u64, u64)> {
    Some((s.key(Role::Namespace, ns)?, s.key(Role::Function, n)?))
}

/// Script-name keys (60 bits) a file answers to: the value of a `script_<hex>` name, or the hash
/// of its clear path with its extension.
pub(crate) fn script_keys_of_file(s: &Scheme, meta: &FileMeta) -> Vec<u64> {
    let stem = meta.stem();
    if let Some(v) = s.spelled(Role::Script, stem) {
        return vec![v & MASK60];
    }
    let Some(f) = s.family(Role::Script) else {
        return Vec::new();
    };
    let ext = meta
        .rel
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    let mut out = vec![f.hash(&format!("{}.{ext}", meta.key)) & MASK60];
    out.push(f.hash(&meta.key) & MASK60);
    out
}

/// Script-name keys an import target may stand for (`script_<hex>`, or a clear path hashed with
/// `.gsc`, `.csc` and without extension).
pub(crate) fn script_keys_of_target(s: &Scheme, target: &str) -> Vec<u64> {
    let t = target.trim().replace('\\', "/").to_ascii_lowercase();
    let t = crate::vfs::strip_script_ext(&t);
    let stem = t.rsplit('/').next().unwrap_or(t);
    if let Some(v) = s.spelled(Role::Script, stem) {
        return vec![v & MASK60];
    }
    let Some(f) = s.family(Role::Script) else {
        return Vec::new();
    };
    [".gsc", ".csc", ""]
        .iter()
        .map(|e| f.hash(&format!("{t}{e}")) & MASK60)
        .collect()
}

/// What [`load_packages`] read.
#[derive(Debug, Default)]
pub struct PackageReport {
    /// Package files read, with their entry count.
    pub files: Vec<(PathBuf, usize)>,
    pub stats: LoadStats,
    /// `path: error` for unreadable or malformed files.
    pub errors: Vec<String>,
}

/// The files of a package path: the file itself, or every `.txt`, `.tsv` and `.names` file of a
/// directory (recursively, sorted).
pub fn package_files(path: &Path) -> std::io::Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut out = Vec::new();
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir)? {
            let p = e?.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| matches!(x.to_ascii_lowercase().as_str(), "txt" | "tsv" | "names"))
            {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Reads hash packages (files or directories) into the dictionary of `scheme`'s game. Entries
/// for other games are skipped; unreadable files are reported, not fatal.
pub fn load_packages(scheme: Scheme, paths: &[PathBuf]) -> (Dictionary, PackageReport) {
    let mut d = Dictionary::new(scheme);
    let mut rep = PackageReport::default();
    for p in paths {
        let files = match package_files(p) {
            Ok(f) => f,
            Err(e) => {
                rep.errors.push(format!("{}: {e}", p.display()));
                continue;
            }
        };
        for f in files {
            let text = match std::fs::read(&f) {
                Ok(b) => String::from_utf8_lossy(&b).into_owned(),
                Err(e) => {
                    rep.errors.push(format!("{}: {e}", f.display()));
                    continue;
                }
            };
            match Package::parse(&text) {
                Ok(pkg) => {
                    rep.stats += d.add_package(&pkg);
                    rep.files.push((f, pkg.len()));
                }
                Err(e) => rep.errors.push(format!("{}: {e}", f.display())),
            }
        }
    }
    (d, rep)
}

/// Where a name came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NameSource {
    /// Written in clear.
    Clear,
    /// A hash a package names.
    Package,
    /// A hash that a clear name of the workspace hashes to.
    Workspace,
    /// A hash nothing names.
    Unknown,
}

impl NameSource {
    /// `clear`, `package`, `workspace`, `unknown`.
    pub fn label(self) -> &'static str {
        match self {
            NameSource::Clear => "clear",
            NameSource::Package => "hash packages",
            NameSource::Workspace => "workspace",
            NameSource::Unknown => "unknown",
        }
    }
}

/// Identifier counts per role and source, from [`Workspace::hash_coverage`].
#[derive(Clone, Debug, Default)]
pub struct Coverage {
    /// Identifier tokens written in clear (any role).
    pub clear_idents: usize,
    /// `#"..."` strings written in clear.
    pub clear_strings: usize,
    /// Hashed occurrences: `(role, source)` to count (`source` is never `Clear`).
    pub counts: std::collections::BTreeMap<(Role, NameSource), usize>,
    /// Distinct hashed values per role: `(all, named)`.
    pub distinct: std::collections::BTreeMap<Role, (usize, usize)>,
}

impl Coverage {
    /// Occurrences of `role` (every role for `None`) from `source`.
    pub fn count(&self, role: Option<Role>, source: NameSource) -> usize {
        self.counts
            .iter()
            .filter(|((r, s), _)| *s == source && role.is_none_or(|x| x == *r))
            .map(|(_, n)| n)
            .sum()
    }
}

impl Workspace {
    /// The hash scheme of `f`'s game, if it hashes identifiers.
    pub fn scheme_of(&self, f: FileId) -> Option<Scheme> {
        self.profile_of(f).scheme
    }

    /// Names hashed from every clear identifier of the workspace (definitions, namespaces,
    /// references, fields, event names), computed on first use until the next change.
    pub fn workspace_names(&self, f: FileId) -> Option<Arc<Dictionary>> {
        let scheme = self.scheme_of(f)?;
        Some(
            self.names_cache
                .get_or_init(|| Arc::new(self.collect_names(scheme)))
                .clone(),
        )
    }

    fn collect_names(&self, scheme: Scheme) -> Dictionary {
        let mut d = Dictionary::new(scheme);
        let add = |d: &mut Dictionary, role: Role, n: &str| {
            if n.is_empty() || scheme.spelled(role, n).is_some() {
                return;
            }
            if let Some(f) = scheme.family(role) {
                d.add_name_with(f, n);
            }
        };
        for f in self.files() {
            let facts = &self.file(f).facts;
            add(&mut d, Role::Namespace, &facts.namespace);
            for def in &facts.defs {
                add(&mut d, Role::Namespace, &def.ns);
                add(&mut d, Role::Function, def.short_name());
                if let Some(c) = def.class() {
                    add(&mut d, Role::Class, c);
                }
            }
            for r in &facts.refs {
                match &r.target {
                    Target::Bare(n) | Target::Method(n) | Target::Keyword(n) => {
                        add(&mut d, Role::Function, n)
                    }
                    Target::Qualified(q, n) => {
                        if let Some(q) = q.as_deref().filter(|q| !q.contains(['/', '\\'])) {
                            add(&mut d, Role::Namespace, q);
                        }
                        add(&mut d, Role::Function, n);
                    }
                    Target::Dynamic(_) => {}
                }
            }
            for field in &facts.fields {
                add(&mut d, Role::Var, field);
            }
            for s in &facts.strings {
                add(&mut d, Role::String, s);
            }
            for e in &facts.events {
                if let crate::events::EventName::Literal(n) = &e.name {
                    add(&mut d, Role::String, n);
                    add(&mut d, Role::Event, n);
                }
            }
        }
        d
    }

    /// The name a hashed spelling stands for (`function_1a2b` -> `foo`), and where it came from:
    /// the packages first, then the workspace's clear names. `None` for a clear name or an unknown
    /// hash.
    pub fn recover(&self, f: FileId, role: Role, written: &str) -> Option<(String, NameSource)> {
        let p = self.profile_of(f);
        let s = p.scheme?;
        let v = s.spelled(role, written)?;
        if let Some(n) = p.names.as_ref().and_then(|d| d.name(role, v)) {
            return Some((n.to_owned(), NameSource::Package));
        }
        self.workspace_names(f)?
            .name(role, v)
            .map(|n| (n.to_owned(), NameSource::Workspace))
    }

    /// An identifier as the editor shows it: `foo (#1a2b)` for a hash a name is known for,
    /// `#1a2b` for an unknown hash, the name as written otherwise.
    pub fn display_name(&self, f: FileId, role: Role, written: &str) -> String {
        let Some(s) = self.scheme_of(f) else {
            return written.to_owned();
        };
        let Some(v) = s.spelled(role, written) else {
            return written.to_owned();
        };
        let h = s.display_hash(role, v);
        match self.recover(f, role, written) {
            Some((n, _)) => format!("{n} ({h})"),
            None => h,
        }
    }

    /// [`Workspace::display_name`] over `ns::name` and `ns::class::method` (each part with its
    /// role); a path qualifier stays as written.
    pub fn display_qualified(&self, f: FileId, qualified: &str) -> String {
        if self.scheme_of(f).is_none() {
            return qualified.to_owned();
        }
        let parts: Vec<&str> = qualified.split("::").collect();
        let n = parts.len();
        parts
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let role = if i + 1 == n {
                    Role::Function
                } else if i == 0 {
                    Role::Namespace
                } else {
                    Role::Class
                };
                if p.contains(['/', '\\']) {
                    (*p).to_owned()
                } else {
                    self.display_name(f, role, p)
                }
            })
            .collect::<Vec<_>>()
            .join("::")
    }

    /// A message with every decompiler spelling in it (`function_1a2b`, `hash_...`) shown as
    /// [`Workspace::display_name`] shows it.
    pub fn display_message(&self, f: FileId, message: &str) -> String {
        if self.scheme_of(f).is_none() {
            return message.to_owned();
        }
        let mut out = String::with_capacity(message.len());
        let b = message.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let word_start = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
            if word_start && b[i].is_ascii_alphabetic() {
                let end = b[i..]
                    .iter()
                    .position(|c| !(c.is_ascii_alphanumeric() || *c == b'_'))
                    .map_or(b.len(), |n| i + n);
                let word = &message[i..end];
                match gsc_hash::parse_any_spelling(word) {
                    Some((role, _)) => out.push_str(&self.display_name(f, role, word)),
                    None => out.push_str(word),
                }
                i = end;
                continue;
            }
            let ch = message[i..].chars().next().unwrap_or(' ');
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    /// The hash of a name, as hover shows it: ``hash `#1a2b` (t10.function)``. BO3 writes its
    /// scripts in clear: there only a hashed spelling gets the line.
    pub fn hash_line(&self, f: FileId, role: Role, written: &str) -> Option<String> {
        let s = self.scheme_of(f)?;
        let fam = s.family(role)?;
        if s.game == gsc_hash::Game::T7 && s.spelled(role, written).is_none() {
            return None;
        }
        let key = s.key(role, written)?;
        Some(format!("hash `{}` ({})", s.display_hash(role, key), fam.id))
    }

    /// Counts the identifiers of the primary files by role and by where their name comes from
    /// (written in clear, named by a package, named by the workspace, unknown). Identifiers are
    /// the `IDENT` and `#"..."` tokens: a `function_`/`var_`/`namespace_`/`class_` spelling is a
    /// hash of that role, any other identifier counts as a clear var or function name.
    pub fn hash_coverage(&self) -> Coverage {
        let mut cov = Coverage::default();
        let Some(f0) = self.primary_files().next() else {
            return cov;
        };
        let Some(scheme) = self.scheme_of(f0) else {
            return cov;
        };
        let ws = self.workspace_names(f0);
        let pkg = self.profile_of(f0).names.clone();
        let mut seen: std::collections::HashMap<(Role, u64), bool> = Default::default();
        let spelled_roles = [Role::Function, Role::Var, Role::Namespace, Role::Class];
        for f in self.primary_files().collect::<Vec<_>>() {
            let data = self.file(f);
            let dialect = self.profile_of(f).dialect(data.decompiled);
            let (tokens, _) = gsc_syntax::tokenize(data.text(), dialect);
            let mut at = 0usize;
            for t in tokens {
                let text = &data.text()[at..at + t.len as usize];
                at += t.len as usize;
                let (role, value) = match t.kind {
                    SyntaxKind::IDENT => {
                        match spelled_roles
                            .iter()
                            .find_map(|&r| scheme.spelled(r, text).map(|v| (r, v)))
                        {
                            Some(x) => x,
                            None => {
                                cov.clear_idents += 1;
                                continue;
                            }
                        }
                    }
                    SyntaxKind::HASH_STRING => {
                        let inner = text.trim_start_matches('#').trim_matches('"');
                        match scheme.spelled(Role::String, inner) {
                            Some(v) => (Role::String, v),
                            None => {
                                cov.clear_strings += 1;
                                continue;
                            }
                        }
                    }
                    _ => continue,
                };
                let src = if pkg.as_ref().and_then(|d| d.name(role, value)).is_some() {
                    NameSource::Package
                } else if ws.as_ref().and_then(|d| d.name(role, value)).is_some() {
                    NameSource::Workspace
                } else {
                    NameSource::Unknown
                };
                *cov.counts.entry((role, src)).or_default() += 1;
                let named = src != NameSource::Unknown;
                if seen.insert((role, value), named).is_none() {
                    let e = cov.distinct.entry(role).or_default();
                    e.0 += 1;
                    e.1 += usize::from(named);
                }
            }
        }
        cov
    }
}
