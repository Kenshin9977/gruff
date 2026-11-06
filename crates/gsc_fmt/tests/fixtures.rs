//! Snapshot tests over the hand-written `.gsc` files of `tests/fixtures` (never game code).

use std::path::Path;

use gsc_fmt::{format, lexemes, BraceStyle, Indent, Options, ParenSpacing, Style, Wrap};
use gsc_syntax::Dialect;

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    // Normalise checkout line endings so the snapshots do not depend on git settings.
    std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
}

/// Format, check the invariants, and return the result.
fn run(text: &str, opts: &Options) -> String {
    let d = Dialect::superset();
    let out = format(text, &d, opts).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(lexemes(text, &d), lexemes(&out, &d), "tokens changed");
    assert_eq!(format(&out, &d, opts).unwrap(), out, "not idempotent");
    out
}

/// The minimal style with the default options.
fn minimal() -> Options {
    Options::default().with_style(Style::Minimal)
}

/// The minimal style with another paren spacing.
fn parens(paren_spacing: ParenSpacing) -> Options {
    Options {
        paren_spacing,
        ..minimal()
    }
}

// ---- minimal style --------------------------------------------------------------------------

#[test]
fn spacing() {
    insta::assert_snapshot!(run(&fixture("spacing.gsc"), &minimal()));
}

#[test]
fn spacing_compact() {
    insta::assert_snapshot!(run(&fixture("spacing.gsc"), &parens(ParenSpacing::Compact)));
}

#[test]
fn spacing_preserve() {
    insta::assert_snapshot!(run(
        &fixture("spacing.gsc"),
        &parens(ParenSpacing::Preserve)
    ));
}

#[test]
fn indent() {
    insta::assert_snapshot!(run(&fixture("indent.gsc"), &minimal()));
}

#[test]
fn indent_spaces() {
    let opts = Options {
        indent: Indent::Spaces(4),
        ..minimal()
    };
    insta::assert_snapshot!(run(&fixture("indent.gsc"), &opts));
}

#[test]
fn verbatim() {
    insta::assert_snapshot!(run(&fixture("verbatim.gsc"), &minimal()));
}

#[test]
fn class() {
    insta::assert_snapshot!(run(&fixture("class.gsc"), &minimal()));
}

#[test]
fn old_style_cod2() {
    insta::assert_snapshot!(run(
        &fixture("old_style.gsc"),
        &Options::for_game("iw2").with_style(Style::Minimal)
    ));
}

#[test]
fn broken_is_skipped() {
    let err = format(
        &fixture("broken.gsc"),
        &Dialect::superset(),
        &Options::default(),
    )
    .unwrap_err();
    insta::assert_snapshot!(err.to_string());
}

#[test]
fn crlf_is_kept() {
    for opts in [minimal(), Options::default()] {
        let text = fixture("spacing.gsc").replace('\n', "\r\n");
        let out = run(&text, &opts);
        assert!(
            !out.replace("\r\n", "").contains('\n'),
            "a bare LF crept in"
        );
        assert_eq!(
            out,
            run(&fixture("spacing.gsc"), &opts).replace('\n', "\r\n")
        );
    }
}

// ---- full style -----------------------------------------------------------------------------

#[test]
fn full_layout() {
    insta::assert_snapshot!(run(&fixture("layout.gsc"), &Options::default()));
}

#[test]
fn full_layout_narrow_one_per_line() {
    let opts = Options {
        max_width: 60,
        wrap: Wrap::OnePerLine,
        ..Options::default()
    };
    insta::assert_snapshot!(run(&fixture("layout.gsc"), &opts));
}

#[test]
fn full_layout_same_line_braces() {
    let opts = Options {
        brace_style: BraceStyle::SameLine,
        indent: Indent::Spaces(4),
        ..Options::default()
    };
    insta::assert_snapshot!(run(&fixture("layout.gsc"), &opts));
}

#[test]
fn full_layout_cod2() {
    insta::assert_snapshot!(run(&fixture("layout.gsc"), &Options::for_game("iw2")));
}

#[test]
fn full_comments() {
    insta::assert_snapshot!(run(&fixture("comments.gsc"), &Options::default()));
}

#[test]
fn full_comments_narrow() {
    let opts = Options {
        max_width: 16,
        ..Options::default()
    };
    insta::assert_snapshot!(run(&fixture("comments.gsc"), &opts));
}

#[test]
fn full_regions() {
    insta::assert_snapshot!(run(&fixture("regions.gsc"), &Options::default()));
}

#[test]
fn full_verbatim() {
    insta::assert_snapshot!(run(&fixture("verbatim.gsc"), &Options::default()));
}

#[test]
fn full_class() {
    insta::assert_snapshot!(run(&fixture("class.gsc"), &Options::default()));
}

#[test]
fn full_indent() {
    insta::assert_snapshot!(run(&fixture("indent.gsc"), &Options::default()));
}

#[test]
fn full_spacing() {
    insta::assert_snapshot!(run(&fixture("spacing.gsc"), &Options::default()));
}
