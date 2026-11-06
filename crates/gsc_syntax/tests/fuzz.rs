//! Property tests: the parser never panics, always terminates, keeps every byte, and reports
//! errors inside the text, whatever the input.

use gsc_syntax::{check_dialect, parse, Dialect, SourceText, TextSize};
use proptest::prelude::*;

const PIECES: &[&str] = &[
    "function",
    "class",
    "var",
    "new",
    "if",
    "else",
    "while",
    "for",
    "foreach",
    "in",
    "do",
    "switch",
    "case",
    "default",
    "return",
    "wait",
    "thread",
    "childthread",
    "self",
    "level",
    "notify",
    "waittill",
    "isdefined",
    "undefined",
    "true",
    "constructor",
    "autoexec",
    "private",
    "x",
    "foo",
    "ns",
    "maps\\mp\\_util",
    "#using",
    "#insert",
    "#namespace",
    "#define",
    "#if",
    "#else",
    "#endif",
    "#precache",
    "#using_animtree",
    "#animtree",
    "/#",
    "#/",
    "/@ d @/",
    "/* c */",
    "// c\n",
    "\"s\"",
    "&\"L\"",
    "#\"h\"",
    "%anim",
    "#k",
    "@n",
    "1",
    "2.5",
    ".5",
    "0x1f",
    "(",
    ")",
    "{",
    "}",
    "[",
    "]",
    "[[",
    "]]",
    ";",
    ",",
    ".",
    "...",
    ":",
    "::",
    "?",
    "->",
    "=",
    "==",
    "===",
    "!=",
    "<",
    ">=",
    "<<",
    "+",
    "-",
    "*",
    "/",
    "%",
    "&",
    "&&",
    "|",
    "||",
    "^",
    "~",
    "!",
    "++",
    "--",
    "+=",
    "$",
    "\\",
    "`",
    "\u{e9}",
    "\r\n",
    "\n",
    " ",
    "\t",
    "\\\n",
];

fn token_soup() -> impl Strategy<Value = String> {
    prop::collection::vec((0..PIECES.len(), prop::bool::ANY), 0..200).prop_map(|v| {
        let mut s = String::new();
        for (i, space) in v {
            s.push_str(PIECES[i]);
            if space {
                s.push(' ');
            }
        }
        s
    })
}

fn check(text: &str, dialect: &Dialect) {
    let p = parse(text, dialect);
    let root = p.syntax();
    assert_eq!(root.to_string(), text, "lossless");
    let end = TextSize::of(text);
    for e in p.errors().iter().chain(&check_dialect(&root, dialect)) {
        assert!(e.range.end() <= end, "error range inside the text: {e:?}");
    }
    // The typed views never panic either.
    let tree = p.tree();
    for f in tree.functions() {
        let _ = (f.name(), f.param_list(), f.body(), f.doc_comment());
    }
}

fn dialects() -> Vec<Dialect> {
    vec![
        Dialect::superset(),
        Dialect::superset().with_decompiled(true),
        Dialect::from_toml_str(
            r#"
id = "tx"
[keywords]
words = ["if", "else", "function", "class", "thread"]
[features]
classes = { value = false, confidence = "assumed", source = [] }
function_keyword = { value = true, confidence = "assumed", source = [] }
dev_blocks = { value = false, confidence = "assumed", source = [] }
"#,
        )
        .unwrap(),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, .. ProptestConfig::default() })]

    #[test]
    fn token_soup_never_panics(text in token_soup()) {
        for d in dialects() {
            check(&text, &d);
        }
    }

    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..400)) {
        let src = SourceText::decode(&bytes);
        let p = parse(&src.text, &Dialect::superset());
        prop_assert_eq!(src.encode(&p.syntax().to_string()), bytes);
    }

    #[test]
    fn mutated_fixture_never_panics(cut in 0usize..2000, len in 0usize..40, insert in 0..PIECES.len()) {
        let base = include_str!("data/ok/expressions.gsc");
        let mut s = base.to_owned();
        let mut at = cut.min(s.len());
        while !s.is_char_boundary(at) {
            at -= 1;
        }
        let mut end = (at + len).min(s.len());
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.replace_range(at..end, PIECES[insert]);
        for d in dialects() {
            check(&s, &d);
        }
    }
}

fn deep_cases(n: usize) -> Vec<String> {
    vec![
        format!(
            "function f() {{ x = {}1{}; }}",
            "(".repeat(n),
            ")".repeat(n)
        ),
        format!("function f() {}{}", "{".repeat(n), "}".repeat(n)),
        format!("function f() {{ {} x(); }}", "if ( a ) ".repeat(n)),
        format!(
            "function f() {{ if ( a ) {{}} {} }}",
            "else if ( b ) {} ".repeat(n)
        ),
        format!("function f() {{ x = {}a; }}", "!-".repeat(n)),
        format!("function f() {{ x = a{}; }}", " + a".repeat(n)),
        format!("function f() {{ x = a{}; }}", ".b".repeat(n)),
    ]
}

/// Nesting is bounded by the parser's depth budget; long left-associative chains (`a + a + ...`)
/// are parsed iteratively. The tree's drop is recursive in rowan, so a very deep chain needs a
/// large stack: the CLI parses on a 256 MiB thread.
#[test]
fn deep_nesting_terminates() {
    for n in [10usize, 300, 1000] {
        for text in deep_cases(n) {
            check(&text, &Dialect::superset());
        }
    }
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(|| {
            for text in deep_cases(20_000) {
                check(&text, &Dialect::superset());
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
