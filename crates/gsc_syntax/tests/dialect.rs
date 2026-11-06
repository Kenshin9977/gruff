//! Dialect gating. A small inline profile gives deterministic snapshots; the real gruff-data
//! profiles are used when the checkout is found (`$GRUFF_DATA` or `$GSC_DATA`, or a `gruff-data`
//! or `gruff-data` folder next to the repo).

use std::path::PathBuf;

use gsc_syntax::{check_dialect, parse, Dialect, Gate, LineIndex, Mode};

fn data(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(rel);
    std::fs::read_to_string(p).unwrap().replace("\r\n", "\n")
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

fn profiles_dir() -> Option<PathBuf> {
    Some(data_root().join("profiles")).filter(|d| d.join("t7.toml").is_file())
}

fn diagnostics(text: &str, dialect: &Dialect) -> String {
    let p = parse(text, dialect);
    assert!(p.errors().is_empty(), "syntax errors: {:?}", p.errors());
    let index = LineIndex::new(text);
    check_dialect(&p.syntax(), dialect)
        .iter()
        .map(|d| {
            let lc = index.line_col(text, d.range.start());
            let span = &text[d.range];
            format!(
                "{}:{}: {} {}  [{}]\n",
                lc.line, lc.col, d.code, d.message, span
            )
        })
        .collect()
}

const TEST_PROFILE: &str = r#"
id = "demo"
name = "Demo dialect"
[keywords]
words = ["if", "else", "while", "for", "return", "wait", "thread", "self", "level", "true",
         "false", "undefined", "notify", "isdefined", "function", "switch", "case", "default",
         "break", "continue", "foreach", "in"]
[features]
classes = { value = false, confidence = "assumed", source = [] }
function_keyword = { value = true, confidence = "assumed", source = [] }
dev_blocks = { value = false, confidence = "assumed", source = [] }
hash_string_literal = { value = false, confidence = "assumed", source = [] }
istring_literal = { value = true, confidence = "assumed", source = [] }
strict_equality = { value = false, confidence = "assumed", source = [] }
function_pointer_style = { value = "colon", confidence = "assumed", source = [] }
anim_reference = { value = "unknown", confidence = "assumed", source = [] }
namespace_qualified_calls = { value = false, confidence = "assumed", source = [], status = "unresolved" }
import_directive = { value = "include", confidence = "assumed", source = [] }
"#;

#[test]
fn inline_profile_snapshot() {
    let d = Dialect::from_toml_str(TEST_PROFILE).unwrap();
    let mut out = String::new();
    for f in ["ok/expressions.gsc", "ok/classes.gsc", "ok/directives.gsc"] {
        out.push_str(&format!("== {f}\n"));
        out.push_str(&diagnostics(&data(f), &d));
    }
    insta::assert_snapshot!(out);
}

#[test]
fn superset_reports_nothing() {
    let text = data("dialect/t7_gating.gsc");
    assert_eq!(diagnostics(&text, &Dialect::superset()), "");
}

#[test]
fn t7_profile_accepts_t7_fixtures() {
    let Some(dir) = profiles_dir() else {
        eprintln!("gruff-data not found; skipping");
        return;
    };
    let t7 = Dialect::find(&dir, "bo3").unwrap();
    assert_eq!(t7.id, "t7");
    for f in [
        "ok/expressions.gsc",
        "ok/classes.gsc",
        "ok/directives.gsc",
        // Not ok/functions.gsc: it uses `codecall`, which the official T7 linker rejects.
        "ok/statements.gsc",
        "ok/preproc_if.gsc",
    ] {
        assert_eq!(diagnostics(&data(f), &t7), "", "{f}");
    }
}

#[test]
fn t7_profile_gating_snapshot() {
    let Some(dir) = profiles_dir() else {
        eprintln!("gruff-data not found; skipping");
        return;
    };
    let t7 = Dialect::find(&dir, "t7").unwrap();
    insta::assert_snapshot!(diagnostics(&data("dialect/t7_gating.gsc"), &t7));
}

#[test]
fn every_profile_loads() {
    let Some(dir) = profiles_dir() else {
        eprintln!("gruff-data not found; skipping");
        return;
    };
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "toml") {
            let d = Dialect::load(&p).unwrap();
            assert!(d.keyword("if").is_some(), "{}: `if` is a keyword", d.id);
        }
    }
}

const GATES_PROFILE: &str = r#"
id = "gx"
name = "Gate test"
[keywords]
words = ["if", "return", "wait", "self", "level", "undefined"]
[features]
casts = { value = false, confidence = "assumed", source = [] }
builtin_call = { value = false, confidence = "assumed", source = [] }
goto_labels = { value = false, confidence = "assumed", source = [] }
preprocessor_define = { value = false, confidence = "assumed", source = [], community = { value = true, tools = ["gsc-tool"], confidence = "documented", source = [] } }
preprocessor_conditionals = { value = false, confidence = "assumed", source = [], community = { value = true, tools = ["gsc-tool"], confidence = "documented", source = [] } }
header_directive = { value = "none", confidence = "assumed", source = [], community = { value = "inline", tools = ["gsc-tool"], confidence = "documented", source = [] } }
"#;

const GATES_TEXT: &str =
    "f()\n{\n    x = (int)y;\n    builtin [[ p ]]( 1 );\n    goto LOC_1;\n    LOC_1:\n}\n";

#[test]
fn casts_builtin_and_goto_have_their_own_codes() {
    let d = Dialect::from_toml_str(GATES_PROFILE).unwrap();
    let p = parse(GATES_TEXT, &d);
    assert!(p.errors().is_empty(), "{:?}", p.errors());
    let codes: Vec<_> = check_dialect(&p.syntax(), &d)
        .iter()
        .map(|e| e.code)
        .collect();
    assert_eq!(codes, ["D005", "D006", "D004", "D004"]);

    // With the features on, nothing is reported.
    let on = GATES_PROFILE
        .replace("casts = { value = false", "casts = { value = true")
        .replace(
            "builtin_call = { value = false",
            "builtin_call = { value = true",
        )
        .replace(
            "goto_labels = { value = false",
            "goto_labels = { value = true",
        );
    let d = Dialect::from_toml_str(&on).unwrap();
    assert!(check_dialect(&parse(GATES_TEXT, &d).syntax(), &d).is_empty());
}

#[test]
fn community_mode_accepts_the_community_preprocessor() {
    let text = "#inline scripts/x.gsh;\n#define N 1\n#if N\nf() {}\n#endif\n";
    let retail = Dialect::from_toml_str(GATES_PROFILE).unwrap();
    let community = retail.clone().with_mode(Mode::Community);
    let report = |d: &Dialect| diagnostics(text, d);
    insta::assert_snapshot!(format!(
        "== retail\n{}== community\n{}",
        report(&retail),
        report(&community)
    ));
    assert_eq!(report(&community), "");
}

#[test]
fn t6_community_mode() {
    let Some(dir) = profiles_dir() else {
        eprintln!("gruff-data not found; skipping");
        return;
    };
    let t6 = Dialect::find(&dir, "t6").unwrap();
    let community = t6.clone().with_mode(Mode::Community);
    assert_eq!(t6.file_kind("gsh"), Gate::Off);
    assert_eq!(community.file_kind("gsh"), Gate::On);
    assert_eq!(community.flag("preprocessor_define"), Gate::On);
    assert_eq!(community.is("header_directive", "inline"), Gate::On);
    assert!(t6.community_features().contains(&"preprocessor_define"));
}

const ISTRUE_OFF: &str = r#"
id = "istrue_off"
name = "istrue keyword without the feature"
[keywords]
words = ["istrue", "thisthread", "function", "thread", "if", "return"]
[features]
istrue_operator = { value = false, confidence = "assumed", source = [] }
thisthread = { value = true, confidence = "assumed", source = [] }
"#;

#[test]
fn istrue_keyword_is_gated() {
    let d = Dialect::from_toml_str(ISTRUE_OFF).unwrap();
    let out = diagnostics("function f( x ) { if ( istrue( x ) ) return; }", &d);
    assert!(out.contains("istrue_operator"), "{out}");
}

#[test]
fn thisthread_value_and_contextual_function() {
    let d = Dialect::from_toml_str(ISTRUE_OFF).unwrap();
    let text =
        "function f( a, function ) { x = thisthread; function thread g(); return function; }";
    let p = parse(text, &d);
    assert!(p.errors().is_empty(), "{:?}", p.errors());
    let tree = p.debug_tree();
    assert!(tree.contains("THIS_THREAD_EXPR"), "{tree}");
    assert_eq!(diagnostics(text, &d), "");
}
