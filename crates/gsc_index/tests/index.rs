//! Snapshot tests of `gruff index` over the fixtures in tests/data (all written for this project).

use std::path::PathBuf;

use gsc_index::{
    build, index, workspace, workspace_from_sources, write, BareCall, Config, Decompiled,
    Resolution,
};
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
    let files = write::render(&index(&ws), "fixture", false);
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
    let files = write::render(&index(&ws), "t7", false);
    let readme = &files["README.md"];
    for f in write::FILES.iter().filter(|f| f.ends_with(".tsv")) {
        assert!(readme.contains(&format!("`{f}`")), "{f}");
    }
    assert!(readme.contains("rg -P"));
}

/// A t8 workspace: the superset dialect under BO4's id, which is what gives it a hash scheme.
fn t8_sources(jobs: usize) -> std::collections::BTreeMap<&'static str, String> {
    let mut dialect = Dialect::superset();
    dialect.id = "t8".to_owned();
    let sources = [
        (
            "scripts/zm/zm_test.gsc",
            "#namespace zm_test;\n\
             function init()\n{\n\
                 level.var_1a2b3c4d = 1;\n\
                 self notify( #\"hash_0011223344556677\" );\n\
                 function_aabbccdd();\n\
             }\n\
             function function_11223344( a )\n{\n    init();\n}\n\
             function spawn_zombie()\n{\n}\n",
        ),
        (
            "scripts/zm/zm_other.gsc",
            "#namespace zm_other;\nfunction run()\n{\n\
                 self.health = 1;\n    function_253cbb6e();\n}\n",
        ),
    ];
    let mut ws = workspace_from_sources(&sources, Profile::new(dialect, None, None));
    ws = ws.with_jobs(jobs);
    build(&mut ws);
    write::render(&index(&ws), "t8", true)
}

#[test]
fn unknowns_lists_every_hash_nothing_names() {
    let files = t8_sources(1);
    let u = &files["UNKNOWNS.tsv"];
    let row = |hash: &str| {
        u.lines()
            .find(|l| l.starts_with(&format!("{hash}\t")))
            .unwrap_or_else(|| panic!("no row for {hash} in\n{u}"))
            .split('\t')
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    // The called function nobody defines: its only context is the function it is called from.
    let called = row("function_aabbccdd");
    assert_eq!(called[2], "function");
    assert_eq!(called[3], "t89.scr");
    assert_eq!(called[4], "32");
    assert_eq!(called[7], "scripts/zm/zm_test.gsc", "scope");
    assert_eq!(called[8], "-", "it is not defined here");
    assert_eq!(called[14], "init", "in_functions");
    assert_eq!(called[15], "init", "callers");
    // The defined one: its definition, namespace and signature, and what it calls.
    let defined = row("function_11223344");
    assert_eq!(defined[8], "scripts/zm/zm_test.gsc:8", "def_at");
    assert_eq!(defined[9], "zm_test", "namespace");
    assert_eq!(defined[10], "a", "params");
    assert_eq!(defined[16], "init", "callees");
    // A field and a #"..." string, each with the function they sit in.
    let var = row("var_1a2b3c4d");
    assert_eq!(var[2], "var");
    assert_eq!(var[12], "level:1", "object");
    assert_eq!(var[13], "write:1", "ops");
    assert_eq!(var[14], "init", "in_functions");
    // Spelled without the leading zero nibble, as acts writes it too (`#"hash_eafc4a7d7b6a027"`).
    let string = row("hash_11223344556677");
    assert_eq!(string[3], "t8.fnv64");
    assert_eq!(string[4], "63");
    assert_eq!(string[13], "notify:1", "ops");
    // A name written in clear is never a target -- though it may well be a neighbour.
    let targets: Vec<&str> = u
        .lines()
        .skip(1)
        .filter_map(|l| l.split('\t').next())
        .collect();
    assert!(
        !targets.contains(&"init") && !targets.contains(&"health"),
        "{u}"
    );
    assert!(called[15].contains("init"), "but it is a neighbour");
}

#[test]
fn vocab_lists_the_names_each_file_uses() {
    let files = t8_sources(1);
    let v = &files["VOCAB.tsv"];
    let row = |file: &str, kind: &str, source: &str| {
        v.lines()
            .find(|l| l.starts_with(&format!("{file}\t{kind}\t{source}\t")))
            .unwrap_or_else(|| panic!("no {kind}/{source} row for {file} in\n{v}"))
            .rsplit('\t')
            .next()
            .unwrap_or("")
            .to_owned()
    };
    assert_eq!(
        row("scripts/zm/zm_test.gsc", "function", "clear"),
        "init,spawn_zombie"
    );
    assert_eq!(
        row("scripts/zm/zm_test.gsc", "namespace", "clear"),
        "zm_test"
    );
    assert_eq!(row("scripts/zm/zm_other.gsc", "field", "clear"), "health");
    // `function_253cbb6e` is `spawn_zombie`, which only the workspace's own clear names say: the
    // vocabulary of the file that CALLS it must hold the name, not the hash.
    assert_eq!(
        row("scripts/zm/zm_other.gsc", "function", "recovered"),
        "spawn_zombie"
    );
    assert!(!v.contains("function_253cbb6e"), "{v}");
    let path = v
        .lines()
        .find(|l| l.starts_with("scripts/zm/zm_test.gsc\tpath\tclear\t"))
        .unwrap_or_else(|| panic!("no path row in\n{v}"));
    for w in ["scripts", "zm", "test", "gsc"] {
        assert!(
            path.ends_with(&format!(",{w}"))
                || path.contains(&format!("\t{w},"))
                || path.contains(&format!(",{w},")),
            "{w} missing from {path}"
        );
    }
}

#[test]
fn recovery_tables_do_not_depend_on_threads() {
    assert_eq!(t8_sources(1), t8_sources(4));
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
