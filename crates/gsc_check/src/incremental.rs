//! Incremental checking for the language server: a [`Session`] keeps the last check and, on
//! [`Session::update`], re-checks only the files whose results may have changed. Results always
//! equal a fresh [`crate::check`] (asserted by `tests/incremental.rs`).
//!
//! Cross-file inputs of a file's diagnostics: other files' exported names (re-check the files
//! mentioning a changed name; imports, namespace, key or class changes re-check everything),
//! the yield bits of the nodes its calls reach, and the fields some file writes.
//! See `docs/lsp.md` ("Incremental diagnostics") for the full argument.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use gsc_sema::{par_map, DefId, DefKind, EventClass, EventName, FileId, Target, Workspace};
use xxhash_rust::xxh3::{xxh3_64, Xxh3};

use crate::workspace::{summarize, CheckFacts, Edge, FileEdges, Summaries, DEAD};
use crate::{check_file, file_check_facts, Diagnostic, FileResult, Options};

/// What an update did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Update {
    /// Files whose text changed since the last update, new files included.
    pub changed: Vec<FileId>,
    /// Files gone from the workspace.
    pub removed: Vec<FileId>,
    /// Primary files whose diagnostics were recomputed, in id order.
    pub rechecked: Vec<FileId>,
    /// A definition changed: the summaries were renumbered.
    pub interface_changed: bool,
    /// Every primary file was re-checked (first run, structural change).
    pub full: bool,
    /// How the summaries were updated: `full`, `renumbered`, `in place` (the changed files'
    /// nodes moved to the end) or `cone` (no definition changed: the callers of the changed
    /// nodes solved again).
    pub summaries: &'static str,
    /// Milliseconds per phase: `hash`, `facts`, `summaries`, `select`, `rules`.
    pub phases: Vec<(&'static str, f64)>,
}

impl Update {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.removed.is_empty() && self.rechecked.is_empty()
    }
}

/// A definition as other files see it: a hash of `(kind, ns, name, params, flags, is a summary
/// node)` (and a macro's body), and the [`word_hash`] of its short name.
type Entry = (u64, u64);

/// What other files can see of a file.
#[derive(Debug, PartialEq, Eq)]
struct Interface {
    /// File key, namespace, imports, classes and whether loading the file has an effect.
    structural: u64,
    /// Every definition, in order.
    entries: Vec<Entry>,
}

fn kind_code(k: DefKind) -> u8 {
    match k {
        DefKind::Function => 0,
        DefKind::Method => 1,
        DefKind::Class => 2,
        DefKind::Define => 3,
        DefKind::Const => 4,
    }
}

fn interface(ws: &Workspace, f: FileId, c: &CheckFacts) -> Interface {
    let data = ws.file(f);
    let facts = &data.facts;
    let mut h = Xxh3::new();
    data.meta.key.hash(&mut h);
    facts.namespace.hash(&mut h);
    for i in &facts.imports {
        (i.directive, &i.target).hash(&mut h);
    }
    for d in facts.defs.iter().filter(|d| d.kind == DefKind::Class) {
        (&d.ns, &d.name, &d.params).hash(&mut h);
    }
    // Whether loading the file does something on its own (I002 keeps an import of it): an
    // engine-called function or a file-scope macro call.
    let load_effect = facts.defs.iter().any(|d| {
        d.flags
            .split(',')
            .any(|s| s == "autoexec" || s == "codecall" || s.starts_with("event_handler"))
    }) || (data.has_tree()
        && data
            .syntax()
            .children()
            .any(|n| n.kind() == gsc_syntax::SyntaxKind::MACRO_CALL_ITEM));
    load_effect.hash(&mut h);
    let nodes: HashSet<u32> = c
        .functions
        .iter()
        .chain(&c.macros)
        .map(|(i, _)| *i)
        .collect();
    Interface {
        structural: h.finish(),
        entries: facts
            .defs
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let mut h = Xxh3::new();
                let node = nodes.contains(&(i as u32));
                (kind_code(d.kind), &d.ns, &d.name, &d.params, &d.flags, node).hash(&mut h);
                // A macro's body: the names it holds count as uses (I002, U001, U002).
                if d.kind == DefKind::Define {
                    d.doc.hash(&mut h);
                }
                (h.finish(), word_hash(d.short_name()))
            })
            .collect(),
    }
}

/// Kept for the session's lifetime: no spare capacity, fields once each.
fn shrink(c: &mut CheckFacts) {
    for (_, y) in c.functions.iter_mut().chain(c.macros.iter_mut()) {
        y.calls.shrink_to_fit();
    }
    c.functions.shrink_to_fit();
    c.macros.shrink_to_fit();
    c.fields_written.sort_unstable();
    c.fields_written.dedup();
    c.fields_written.shrink_to_fit();
}

fn word_hash(w: &str) -> u64 {
    xxh3_64(w.to_ascii_lowercase().as_bytes())
}

/// The lower-case short name of an interface entry, hashed as a word.
fn entry_word(e: &Entry) -> u64 {
    e.1
}

/// Every word (`[A-Za-z0-9_]+`, not all digits) of `text`, lower case, hashed, sorted and
/// unique.
fn words(text: &str) -> Vec<u64> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_alphanumeric() || b[i] == b'_' {
            let s = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            // Numbers name nothing (event names made of digits are handled apart).
            if !b[s..i].iter().all(u8::is_ascii_digit) {
                out.push(word_hash(&text[s..i]));
            }
        } else {
            i += 1;
        }
    }
    out.sort_unstable();
    out.dedup();
    out.shrink_to_fit();
    out
}

/// Words of the entries one interface has and the other has not.
fn changed_names(old: &Interface, new: &Interface, out: &mut HashSet<u64>) {
    let mut count: HashMap<&Entry, i32> = HashMap::new();
    for e in &old.entries {
        *count.entry(e).or_default() += 1;
    }
    for e in &new.entries {
        *count.entry(e).or_default() -= 1;
    }
    out.extend(
        count
            .into_iter()
            .filter(|(_, n)| *n != 0)
            .map(|(e, _)| entry_word(e)),
    );
}

/// What a file adds to the event tables and the thread-start index, without positions; each
/// list sorted.
#[derive(Debug, Default, PartialEq, Eq)]
struct Events {
    /// `(0 literal emit | 1 literal wait or endon | 2 pattern or dynamic, lower name, details)`.
    sites: Vec<(u8, String, u64)>,
    /// Event-like strings used elsewhere than as an event name.
    strings: Vec<String>,
    /// `(callee word, kind and receiver)` of every reference.
    refs: Vec<(u64, u64)>,
}

fn events_of(ws: &Workspace, f: FileId) -> Events {
    let data = ws.file(f);
    let facts = &data.facts;
    let mut sites: Vec<(u8, String, u64)> = facts
        .events
        .iter()
        .map(|s| {
            let mut h = Xxh3::new();
            (&s.op, s.class as u8, s.receiver, s.argc, data.meta.vm).hash(&mut h);
            let tag = match (&s.name, s.class) {
                (EventName::Literal(_), EventClass::Emit) => 0,
                (EventName::Literal(_), _) => 1,
                _ => 2,
            };
            (tag, s.name.render().to_ascii_lowercase(), h.finish())
        })
        .collect();
    sites.sort_unstable();
    let mut strings = facts.strings.clone();
    strings.sort_unstable();
    let mut refs: Vec<(u64, u64)> = facts
        .refs
        .iter()
        .filter_map(|r| {
            let name = match &r.target {
                Target::Bare(n) | Target::Qualified(_, n) | Target::Method(n) => n,
                Target::Keyword(_) | Target::Dynamic(_) => return None,
            };
            Some((
                word_hash(name),
                xxh3_64(format!("{}|{}", r.kind, r.receiver).as_bytes()),
            ))
        })
        .collect();
    refs.sort_unstable();
    Events {
        sites,
        strings,
        refs,
    }
}

/// The items of two sorted lists that only one of them has (as multisets).
fn sym_diff<'a, T: Ord>(a: &'a [T], b: &'a [T]) -> Vec<&'a T> {
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < a.len() || j < b.len() {
        match (a.get(i), b.get(j)) {
            (Some(x), Some(y)) if x == y => {
                i += 1;
                j += 1;
            }
            (Some(x), Some(y)) if x < y => {
                out.push(x);
                i += 1;
            }
            (Some(_), Some(y)) => {
                out.push(y);
                j += 1;
            }
            (Some(x), None) => {
                out.push(x);
                i += 1;
            }
            (None, Some(y)) => {
                out.push(y);
                j += 1;
            }
            (None, None) => break,
        }
    }
    out
}

/// Every word of an event name, hashed. False when a word is all digits, which [`words`]
/// leaves out: then no file can be told apart.
fn name_words(name: &str, out: &mut HashSet<u64>) -> bool {
    let mut ok = true;
    for w in name
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
    {
        ok &= !w.bytes().all(|b| b.is_ascii_digit());
        out.insert(word_hash(w));
    }
    ok
}

/// What changed between two files' event contributions: words of the names, the emitted names
/// (for the misspelling suggestions), and whether a pattern or dynamic site changed.
#[derive(Default)]
struct EventChange {
    emitted: Vec<String>,
    patterns: bool,
}

fn event_change(old: &Events, new: &Events, names: &mut HashSet<u64>, out: &mut EventChange) {
    for (tag, name, _) in sym_diff(&old.sites, &new.sites) {
        match tag {
            0 => {
                out.patterns |= !name_words(name, names);
                out.emitted.push(name.clone());
            }
            1 => out.patterns |= !name_words(name, names),
            _ => out.patterns = true,
        }
    }
    for s in sym_diff(&old.strings, &new.strings) {
        out.patterns |= !name_words(s, names);
    }
    for (w, _) in sym_diff(&old.refs, &new.refs) {
        names.insert(*w);
    }
}

/// Old to new ids of the definitions of changed files, matched by entry (the n-th equal
/// entry to the n-th).
fn remap_defs<'a>(
    old: &HashMap<FileId, Vec<Entry>>,
    new: impl Fn(FileId) -> &'a Vec<Entry>,
) -> HashMap<DefId, DefId> {
    let mut out = HashMap::new();
    for (&f, old_entries) in old {
        let mut by_entry: HashMap<&Entry, std::collections::VecDeque<u32>> = HashMap::new();
        for (i, e) in old_entries.iter().enumerate() {
            by_entry.entry(e).or_default().push_back(i as u32);
        }
        for (j, e) in new(f).iter().enumerate() {
            if let Some(i) = by_entry.get_mut(e).and_then(|q| q.pop_front()) {
                out.insert(
                    DefId { file: f, index: i },
                    DefId {
                        file: f,
                        index: j as u32,
                    },
                );
            }
        }
    }
    out
}

struct State {
    text: u64,
    iface: Interface,
    facts: CheckFacts,
    events: Events,
    /// [`words`] of the text.
    words: Vec<u64>,
    /// The edges of the file's summary nodes, in the current numbering.
    edges: Arc<FileEdges>,
    /// The index of the file's first node.
    start: usize,
    /// For files of non-library roots.
    primary: Option<Primary>,
}

#[derive(Default)]
struct Primary {
    /// The nodes the file's sync calls reach, sorted: what its loops' summaries consult.
    reach: Vec<DefId>,
    /// Lower-case names the file waits for or ends on, and whether it has event sites.
    waits: Vec<String>,
    has_events: bool,
    diags: Vec<Diagnostic>,
}

/// The cached results of the last check of a workspace.
#[derive(Default)]
pub struct Session {
    files: HashMap<FileId, State>,
    sums: Option<Summaries>,
    /// Callers of each summary node (one entry per edge).
    rdeps: Vec<Vec<u32>>,
    /// How many files write each field.
    fields: HashMap<String, u32>,
}

/// The nodes `f`'s sync calls reach as code outside any function would (the file's namespace,
/// no class): a superset of what any loop of the file asks [`Summaries::yields`].
fn reach(ws: &Workspace, s: &Summaries, f: FileId) -> Vec<DefId> {
    let data = ws.file(f);
    let ns = &data.facts.namespace;
    let y = summarize(&data.syntax());
    let mut out: Vec<DefId> = Vec::new();
    for t in &y.calls {
        out.extend(
            s.edge(ws, f, ns, None, t)
                .targets()
                .iter()
                .map(|&i| s.keys[i]),
        );
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// `edges` with every target renumbered, `None` when a target has no new number.
fn remap_edges(edges: &FileEdges, to_new: &dyn Fn(usize) -> Option<usize>) -> Option<FileEdges> {
    edges
        .iter()
        .map(|es| {
            es.iter()
                .map(|e| match e {
                    Edge::Known(b) => Some(Edge::Known(*b)),
                    Edge::One(t) => to_new(*t).map(Edge::One),
                    Edge::Any(ts) => ts
                        .iter()
                        .map(|&t| to_new(t))
                        .collect::<Option<Vec<usize>>>()
                        .map(Edge::Any),
                })
                .collect()
        })
        .collect()
}

fn any_nodes(e: &Edge) -> Option<&[usize]> {
    Some(e.targets())
}

fn intersects<T: Hash + Eq>(items: &[T], set: &HashSet<T>) -> bool {
    !set.is_empty() && items.iter().any(|x| set.contains(x))
}

impl Session {
    pub fn new() -> Session {
        Session::default()
    }

    /// Bring the results up to date with the workspace.
    pub fn update(&mut self, ws: &Workspace, opts: &Options) -> Update {
        let jobs = ws.jobs();
        let mut lap = std::time::Instant::now();
        let mut phases = Vec::new();
        let mut phase = |name: &'static str| {
            phases.push((name, lap.elapsed().as_secs_f64() * 1000.0));
            lap = std::time::Instant::now();
        };
        let live: Vec<FileId> = ws.files().collect();
        let hashes = par_map(&live, jobs, |&f| xxh3_64(ws.file(f).text().as_bytes()));
        let changed: Vec<(FileId, u64)> = live
            .iter()
            .zip(hashes)
            .filter(|(f, h)| self.files.get(f).is_none_or(|s| s.text != *h))
            .map(|(f, h)| (*f, h))
            .collect();
        let live_set: HashSet<FileId> = live.iter().copied().collect();
        let mut removed: Vec<FileId> = self
            .files
            .keys()
            .filter(|f| !live_set.contains(f))
            .copied()
            .collect();
        removed.sort_unstable();
        let mut up = Update {
            changed: changed.iter().map(|(f, _)| *f).collect(),
            removed: removed.clone(),
            ..Update::default()
        };
        if changed.is_empty() && removed.is_empty() && self.sums.is_some() {
            return up;
        }
        phase("hash");

        // The changed files' facts, and what other files can see of them.
        let fresh = par_map(&changed, jobs, |&(f, h)| {
            let mut c = file_check_facts(ws, f);
            shrink(&mut c);
            let i = interface(ws, f, &c);
            (f, h, c, i, events_of(ws, f), words(ws.file(f).text()))
        });
        let mut full = self.sums.is_none() || !removed.is_empty();
        let mut iface_changed = full;
        let mut names: HashSet<u64> = HashSet::new();
        // Files whose definitions changed (or went), with their old definitions.
        let mut touched: HashSet<FileId> = removed.iter().copied().collect();
        let mut old_entries: HashMap<FileId, Vec<Entry>> = HashMap::new();
        let mut ev = EventChange::default();
        for (f, _, _, i, e, _) in &fresh {
            if let Some(old) = self.files.get(f) {
                event_change(&old.events, e, &mut names, &mut ev);
            }
            match self.files.get(f) {
                None => full = true,
                Some(old) if old.iface.structural != i.structural => full = true,
                Some(old) if old.iface != *i => {
                    iface_changed = true;
                    changed_names(&old.iface, i, &mut names);
                    touched.insert(*f);
                    old_entries.insert(*f, old.iface.entries.clone());
                }
                Some(_) => {}
            }
        }
        iface_changed |= full;
        phase("facts");

        // Fields written: counts, and the names whose status may have flipped.
        let mut flipped: HashSet<String> = HashSet::new();
        {
            let fields = &mut self.fields;
            let mut bump = |c: &CheckFacts, up: bool| {
                let uniq: HashSet<&String> = c.fields_written.iter().collect();
                for name in uniq {
                    let n = fields.entry(name.clone()).or_default();
                    let before = *n > 0;
                    *n = if up { *n + 1 } else { n.saturating_sub(1) };
                    if before != (*n > 0) {
                        flipped.insert(name.clone());
                    }
                }
            };
            for f in &removed {
                if let Some(s) = self.files.get(f) {
                    bump(&s.facts, false);
                }
            }
            for (f, _, c, _, _, _) in &fresh {
                if let Some(s) = self.files.get(f) {
                    bump(&s.facts, false);
                }
                bump(c, true);
            }
            fields.retain(|_, n| *n > 0);
        }
        // Removed by one file and added by another is no change.
        let was_written = |n: &str| {
            self.sums
                .as_ref()
                .is_some_and(|s| s.fields_written.contains(n))
        };
        flipped.retain(|n| self.fields.contains_key(n) != was_written(n));
        names.extend(flipped.iter().map(|n| word_hash(n)));

        // Install the new states.
        for f in &removed {
            self.files.remove(f);
        }
        for (f, h, c, i, events, w) in fresh {
            let primary = !ws.is_library(f);
            let old = self.files.remove(&f);
            let start = old.as_ref().map_or(0, |o| o.start);
            let edges = old
                .as_ref()
                .map_or_else(|| Arc::new(Vec::new()), |o| o.edges.clone());
            self.files.insert(
                f,
                State {
                    text: h,
                    iface: i,
                    facts: c,
                    events,
                    words: w,
                    edges,
                    start,
                    primary: primary.then(|| old.and_then(|o| o.primary).unwrap_or_default()),
                },
            );
        }

        // The summaries, and the nodes whose bit changed.
        let mut changed_keys: HashSet<DefId> = HashSet::new();
        let mut remap: HashMap<DefId, DefId> = HashMap::new();
        let sums = if full {
            let order: Vec<(FileId, &CheckFacts)> =
                live.iter().map(|f| (*f, &self.files[f].facts)).collect();
            let mut s = Summaries::number(ws, &order);
            let edges = par_map(&order, jobs, |(f, c)| Arc::new(s.file_edges(ws, *f, c)));
            s.solve(edges.iter().map(|e| &**e));
            self.install_edges(&live, edges, s.keys.len());
            up.summaries = "full";
            s
        } else if iface_changed && self.in_place(&touched) {
            // The changed files' nodes move to the end; edges into them are renumbered.
            let mut s = self.sums.take().expect("summaries after the first update");
            remap = remap_defs(&old_entries, |f| &self.files[&f].iface.entries);
            let old_yields = s.yields.clone();
            let old_len = s.keys.len();
            let mut moved: HashMap<usize, Option<usize>> = HashMap::new();
            let mut old_of_new: HashMap<usize, usize> = HashMap::new();
            let mut old_key: HashMap<usize, DefId> = HashMap::new();
            let mut new_start: HashMap<FileId, usize> = HashMap::new();
            let mut seeds = Vec::new();
            for f in live.iter().filter(|f| touched.contains(f)) {
                let st = &self.files[f];
                let range = st.start..st.start + st.edges.len();
                let keys: Vec<DefId> = range.clone().map(|i| s.keys[i]).collect();
                let at = s.replace_file(ws, *f, range.clone(), &st.facts);
                let pos: HashMap<DefId, usize> =
                    (at..s.keys.len()).map(|i| (s.keys[i], i)).collect();
                for (o, k) in range.zip(keys) {
                    let n = remap.get(&k).and_then(|d| pos.get(d)).copied();
                    moved.insert(o, n);
                    old_key.insert(o, k);
                    if let Some(n) = n {
                        old_of_new.insert(n, o);
                    }
                }
                seeds.extend(at..s.keys.len());
                new_start.insert(*f, at);
            }
            for n in &flipped {
                if self.fields.contains_key(n) {
                    s.fields_written.insert(n.clone());
                } else {
                    s.fields_written.remove(n);
                }
            }
            let text_changed: HashSet<FileId> = changed.iter().map(|(f, _)| *f).collect();
            // 0: unchanged, 1: renumbered, 2: recomputed.
            let redo = par_map(&live, jobs, |f| {
                let st = &self.files[f];
                if touched.contains(f) || text_changed.contains(f) || intersects(&st.words, &names)
                {
                    return (2, Some(Arc::new(s.file_edges(ws, *f, &st.facts))));
                }
                let into_moved = st
                    .edges
                    .iter()
                    .flatten()
                    .filter_map(any_nodes)
                    .flatten()
                    .any(|t| moved.contains_key(t));
                if !into_moved {
                    return (0, None);
                }
                let to_new = |t: usize| match moved.get(&t) {
                    Some(n) => *n,
                    None => Some(t),
                };
                match remap_edges(&st.edges, &to_new) {
                    Some(e) => (1, Some(Arc::new(e))),
                    None => (2, Some(Arc::new(s.file_edges(ws, *f, &st.facts)))),
                }
            });
            self.rdeps.resize(s.keys.len(), Vec::new());
            for (f, (how, e)) in live.iter().zip(redo) {
                let Some(e) = e else { continue };
                let st = self.files.get_mut(f).expect("a live state");
                for (k, es) in st.edges.iter().enumerate() {
                    let u = (st.start + k) as u32;
                    for t in es.iter().filter_map(any_nodes).flatten() {
                        if let Some(p) = self.rdeps[*t].iter().position(|&x| x == u) {
                            self.rdeps[*t].swap_remove(p);
                        }
                    }
                }
                if let Some(&at) = new_start.get(f) {
                    st.start = at;
                }
                for (k, es) in e.iter().enumerate() {
                    for t in es.iter().filter_map(any_nodes).flatten() {
                        self.rdeps[*t].push((st.start + k) as u32);
                    }
                }
                if how == 2 {
                    seeds.extend(st.start..st.start + e.len());
                }
                st.edges = e;
            }
            let cone = self.cone(seeds, s.keys.len());
            let node_edges = self.node_edges(&live, s.keys.len());
            s.solve_cone(&node_edges, &cone);
            for &i in &cone {
                if i >= old_len {
                    if let Some(o) = old_of_new.get(&i) {
                        if old_yields[*o] != s.node_yields(i) {
                            changed_keys.insert(old_key[o]);
                        }
                    }
                } else if s.keys[i] != DEAD && old_yields[i] != s.node_yields(i) {
                    changed_keys.insert(s.keys[i]);
                }
            }
            for (o, n) in &moved {
                if n.is_none() {
                    changed_keys.insert(old_key[o]);
                }
            }
            up.summaries = "in place";
            s
        } else if iface_changed {
            // Renumber; the edges of files that mention no changed name keep their targets.
            let old = self.sums.take().expect("summaries after the first update");
            remap = remap_defs(&old_entries, |f| &self.files[&f].iface.entries);
            let moved = |k: DefId| -> Option<DefId> {
                if touched.contains(&k.file) {
                    remap.get(&k).copied()
                } else {
                    Some(k)
                }
            };
            let order: Vec<(FileId, &CheckFacts)> =
                live.iter().map(|f| (*f, &self.files[f].facts)).collect();
            let mut s = Summaries::number(ws, &order);
            let new_index: HashMap<DefId, usize> =
                s.keys.iter().enumerate().map(|(i, k)| (*k, i)).collect();
            let to_new = |i: usize| moved(old.keys[i]).and_then(|k| new_index.get(&k).copied());
            let text_changed: HashSet<FileId> = changed.iter().map(|(f, _)| *f).collect();
            let edges = par_map(&order, jobs, |(f, c)| {
                let st = &self.files[f];
                let redo = text_changed.contains(f) || intersects(&st.words, &names);
                if !redo {
                    if let Some(e) = remap_edges(&st.edges, &to_new) {
                        return (false, Arc::new(e));
                    }
                }
                (true, Arc::new(s.file_edges(ws, *f, c)))
            });
            let mut old_of: Vec<Option<usize>> = vec![None; s.keys.len()];
            for i in 0..old.keys.len() {
                if let Some(n) = to_new(i) {
                    old_of[n] = Some(i);
                }
            }
            s.yields = (0..s.keys.len())
                .map(|i| old_of[i].map_or(s.direct[i], |o| old.node_yields(o)))
                .collect();
            let mut seeds: Vec<usize> =
                (0..s.keys.len()).filter(|&i| old_of[i].is_none()).collect();
            let mut start = 0;
            for (redo, e) in &edges {
                if *redo {
                    seeds.extend(start..start + e.len());
                }
                start += e.len();
            }
            self.install_edges(
                &live,
                edges.into_iter().map(|(_, e)| e).collect(),
                s.keys.len(),
            );
            let cone = self.cone(seeds, s.keys.len());
            let node_edges = self.node_edges(&live, s.keys.len());
            s.solve_cone(&node_edges, &cone);
            for &i in &cone {
                if let Some(o) = old_of[i] {
                    if old.node_yields(o) != s.node_yields(i) {
                        changed_keys.insert(old.keys[o]);
                    }
                }
            }
            // Old nodes of changed files that went.
            for (o, k) in old.keys.iter().enumerate() {
                if touched.contains(&k.file) && to_new(o).is_none() {
                    changed_keys.insert(*k);
                }
            }
            up.summaries = "renumbered";
            s
        } else {
            up.summaries = "cone";
            let mut s = self.sums.take().expect("summaries after the first update");
            for (f, _) in &changed {
                let st = &self.files[f];
                let ys = st.facts.functions.iter().chain(&st.facts.macros);
                for (k, (_, y)) in ys.enumerate() {
                    s.direct[st.start + k] = y.direct;
                }
            }
            let edges = par_map(&changed, jobs, |(f, _)| {
                Arc::new(s.file_edges(ws, *f, &self.files[f].facts))
            });
            // Callers: the changed files' old edges out, their new ones in.
            let mut seeds = Vec::new();
            for ((f, _), e) in changed.iter().zip(edges) {
                let st = self.files.get_mut(f).expect("a changed state");
                for (k, (old, new)) in st.edges.iter().zip(e.iter()).enumerate() {
                    let u = (st.start + k) as u32;
                    seeds.push(st.start + k);
                    for t in old.iter().filter_map(any_nodes).flatten() {
                        if let Some(p) = self.rdeps[*t].iter().position(|&x| x == u) {
                            self.rdeps[*t].swap_remove(p);
                        }
                    }
                    for t in new.iter().filter_map(any_nodes).flatten() {
                        self.rdeps[*t].push(u);
                    }
                }
                st.edges = e;
            }
            for n in &flipped {
                if self.fields.contains_key(n) {
                    s.fields_written.insert(n.clone());
                } else {
                    s.fields_written.remove(n);
                }
            }
            // Only the nodes that reach a changed one can change.
            let cone = self.cone(seeds, s.keys.len());
            let node_edges = self.node_edges(&live, s.keys.len());
            let before: Vec<bool> = cone.iter().map(|&i| s.node_yields(i)).collect();
            s.solve_cone(&node_edges, &cone);
            for (&i, old) in cone.iter().zip(before) {
                if old != s.node_yields(i) {
                    changed_keys.insert(s.keys[i]);
                }
            }
            s
        };

        phase("summaries");

        // The primary files to re-check.
        let text_changed: HashSet<FileId> = changed.iter().map(|(f, _)| *f).collect();
        let dirty: Vec<FileId> = ws
            .primary_files()
            .filter(|f| {
                if full || text_changed.contains(f) {
                    return true;
                }
                let Some(st) = self.files.get(f) else {
                    return true;
                };
                let Some(p) = st.primary.as_ref() else {
                    return true;
                };
                intersects(&st.words, &names)
                    || (ev.patterns && p.has_events)
                    || ev.emitted.iter().any(|n| {
                        p.waits
                            .iter()
                            .any(|w| gsc_sema::data::levenshtein(w, n, 2).is_some())
                    })
                    || intersects(&p.reach, &changed_keys)
            })
            .collect();
        if !remap.is_empty() {
            // The files not re-checked keep their reach, in the new numbering.
            for st in self.files.values_mut() {
                if let Some(p) = st.primary.as_mut() {
                    let mut moved = false;
                    for k in p.reach.iter_mut() {
                        if let Some(n) = remap.get(k) {
                            moved |= n != k;
                            *k = *n;
                        }
                    }
                    if moved {
                        p.reach.sort_unstable();
                    }
                }
            }
        }
        phase("select");
        let redo = par_map(&dirty, jobs, |&f| {
            let waits = text_changed.contains(&f).then(|| {
                let data = ws.file(f);
                let mut waits: Vec<String> = data
                    .facts
                    .events
                    .iter()
                    .filter(|s| s.class != EventClass::Emit)
                    .filter_map(|s| s.key())
                    .collect();
                waits.sort_unstable();
                waits.dedup();
                (waits, !data.facts.events.is_empty())
            });
            (waits, reach(ws, &sums, f), check_file(ws, opts, &sums, f))
        });
        for (f, (w, r, d)) in dirty.iter().zip(redo) {
            let p = self
                .files
                .get_mut(f)
                .and_then(|s| s.primary.as_mut())
                .expect("a primary state");
            if let Some((waits, has_events)) = w {
                p.waits = waits;
                p.has_events = has_events;
            }
            p.reach = r;
            p.diags = d;
        }
        self.sums = Some(sums);
        phase("rules");
        up.phases = phases;
        up.rechecked = dirty;
        up.interface_changed = iface_changed;
        up.full = full;
        up
    }

    /// Can the changed files' nodes move to the end of the numbering? Not when dead slots would
    /// pass a quarter of the nodes: then everything is renumbered.
    fn in_place(&self, touched: &HashSet<FileId>) -> bool {
        let Some(s) = &self.sums else { return false };
        let moving: usize = touched
            .iter()
            .filter_map(|f| self.files.get(f))
            .map(|st| st.edges.len())
            .sum();
        (s.dead() + moving) * 4 < s.keys.len()
    }

    /// Store the edges of every live file (in `live` order) and their callers.
    fn install_edges(&mut self, live: &[FileId], edges: Vec<Arc<FileEdges>>, nodes: usize) {
        self.rdeps = vec![Vec::new(); nodes];
        let mut start = 0;
        for (f, e) in live.iter().zip(edges) {
            for (k, es) in e.iter().enumerate() {
                for t in es.iter().filter_map(any_nodes).flatten() {
                    self.rdeps[*t].push((start + k) as u32);
                }
            }
            let n = e.len();
            let st = self.files.get_mut(f).expect("a state per live file");
            st.edges = e;
            st.start = start;
            start += n;
        }
    }

    /// The edges of every node, by node (none for dead slots).
    fn node_edges(&self, live: &[FileId], nodes: usize) -> Vec<&Vec<Edge>> {
        static NONE: Vec<Edge> = Vec::new();
        let mut out: Vec<&Vec<Edge>> = vec![&NONE; nodes];
        for f in live {
            let st = &self.files[f];
            for (k, es) in st.edges.iter().enumerate() {
                out[st.start + k] = es;
            }
        }
        out
    }

    /// `seeds` and every node that reaches one of them, sorted.
    fn cone(&self, mut seeds: Vec<usize>, nodes: usize) -> Vec<usize> {
        let mut in_cone = vec![false; nodes];
        let mut cone = Vec::new();
        while let Some(i) = seeds.pop() {
            if std::mem::replace(&mut in_cone[i], true) {
                continue;
            }
            cone.push(i);
            seeds.extend(self.rdeps[i].iter().map(|&u| u as usize));
        }
        cone.sort_unstable();
        cone
    }

    /// The diagnostics of a primary file, as of the last update.
    pub fn diagnostics(&self, f: FileId) -> Option<&[Diagnostic]> {
        self.files
            .get(&f)?
            .primary
            .as_ref()
            .map(|p| p.diags.as_slice())
    }

    /// Every primary file's results, in file order, as [`crate::check`] returns them.
    pub fn results(&self, ws: &Workspace) -> Vec<FileResult> {
        ws.primary_files()
            .map(|f| {
                let data = ws.file(f);
                FileResult {
                    file: f,
                    path: data.meta.path.clone(),
                    display: data.meta.display.clone(),
                    source: data.source.clone(),
                    diagnostics: self.diagnostics(f).map(<[_]>::to_vec).unwrap_or_default(),
                }
            })
            .collect()
    }
}
