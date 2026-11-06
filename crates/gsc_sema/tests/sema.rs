//! Snapshot tests of resolution and IDE queries over hand-written fixtures (tests/fixtures, no
//! game code) and a tiny made-up builtin catalogue.

use std::fmt::Write as _;
use std::path::PathBuf;

use gsc_sema::{
    BareCall, Callee, Catalogue, Config, FieldCatalogue, FileId, NavTarget, Profile, Resolution,
    Root, Symbol, Workspace,
};
use gsc_syntax::{Dialect, TextSize};

const BUILTINS: &str = r#"
game = "fixture"

[[builtin]]
name = "spawnstruct"
vm = "gsc"
kind = "function"
min_args = 0
max_args = 0

[[builtin]]
name = "getentitynumber"
vm = "gsc"
kind = "method"
min_args = 0
max_args = 0

[[builtin]]
name = "getplayers"
vm = "gsc"
kind = "function"
min_args = 0
max_args = 1

[[builtin]]
name = "earthquake"
vm = "gsc"
kind = "function"
min_args = 4
max_args = 4

[[builtin]]
name = "playfx"
vm = "csc"
kind = "function"
min_args = 2
max_args = 6

[[builtin]]
name = "iprintln"
vm = "gsc"
kind = "function"
min_args = 1
max_args = 1
type_flags = 1
"#;

const FIELDS: &str = r#"
game = "fixture"

[[field]]
name = "health"

[[field]]
name = "origin"
"#;

fn fixtures(sub: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(sub)
}

fn profile(resolution: Resolution, bare_call: BareCall) -> Profile {
    let cat = Catalogue::from_toml_str(BUILTINS).unwrap();
    let fields = FieldCatalogue::from_toml_str(FIELDS).unwrap();
    Profile::new(Dialect::superset(), Some(cat), Some(fields))
        .with_config(Config::names(resolution, bare_call, true))
}

/// The T7 mod with the game scripts as a library root.
fn t7() -> Workspace {
    let mut ws =
        Workspace::new(profile(Resolution::Namespace, BareCall::NamespaceFirst)).with_jobs(2);
    ws.add_root(Root::new(fixtures("t7/mod")));
    ws.add_root(Root::new(fixtures("t7/game")).library(true));
    assert!(ws.load().errors.is_empty());
    ws
}

fn include_family(sub: &str) -> Workspace {
    let mut ws = Workspace::new(profile(Resolution::Include, BareCall::BuiltinFirst)).with_jobs(1);
    ws.add_root(Root::new(fixtures(sub)));
    assert!(ws.load().errors.is_empty());
    ws
}

/// A `file-first` game (T4, IW3): the same file, the builtins, the `#include`d files.
fn file_first(sub: &str) -> Workspace {
    let mut ws = Workspace::new(profile(Resolution::Include, BareCall::FileFirst)).with_jobs(1);
    ws.add_root(Root::new(fixtures(sub)));
    assert!(ws.load().errors.is_empty());
    ws
}

/// A `builtin-strict` game (T6): the builtins, the same file, the `#include`d files.
fn builtin_strict(sub: &str) -> Workspace {
    let mut ws = Workspace::new(profile(Resolution::Include, BareCall::BuiltinStrict)).with_jobs(1);
    ws.add_root(Root::new(fixtures(sub)));
    assert!(ws.load().errors.is_empty());
    ws
}

fn line_col(ws: &Workspace, f: FileId, at: TextSize) -> String {
    let text = ws.file(f).text();
    let before = &text[..usize::from(at)];
    let line = before.matches('\n').count() + 1;
    let col = before.len() - before.rfind('\n').map_or(0, |i| i + 1) + 1;
    format!("{line}:{col}")
}

fn place(ws: &Workspace, f: FileId, at: TextSize) -> String {
    format!("{}:{}", ws.file(f).meta.rel, line_col(ws, f, at))
}

fn nav(ws: &Workspace, n: &NavTarget) -> String {
    format!(
        "{:?} {} at {}",
        n.kind,
        n.name,
        place(ws, n.file, n.focus_range.start())
    )
}

/// Every reference of every file with its resolution.
fn dump(ws: &Workspace) -> String {
    let mut out = String::new();
    for f in ws.files() {
        let data = ws.file(f);
        let res = ws.resolution(f);
        let _ = writeln!(
            out,
            "== {} (namespace {})",
            data.meta.rel, data.facts.namespace
        );
        for (imp, found) in data.facts.imports.iter().zip(&res.imports) {
            let to = found.map_or("?".to_owned(), |g| ws.file(g).meta.rel.clone());
            let _ = writeln!(out, "  #{} {} -> {to}", imp.directive, imp.target);
        }
        for (x, r) in data.facts.refs.iter().zip(&res.refs) {
            let def = r.def.map_or(String::new(), |d| {
                format!(" @ {}", place(ws, d.file, ws.def(d).name_range.start()))
            });
            let _ = writeln!(
                out,
                "  {} {} {:?} -> {} {}{def}",
                line_col(ws, f, x.range.start()),
                x.kind,
                x.target,
                r.how.label(),
                r.callee
            );
        }
    }
    out
}

/// The file whose path ends with `rel`, and the offset of `needle` in it plus `delta`.
fn at(ws: &Workspace, rel: &str, needle: &str, delta: u32) -> (FileId, TextSize) {
    let f = ws
        .files()
        .find(|&f| ws.file(f).meta.rel.ends_with(rel))
        .unwrap_or_else(|| panic!("no file {rel}"));
    let i = ws
        .file(f)
        .text()
        .find(needle)
        .unwrap_or_else(|| panic!("no {needle:?} in {rel}"));
    (f, TextSize::from(i as u32 + delta))
}

#[test]
fn t7_resolution() {
    insta::assert_snapshot!(dump(&t7()));
}

#[test]
fn iw_resolution() {
    insta::assert_snapshot!(dump(&include_family("iw")));
}

/// `bare_call_resolution = "file-first"` (T4, IW3, measured in their VMs): an `#include`d
/// function never shadows a builtin; `thread f()` and `::f` skip the builtins.
#[test]
fn file_first_resolution() {
    insta::assert_snapshot!(dump(&file_first("t4")));
}

/// `bare_call_resolution = "builtin-strict"` (T6, measured in its VM): a builtin comes before
/// a function of the same file; `path::f` reaches the function; `thread f()` skips the
/// builtins; `::f` does not.
#[test]
fn builtin_strict_resolution() {
    let ws = builtin_strict("t6_strict");
    insta::assert_snapshot!(dump(&ws));
    // Go-to-definition follows: the bare call has none, the qualified one the file's function.
    let (f, o) = at(&ws, "maps/main.gsc", "p = getplayers", 4);
    assert!(ws.definition(f, o).is_empty());
    let (f, o) = at(&ws, "maps/main.gsc", "main::getplayers", 6);
    let defs: Vec<String> = ws.definition(f, o).iter().map(|d| nav(&ws, d)).collect();
    assert_eq!(defs.len(), 1, "{defs:?}");
    // Only a same-file function shadowed a builtin in `file-first`; nothing does here.
    assert!(ws.builtin_shadow(f, "getplayers").is_none());
    let ff = file_first("t6_strict");
    let (f, _) = at(&ff, "maps/main.gsc", "main()", 0);
    assert!(ff.builtin_shadow(f, "getplayers").is_some());
}

#[test]
fn t6_resolution() {
    insta::assert_snapshot!(dump(&include_family("t6")));
}

#[test]
fn t7_ide() {
    let ws = t7();
    let mut out = String::new();
    let mut q = |label: &str, rel: &str, needle: &str, delta: u32| {
        let (f, o) = at(&ws, rel, needle, delta);
        let _ = writeln!(out, "## {label}: {rel} {needle:?}+{delta}");
        for d in ws.definition(f, o) {
            let _ = writeln!(out, "definition: {}", nav(&ws, &d));
        }
        if let Some(h) = ws.hover(f, o) {
            let _ = writeln!(out, "hover:\n{}", h.markdown());
        }
        if let Some(s) = ws.symbol_at(f, o) {
            let refs: Vec<String> = ws
                .references(&s, true)
                .iter()
                .map(|r| place(&ws, r.file, r.range.start()))
                .collect();
            let _ = writeln!(out, "references: {}", refs.join(", "));
        }
        let _ = writeln!(out);
    };
    q("qualified call", "mymod.gsc", "waittill_any_timeout(", 3);
    q(
        "namespace before builtin",
        "mymod.gsc",
        "s = spawnstruct",
        6,
    );
    q("builtin method", "mymod.gsc", "getentitynumber", 0);
    q("sys builtin", "mymod.gsc", "sys::spawnstruct", 6);
    q("macro from #insert", "mymod.gsc", "IS_TRUE", 1);
    q(
        "same namespace, other file",
        "mymod.gsc",
        "helper_in_namespace",
        0,
    );
    q("#using'd file", "mymod.gsc", "helper_from_using", 0);
    q("builtin before #using", "mymod.gsc", "earthquake", 0);
    q("other-VM builtin", "mymod.gsc", "playfx", 0);
    q("class method via base", "mymod.gsc", "toggle();", 0);
    q("unresolved", "mymod.gsc", "nobody_defines_this", 0);
    q("function pointer", "mymod.gsc", "&watch", 1);
    q("definition name", "mymod.gsc", "function watch", 9);
    q("parameter", "mymod.gsc", "count + player", 0);
    q("foreach binding", "mymod.gsc", "player.score", 0);
    q("waittill binding", "mymod.gsc", "who.health", 0);
    q("engine field", "mymod.gsc", "who.health", 4);
    q("script field", "util_shared.gsc", "util_ready", 0);
    q("import path", "mymod.gsc", "util_shared;", 2);
    q(
        "/// doc and autoexec",
        "util_shared.gsc",
        "function autoexec init",
        18,
    );
    q(
        "namespace before builtin, other file",
        "mymod_more.gsc",
        "return spawnstruct",
        7,
    );
    q(
        "builtin before a #using'd function",
        "mymod_helpers.gsc",
        "return spawnstruct",
        7,
    );
    insta::assert_snapshot!(out);
}

#[test]
fn t7_outline_symbols_completion_calls() {
    let ws = t7();
    let mut out = String::new();
    let (f, _) = at(&ws, "mymod.gsc", "#namespace", 0);
    let _ = writeln!(out, "## document_symbols mymod.gsc");
    for s in ws.document_symbols(f) {
        let _ = writeln!(out, "{:?} {} ({})", s.kind, s.name, s.detail);
        for c in &s.children {
            let _ = writeln!(out, "  {:?} {} ({})", c.kind, c.name, c.detail);
        }
    }
    let _ = writeln!(out, "\n## workspace_symbols \"wat\"");
    for n in ws.workspace_symbols("wat", 10) {
        let _ = writeln!(out, "{}", nav(&ws, &n));
    }
    let _ = writeln!(out, "\n## workspace_symbols \"util::\"");
    for n in ws.workspace_symbols("util::", 10) {
        let _ = writeln!(out, "{}", nav(&ws, &n));
    }
    let mut complete = |label: &str, needle: &str, delta: u32| {
        let (f, o) = at(&ws, "mymod.gsc", needle, delta);
        let items: Vec<String> = ws
            .completions(f, o)
            .iter()
            .map(|c| format!("{}:{:?}", c.label, c.kind))
            .collect();
        let _ = writeln!(out, "\n## completions {label}\n{}", items.join(" "));
    };
    complete("in watch, typed `to`", "total = count", 2);
    complete("after `who.`", "who.health", 4);
    complete("after `util::`", "util::waittill", 6);
    complete("in a method, typed `tog`", "toggle();", 3);
    complete("bare, typed `hel`", "helper_from_using();", 3);

    let (f, o) = at(&ws, "mymod.gsc", "function watch", 9);
    let target = ws.call_hierarchy_target(f, o).unwrap();
    let _ = writeln!(out, "\n## call hierarchy of {}", ws.def(target).qualified());
    for c in ws.incoming_calls(target) {
        let from = c
            .from
            .map_or("<file>".to_owned(), |d| ws.def(d).qualified());
        let sites: Vec<String> = c
            .ranges
            .iter()
            .map(|r| line_col(&ws, c.file, r.start()))
            .collect();
        let _ = writeln!(out, "incoming from {from} at {}", sites.join(", "));
    }
    let (f, o) = at(&ws, "mymod.gsc", "function autoexec main", 18);
    let main = ws.call_hierarchy_target(f, o).unwrap();
    for c in ws.outgoing_calls(main) {
        let to = match &c.to {
            Callee::Def(d) => ws.def(*d).qualified(),
            Callee::Builtin(b) => format!("builtin {b}"),
            Callee::Other(o) => format!("other {o}"),
        };
        let _ = writeln!(out, "outgoing to {to} x{}", c.ranges.len());
    }
    insta::assert_snapshot!(out);
}

#[test]
fn incremental_update_matches_a_fresh_load() {
    let mut ws = t7();
    let (helpers, _) = at(&ws, "mymod_helpers.gsc", "#namespace", 0);
    let (main, o) = at(&ws, "mymod.gsc", "nobody_defines_this", 0);
    let how = |ws: &Workspace| {
        let s = ws.symbol_at(main, o);
        let i = ws
            .file(main)
            .facts
            .refs
            .iter()
            .position(|r| r.range.contains(o))
            .unwrap();
        (ws.resolution(main).refs[i].how, s)
    };
    assert_eq!(how(&ws).0, gsc_sema::How::Unresolved);
    // Defined in the #using'd file's own namespace: a bare call still does not reach it.
    let other = format!(
        "{}\nfunction nobody_defines_this()\n{{\n}}\n",
        ws.file(helpers).text()
    );
    ws.set_text(helpers, &other);
    assert_eq!(how(&ws).0, gsc_sema::How::Unresolved);
    let found = ws.other_namespace_fns(main, "mymod", "nobody_defines_this");
    assert_eq!(found.len(), 1);
    assert_eq!(ws.def(found[0]).ns, "mymod_helpers");
    // Defined in the caller's namespace: the call now resolves there.
    let text = format!(
        "{}\n#namespace mymod;\nfunction nobody_defines_this()\n{{\n}}\n",
        ws.file(helpers).text()
    );
    ws.set_text(helpers, &text);
    let (h, s) = how(&ws);
    assert_eq!(h, gsc_sema::How::Namespace);
    assert!(matches!(s, Some(Symbol::Def(d)) if d.file == helpers));
    let after = dump(&ws);
    // The same as loading the edited tree from scratch.
    let mut fresh =
        Workspace::new(profile(Resolution::Namespace, BareCall::NamespaceFirst)).with_jobs(2);
    let root = fresh.add_root(Root::new(fixtures("t7/mod")));
    fresh.add_root(Root::new(fixtures("t7/game")).library(true));
    fresh.load();
    let path = ws.file(helpers).meta.path.clone();
    fresh.add_file(root, path, "scripts/zm/mymod_helpers.gsc", &text);
    assert_eq!(after, dump(&fresh));
    // Removing the file makes it unresolved again.
    ws.remove_file(helpers);
    assert_eq!(how(&ws).0, gsc_sema::How::Unresolved);
    assert!(ws.get(helpers).is_none());
}

#[test]
fn library_roots_are_not_primary() {
    let ws = t7();
    let primary: Vec<String> = ws
        .primary_files()
        .map(|f| ws.file(f).meta.rel.clone())
        .collect();
    assert_eq!(
        primary,
        vec![
            "scripts/zm/mymod.gsc",
            "scripts/zm/mymod_helpers.gsc",
            "scripts/zm/mymod_more.gsc"
        ]
    );
    assert_eq!(ws.len(), 6);
}
