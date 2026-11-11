//! `UNKNOWNS.tsv` and `VOCAB.tsv`: the hashes nothing names, with the context that constrains
//! them, and the clear vocabulary of each file. Columns and method: `docs/hash-recovery.md`.
//!
//! The rows are a pure function of the inputs: every list is sorted and truncated the same way
//! whatever the thread count.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use gsc_hash::{mask, Dictionary, Role, Scheme};
use gsc_sema::facts::{DefKind, Target};
use gsc_sema::{par_map, FileId};
use gsc_syntax::SyntaxKind;

use crate::write::cell;
use crate::{EventName, Index};

/// At most this many names in a list cell.
const MAX_LIST: usize = 8;

/// The roles a decompiler spelling can carry in an identifier token.
const IDENT_ROLES: [Role; 4] = [Role::Function, Role::Var, Role::Namespace, Role::Class];

/// The hash a `#"..."` or `"..."` token stands for. A decompiler writes a hashed string it cannot
/// name either as `hash_<hex>` or, inside a plain string, as the bare value; a bare value is read
/// only at exactly 16 hex digits, a shape no clear name of the corpus takes.
fn string_value(scheme: &Scheme, inner: &str) -> Option<u64> {
    if let Some(v) = scheme.spelled(Role::String, inner) {
        return Some(v);
    }
    if inner.len() == 16 && inner.bytes().all(|c| c.is_ascii_hexdigit()) {
        let bits = scheme.family(Role::String)?.bits;
        return u64::from_str_radix(inner, 16).ok().map(|v| v & mask(bits));
    }
    None
}

/// One file's view of one hash: occurrences, and the definitions they sit in.
type Hits = (usize, BTreeSet<u32>);

/// Where a hash occurs: how often, in which files (index into `Index::files`), and in which
/// definitions of them (`(file, definition)`, sorted).
#[derive(Default)]
struct Seen {
    sites: usize,
    files: BTreeMap<usize, usize>,
    in_defs: Vec<(usize, u32)>,
}

/// What the definition of a hashed function or class says about it.
struct DefInfo {
    file: usize,
    at: String,
    ns: String,
    params: String,
    flags: String,
}

/// `a:2,b:1`, most frequent first.
fn counted<K: std::fmt::Display + Ord>(m: &BTreeMap<K, usize>) -> String {
    let mut v: Vec<_> = m.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    v.iter()
        .map(|(k, n)| format!("{k}:{n}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// The first [`MAX_LIST`] names of a set, comma-separated, with `+n` for the rest.
fn listed(s: Option<&BTreeSet<String>>) -> String {
    let Some(s) = s else {
        return String::new();
    };
    let mut out: Vec<&str> = s.iter().take(MAX_LIST).map(String::as_str).collect();
    let extra = s.len().saturating_sub(out.len());
    let tail;
    if extra > 0 {
        tail = format!("+{extra}");
        out.push(&tail);
    }
    out.join(",")
}

/// The name a hashed spelling stands for: the packages first, then the workspace's own clear
/// names. `None` for a name already in clear, and for a hash nothing names.
fn recovered<'a>(
    scheme: &Scheme,
    pkg: Option<&'a Dictionary>,
    wsn: Option<&'a Dictionary>,
    role: Role,
    written: &str,
) -> Option<&'a str> {
    let v = scheme.spelled(role, written)?;
    named(pkg, wsn, role, v)
}

/// The same by value, for the roles whose spelling is not an identifier (`#"..."` strings).
fn named<'a>(
    pkg: Option<&'a Dictionary>,
    wsn: Option<&'a Dictionary>,
    role: Role,
    value: u64,
) -> Option<&'a str> {
    pkg.and_then(|d| d.name(role, value))
        .or_else(|| wsn.and_then(|d| d.name(role, value)))
}

/// The short name of a `ns::fn` or `ns::class::method`.
fn short(q: &str) -> &str {
    q.rsplit("::").next().unwrap_or(q)
}

/// Is this a name worth listing as a neighbour? A dynamic call (`[[expr]]`), a receiver or a
/// placeholder is not a word, and an unreadable neighbour is noise rather than context.
fn is_word(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

/// One row per hash of the workspace that neither the hash packages nor the workspace's own clear
/// names name, with where it sits and which named symbols surround it.
pub fn unknowns(ix: &Index) -> String {
    let mut out = String::from(
        "hash\tvalue\trole\tfamily\tbits\tsites\tfiles\tscope\tdef_at\tnamespace\tparams\tflags\
         \tobject\tops\tin_functions\tcallers\tcallees\n",
    );
    let Some(f0) = ix.files.first() else {
        return out;
    };
    let Some(scheme) = ix.ws.scheme_of(f0.id) else {
        return out;
    };
    let pkg = ix.ws.profile_of(f0.id).names.clone();
    let wsn = ix.ws.workspace_names(f0.id);
    // A hash a dictionary names is not a target.
    let known = |r: Role, v: u64| named(pkg.as_deref(), wsn.as_deref(), r, v).is_some();
    // A token as a reader would see it: itself when clear, its recovered name when one is known,
    // `None` when it is a hash nothing names -- an unreadable neighbour is noise, not context.
    let name_of = |r: Role, written: &str| -> Option<String> {
        if !is_word(written) {
            return None;
        }
        if scheme.spelled(r, written).is_none() {
            return Some(written.to_owned());
        }
        recovered(&scheme, pkg.as_deref(), wsn.as_deref(), r, written).map(str::to_owned)
    };

    // Every hashed token of every file, counted. The same rule as `Workspace::hash_coverage`, so
    // the rows here and the counts it prints are the same set.
    let ids: Vec<FileId> = ix.files.iter().map(|f| f.id).collect();
    let per: Vec<HashMap<(Role, u64), Hits>> = par_map(&ids, ix.ws.jobs(), |&id| {
        let data = ix.ws.file(id);
        let dialect = ix.ws.profile_of(id).dialect(data.decompiled);
        let text = data.text();
        let (tokens, _) = gsc_syntax::tokenize(text, dialect);
        let defs = &data.facts.defs;
        let mut m: HashMap<(Role, u64), Hits> = HashMap::new();
        let mut at = 0usize;
        // Definitions come in tree order, so the one a token sits in is the last one that starts
        // before it. A token between two methods of a class is left without one.
        let mut di = 0usize;
        for t in tokens {
            let start = at as u32;
            let s = &text[at..at + t.len as usize];
            at += t.len as usize;
            let key = match t.kind {
                SyntaxKind::IDENT => IDENT_ROLES
                    .iter()
                    .find_map(|&r| scheme.spelled(r, s).map(|v| (r, v))),
                SyntaxKind::HASH_STRING | SyntaxKind::STRING => {
                    let inner = s.trim_start_matches('#').trim_matches('"');
                    string_value(&scheme, inner).map(|v| (Role::String, v))
                }
                _ => None,
            };
            let Some(k) = key else { continue };
            while di < defs.len() && u32::from(defs[di].range.start()) <= start {
                di += 1;
            }
            let e = m.entry(k).or_default();
            e.0 += 1;
            if let Some(d) = di
                .checked_sub(1)
                .filter(|&i| u32::from(defs[i].range.end()) > start)
            {
                e.1.insert(d as u32);
            }
        }
        m
    });
    let mut seen: BTreeMap<(Role, u64), Seen> = BTreeMap::new();
    for (fi, m) in per.iter().enumerate() {
        for (k, (n, in_defs)) in m {
            let e = seen.entry(*k).or_default();
            e.sites += n;
            *e.files.entry(fi).or_default() += n;
            for &d in in_defs {
                e.in_defs.push((fi, d));
            }
        }
    }
    for e in seen.values_mut() {
        e.in_defs.sort_unstable();
    }
    seen.retain(|&(r, v), _| !known(r, v));

    // Where a hashed function or class is defined, and under which namespace.
    let mut defs: HashMap<(Role, u64), DefInfo> = HashMap::new();
    for (fi, f) in ix.files.iter().enumerate() {
        for d in &ix.ws.file(f.id).facts.defs {
            let (role, written) = match d.kind {
                DefKind::Class => (Role::Class, d.name.as_str()),
                DefKind::Function | DefKind::Method => (Role::Function, d.short_name()),
                DefKind::Define | DefKind::Const => continue,
            };
            let Some(v) = scheme.spelled(role, written) else {
                continue;
            };
            defs.entry((role, v)).or_insert_with(|| DefInfo {
                file: fi,
                at: f.at(d.focus),
                ns: name_of(Role::Namespace, &d.ns).unwrap_or_else(|| d.ns.clone()),
                params: d.params.clone(),
                flags: d.flags.clone(),
            });
        }
    }

    // Who calls a hashed function, and what a hashed function calls -- named neighbours only.
    let mut callers: HashMap<u64, BTreeSet<String>> = HashMap::new();
    let mut callees: HashMap<u64, BTreeSet<String>> = HashMap::new();
    for f in &ix.files {
        let facts = &ix.ws.file(f.id).facts;
        for (c, r) in facts.refs.iter().zip(&f.resolution.refs) {
            let caller_q = facts.caller_name(c.caller);
            if caller_q == "<file>" {
                continue;
            }
            let (caller, callee) = (short(&caller_q), short(&r.callee));
            if let Some(v) = scheme.spelled(Role::Function, callee) {
                if let Some(n) = name_of(Role::Function, caller) {
                    callers.entry(v).or_default().insert(n);
                }
            }
            if let Some(v) = scheme.spelled(Role::Function, caller) {
                if let Some(n) = name_of(Role::Function, callee) {
                    callees.entry(v).or_default().insert(n);
                }
            }
        }
    }

    // A hashed field: which object carries it, read or written, in which named functions. A hashed
    // event: which operation on which receiver, in which named functions.
    let mut obj: HashMap<u64, BTreeMap<&'static str, usize>> = HashMap::new();
    let mut ops: HashMap<(Role, u64), BTreeMap<String, usize>> = HashMap::new();
    for f in &ix.files {
        for x in &f.fields {
            let Some(v) = scheme.spelled(Role::Var, &x.name) else {
                continue;
            };
            *obj.entry(v).or_default().entry(x.obj).or_default() += 1;
            *ops.entry((Role::Var, v))
                .or_default()
                .entry(x.op.to_owned())
                .or_default() += 1;
        }
        for e in &f.events {
            let EventName::Literal(lit) = &e.name else {
                continue;
            };
            let Some(v) = string_value(&scheme, lit) else {
                continue;
            };
            *obj.entry(v).or_default().entry(e.receiver).or_default() += 1;
            *ops.entry((Role::String, v))
                .or_default()
                .entry(e.op.clone())
                .or_default() += 1;
        }
    }

    let mut rows: Vec<(&(Role, u64), &Seen)> = seen.iter().collect();
    rows.sort_by(|a, b| {
        (a.0 .0)
            .cmp(&b.0 .0)
            .then_with(|| b.1.sites.cmp(&a.1.sites))
            .then_with(|| (a.0 .1).cmp(&b.0 .1))
    });
    for (&(role, value), s) in rows {
        let family = scheme.family(role);
        let def = defs.get(&(role, value));
        // The file a name belongs to: where it is defined, else where it is used most.
        let scope = def.map(|d| d.file).or_else(|| {
            s.files
                .iter()
                .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
                .map(|(fi, _)| *fi)
        });
        // The named definitions the occurrences sit in: the one context every role has.
        let mut in_functions: BTreeSet<String> = BTreeSet::new();
        for &(fi, d) in &s.in_defs {
            if in_functions.len() > MAX_LIST * 4 {
                break;
            }
            let def = &ix.ws.file(ix.files[fi].id).facts.defs[d as usize];
            if let Some(n) = name_of(Role::Function, def.short_name()) {
                in_functions.insert(n);
            }
        }
        let cells = [
            scheme.spelling(role, value),
            format!("{value:x}"),
            role.label().to_owned(),
            family.map_or_else(String::new, |f| f.id.to_owned()),
            family.map_or_else(String::new, |f| f.bits.to_string()),
            s.sites.to_string(),
            s.files.len().to_string(),
            scope.map_or_else(String::new, |i| ix.files[i].rel.clone()),
            def.map_or_else(String::new, |d| d.at.clone()),
            def.map_or_else(String::new, |d| d.ns.clone()),
            def.map_or_else(String::new, |d| d.params.clone()),
            def.map_or_else(String::new, |d| d.flags.clone()),
            obj.get(&value).map_or_else(String::new, counted),
            ops.get(&(role, value)).map_or_else(String::new, counted),
            listed(Some(&in_functions)),
            listed(callers.get(&value)),
            listed(callees.get(&value)),
        ];
        let line: Vec<String> = cells.iter().map(|c| cell(c)).collect();
        out.push_str(&line.join("\t"));
        out.push('\n');
    }
    out
}

/// The words of a path: its directories, its stem, and the parts of each.
fn path_tokens(rel: &str) -> BTreeSet<String> {
    rel.split(['/', '\\', '.', '_', '-'])
        .filter(|p| p.len() > 1)
        .map(str::to_ascii_lowercase)
        .collect()
}

/// One row per file, kind and source: the names the file uses, as a generator should draw on them
/// for the unknowns whose `scope` is that file. `clear` is what the file spells out, `recovered`
/// what a hash package or the workspace's own clear names give back -- on a decompiled corpus the
/// second is the larger half, and leaving it out starved every block.
pub fn vocab(ix: &Index) -> String {
    let mut out = String::from("file\tkind\tsource\tcount\ttokens\n");
    let Some(f0) = ix.files.first() else {
        return out;
    };
    let Some(scheme) = ix.ws.scheme_of(f0.id) else {
        return out;
    };
    let pkg = ix.ws.profile_of(f0.id).names.clone();
    let wsn = ix.ws.workspace_names(f0.id);
    let (pkg, wsn) = (pkg.as_deref(), wsn.as_deref());

    for f in &ix.files {
        let facts = &ix.ws.file(f.id).facts;
        let mut sets: BTreeMap<(&str, &str), BTreeSet<String>> = BTreeMap::new();
        let mut add = |kind: &'static str, role: Role, written: &str| {
            if !is_word(written) {
                return;
            }
            let (source, name) = if scheme.spelled(role, written).is_none() {
                ("clear", written.to_owned())
            } else {
                match recovered(&scheme, pkg, wsn, role, written) {
                    Some(n) => ("recovered", n.to_owned()),
                    None => return,
                }
            };
            sets.entry((kind, source)).or_default().insert(name);
        };

        // what the file defines, and what it calls: a callee's name is vocabulary too
        for d in &facts.defs {
            if d.kind.is_callable() {
                add("function", Role::Function, d.short_name());
            }
            add("namespace", Role::Namespace, &d.ns);
        }
        for (r, x) in facts.refs.iter().zip(&f.resolution.refs) {
            add("function", Role::Function, short(&x.callee));
            if let Target::Qualified(Some(q), _) = &r.target {
                if !q.contains(['/', '\\']) {
                    add("namespace", Role::Namespace, q);
                }
            }
        }
        for n in &facts.fields {
            add("field", Role::Var, n);
        }
        for n in &facts.strings {
            add("string", Role::String, n);
        }
        drop(add);

        // events and the remaining strings go by value: a `#"..."` is not an identifier spelling
        for e in &f.events {
            let EventName::Literal(lit) = &e.name else {
                continue;
            };
            let name = match string_value(&scheme, lit) {
                None => lit.clone(),
                Some(v) => match named(pkg, wsn, Role::String, v) {
                    Some(n) => n.to_owned(),
                    None => continue,
                },
            };
            let source = if string_value(&scheme, lit).is_none() {
                "clear"
            } else {
                "recovered"
            };
            sets.entry(("event", source)).or_default().insert(name);
        }
        sets.insert(("path", "clear"), path_tokens(&f.rel));

        for ((kind, source), set) in sets {
            if set.is_empty() {
                continue;
            }
            let tokens: Vec<&str> = set.iter().map(String::as_str).collect();
            let row = [
                cell(&f.rel),
                kind.to_owned(),
                source.to_owned(),
                set.len().to_string(),
                cell(&tokens.join(",")),
            ];
            out.push_str(&row.join("\t"));
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listed_truncates_and_counts_the_rest() {
        let s: BTreeSet<String> = (0..12).map(|i| format!("n{i:02}")).collect();
        let got = listed(Some(&s));
        assert_eq!(got.matches(',').count(), MAX_LIST);
        assert!(got.ends_with("+4"), "{got}");
        assert_eq!(listed(None), "");
    }

    #[test]
    fn counted_puts_the_most_frequent_first() {
        let mut m = BTreeMap::new();
        m.insert("read", 3usize);
        m.insert("write", 9usize);
        assert_eq!(counted(&m), "write:9,read:3");
    }

    #[test]
    fn path_tokens_split_on_every_separator() {
        let t = path_tokens("scripts/zm_common/zm_loadout.gsc");
        assert!(t.contains("scripts") && t.contains("zm") && t.contains("loadout"));
        assert!(!t.contains("zm_loadout"));
    }

    #[test]
    fn a_bare_hex_string_is_a_hash_only_at_full_width() {
        let s = Scheme::for_profile("t8").expect("t8 hashes identifiers");
        assert!(string_value(&s, "59cd2610cddc7a0b").is_some());
        assert!(string_value(&s, "deadbeef").is_none());
        assert!(string_value(&s, "zombie_death").is_none());
        assert_eq!(
            string_value(&s, "hash_59cd2610cddc7a0b"),
            string_value(&s, "59cd2610cddc7a0b")
        );
    }
}
