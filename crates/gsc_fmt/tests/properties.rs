//! Property tests: on generated programs and on whitespace-shuffled fixtures, formatting
//! succeeds, is idempotent and changes nothing but whitespace.

use std::path::Path;

use gsc_fmt::{
    format, lexemes, BraceStyle, Indent, KeywordSpace, Options, ParenSpacing, Style, Wrap,
};
use gsc_syntax::{tokenize, Dialect, SyntaxKind};
use proptest::prelude::*;

fn check(text: &str, opts: &Options) -> Result<(), TestCaseError> {
    let d = Dialect::superset();
    let out = match format(text, &d, opts) {
        Ok(out) => out,
        Err(gsc_fmt::FormatError::Syntax(_)) => return Err(TestCaseError::reject("syntax")),
        Err(e) => return Err(TestCaseError::fail(format!("{e}\n---\n{text}"))),
    };
    prop_assert_eq!(
        lexemes(text, &d),
        lexemes(&out, &d),
        "tokens changed:\n{}",
        text
    );
    let again = format(&out, &d, opts).map_err(|e| TestCaseError::fail(e.to_string()))?;
    prop_assert_eq!(&again, &out, "not idempotent:\n{}", text);
    prop_assert!(out.ends_with('\n'));
    prop_assert!(!out.ends_with("\n\n") && !out.ends_with("\n\r\n"));
    if !text.contains('\r') {
        prop_assert!(!out.contains('\r'));
    }
    Ok(())
}

fn options() -> impl Strategy<Value = Options> {
    (
        prop_oneof![Just(Indent::Tabs), (1u8..=8).prop_map(Indent::Spaces)],
        prop_oneof![
            Just(ParenSpacing::Padded),
            Just(ParenSpacing::Compact),
            Just(ParenSpacing::Preserve)
        ],
        prop_oneof![Just(Style::Full), Just(Style::Full), Just(Style::Minimal)],
        prop_oneof![Just(100u16), Just(40u16), Just(20u16)],
        prop_oneof![Just(BraceStyle::NextLine), Just(BraceStyle::SameLine)],
        prop_oneof![Just(Wrap::Fill), Just(Wrap::OnePerLine)],
        prop_oneof![
            Just(KeywordSpace::Space),
            Just(KeywordSpace::None),
            Just(KeywordSpace::Preserve)
        ],
        0u8..3,
    )
        .prop_map(
            |(indent, paren_spacing, style, max_width, brace_style, wrap, keyword_space, blank)| {
                Options {
                    indent,
                    paren_spacing,
                    style,
                    max_width,
                    brace_style,
                    wrap,
                    keyword_space,
                    bracket_spacing: paren_spacing,
                    max_blank_lines: blank,
                    ..Options::default()
                }
            },
        )
}

// ---- generated programs -------------------------------------------------------------------

/// A program as a list of tokens; `"\n"` entries force a line break (after `//` comments).
type Toks = Vec<String>;

fn t(s: &str) -> String {
    s.to_owned()
}

fn ident() -> impl Strategy<Value = String> {
    prop::sample::select(vec!["a", "b", "foo", "bar_2", "level", "self", "x1"]).prop_map(t)
}

fn leaf() -> impl Strategy<Value = Toks> {
    prop_oneof![
        ident().prop_map(|i| vec![i]),
        (0u32..1000).prop_map(|n| vec![n.to_string()]),
        Just(vec![t("1.5")]),
        Just(vec![t("\"a  ,( b )  \"")]),
        Just(vec![t("&\"MENU_X  Y\"")]),
        Just(vec![t("undefined")]),
        Just(vec![t("&"), t("foo")]),
        Just(vec![t("foo"), t("::"), t("bar")]),
    ]
}

fn expr() -> impl Strategy<Value = Toks> {
    leaf().prop_recursive(4, 24, 3, |inner| {
        let bin = prop::sample::select(vec![
            "+", "-", "*", "/", "%", "==", "!=", "<", "<=", ">", ">=", "&&", "||", "&", "|", "^",
            "<<", ">>",
        ]);
        prop_oneof![
            (inner.clone(), bin, inner.clone()).prop_map(|(a, op, b)| {
                let mut v = a;
                v.push(t(op));
                v.extend(b);
                v
            }),
            inner.clone().prop_map(|a| {
                let mut v = vec![t("(")];
                v.extend(a);
                v.push(t(")"));
                v
            }),
            (prop::sample::select(vec!["!", "-", "~"]), inner.clone()).prop_map(|(op, a)| {
                let mut v = vec![t(op), t("(")];
                v.extend(a);
                v.push(t(")"));
                v
            }),
            (ident(), prop::collection::vec(inner.clone(), 0..4))
                .prop_map(|(f, args)| call(vec![f], args)),
            (ident(), ident(), prop::collection::vec(inner.clone(), 0..3)).prop_map(
                |(obj, f, args)| {
                    let mut v = vec![obj];
                    v.extend(call(vec![f], args));
                    v
                }
            ),
            (inner.clone(), inner.clone(), inner.clone()).prop_map(|(a, b, c)| {
                let mut v = vec![t("(")];
                v.extend(a);
                v.push(t(","));
                v.extend(b);
                v.push(t(","));
                v.extend(c);
                v.push(t(")"));
                v
            }),
            (ident(), inner.clone()).prop_map(|(a, i)| {
                let mut v = vec![a, t("[")];
                v.extend(i);
                v.push(t("]"));
                v
            }),
            (ident(), ident()).prop_map(|(a, f)| vec![a, t("."), f]),
            (inner.clone(), inner.clone(), inner).prop_map(|(c, a, b)| {
                let mut v = vec![t("(")];
                v.extend(c);
                v.push(t("?"));
                v.extend(a);
                v.push(t(":"));
                v.extend(b);
                v.push(t(")"));
                v
            }),
        ]
    })
}

fn call(callee: Toks, args: Vec<Toks>) -> Toks {
    let mut v = callee;
    v.push(t("("));
    for (i, a) in args.into_iter().enumerate() {
        if i > 0 {
            v.push(t(","));
        }
        v.extend(a);
    }
    v.push(t(")"));
    v
}

fn wrap(head: Vec<&str>, e: Toks, tail: Vec<&str>) -> Toks {
    let mut v: Toks = head.into_iter().map(t).collect();
    v.extend(e);
    v.extend(tail.into_iter().map(t));
    v
}

fn stmt() -> impl Strategy<Value = Toks> {
    let simple = prop_oneof![
        (ident(), expr()).prop_map(|(x, e)| wrap(vec![], vec![x], vec!["="])
            .into_iter()
            .chain(e)
            .chain([t(";")])
            .collect()),
        (ident(), prop::collection::vec(expr(), 0..3))
            .prop_map(|(f, args)| { call(vec![f], args).into_iter().chain([t(";")]).collect() }),
        expr().prop_map(|e| wrap(vec!["return"], e, vec![";"])),
        expr().prop_map(|e| wrap(vec!["wait"], e, vec![";"])),
        Just(vec![t("x1"), t("++"), t(";")]),
        Just(vec![t("a"), t("+="), t("2"), t(";")]),
        Just(vec![t("break"), t(";")]),
    ];
    simple.prop_recursive(3, 24, 4, |inner| {
        let block = prop::collection::vec(commented(inner.clone()), 0..4).prop_map(|ss| {
            let mut v = vec![t("{")];
            for s in ss {
                v.extend(s);
            }
            v.push(t("}"));
            v
        });
        prop_oneof![
            (expr(), inner.clone()).prop_map(|(c, s)| {
                let mut v = wrap(vec!["if", "("], c, vec![")"]);
                v.extend(s);
                v
            }),
            (expr(), inner.clone(), inner.clone()).prop_map(|(c, a, b)| {
                let mut v = wrap(vec!["if", "("], c, vec![")"]);
                v.extend(a);
                v.push(t("else"));
                v.extend(b);
                v
            }),
            (expr(), block.clone()).prop_map(|(c, b)| {
                let mut v = wrap(vec!["while", "("], c, vec![")"]);
                v.extend(b);
                v
            }),
            (expr(), inner.clone()).prop_map(|(c, s)| {
                let mut v = vec![t("for"), t("("), t("x1"), t("="), t("0"), t(";"), t("x1")];
                v.push(t("<"));
                v.extend(c);
                v.extend([t(";"), t("x1"), t("++"), t(")")]);
                v.extend(s);
                v
            }),
            inner.clone().prop_map(|s| {
                let mut v: Toks = ["for", "(", ";", ";", ")"].map(t).to_vec();
                v.extend(s);
                v
            }),
            (expr(), inner.clone()).prop_map(|(e, s)| {
                let mut v = wrap(vec!["foreach", "(", "a", ",", "b", "in"], e, vec![")"]);
                v.extend(s);
                v
            }),
            (expr(), prop::collection::vec(inner.clone(), 0..3)).prop_map(|(e, ss)| {
                let mut v = wrap(vec!["switch", "("], e, vec![")", "{", "case", "1", ":"]);
                for s in ss {
                    v.extend(s);
                }
                v.extend(["default", ":", "break", ";", "}"].map(t));
                v
            }),
            block.clone(),
            block.prop_map(|b| {
                let mut v = vec![t("\n"), t("/#")];
                v.extend(b);
                v.push(t("#/"));
                v
            }),
        ]
    })
}

/// A statement, maybe preceded by a comment.
fn commented(s: BoxedStrategy<Toks>) -> BoxedStrategy<Toks> {
    let comment = prop::sample::select(vec![
        vec![],
        vec![],
        vec![t("// a  comment ,( x )  "), t("\n")],
        vec![t("/*  block ( a,b )  */")],
        vec![t("/*  two\n   lines  */")],
    ]);
    (comment, s)
        .prop_map(|(c, s)| c.into_iter().chain(s).collect())
        .boxed()
}

fn program() -> impl Strategy<Value = Toks> {
    prop::collection::vec(
        (
            ident(),
            prop::collection::vec(ident(), 0..3),
            prop::collection::vec(commented(stmt().boxed()), 0..5),
        ),
        1..3,
    )
    .prop_map(|fns| {
        let mut v = vec![t("#using"), t("scripts\\shared\\util"), t(";"), t("\n")];
        for (i, (name, params, body)) in fns.into_iter().enumerate() {
            v.extend([t("function"), format!("{name}{i}"), t("(")]);
            for (j, p) in params.into_iter().enumerate() {
                if j > 0 {
                    v.push(t(","));
                }
                v.push(format!("{p}_{j}"));
            }
            v.extend([t(")"), t("{")]);
            for s in body {
                v.extend(s);
            }
            v.push(t("}"));
        }
        v
    })
}

fn gap() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![
        "", "", "", " ", " ", "  ", "\t", " \t ", "\n", "\n\t", "\n\t\t", "  \n", "\n\n    ",
        "\r\n\t",
    ])
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '"' || c == '.'
}

/// Join tokens with the gaps, adding a space where two tokens would otherwise merge.
fn render(toks: &[String], gaps: &[&str]) -> String {
    let mut out = String::new();
    let mut need_newline = false;
    for (i, tok) in toks.iter().enumerate() {
        if tok == "\n" {
            need_newline = true;
            continue;
        }
        if i > 0 {
            let g = gaps[i % gaps.len()];
            let prev = out.chars().last().unwrap_or(' ');
            let next = tok.chars().next().unwrap();
            let merge = (is_word(prev) && is_word(next))
                || (prev.is_ascii_punctuation() && next.is_ascii_punctuation());
            if need_newline && !g.contains('\n') {
                out.push('\n');
            } else if g.is_empty() && merge {
                out.push(' ');
            }
            out.push_str(g);
        }
        need_newline = false;
        out.push_str(tok);
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, max_global_rejects: 4096, ..ProptestConfig::default() })]

    #[test]
    fn generated_programs(toks in program(), gaps in prop::collection::vec(gap(), 1..64), opts in options()) {
        let text = render(&toks, &gaps);
        // The generator only makes valid programs; a syntax error is a generator bug.
        let parse = gsc_syntax::parse(&text, &Dialect::superset());
        prop_assert!(parse.errors().is_empty(), "{:?}\n{}", parse.errors(), text);
        check(&text, &opts)?;
    }
}

// ---- whitespace-shuffled fixtures ---------------------------------------------------------

fn fixtures() -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures");
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.file_name().is_some_and(|n| n != "broken.gsc"))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|p| std::fs::read_to_string(p).unwrap())
        .collect()
}

/// Replace every whitespace token by another of the same shape: with line breaks if it had
/// any, non-empty if it was. Splices (`\`) stay.
fn shuffle(text: &str, picks: &[(u8, u8)]) -> String {
    const SAME_LINE: &[&str] = &[" ", "  ", "\t", " \t", "\t\t  "];
    const BREAKS: &[&str] = &["\n", "\n\t", "  \n\t\t", "\n\n", "\t\n   "];
    let (tokens, _) = tokenize(text, &Dialect::superset());
    let mut out = String::new();
    let mut pos = 0;
    for (i, tk) in tokens.iter().enumerate() {
        let s = &text[pos..pos + tk.len as usize];
        pos += tk.len as usize;
        let (a, b) = picks[i % picks.len()];
        if tk.kind != SyntaxKind::WHITESPACE || s.contains('\\') || a % 3 == 0 {
            out.push_str(s);
        } else if s.contains('\n') {
            // Keep the number of line breaks so line-based constructs stay put.
            let n = s.matches('\n').count();
            for _ in 1..n {
                out.push('\n');
            }
            out.push_str(BREAKS[b as usize % BREAKS.len()]);
        } else {
            out.push_str(SAME_LINE[b as usize % SAME_LINE.len()]);
        }
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn shuffled_fixtures(which in 0usize..16, picks in prop::collection::vec((any::<u8>(), any::<u8>()), 1..200), opts in options()) {
        let all = fixtures();
        let text = shuffle(&all[which % all.len()], &picks);
        let d = Dialect::superset();
        prop_assume!(lexemes(&text, &d) == lexemes(&all[which % all.len()], &d));
        check(&text, &opts)?;
    }
}
