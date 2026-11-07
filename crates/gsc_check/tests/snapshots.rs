//! Snapshot tests over hand-written fixtures (`tests/fixtures`, no game code) and a tiny
//! made-up builtin catalogue.

use std::path::{Path, PathBuf};

use gsc_check::output::{self, Format};
use gsc_check::{apply_fixes, check, select, Catalogue, FileResult, Layer, Options, Selection};
use gsc_sema::{Profile, Root, Workspace};
use gsc_syntax::Dialect;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn field_catalogue() -> gsc_check::FieldCatalogue {
    gsc_check::FieldCatalogue::load(&fixtures().join("fields.toml")).unwrap()
}

fn catalogue() -> Catalogue {
    Catalogue::load(&fixtures().join("builtins.toml"))
        .unwrap()
        .with_intrinsics(["gettime".to_owned()])
}

/// A workspace over fixture roots (the last `library` of them library roots), files displayed
/// relative to `tests/fixtures`.
fn workspace(dirs: &[&str], library: usize, jobs: usize) -> Workspace {
    let profile = Profile::new(
        Dialect::superset(),
        Some(catalogue()),
        Some(field_catalogue()),
    );
    let mut ws = Workspace::new(profile).with_jobs(jobs);
    for (i, d) in dirs.iter().enumerate() {
        ws.add_root(Root::new(fixtures().join(d)).library(i + library >= dirs.len()));
    }
    let report = ws.load();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let files: Vec<_> = ws.files().collect();
    for f in files {
        let path = ws.file(f).meta.path.clone();
        let rel = path.strip_prefix(fixtures()).unwrap();
        ws.set_display(f, rel.to_string_lossy().replace('\\', "/"));
    }
    ws
}

fn basic(jobs: usize) -> Workspace {
    workspace(&["basic"], 0, jobs)
}

fn run(ws: &Workspace, selection: &Selection) -> Vec<FileResult> {
    let opts = Options {
        selection,
        per_file_ignores: &[],
        root: None,
    };
    check(ws, &opts)
}

fn preview() -> Selection {
    Selection {
        layers: vec![],
        preview: true,
    }
}

#[test]
fn text_output() {
    let results = run(&basic(2), &preview());
    insta::assert_snapshot!(output::render(&results, Format::Text));
}

#[test]
fn machine_outputs() {
    let results = run(&basic(2), &preview());
    insta::assert_snapshot!("json", output::render(&results, Format::Json));
    insta::assert_snapshot!("github", output::render(&results, Format::Github));
    insta::assert_snapshot!("sarif", output::render(&results, Format::Sarif));
    insta::assert_snapshot!("statistics", output::statistics(&results));
}

#[test]
fn preview_rules_are_off_by_default() {
    let results = run(&basic(2), &Selection::default());
    let codes: Vec<&str> = results
        .iter()
        .flat_map(|r| &r.diagnostics)
        .map(|d| d.code.as_str())
        .collect();
    assert!(!codes.contains(&"B003"));
    assert!(codes.contains(&"B001"));
}

#[test]
fn select_and_ignore() {
    let sel = Selection {
        layers: vec![Layer {
            select: Some(select::parse_list(&["B", "infinite-loop-without-wait"]).unwrap()),
            ignore: select::parse_list(&["B001"]).unwrap(),
            ..Layer::default()
        }],
        preview: false,
    };
    let results = run(&basic(2), &sel);
    let mut codes: Vec<&str> = results
        .iter()
        .flat_map(|r| &r.diagnostics)
        .map(|d| d.code.as_str())
        .collect();
    codes.sort();
    codes.dedup();
    assert_eq!(codes, vec!["B002", "L001"]);
}

#[test]
fn per_file_ignores() {
    let sel = Selection::default();
    let pfi = vec![("*.csc".to_owned(), select::parse_list(&["B"]).unwrap())];
    let root = fixtures();
    let opts = Options {
        selection: &sel,
        per_file_ignores: &pfi,
        root: Some(&root),
    };
    let results = check(&basic(1), &opts);
    let csc = results
        .iter()
        .find(|r| r.display.ends_with("client.csc"))
        .unwrap();
    assert!(csc.diagnostics.is_empty(), "{:?}", csc.diagnostics);
}

#[test]
fn fixes() {
    let mut ws = basic(2);
    let results = run(&ws, &preview());
    let demo = results
        .iter()
        .find(|r| r.display.ends_with("demo.gsc"))
        .unwrap();
    let (safe, n_safe) = apply_fixes(&demo.source.text, &demo.diagnostics, false);
    let (all, n_all) = apply_fixes(&demo.source.text, &demo.diagnostics, true);
    assert_eq!(n_safe, 3);
    assert!(n_all > n_safe);
    insta::assert_snapshot!("fixed_safe", safe);
    insta::assert_snapshot!("fixed_unsafe", all);

    // After the safe fixes nothing fixable-safe is left, and the unsafe result re-parses clean.
    ws.set_text(demo.file, &all);
    let after = run(&ws, &preview());
    let demo = after
        .iter()
        .find(|r| r.display.ends_with("demo.gsc"))
        .unwrap();
    assert!(demo
        .diagnostics
        .iter()
        .all(|d| d.code != "L001" && !d.code.starts_with('E')));
}

#[test]
fn output_does_not_depend_on_threads() {
    let one = output::render(&run(&basic(1), &preview()), Format::Json);
    let many = output::render(&run(&basic(4), &preview()), Format::Json);
    assert_eq!(one, many);
}

#[test]
fn library_roots_resolve_but_are_not_checked() {
    let codes = |ws: &Workspace| -> Vec<(String, String)> {
        run(ws, &preview())
            .iter()
            .flat_map(|r| {
                r.diagnostics
                    .iter()
                    .map(|d| (r.display.clone(), d.code.clone()))
            })
            .collect()
    };
    // Alone, the mod calls a function nothing defines, but its `#using` is missing from the
    // workspace: the function may be there, so no B001; a loop calling it seems to yield.
    let alone = codes(&workspace(&["library/mod"], 0, 1));
    assert_eq!(alone, vec![]);
    // With the game scripts as a library root, the function is found, its summary says it
    // never waits, and the library file itself (which has an L001 of its own) is not reported.
    let with = codes(&workspace(&["library/mod", "library/game"], 1, 1));
    assert_eq!(
        with,
        vec![(
            "library/mod/scripts/mod_main.gsc".to_owned(),
            "L001".to_owned()
        )]
    );
}

#[test]
fn explain_every_rule() {
    for r in gsc_check::RULES {
        let page = output::explain(r.code).unwrap();
        assert!(page.contains("## How to suppress"), "{}", r.code);
        assert_eq!(output::explain(r.name).unwrap(), page);
    }
    assert!(output::explain("E0999").is_some());
    assert!(output::explain("Z999").is_none());
}

#[test]
fn rule_list() {
    insta::assert_snapshot!(output::rule_list());
}

/// `$GRUFF_DATA` or `$GSC_DATA`, else a `gruff-data` (or the older `gsc-data`) folder next to the
/// repository.
fn data_root() -> PathBuf {
    if let Some(d) = ["GRUFF_DATA", "GSC_DATA"]
        .iter()
        .find_map(|v| std::env::var_os(v).filter(|d| !d.is_empty()))
    {
        return PathBuf::from(d);
    }
    let up = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    ["gruff-data", "gsc-data"]
        .iter()
        .map(|n| up.join(n))
        .find(|d| d.is_dir())
        .unwrap_or_else(|| up.join("gruff-data"))
}

/// Run on a local corpus: `GSC_CORPUS=.../corpus GRUFF_DATA=.../gruff-data cargo test --release -p
/// gsc_check --test snapshots -- --ignored --nocapture`.
#[test]
#[ignore]
fn corpus() {
    let Some(corpus) = std::env::var_os("GSC_CORPUS").map(PathBuf::from) else {
        eprintln!("GSC_CORPUS is not set");
        return;
    };
    let data = data_root();
    let dialect = Dialect::find(&data.join("profiles"), "t7").unwrap();
    let cat = Catalogue::load(&data.join("builtins/t7.toml")).unwrap();
    let fields = Some(data.join("fields/t7.toml"))
        .filter(|p| p.is_file())
        .map(|p| gsc_check::FieldCatalogue::load(&p).unwrap());
    let profile = Profile::new(dialect, Some(cat), fields);
    // Budgets with preview rules on; the clean source must have no syntax or builtin finding.
    for (dir, budget) in [("t7/source-modtools", 20usize), ("t7/decompiled", 400)] {
        let path: &Path = &corpus.join(dir);
        if !path.is_dir() {
            eprintln!("{} is missing", path.display());
            continue;
        }
        let mut ws = Workspace::new(profile.clone());
        ws.add_root(Root::new(path));
        ws.load();
        let results = run(&ws, &preview());
        let total: usize = results.iter().map(|r| r.diagnostics.len()).sum();
        eprintln!("{dir}: {} files, {total} diagnostics", ws.len());
        eprint!("{}", output::statistics(&results));
        if dir.contains("source") {
            let bad = results
                .iter()
                .flat_map(|r| &r.diagnostics)
                .filter(|d| d.code.starts_with('B') || d.code.starts_with('E'))
                .count();
            assert_eq!(bad, 0, "{dir}: syntax or builtin findings in clean source");
        }
        assert!(
            total <= budget,
            "{dir}: {total} diagnostics, budget {budget}"
        );
    }
}

#[test]
fn decompiler_names_skip_tree_rules() {
    let profile = Profile::new(Dialect::superset(), Some(catalogue()), None);
    let mut ws = Workspace::new(profile)
        .with_jobs(1)
        .with_decompiled(gsc_check::Decompiled::On);
    let root = ws.add_root(Root::new("."));
    let text = "function f()\n{\n\tinit heli sounds max_accuracy = 1;\n\twhile ( true )\n\t{\n\t\tno_such_builtin();\n\t}\n}\n";
    ws.add_file(root, "a.gsc".into(), "a.gsc", text);
    let codes: Vec<String> = run(&ws, &Selection::default())
        .iter()
        .flat_map(|r| r.diagnostics.iter().map(|d| d.code.clone()))
        .collect();
    assert!(codes.iter().any(|c| c == "E0301"), "{codes:?}");
    assert!(
        codes
            .iter()
            .all(|c| !c.starts_with('B') && !c.starts_with('L')),
        "{codes:?}"
    );
}

/// B004 and B005 under `bare_call_resolution = "namespace-first"` (T7+); silent under
/// `builtin-first`.
#[test]
fn namespace_first_bare_calls() {
    use gsc_sema::{BareCall, Config, Resolution};
    let load = |bare_call: BareCall| {
        let profile = Profile::new(Dialect::superset(), Some(catalogue()), None)
            .with_config(Config::names(Resolution::Namespace, bare_call, true));
        let mut ws = Workspace::new(profile).with_jobs(1);
        ws.add_root(Root::new(fixtures().join("namespaces")));
        assert!(ws.load().errors.is_empty());
        let files: Vec<_> = ws.files().collect();
        for f in files {
            let rel = ws.file(f).meta.rel.clone();
            ws.set_display(f, rel);
        }
        ws
    };
    let selection = Selection {
        layers: vec![Layer {
            select: Some(select::parse_list(&["B"]).unwrap()),
            ..Default::default()
        }],
        preview: false,
    };
    let results = run(&load(BareCall::NamespaceFirst), &selection);
    insta::assert_snapshot!(output::render(&results, Format::Concise));
    let legacy = run(&load(BareCall::BuiltinFirst), &selection);
    let codes: Vec<&str> = legacy
        .iter()
        .flat_map(|r| &r.diagnostics)
        .map(|d| d.code.as_str())
        .filter(|c| matches!(*c, "B004" | "B005"))
        .collect();
    assert!(codes.is_empty(), "{codes:?}");
}

/// B005 and B002 under `bare_call_resolution = "file-first"` (T4, IW3): `thread f()` and `::f`
/// never reach a builtin, and an `#include`d function does not shadow one for a plain call.
/// Under `builtin-first` neither B005 nor the B002 fires.
#[test]
fn file_first_bare_calls() {
    use gsc_sema::{BareCall, Config, Resolution};
    let load = |bare_call: BareCall| {
        let profile = Profile::new(Dialect::superset(), Some(catalogue()), None)
            .with_config(Config::names(Resolution::Include, bare_call, true));
        let mut ws = Workspace::new(profile).with_jobs(1);
        ws.add_root(Root::new(fixtures().join("file_first")));
        assert!(ws.load().errors.is_empty());
        let files: Vec<_> = ws.files().collect();
        for f in files {
            let rel = ws.file(f).meta.rel.clone();
            ws.set_display(f, rel);
        }
        ws
    };
    let selection = Selection {
        layers: vec![Layer {
            select: Some(select::parse_list(&["B"]).unwrap()),
            ..Default::default()
        }],
        preview: false,
    };
    let results = run(&load(BareCall::FileFirst), &selection);
    insta::assert_snapshot!(output::render(&results, Format::Concise));
    let legacy = run(&load(BareCall::BuiltinFirst), &selection);
    let codes: Vec<&str> = legacy
        .iter()
        .flat_map(|r| &r.diagnostics)
        .map(|d| d.code.as_str())
        .collect();
    assert!(codes.is_empty(), "{codes:?}");
}

/// B005 and B002 under `bare_call_resolution = "builtin-strict"` (T6): `thread f()` never
/// reaches a builtin, `::f` takes the builtin's native handler, and no script function, even
/// one of the same file, shadows a builtin for a plain call. Under `builtin-first` neither B005
/// nor the B002 fires.
#[test]
fn builtin_strict_bare_calls() {
    use gsc_sema::{BareCall, Config, Resolution};
    let load = |bare_call: BareCall| {
        let profile = Profile::new(Dialect::superset(), Some(catalogue()), None)
            .with_config(Config::names(Resolution::Include, bare_call, true));
        let mut ws = Workspace::new(profile).with_jobs(1);
        ws.add_root(Root::new(fixtures().join("builtin_strict")));
        assert!(ws.load().errors.is_empty());
        let files: Vec<_> = ws.files().collect();
        for f in files {
            let rel = ws.file(f).meta.rel.clone();
            ws.set_display(f, rel);
        }
        ws
    };
    let selection = Selection {
        layers: vec![Layer {
            select: Some(select::parse_list(&["B"]).unwrap()),
            ..Default::default()
        }],
        preview: false,
    };
    let results = run(&load(BareCall::BuiltinStrict), &selection);
    insta::assert_snapshot!(output::render(&results, Format::Concise));
    let legacy = run(&load(BareCall::BuiltinFirst), &selection);
    let codes: Vec<&str> = legacy
        .iter()
        .flat_map(|r| &r.diagnostics)
        .map(|d| d.code.as_str())
        .collect();
    assert!(codes.is_empty(), "{codes:?}");
}

/// Game modes in a `builtin-strict` game: each file sees the builtins of its mode, from its path
/// (`maps/mp/` multiplayer, `zombies` zombies, other `maps/` campaign, `common_scripts/` none)
/// or from `game-mode`; B002 takes the mode's `mode_args`; B006 reports the multiplayer
/// function a builtin shadows, not the campaign one no campaign builtin shadows.
#[test]
fn game_modes() {
    use gsc_sema::{BareCall, Config, Mode, ModeOverrides, Resolution};
    let load = |modes: ModeOverrides| {
        let mut dialect = Dialect::superset();
        dialect.id = "t6".to_owned();
        let cat = Catalogue::load(&fixtures().join("modes.toml")).unwrap();
        let profile = Profile::new(dialect, Some(cat), None)
            .with_config(Config::names(
                Resolution::Include,
                BareCall::BuiltinStrict,
                true,
            ))
            .with_modes(modes);
        let mut ws = Workspace::new(profile).with_jobs(1);
        ws.add_root(Root::new(fixtures().join("modes")));
        assert!(ws.load().errors.is_empty());
        let files: Vec<_> = ws.files().collect();
        for f in files {
            let rel = ws.file(f).meta.rel.clone();
            ws.set_display(f, rel);
        }
        ws
    };
    let selection = Selection {
        layers: vec![Layer {
            select: Some(select::parse_list(&["B"]).unwrap()),
            ..Default::default()
        }],
        preview: true,
    };
    let ws = load(ModeOverrides::default());
    let modes: Vec<(String, Option<Mode>)> = ws
        .files()
        .map(|f| (ws.file(f).meta.rel.clone(), ws.mode_of(f)))
        .collect();
    assert!(modes.contains(&("maps/mp/zombies/_zm.gsc".to_owned(), Some(Mode::Zm))));
    assert!(modes.contains(&("common_scripts/shared.gsc".to_owned(), None)));
    insta::assert_snapshot!(output::render(&run(&ws, &selection), Format::Concise));
    // Every file read as a campaign script: the multiplayer file's calls are now fine (its
    // `vectorcross` is the function), the zombies file's `getaiarray` too; only `randomint`.
    let all_sp = load(ModeOverrides {
        globs: vec![],
        all: Some(Some(Mode::Sp)),
    });
    let codes: Vec<(String, String)> = run(&all_sp, &selection)
        .iter()
        .flat_map(|r| {
            r.diagnostics
                .iter()
                .map(|d| (r.display.clone(), d.code.clone()))
        })
        .collect();
    assert_eq!(
        codes,
        vec![("maps/_sp.gsc".to_owned(), "B002".to_owned())],
        "{codes:?}"
    );
}

/// B001 stays quiet on a name no builtin has when the file (or a file it imports) has an import
/// missing from the workspace: the name may be a function of that file. Builtins keep B002.
#[test]
fn b001_with_a_missing_import() {
    let ws = workspace(&["missing_import"], 0, 1);
    let codes: Vec<(String, String)> = run(&ws, &preview())
        .iter()
        .flat_map(|r| {
            r.diagnostics
                .iter()
                .filter(|d| d.code.starts_with('B'))
                .map(|d| (r.display.clone(), d.code.clone()))
        })
        .collect();
    assert_eq!(
        codes,
        vec![
            (
                "missing_import/maps/complete.gsc".to_owned(),
                "B001".to_owned()
            ),
            (
                "missing_import/maps/with_missing.gsc".to_owned(),
                "B002".to_owned()
            ),
        ],
        "{codes:?}"
    );
}

/// V001 under `hash_equality = "script-error"` (T7, measured in the emulated VM): `==` with a
/// hash literal; `!=` and string comparisons are left alone. Silent when hashes compare by value.
#[test]
fn hash_equality() {
    use gsc_sema::{BareCall, Config, Resolution};
    let load = |error: bool| {
        let mut cfg = Config::names(Resolution::Namespace, BareCall::NamespaceFirst, true);
        cfg.hash_equality_error = error;
        let profile = Profile::new(Dialect::superset(), None, None).with_config(cfg);
        let mut ws = Workspace::new(profile).with_jobs(1);
        ws.add_root(Root::new(fixtures().join("values")));
        assert!(ws.load().errors.is_empty());
        let files: Vec<_> = ws.files().collect();
        for f in files {
            let rel = ws.file(f).meta.rel.clone();
            ws.set_display(f, rel);
        }
        ws
    };
    let selection = Selection {
        layers: vec![Layer {
            select: Some(select::parse_list(&["V"]).unwrap()),
            ..Default::default()
        }],
        preview: true,
    };
    insta::assert_snapshot!(output::render(
        &run(&load(true), &selection),
        Format::Concise
    ));
    let by_value = run(&load(false), &selection);
    assert!(by_value.iter().all(|r| r.diagnostics.is_empty()));
}
