//! Render the index as TSV files and a README.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use crate::extract::{EventClass, EventName};
use crate::Index;

/// The files `gruff index` writes, in order.
pub const FILES: &[&str] = &[
    "SYMBOLS.tsv",
    "CALLS.tsv",
    "EVENTS.tsv",
    "EVENTS_SUMMARY.tsv",
    "FIELDS.tsv",
    "INCLUDES.tsv",
    "README.md",
];

/// A TSV cell: no tab, no line break, never empty (`-`).
fn cell(s: &str) -> String {
    if s.is_empty() {
        return "-".to_owned();
    }
    if s.contains(['\t', '\n', '\r']) {
        return s.replace(['\t', '\n', '\r'], " ");
    }
    s.to_owned()
}

macro_rules! row {
    ($out:expr, $($v:expr),+ $(,)?) => {{
        let cells: Vec<String> = vec![$(cell(&$v.to_string())),+];
        $out.push_str(&cells.join("\t"));
        $out.push('\n');
    }};
}

pub fn symbols(ix: &Index) -> String {
    let mut out = String::from("kind\tnamespace\tname\tat\tparams\tflags\tdoc\n");
    for f in &ix.files {
        for s in &ix.ws.file(f.id).facts.defs {
            let at = f.at(s.focus);
            row!(
                out,
                s.kind.label(),
                s.ns,
                s.name,
                at,
                s.params,
                s.flags,
                s.doc
            );
        }
    }
    out
}

pub fn calls(ix: &Index) -> String {
    let mut out = String::from("caller\tcallee\tresolution\tkind\treceiver\tat\tvia_macro\n");
    for f in &ix.files {
        let facts = &ix.ws.file(f.id).facts;
        for (c, r) in facts.refs.iter().zip(&f.resolution.refs) {
            let at = f.at(c.focus);
            let caller = facts.caller_name(c.caller);
            row!(
                out,
                caller,
                r.callee,
                r.how.label(),
                c.kind,
                c.receiver,
                at,
                "-"
            );
        }
        for c in &f.macro_calls {
            let at = format!("{}:{}", f.rel, c.line);
            row!(
                out,
                c.caller,
                c.callee,
                c.resolution,
                c.kind,
                c.receiver,
                at,
                c.via
            );
        }
    }
    out
}

pub fn events(ix: &Index) -> String {
    let mut out =
        String::from("event\top\treceiver\treceiver_text\targs\tfunction\tat\texpr\tvia_macro\n");
    for f in &ix.files {
        for e in &f.events {
            let at = format!("{}:{}", f.rel, e.line);
            row!(
                out,
                e.name.render(),
                e.op,
                e.receiver,
                e.receiver_text,
                e.argc,
                e.func,
                at,
                e.expr,
                e.via_macro.as_deref().unwrap_or("-")
            );
        }
    }
    out
}

pub fn fields(ix: &Index) -> String {
    let mut out = String::from("object\tfield\top\tfunction\tat\tvia_macro\n");
    for f in &ix.files {
        for x in &f.fields {
            let at = format!("{}:{}", f.rel, x.line);
            let via = x.via_macro.as_deref().unwrap_or("-");
            row!(out, x.obj, x.name, x.op, x.func, at, via);
        }
    }
    out
}

pub fn includes(ix: &Index) -> String {
    let mut out = String::from("file\tdirective\ttarget\tresolved\tat\n");
    for f in &ix.files {
        let facts = &ix.ws.file(f.id).facts;
        for (inc, x) in facts.imports.iter().zip(&f.resolution.imports) {
            let resolved = x.map_or("?", |i| ix.rel(i));
            let at = f.at(inc.focus);
            row!(out, f.rel, inc.directive, inc.target, resolved, at);
        }
    }
    out
}

/// Does the `*` glob `pat` match `s`?
pub fn glob(pat: &str, s: &str) -> bool {
    let parts: Vec<&str> = pat.split('*').collect();
    if parts.len() == 1 {
        return pat == s;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !s.starts_with(first) || s.len() < first.len() + last.len() || !s.ends_with(last) {
        return false;
    }
    let mut rest = &s[first.len()..s.len() - last.len()];
    for p in &parts[1..parts.len() - 1] {
        match rest.find(p) {
            Some(i) => rest = &rest[i + p.len()..],
            None => return false,
        }
    }
    true
}

/// Can two patterns name the same event? Compares their fixed prefixes and suffixes.
fn patterns_meet(a: &str, b: &str) -> bool {
    let pre = |p: &str| p.split('*').next().unwrap_or("").to_owned();
    let suf = |p: &str| p.rsplit('*').next().unwrap_or("").to_owned();
    let (pa, pb, sa, sb) = (pre(a), pre(b), suf(a), suf(b));
    (pa.starts_with(&pb) || pb.starts_with(&pa)) && (sa.ends_with(&sb) || sb.ends_with(&sa))
}

#[derive(Default)]
struct Summary {
    emit: usize,
    wait: usize,
    endon: usize,
    files: BTreeSet<usize>,
    receivers: BTreeMap<&'static str, usize>,
    pattern: bool,
}

pub fn events_summary(ix: &Index) -> String {
    let mut by: BTreeMap<String, Summary> = BTreeMap::new();
    for (fi, f) in ix.files.iter().enumerate() {
        for e in &f.events {
            let s = by.entry(e.name.render().to_owned()).or_default();
            match e.class {
                EventClass::Emit => s.emit += 1,
                EventClass::Wait => s.wait += 1,
                EventClass::Endon => s.endon += 1,
            }
            s.files.insert(fi);
            *s.receivers.entry(e.receiver).or_default() += 1;
            s.pattern = matches!(e.name, EventName::Pattern(_));
        }
    }
    let emit_literals: Vec<&str> = by
        .iter()
        .filter(|(n, s)| s.emit > 0 && !s.pattern && n.as_str() != "<dynamic>")
        .map(|(n, _)| n.as_str())
        .collect();
    let emit_patterns: Vec<&str> = by
        .iter()
        .filter(|(_, s)| s.emit > 0 && s.pattern)
        .map(|(n, _)| n.as_str())
        .collect();
    let listen_literals: Vec<&str> = by
        .iter()
        .filter(|(n, s)| s.wait + s.endon > 0 && !s.pattern && n.as_str() != "<dynamic>")
        .map(|(n, _)| n.as_str())
        .collect();
    let listen_patterns: Vec<&str> = by
        .iter()
        .filter(|(_, s)| s.wait + s.endon > 0 && s.pattern)
        .map(|(n, _)| n.as_str())
        .collect();

    let mut out = String::from("event\temit\twait\tendon\tfiles\treceivers\tflags\n");
    for (name, s) in &by {
        let mut flags = Vec::new();
        if name != "<dynamic>" {
            if s.emit == 0 {
                let emitted = if s.pattern {
                    emit_literals.iter().any(|l| glob(name, l))
                        || emit_patterns.iter().any(|p| patterns_meet(name, p))
                } else {
                    emit_patterns.iter().any(|p| glob(p, name))
                };
                if !emitted {
                    flags.push("no_script_emitter");
                }
            }
            if s.wait + s.endon == 0 {
                let heard = if s.pattern {
                    listen_literals.iter().any(|l| glob(name, l))
                        || listen_patterns.iter().any(|p| patterns_meet(name, p))
                } else {
                    listen_patterns.iter().any(|p| glob(p, name))
                };
                if !heard {
                    flags.push("no_script_listener");
                }
            }
            if s.pattern {
                flags.push("pattern");
            }
        }
        let receivers = s
            .receivers
            .iter()
            .map(|(k, v)| format!("{k}:{v}"))
            .collect::<Vec<_>>()
            .join(",");
        row!(
            out,
            name,
            s.emit,
            s.wait,
            s.endon,
            s.files.len(),
            receivers,
            flags.join(",")
        );
    }
    out
}

fn count_lines(s: &str) -> usize {
    s.lines().count().saturating_sub(1)
}

/// The README of the output directory: what each file holds and grep recipes.
pub fn readme(ix: &Index, rendered: &BTreeMap<&str, String>, game: &str) -> String {
    let n = |f: &str| rendered.get(f).map_or(0, |s| count_lines(s));
    let mut by_how: BTreeMap<&str, usize> = BTreeMap::new();
    for f in &ix.files {
        for r in &f.resolution.refs {
            *by_how.entry(r.how.label()).or_default() += 1;
        }
    }
    let mut o = String::new();
    let _ = writeln!(o, "# GSC index\n");
    let _ = writeln!(
        o,
        "Generated by `gruff index` (game `{game}`) over {} files. Do not edit: run the command again.",
        ix.files.len()
    );
    let _ = writeln!(
        o,
        "Every `.tsv` has a header line and one fact per line, tab-separated; an empty cell is `-`. \
         Paths are relative to the indexed root, `at` is `path:line` (1-based). In games with \
         case-insensitive identifiers, function, namespace and field names are lower case; event \
         names and macros keep their spelling. Use `rg -i` when unsure.\n"
    );
    let _ = writeln!(o, "| file | rows | one line is |");
    let _ = writeln!(o, "|---|---|---|");
    let rows = [
        ("SYMBOLS.tsv", "a declaration: `kind namespace name at params flags doc`. kind is function, method (`class::method`), class (params = bases), const or define (doc = `= body`). flags: autoexec, private, codecall, event_handler[x], dev (inside `/# #/`). doc: the `Summary:` of a `/@ @/` block or the `///` lines above."),
        ("CALLS.tsv", "a call site or function reference: `caller callee resolution kind receiver at via_macro`. caller and callee are `ns::fn` (`<file>` outside functions); a builtin callee is its bare name. resolution: local (same file), namespace (same namespace, another file), using (a `#using`'d file), include (an `#include`'d file), qualified (`ns::f` found), method (a class method), builtin (engine catalogue), builtin_othervm (only in the other VM's catalogue), macro (a `#define`, callee spelled as defined), dynamic (`[[expr]]` or `->m`), unresolved (name as written). kind: call, method (has a receiver), thread, childthread, pointer (`&f`, `::f`). receiver: self, level, player (any expression mentioning a player), game, anim, other, `-`. via_macro: the macro whose expansion holds the call (`at` is then the macro use), else `-`."),
        ("EVENTS.tsv", "an event site: `event op receiver receiver_text args function at expr via_macro`. event is the literal, a pattern (`trigger_*` from `\"trigger_\" + n`) or `<dynamic>`; expr holds the source text when it is not a literal. op: notify, waittill, waittillmatch, endon, or the wrapper name (waittill_any, waittill_any_timeout, waittill_either, delay_notify, wait_endon...). receiver `self` with receiver_text `-` is the implicit self. args: payload of a notify, variables bound by a waittill, other arguments of a wrapper. via_macro as in CALLS.tsv."),
        ("EVENTS_SUMMARY.tsv", "one event: `event emit wait endon files receivers flags`, counts of sites. flags: no_script_emitter (waited or endon'd, never notified by a script: an engine event, or a typo), no_script_listener (notified, never waited), pattern."),
        ("FIELDS.tsv", "a field access: `object field op function at via_macro`. object: level, self, anim, game, other. op: read, write, write_index (`level.a[ i ] = x`). `.size` is left out. via_macro as in CALLS.tsv."),
        ("INCLUDES.tsv", "an import: `file directive target resolved at`. directive: using, include, insert, inline; resolved is the indexed file it names, or `?`."),
    ];
    for (f, what) in rows {
        let _ = writeln!(o, "| `{f}` | {} | {what} |", n(f));
    }
    let _ = writeln!(o);
    let how: Vec<String> = by_how.iter().map(|(k, v)| format!("{k} {v}")).collect();
    let _ = writeln!(o, "Call resolution: {}.\n", how.join(", "));
    let _ = writeln!(o, "## Recipes\n");
    let _ = writeln!(
        o,
        "Run from this directory. `\\t` anchors a column, so `rg -P` or `rg` with a literal tab."
    );
    let _ = writeln!(o, "\n```sh");
    let recipes = [
        (
            "Where is function foo defined (any namespace)?",
            "rg -P '^function\\t[^\\t]*\\tfoo\\t' SYMBOLS.tsv",
        ),
        (
            "Where is ns::foo defined?",
            "rg -P '^(function|method)\\tns\\tfoo\\t' SYMBOLS.tsv",
        ),
        (
            "Every function of a namespace",
            "rg -P '^function\\tns\\t' SYMBOLS.tsv | cut -f3,4",
        ),
        (
            "All callers of ns::foo (calls, threads and &pointers)",
            "rg -P '\\tns::foo\\t' CALLS.tsv | cut -f1,4,6",
        ),
        (
            "What does ns::foo call?",
            "rg -P '^ns::foo\\t' CALLS.tsv | cut -f2,3,4",
        ),
        (
            "Who threads ns::foo on a player?",
            "rg -P '\\tns::foo\\t[^\\t]+\\tthread\\tplayer\\t' CALLS.tsv",
        ),
        (
            "Callers of the builtin playfx",
            "rg -P '\\tplayfx\\tbuiltin\\t' CALLS.tsv",
        ),
        (
            "Calls nobody resolves (missing #using, typo, unknown builtin)",
            "rg -P '\\tunresolved\\t' CALLS.tsv | cut -f2 | sort | uniq -c | sort -rn | head",
        ),
        (
            "Who notifies \"round_start\"?",
            "rg -P '^round_start\\tnotify\\t' EVENTS.tsv",
        ),
        (
            "Who waits for or ends on \"death\"?",
            "rg -P '^death\\t(waittill|endon|waittill_)' EVENTS.tsv",
        ),
        (
            "Is \"my_event\" ever notified? (counts and flags)",
            "rg -P '^my_event\\t' EVENTS_SUMMARY.tsv",
        ),
        (
            "Events waited for that no script notifies",
            "rg 'no_script_emitter' EVENTS_SUMMARY.tsv | cut -f1-4",
        ),
        (
            "Every event notify'd on level",
            "rg -P '\\tnotify\\tlevel\\t' EVENTS.tsv | cut -f1 | sort -u",
        ),
        (
            "Who writes level.foo?",
            "rg -P '^level\\tfoo\\twrite' FIELDS.tsv",
        ),
        (
            "Who reads self.foo?",
            "rg -P '^self\\tfoo\\tread\\t' FIELDS.tsv | cut -f4,5",
        ),
        (
            "Which files #using a script?",
            "rg -P '\\tscripts/shared/util_shared\\t' INCLUDES.tsv | cut -f1",
        ),
        (
            "What does a file import?",
            "rg -P '^scripts/zm/_zm\\.gsc\\t' INCLUDES.tsv | cut -f2,3,4",
        ),
        (
            "Autoexec functions",
            "rg -P '\\tautoexec' SYMBOLS.tsv | cut -f2,3,4",
        ),
    ];
    for (q, cmd) in recipes {
        let _ = writeln!(o, "# {q}\n{cmd}");
    }
    let _ = writeln!(o, "```\n");
    let _ = writeln!(o, "## Limits\n");
    let _ = writeln!(
        o,
        "- Resolution is static: `[[ level.callback ]]()` stays `dynamic`; look the field up in \
         FIELDS.tsv (`write` rows) and the `&f` stored into it in CALLS.tsv (`pointer` rows).\n\
         - Wrappers are recognised by name only (list in EVENTS.tsv `op`); a renamed or hashed \
         wrapper (`function_1a2b...`) is an ordinary call and its events are missed.\n\
         - Names built at run time are `<dynamic>`; the `expr` column shows the expression.\n\
         - A bare call resolves in the profile's `bare_call_resolution` order: T7 and later \
         (`namespace-first`) the current namespace, then the builtins, never another namespace \
         (`unresolved` even when a `#using`'d file defines it); IW games: same file, builtins, \
         `#include`'d files; T4, IW3 and T5 (`file-first`) the same, but `thread f()` and \
         `::f` skip the builtins; T6 (`builtin-strict`) the builtins before everything, even \
         the same file (only `path::f` reaches a function named like a builtin), then the same \
         file, then the `#include`'d files, and `thread f()` skips the builtins.\n\
         - Calls, events and fields inside a `#define` body are indexed at each use \
         (`via_macro`), with the macros of the file and of the `#insert`ed headers found in the \
         index; functions a file-scope macro defines are not in SYMBOLS.tsv. Comments and \
         inactive `#if` branches are skipped.\n\
         - Hashed names (`hash_1a2b...`, `function_1a2b...`) are indexed as written."
    );
    o
}

/// Render every file of the index. The tables render in parallel; the result is the same.
pub fn render(ix: &Index, game: &str) -> BTreeMap<&'static str, String> {
    type Render = fn(&Index) -> String;
    let tables: [(&'static str, Render); 6] = [
        ("SYMBOLS.tsv", symbols),
        ("CALLS.tsv", calls),
        ("EVENTS.tsv", events),
        ("EVENTS_SUMMARY.tsv", events_summary),
        ("FIELDS.tsv", fields),
        ("INCLUDES.tsv", includes),
    ];
    let mut m: BTreeMap<&'static str, String> = std::thread::scope(|s| {
        let handles: Vec<_> = tables
            .iter()
            .map(|(name, f)| (*name, s.spawn(move || f(ix))))
            .collect();
        handles
            .into_iter()
            .map(|(name, h)| (name, h.join().expect("a render thread panicked")))
            .collect()
    });
    let readme = readme(ix, &m, game);
    m.insert("README.md", readme);
    m
}

/// One written file: name, bytes and data rows (lines after the header).
#[derive(Clone, Debug)]
pub struct Written {
    pub name: String,
    pub bytes: u64,
    pub rows: usize,
}

/// Write every file of the index into `dir` (created if missing).
pub fn write_dir(ix: &Index, dir: &Path, game: &str) -> std::io::Result<Vec<Written>> {
    std::fs::create_dir_all(dir)?;
    let mut out = Vec::new();
    for (name, text) in render(ix, game) {
        std::fs::write(dir.join(name), text.as_bytes())?;
        out.push(Written {
            name: name.to_owned(),
            bytes: text.len() as u64,
            rows: count_lines(&text),
        });
    }
    Ok(out)
}
