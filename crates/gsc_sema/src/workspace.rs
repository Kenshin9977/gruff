//! The in-memory file set: roots, profiles, parsed files with their facts, and the global name
//! tables built from the facts.
//!
//! Incrementality is map-reduce, not salsa: a file's parse and facts depend on its text alone, so
//! [`Workspace::set_text`] re-parses that one file and patches the global tables (its entries out,
//! the new ones in). Resolution results are cached per file and dropped on every change, since one
//! new definition can change what any other file's bare calls reach; re-resolving is lazy, per
//! file, on the next query.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use gsc_syntax::{is_binary, parse, GreenNode, SourceText, SyntaxError, SyntaxNode};

use crate::facts::{self, DefKind, FileFacts};
use crate::modes::Mode;
use crate::par::par_map;
use crate::profile::{Decompiled, Profile};
use crate::resolve::FileResolution;
use crate::vfs::{collect_files, FileMeta, Root, RootId};

/// A file of the workspace. Ids are dense in load order and stay valid until the file is removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u32);

impl FileId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A definition: its file and its index in the file's facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DefId {
    pub file: FileId,
    pub index: u32,
}

/// One parsed file.
#[derive(Clone, Debug)]
pub struct FileData {
    pub meta: FileMeta,
    pub source: SourceText,
    /// Read as decompiler output.
    pub decompiled: bool,
    /// The game mode it runs in (`gruff.toml`, else its path); `None`: unknown or shared.
    pub mode: Option<Mode>,
    /// `None` once [`Workspace::drop_library_trees`] let it go: [`FileData::syntax`] re-parses.
    green: Option<GreenNode>,
    /// The profile the file was parsed with, to parse it again.
    profile: Arc<Profile>,
    pub errors: Vec<SyntaxError>,
    pub facts: FileFacts,
    /// Functions of this file by name: the first definition (kind function only).
    pub(crate) local_fns: HashMap<String, u32>,
    /// The same by hash, in games that hash identifiers ([`gsc_hash::Scheme::key`]).
    pub(crate) local_fns_h: HashMap<u64, u32>,
}

impl FileData {
    /// The root of the file's syntax tree: a new cursor over the shared green tree, or a fresh
    /// parse (not kept) when the tree was dropped.
    pub fn syntax(&self) -> SyntaxNode {
        match &self.green {
            Some(g) => SyntaxNode::new_root(g.clone()),
            None => parse(&self.source.text, self.profile.dialect(self.decompiled)).syntax(),
        }
    }

    /// Is the syntax tree in memory?
    pub fn has_tree(&self) -> bool {
        self.green.is_some()
    }

    pub fn text(&self) -> &str {
        &self.source.text
    }
}

/// Global tables, patched per file. Every list is sorted by file id, then definition index.
#[derive(Debug, Default)]
pub(crate) struct Tables {
    /// File key to files.
    pub by_key: HashMap<String, Vec<FileId>>,
    /// Functions and methods by short name.
    pub callables: HashMap<String, Vec<DefId>>,
    /// Functions (not methods) by `(namespace, name)`.
    pub ns_fns: HashMap<(String, String), Vec<DefId>>,
    /// Class definitions by name.
    pub classes: HashMap<String, Vec<DefId>>,
    /// Methods by short name.
    pub methods: HashMap<String, Vec<DefId>>,
    /// Macros by lower-case name.
    pub macros: HashMap<String, Vec<DefId>>,
    /// Hashed games: functions by `(namespace hash, function hash)`.
    pub ns_fns_h: HashMap<(u64, u64), Vec<DefId>>,
    /// Hashed games: functions and methods by short-name hash.
    pub callables_h: HashMap<u64, Vec<DefId>>,
    /// Hashed games: files by script-name hash (60 bits), from `script_<hex>` names and from the
    /// hash of clear paths.
    pub by_script: HashMap<u64, Vec<FileId>>,
    /// Event sites and event-like strings.
    pub events: crate::events::EventTables,
}

fn insert<K: std::hash::Hash + Eq>(map: &mut HashMap<K, Vec<DefId>>, k: K, d: DefId) {
    let v = map.entry(k).or_default();
    let at = v.partition_point(|x| *x <= d);
    v.insert(at, d);
}

fn insert_file(map: &mut HashMap<u64, Vec<FileId>>, k: u64, f: FileId) {
    let v = map.entry(k).or_default();
    let at = v.partition_point(|x| *x <= f);
    if v.get(at) != Some(&f) {
        v.insert(at, f);
    }
}

fn remove<K: std::hash::Hash + Eq>(map: &mut HashMap<K, Vec<DefId>>, k: &K, file: FileId) {
    if let Some(v) = map.get_mut(k) {
        v.retain(|d| d.file != file);
        if v.is_empty() {
            map.remove(k);
        }
    }
}

/// What loading found besides the files.
#[derive(Debug, Default)]
pub struct LoadReport {
    /// `path: error` for unreadable files and roots.
    pub errors: Vec<String>,
    /// Compiled (binary) scripts skipped.
    pub binary: usize,
}

/// The workspace: roots, profiles, files and global tables.
pub struct Workspace {
    roots: Vec<Root>,
    profiles: Vec<Arc<Profile>>,
    decompiled: Decompiled,
    files: Vec<Option<FileData>>,
    by_path: HashMap<PathBuf, FileId>,
    pub(crate) tables: Tables,
    pub(crate) cache: Vec<OnceLock<Arc<FileResolution>>>,
    /// Event-graph indexes derived from every file (dropped on every change).
    pub(crate) event_cache: crate::events::Caches,
    /// The workspace's clear names by hash ([`Workspace::workspace_names`], dropped on every change).
    pub(crate) names_cache: OnceLock<Arc<gsc_hash::Dictionary>>,
    jobs: usize,
}

impl Workspace {
    /// An empty workspace whose files are read as `profile` (profile 0).
    pub fn new(profile: Profile) -> Workspace {
        Workspace {
            roots: Vec::new(),
            profiles: vec![Arc::new(profile)],
            decompiled: Decompiled::Auto,
            files: Vec::new(),
            by_path: HashMap::new(),
            tables: Tables::default(),
            cache: Vec::new(),
            event_cache: Default::default(),
            names_cache: OnceLock::new(),
            jobs: crate::par::default_jobs(),
        }
    }

    pub fn with_decompiled(mut self, d: Decompiled) -> Workspace {
        self.decompiled = d;
        self
    }

    /// Worker threads for loading and batch queries.
    pub fn with_jobs(mut self, jobs: usize) -> Workspace {
        self.jobs = jobs.max(1);
        self
    }

    pub fn jobs(&self) -> usize {
        self.jobs
    }

    /// Add another game profile; roots refer to it by the returned index.
    pub fn add_profile(&mut self, p: Profile) -> usize {
        self.profiles.push(Arc::new(p));
        self.profiles.len() - 1
    }

    pub fn add_root(&mut self, root: Root) -> RootId {
        self.roots.push(root);
        self.roots.len() - 1
    }

    pub fn roots(&self) -> &[Root] {
        &self.roots
    }

    pub fn root(&self, r: RootId) -> &Root {
        &self.roots[r]
    }

    pub fn profile(&self, i: usize) -> &Profile {
        &self.profiles[i]
    }

    /// The game mode file `f` runs in, if known.
    pub fn mode_of(&self, f: FileId) -> Option<Mode> {
        self.file(f).mode
    }

    /// The profile a file is read as.
    pub fn profile_of(&self, f: FileId) -> &Profile {
        let root = self.file(f).meta.root;
        &self.profiles[self.roots.get(root).map_or(0, |r| r.profile)]
    }

    /// The script files of every root, in root order then path order.
    pub fn discover(&self) -> (Vec<(RootId, PathBuf)>, Vec<String>) {
        let mut out = Vec::new();
        let mut errors = Vec::new();
        for (ri, root) in self.roots.iter().enumerate() {
            let mut files = Vec::new();
            match collect_files(&root.path, &mut files) {
                Ok(()) => out.extend(files.into_iter().map(|f| (ri, f))),
                Err(e) => errors.push(format!("{}: {e}", root.path.display())),
            }
        }
        (out, errors)
    }

    /// Discover, read and parse every file of every root, on the workspace's threads. Files
    /// already loaded are kept.
    pub fn load(&mut self) -> LoadReport {
        let (found, errors) = self.discover();
        let mut report = LoadReport {
            errors,
            ..LoadReport::default()
        };
        let read = par_map(&found, self.jobs, |(root, path)| {
            match std::fs::read(path) {
                Ok(bytes) if is_binary(&bytes) => Ok(None),
                Ok(bytes) => {
                    let rel = self.roots[*root].relative(path);
                    let meta = FileMeta::new(path.clone(), rel, *root, self.ci_of_root(*root));
                    Ok(Some(self.analyze(meta, SourceText::decode(&bytes))))
                }
                Err(e) => Err(format!("{}: {e}", path.display())),
            }
        });
        for r in read {
            match r {
                Ok(Some(data)) => {
                    self.push(data);
                }
                Ok(None) => report.binary += 1,
                Err(e) => report.errors.push(e),
            }
        }
        self.invalidate();
        report
    }

    fn ci_of_root(&self, root: RootId) -> bool {
        let p = self.roots.get(root).map_or(0, |r| r.profile);
        self.profiles[p].config.case_insensitive
    }

    fn profile_of_root(&self, root: RootId) -> &Profile {
        &self.profiles[self.roots.get(root).map_or(0, |r| r.profile)]
    }

    /// Parse a file and extract its facts (no workspace state touched).
    fn analyze(&self, meta: FileMeta, source: SourceText) -> FileData {
        let profile = self.profile_of_root(meta.root);
        let decompiled = self.decompiled.decide(&source.text);
        let p = parse(&source.text, profile.dialect(decompiled));
        let root = p.syntax();
        let facts = facts::extract(&root, &meta.key, &profile.config);
        let mut local_fns = HashMap::new();
        let mut local_fns_h = HashMap::new();
        for (i, d) in facts.defs.iter().enumerate() {
            if d.kind == DefKind::Function {
                local_fns.entry(d.name.clone()).or_insert(i as u32);
                if let Some(k) = crate::hashing::fn_key(profile.scheme.as_ref(), &d.name) {
                    local_fns_h.entry(k).or_insert(i as u32);
                }
            }
        }
        let library = self.roots.get(meta.root).is_some_and(|r| r.library);
        let mode = (!library)
            .then(|| profile.modes.mode_of(&meta.rel))
            .flatten()
            .unwrap_or_else(|| crate::modes::mode_of_path(&profile.dialect.id, &meta.rel));
        let profile = self.profiles[self.roots.get(meta.root).map_or(0, |r| r.profile)].clone();
        FileData {
            meta,
            decompiled,
            mode,
            green: Some(p.green().clone()),
            profile,
            errors: p.errors().to_vec(),
            source,
            facts,
            local_fns,
            local_fns_h,
        }
    }

    fn push(&mut self, data: FileData) -> FileId {
        let id = FileId(self.files.len() as u32);
        self.by_path.insert(data.meta.path.clone(), id);
        self.files.push(Some(data));
        self.index_file(id);
        id
    }

    /// Add a file from memory (an unsaved editor buffer, a test fixture). `rel` is its path
    /// relative to `root`.
    pub fn add_file(&mut self, root: RootId, path: PathBuf, rel: &str, text: &str) -> FileId {
        if let Some(&id) = self.by_path.get(&path) {
            self.set_text(id, text);
            return id;
        }
        let meta = FileMeta::new(path, rel.to_owned(), root, self.ci_of_root(root));
        let data = self.analyze(meta, SourceText::decode(text.as_bytes()));
        let id = self.push(data);
        self.invalidate();
        id
    }

    /// Replace a file's text: re-parse it, re-extract its facts and patch the global tables.
    pub fn set_text(&mut self, id: FileId, text: &str) {
        let Some(old) = self.files[id.index()].take() else {
            return;
        };
        self.unindex_file(id, &old);
        let mut source = old.source;
        source.text = text.to_owned();
        let data = self.analyze(old.meta, source);
        self.files[id.index()] = Some(data);
        self.index_file(id);
        // The workspace's clear names only name hashes for display: an edit keeps them (a name
        // typed since counts after the next load), sparing a rebuild per keystroke.
        let names = self.names_cache.take();
        self.invalidate();
        if let Some(n) = names {
            let _ = self.names_cache.set(n);
        }
    }

    /// Forget a file. Its id is not reused.
    pub fn remove_file(&mut self, id: FileId) {
        if let Some(old) = self.files[id.index()].take() {
            self.unindex_file(id, &old);
            self.by_path.remove(&old.meta.path);
            self.invalidate();
        }
    }

    /// Free the syntax trees of library files, most of a game's memory: their facts stay, and
    /// [`FileData::syntax`] parses one again when a query needs it. A changed file gets its
    /// tree back. Returns how many trees were dropped.
    pub fn drop_library_trees(&mut self) -> usize {
        let mut trees = Vec::new();
        for f in self.files.iter_mut().flatten() {
            if self.roots.get(f.meta.root).is_some_and(|r| r.library) {
                trees.extend(f.green.take());
            }
        }
        let n = trees.len();
        // Freeing millions of nodes takes a while: off the caller's thread.
        if n > 0 {
            std::thread::spawn(move || drop(trees));
        }
        n
    }

    /// Change how a file is printed (tests, embedders with their own path conventions).
    pub fn set_display(&mut self, id: FileId, display: String) {
        if let Some(f) = self.files[id.index()].as_mut() {
            f.meta.display = display;
        }
    }

    fn invalidate(&mut self) {
        self.event_cache = Default::default();
        self.names_cache = OnceLock::new();
        let fresh = (0..self.files.len()).map(|_| OnceLock::new()).collect();
        let old = std::mem::replace(&mut self.cache, fresh);
        // Freeing every file's resolution costs tens of milliseconds on a large workspace:
        // do it off the editing thread.
        if old.iter().any(|c| c.get().is_some()) {
            std::thread::spawn(move || drop(old));
        }
    }

    fn index_file(&mut self, id: FileId) {
        let f = self.files[id.index()].as_ref().expect("a live file");
        let scheme = self.profile_of_root(f.meta.root).scheme;
        let t = &mut self.tables;
        if let Some(s) = &scheme {
            for k in crate::hashing::script_keys_of_file(s, &f.meta) {
                insert_file(&mut t.by_script, k, id);
            }
        }
        let keys = t.by_key.entry(f.meta.key.clone()).or_default();
        let at = keys.partition_point(|x| *x <= id);
        keys.insert(at, id);
        t.events.index(id, &f.facts);
        for (i, d) in f.facts.defs.iter().enumerate() {
            let def = DefId {
                file: id,
                index: i as u32,
            };
            match d.kind {
                DefKind::Function => {
                    insert(&mut t.callables, d.name.clone(), def);
                    insert(&mut t.ns_fns, (d.ns.clone(), d.name.clone()), def);
                    if let Some(s) = &scheme {
                        let (ns, n) = crate::hashing::def_keys(s, d);
                        insert(&mut t.callables_h, n, def);
                        insert(&mut t.ns_fns_h, (ns, n), def);
                    }
                }
                DefKind::Method => {
                    insert(&mut t.callables, d.short_name().to_owned(), def);
                    insert(&mut t.methods, d.short_name().to_owned(), def);
                    if let Some(s) = &scheme {
                        insert(&mut t.callables_h, crate::hashing::def_keys(s, d).1, def);
                    }
                }
                DefKind::Class => insert(&mut t.classes, d.name.clone(), def),
                DefKind::Define => insert(&mut t.macros, d.name.to_ascii_lowercase(), def),
                DefKind::Const => {}
            }
        }
    }

    fn unindex_file(&mut self, id: FileId, f: &FileData) {
        let scheme = self.profile_of_root(f.meta.root).scheme;
        let t = &mut self.tables;
        t.events.unindex(id, &f.facts);
        if let Some(s) = &scheme {
            for k in crate::hashing::script_keys_of_file(s, &f.meta) {
                if let Some(v) = t.by_script.get_mut(&k) {
                    v.retain(|x| *x != id);
                    if v.is_empty() {
                        t.by_script.remove(&k);
                    }
                }
            }
        }
        if let Some(v) = t.by_key.get_mut(&f.meta.key) {
            v.retain(|x| *x != id);
            if v.is_empty() {
                t.by_key.remove(&f.meta.key);
            }
        }
        for d in &f.facts.defs {
            match d.kind {
                DefKind::Function => {
                    remove(&mut t.callables, &d.name, id);
                    remove(&mut t.ns_fns, &(d.ns.clone(), d.name.clone()), id);
                    if let Some(s) = &scheme {
                        let (ns, n) = crate::hashing::def_keys(s, d);
                        remove(&mut t.callables_h, &n, id);
                        remove(&mut t.ns_fns_h, &(ns, n), id);
                    }
                }
                DefKind::Method => {
                    remove(&mut t.callables, &d.short_name().to_owned(), id);
                    remove(&mut t.methods, &d.short_name().to_owned(), id);
                    if let Some(s) = &scheme {
                        remove(&mut t.callables_h, &crate::hashing::def_keys(s, d).1, id);
                    }
                }
                DefKind::Class => remove(&mut t.classes, &d.name, id),
                DefKind::Define => remove(&mut t.macros, &d.name.to_ascii_lowercase(), id),
                DefKind::Const => {}
            }
        }
    }

    /// Every live file, in id order.
    pub fn files(&self) -> impl Iterator<Item = FileId> + '_ {
        self.files
            .iter()
            .enumerate()
            .filter(|(_, f)| f.is_some())
            .map(|(i, _)| FileId(i as u32))
    }

    /// The files of non-library roots: what a linter reports on.
    pub fn primary_files(&self) -> impl Iterator<Item = FileId> + '_ {
        self.files().filter(|&f| !self.is_library(f))
    }

    /// Is file `f` in a library root (read for resolution, not reported on)?
    pub fn is_library(&self, f: FileId) -> bool {
        self.roots
            .get(self.file(f).meta.root)
            .is_some_and(|r| r.library)
    }

    pub fn len(&self) -> usize {
        self.files.iter().filter(|f| f.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A live file. Panics on a removed id.
    pub fn file(&self, id: FileId) -> &FileData {
        self.files[id.index()].as_ref().expect("a live file id")
    }

    pub fn get(&self, id: FileId) -> Option<&FileData> {
        self.files.get(id.index()).and_then(Option::as_ref)
    }

    pub fn file_id(&self, path: &Path) -> Option<FileId> {
        self.by_path.get(path).copied()
    }

    pub fn def(&self, d: DefId) -> &facts::Def {
        &self.file(d.file).facts.defs[d.index as usize]
    }

    /// Every file whose key is `key`.
    pub fn files_with_key(&self, key: &str) -> &[FileId] {
        self.tables.by_key.get(key).map_or(&[], Vec::as_slice)
    }

    /// Functions and methods named `name` (short name, identifier case) anywhere.
    pub fn callables_named(&self, name: &str) -> &[DefId] {
        self.tables.callables.get(name).map_or(&[], Vec::as_slice)
    }

    /// Functions and methods `name` reaches from file `f`: by name, else, in a game that hashes
    /// identifiers, by hash (a call written `foo` reaches `function_<hash of foo>`, and the
    /// reverse).
    pub fn callables_matching(&self, f: FileId, name: &str) -> &[DefId] {
        let by_name = self.callables_named(name);
        if !by_name.is_empty() {
            return by_name;
        }
        crate::hashing::fn_key(self.profile_of(f).scheme.as_ref(), name)
            .and_then(|k| self.tables.callables_h.get(&k))
            .map_or(&[], Vec::as_slice)
    }

    /// Macros spelled `name` exactly (macros are case-sensitive).
    pub fn macros_spelled<'a>(&'a self, name: &'a str) -> impl Iterator<Item = DefId> + 'a {
        self.tables
            .macros
            .get(&name.to_ascii_lowercase())
            .into_iter()
            .flatten()
            .copied()
            .filter(move |&d| self.def(d).name == name)
    }

    /// Every definition, file by file.
    pub fn all_defs(&self) -> impl Iterator<Item = DefId> + '_ {
        self.files().flat_map(move |f| {
            (0..self.file(f).facts.defs.len()).map(move |i| DefId {
                file: f,
                index: i as u32,
            })
        })
    }
}
