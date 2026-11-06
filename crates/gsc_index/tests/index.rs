//! Snapshot tests of `gruff index` over the fixtures in tests/data (all written for this project).

use std::path::PathBuf;

use gsc_index::{build, index, workspace, write, BareCall, Config, Decompiled, Resolution};
use gsc_sema::{Catalogue, Profile};
use gsc_syntax::Dialect;

const BUILTINS: &str = r#"
schema_version = 1
game = "fixture"

[[builtin]]
name = "getplayers"
vm = "gsc"
kind = "function"

[[builtin]]
name = "setmodel"
vm = "gsc"
kind = "method"

[[builtin]]
name = "playfx"
vm = "csc"
kind = "function"

[[builtin]]
hash = "0x1234"
vm = "gsc"
kind = "function"
"#;

fn data(sub: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(sub)
}

fn t7() -> Config {
    Config::names(Resolution::Namespace, BareCall::NamespaceFirst, true)
}

fn iw() -> Config {
    Config::names(Resolution::Include, BareCall::BuiltinFirst, true)
}

fn profile(cfg: &Config, builtins: Option<Catalogue>) -> Profile {
    Profile::new(Dialect::superset(), builtins, None).with_config(*cfg)
}

fn catalogue() -> Catalogue {
    Catalogue::from_toml_str(BUILTINS).unwrap()
}

/// Every TSV of the index, concatenated, with fixtures read as LF whatever the checkout did.
fn render(sub: &str, cfg: &Config, builtins: Option<Catalogue>, jobs: usize) -> String {
    let mut ws = workspace(&[data(sub)], profile(cfg, builtins), Decompiled::Off, jobs);
    let stats = build(&mut ws);
    assert!(stats.unreadable.is_empty());
    let files = write::render(&index(&ws), "fixture");
    let mut out = String::new();
    for (name, text) in files {
        if name == "README.md" {
            continue;
        }
        out.push_str(&format!("===== {name}\n{}", text.replace('\r', "")));
    }
    out
}

#[test]
fn t7_index() {
    let b = catalogue();
    assert_eq!(b.len(), 4);
    insta::assert_snapshot!(render("t7", &t7(), Some(b), 1));
}

#[test]
fn iw_index() {
    insta::assert_snapshot!(render("iw", &iw(), Some(catalogue()), 1));
}

#[test]
fn output_does_not_depend_on_threads() {
    assert_eq!(
        render("t7", &t7(), Some(catalogue()), 1),
        render("t7", &t7(), Some(catalogue()), 4)
    );
}

#[test]
fn several_roots_are_labelled() {
    let mut ws = workspace(
        &[data("t7"), data("iw")],
        profile(&t7(), None),
        Decompiled::Off,
        2,
    );
    build(&mut ws);
    let ix = index(&ws);
    assert!(ix
        .files
        .iter()
        .any(|f| f.rel == "t7/scripts/shared/beacon.gsc"));
    assert!(ix.files.iter().any(|f| f.rel == "iw/maps/mp/_helpers.gsc"));
    assert!(ws.files().all(|f| !ws.file(f).meta.key.starts_with("t7/")));
}

#[test]
fn readme_lists_every_file() {
    let mut ws = workspace(&[data("t7")], profile(&t7(), None), Decompiled::Off, 1);
    build(&mut ws);
    let files = write::render(&index(&ws), "t7");
    let readme = &files["README.md"];
    for f in write::FILES.iter().filter(|f| f.ends_with(".tsv")) {
        assert!(readme.contains(&format!("`{f}`")), "{f}");
    }
    assert!(readme.contains("rg -P"));
}

#[test]
fn glob_patterns() {
    assert!(write::glob("trigger_*", "trigger_7"));
    assert!(write::glob("trigger_*_hit", "trigger_7_hit"));
    assert!(!write::glob("trigger_*_hit", "trigger_7"));
    assert!(write::glob("a*b*c", "axxbyyc"));
    assert!(!write::glob("abc", "abcd"));
}

#[test]
fn from_dialect_reads_the_resolution_features() {
    let ns = Dialect::from_toml_str(
        r#"
id = "x"
name = "x"
[features]
name_resolution = { value = "merge", confidence = "documented", source = ["s"] }
bare_call_resolution = { value = "namespace-first", confidence = "documented", source = ["s"] }
"#,
    );
    if let Ok(d) = ns {
        let c = Config::from_dialect(&d);
        assert_eq!(c.resolution, Resolution::Include);
        assert_eq!(c.bare_call, BareCall::NamespaceFirst);
    }
    let ff_text = r#"
id = "x"
name = "x"
[keywords]
words = []
[features]
name_resolution = { value = "merge", confidence = "documented", source = ["s"] }
bare_call_resolution = { value = "file-first", confidence = "documented", source = ["s"] }
hash_equality = { value = "n/a", confidence = "documented", source = ["s"] }
"#;
    let ff = Dialect::from_toml_str(ff_text).unwrap();
    let c = Config::from_dialect(&ff);
    assert_eq!(c.resolution, Resolution::Include);
    assert_eq!(c.bare_call, BareCall::FileFirst);
    assert!(!c.hash_equality_error);
    let strict = Dialect::from_toml_str(&ff_text.replace("file-first", "builtin-strict")).unwrap();
    assert_eq!(
        Config::from_dialect(&strict).bare_call,
        BareCall::BuiltinStrict
    );
    let c = Config::from_dialect(&Dialect::superset());
    assert_eq!(c.resolution, Resolution::Namespace);
    assert_eq!(c.bare_call, BareCall::BuiltinFirst);
}
