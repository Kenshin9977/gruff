//! Snapshot tests over the fixtures in tests/data (all written for this project).
//!
//! `ok/` must parse without errors, `err/` exercises recovery, `decompiled/` uses the decompiler
//! lexing mode. Each snapshot holds the diagnostics and the full tree.

use std::path::{Path, PathBuf};

use gsc_syntax::{check_dialect, parse, Dialect, LineIndex, Parse, SyntaxError};

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn fixtures(sub: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(data_dir().join(sub))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "gsc"))
        .collect();
    v.sort();
    v
}

fn read(path: &Path) -> String {
    // Fixtures are stored with LF; normalise in case a checkout converted them.
    std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
}

pub fn render(text: &str, parse: &Parse, extra: &[SyntaxError]) -> String {
    let index = LineIndex::new(text);
    let mut out = String::new();
    for e in parse.errors().iter().chain(extra) {
        let lc = index.line_col(text, e.range.start());
        out.push_str(&format!(
            "{}:{}: {} {}\n",
            lc.line, lc.col, e.code, e.message
        ));
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&parse.debug_tree());
    out
}

fn snapshot(sub: &str, dialect: &Dialect, expect_clean: bool) {
    let files = fixtures(sub);
    assert!(!files.is_empty());
    for path in files {
        let text = read(&path);
        let p = parse(&text, dialect);
        assert_eq!(
            p.syntax().to_string(),
            text,
            "{} round-trips",
            path.display()
        );
        if expect_clean {
            assert!(
                p.errors().is_empty(),
                "{}: {:?}",
                path.display(),
                p.errors()
            );
        } else {
            assert!(
                !p.errors().is_empty(),
                "{} should have errors",
                path.display()
            );
        }
        let name = format!("{sub}__{}", path.file_stem().unwrap().to_string_lossy());
        insta::assert_snapshot!(
            name,
            render(&text, &p, &check_dialect(&p.syntax(), dialect))
        );
    }
}

#[test]
fn ok_fixtures() {
    snapshot("ok", &Dialect::superset(), true);
}

#[test]
fn err_fixtures() {
    snapshot("err", &Dialect::superset(), false);
}

#[test]
fn decompiled_fixtures() {
    snapshot(
        "decompiled",
        &Dialect::superset().with_decompiled(true),
        true,
    );
}

#[test]
fn crlf_and_windows_1252_round_trip() {
    let bytes = b"function f()\r\n{\r\n\tx = \"caf\xe9\"; // \x93quoted\x94\r\n}\r\n".to_vec();
    let src = gsc_syntax::SourceText::decode(&bytes);
    assert_eq!(src.encoding, gsc_syntax::Encoding::Windows1252);
    let p = parse(&src.text, &Dialect::superset());
    assert!(p.errors().is_empty(), "{:?}", p.errors());
    assert_eq!(src.encode(&p.syntax().to_string()), bytes);
}
