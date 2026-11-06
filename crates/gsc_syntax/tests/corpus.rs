//! Corpus tests, ignored by default because the corpora are local game data that never enter the
//! repository. Run them with the paths in environment variables:
//!
//! ```sh
//! GSC_CORPUS=.../corpus GSC_T7_SRC=.../t7-src/scripts GRUFF_DATA=.../gruff-data \
//!     cargo test --release -p gsc_syntax --test corpus -- --ignored --nocapture
//! ```
//!
//! `GSC_CORPUS` is the folder holding `t7/source-modtools`, `t7/decompiled` and one folder per
//! game; `GRUFF_DATA` defaults to `../gruff-data` next to the repository.

use std::path::{Path, PathBuf};

use gsc_syntax::{
    check_dialect, header_paths, is_binary, looks_decompiled, macro_expansions_with, parse,
    Dialect, HeaderSet, SourceText, SyntaxKind,
};

fn env_dir(var: &str) -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os(var)?);
    p.is_dir().then_some(p)
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

fn profiles() -> PathBuf {
    data_root().join("profiles")
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            files(&p, out);
        } else if p
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| ["gsc", "csc", "gsh"].contains(&x.to_ascii_lowercase().as_str()))
        {
            out.push(p);
        }
    }
}

#[derive(Default, Debug)]
struct Report {
    files: usize,
    binary: usize,
    error_files: Vec<PathBuf>,
    /// Files with an error other than E0301 (a decompiler name no tokenizer can read).
    other_error_files: Vec<PathBuf>,
    dialect_files: usize,
    round_trip_failures: Vec<PathBuf>,
}

fn run(dir: &Path, dialect: &Dialect) -> Report {
    let decompiled = dialect.clone().with_decompiled(true);
    let mut list = Vec::new();
    files(dir, &mut list);
    let mut r = Report::default();
    for path in list {
        let bytes = std::fs::read(&path).unwrap();
        if is_binary(&bytes) {
            r.binary += 1;
            continue;
        }
        r.files += 1;
        let src = SourceText::decode(&bytes);
        let d = if looks_decompiled(&src.text) {
            &decompiled
        } else {
            dialect
        };
        let p = parse(&src.text, d);
        let root = p.syntax();
        if src.encode(&root.to_string()) != bytes {
            r.round_trip_failures.push(path.clone());
        }
        if !p.errors().is_empty() {
            r.error_files.push(path.clone());
            if !p.errors().iter().all(|e| e.is_decompiler_name()) {
                r.other_error_files.push(path.clone());
            }
        }
        if !check_dialect(&root, d).is_empty() {
            r.dialect_files += 1;
        }
    }
    println!(
        "{}: {} files ({} binary skipped), {} with syntax errors ({:.2}%), {} with dialect \
         diagnostics, {} round-trip failures",
        dir.display(),
        r.files,
        r.binary,
        r.error_files.len(),
        100.0 * r.error_files.len() as f64 / r.files.max(1) as f64,
        r.dialect_files,
        r.round_trip_failures.len()
    );
    for f in r.error_files.iter().take(20) {
        println!("  error: {}", f.display());
    }
    r
}

#[test]
#[ignore = "needs GSC_CORPUS"]
fn t7_modtools_sources_are_clean() {
    let Some(corpus) = env_dir("GSC_CORPUS") else {
        panic!("set GSC_CORPUS")
    };
    let t7 = Dialect::find(&profiles(), "t7").unwrap();
    let r = run(&corpus.join("t7/source-modtools"), &t7);
    assert!(r.files > 900);
    assert!(r.error_files.is_empty(), "{:?}", r.error_files);
    assert_eq!(r.dialect_files, 0);
    assert!(r.round_trip_failures.is_empty());
}

/// Decompiled T7 dumps: every file round-trips. 11 files hold names that a hash lookup resolved
/// to text with spaces (`init heli sounds max_accuracy`), which no GSC lexer can read: their only
/// errors are E0301. `GSC_MAX_ERROR_FILES` (default 11) bounds them; no other error is allowed.
fn decompiled(dir: &Path) {
    let t7 = Dialect::find(&profiles(), "t7").unwrap();
    let r = run(dir, &t7);
    assert!(r.files > 2000);
    assert!(
        r.round_trip_failures.is_empty(),
        "{:?}",
        r.round_trip_failures
    );
    let max: usize = std::env::var("GSC_MAX_ERROR_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(11);
    assert!(
        r.error_files.len() <= max,
        "{} files with errors",
        r.error_files.len()
    );
}

#[test]
#[ignore = "needs GSC_CORPUS"]
fn t7_decompiled_corpus() {
    let corpus = env_dir("GSC_CORPUS").expect("set GSC_CORPUS");
    decompiled(&corpus.join("t7/decompiled"));
}

#[test]
#[ignore = "needs GSC_T7_SRC"]
fn t7_src_scripts() {
    decompiled(&env_dir("GSC_T7_SRC").expect("set GSC_T7_SRC"));
}

/// Every game folder of the corpus with its profile: round-trip is exact everywhere; the error
/// rate is printed, not asserted.
#[test]
#[ignore = "needs GSC_CORPUS"]
fn every_game_round_trips() {
    let corpus = env_dir("GSC_CORPUS").expect("set GSC_CORPUS");
    let mut games: Vec<PathBuf> = std::fs::read_dir(&corpus)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    games.sort();
    for g in games {
        let name = g.file_name().unwrap().to_string_lossy().into_owned();
        let profile = match name.as_str() {
            "jup_sp" => "jup",
            "codol" => "iw5",
            other => other,
        };
        let Ok(d) = Dialect::find(&profiles(), profile) else {
            println!("{name}: no profile, skipped");
            continue;
        };
        let r = run(&g, &d);
        assert!(
            r.round_trip_failures.is_empty(),
            "{name}: {:?}",
            r.round_trip_failures
        );
    }
}

/// Macro expansion over the mod-tools sources: headers found through `#insert`, every use
/// expanded, and the expansions parse as statements.
#[test]
#[ignore = "needs GSC_CORPUS"]
fn t7_modtools_macros_expand() {
    let corpus = env_dir("GSC_CORPUS").expect("set GSC_CORPUS");
    let t7 = Dialect::find(&profiles(), "t7").unwrap();
    let mut list = Vec::new();
    files(&corpus.join("t7/source-modtools"), &mut list);
    let mut parsed = Vec::new();
    let mut headers = HeaderSet::new();
    for path in &list {
        let bytes = std::fs::read(path).unwrap();
        if is_binary(&bytes) {
            continue;
        }
        let p = parse(&SourceText::decode(&bytes).text, &t7);
        if path.extension().is_some_and(|x| x == "gsh") {
            headers.add(&path.to_string_lossy(), &p.syntax(), &t7);
        }
        parsed.push((path.clone(), p));
    }
    let (mut uses, mut bad, mut calls) = (0usize, 0usize, 0usize);
    for (path, p) in &parsed {
        let root = p.syntax();
        let table = headers.macros_for(&header_paths(&root));
        for e in macro_expansions_with(&root, &table, &t7) {
            uses += 1;
            let ep = e.parse(&t7);
            calls += ep
                .syntax()
                .descendants()
                .filter(|n| n.kind() == SyntaxKind::CALL_EXPR)
                .count();
            if !e.complete || !ep.errors().is_empty() {
                bad += 1;
                if bad <= 10 {
                    println!("  {}: {} => {}", path.display(), e.name, e.text());
                }
            }
        }
    }
    println!("{uses} macro uses expanded, {bad} do not parse as statements, {calls} calls inside");
    assert!(uses > 10_000);
    assert_eq!(bad, 0, "{bad} of {uses} expansions do not parse");
}
