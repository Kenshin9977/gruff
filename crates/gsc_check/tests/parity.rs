//! The rules that follow gscode's checks (I, L002-L005, M, N, U, V002-V005), on hand-written
//! fixtures (`tests/fixtures/parity*`, no game code).

use std::path::PathBuf;

use gsc_check::output::{self, Format};
use gsc_check::{apply_fixes, check, select, FileResult, Layer, Options, Selection};
use gsc_sema::{BareCall, Config, Profile, Resolution, Root, Workspace};
use gsc_syntax::Dialect;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

const CODES: &[&str] = &[
    "I", "L002", "L003", "L004", "L005", "M", "N", "U", "V002", "V003", "V004", "V005",
];

fn selection() -> Selection {
    Selection {
        layers: vec![Layer {
            select: Some(select::parse_list(CODES).unwrap()),
            ..Default::default()
        }],
        preview: true,
    }
}

fn workspace(cfg: Config, roots: &[(&str, bool)]) -> Workspace {
    let profile = Profile::new(Dialect::superset(), None, None).with_config(cfg);
    let mut ws = Workspace::new(profile).with_jobs(1);
    for (dir, library) in roots {
        ws.add_root(Root::new(fixtures().join(dir)).library(*library));
    }
    assert!(ws.load().errors.is_empty());
    let files: Vec<_> = ws.files().collect();
    for f in files {
        let rel = ws.file(f).meta.rel.clone();
        ws.set_display(f, rel);
    }
    ws
}

fn namespaces(thread_call_result: Option<bool>) -> Workspace {
    let mut cfg = Config::names(Resolution::Namespace, BareCall::NamespaceFirst, true);
    cfg.thread_call_result = thread_call_result;
    workspace(cfg, &[("parity", false)])
}

fn run(ws: &Workspace) -> Vec<FileResult> {
    let sel = selection();
    let opts = Options {
        selection: &sel,
        per_file_ignores: &[],
        root: None,
    };
    check(ws, &opts)
}

#[test]
fn parity_rules() {
    let results = run(&namespaces(Some(true)));
    insta::assert_snapshot!(output::render(&results, Format::Concise));
}

#[test]
fn import_fixes() {
    let results = run(&namespaces(Some(true)));
    let main = results
        .iter()
        .find(|r| r.display.ends_with("main.gsc"))
        .unwrap();
    let (fixed, n) = apply_fixes(&main.source.text, &main.diagnostics, false);
    assert_eq!(n, 2, "the duplicate and the unused #using");
    insta::assert_snapshot!(fixed);
}

/// V002 follows the profile: silent where a thread call's value is a `finalization` (Cold War)
/// and where it is unknown.
#[test]
fn thread_call_value_follows_the_profile() {
    for value in [Some(false), None] {
        let codes: Vec<String> = run(&namespaces(value))
            .iter()
            .flat_map(|r| r.diagnostics.iter().map(|d| d.code.clone()))
            .collect();
        assert!(!codes.iter().any(|c| c == "V002"), "{value:?}: {codes:?}");
    }
}

/// `#include` merges what the included file includes: an include used only through its own
/// includes is used.
#[test]
fn unused_include() {
    let cfg = Config::names(Resolution::Include, BareCall::FileFirst, true);
    let results = run(&workspace(cfg, &[("parity_include", false)]));
    insta::assert_snapshot!(output::render(&results, Format::Concise));
}

/// I003 needs the game's scripts as a library root: without one, every stock import would be
/// missing.
#[test]
fn unresolved_import_needs_a_library() {
    let text =
        "#using scripts\\zz\\lib;\n#using scripts\\zz\\no_such_file;\n\nfunction f()\n{\n}\n";
    let codes = |library: bool| -> Vec<String> {
        let cfg = Config::names(Resolution::Namespace, BareCall::NamespaceFirst, true);
        let profile = Profile::new(Dialect::superset(), None, None).with_config(cfg);
        let mut ws = Workspace::new(profile).with_jobs(1);
        let root = ws.add_root(Root::new(std::env::temp_dir().join("gsc-parity-mod")));
        ws.add_root(Root::new(fixtures().join("parity")).library(library));
        ws.load();
        ws.add_file(root, "mod.gsc".into(), "scripts/mod/mod.gsc", text);
        run(&ws)
            .iter()
            .filter(|r| r.display.ends_with("mod.gsc"))
            .flat_map(|r| r.diagnostics.iter().map(|d| d.code.clone()))
            .filter(|c| c == "I003")
            .collect()
    };
    assert_eq!(codes(true), vec!["I003"]);
    assert!(codes(false).is_empty());
}
