use super::*;

fn fmt(text: &str) -> String {
    fmt_with(text, &Options::default())
}

fn fmt_with(text: &str, opts: &Options) -> String {
    let d = Dialect::superset();
    let out = format(text, &d, opts).unwrap_or_else(|e| panic!("{e}\n{text}"));
    let again = format(&out, &d, opts).unwrap();
    assert_eq!(again, out, "not idempotent");
    out
}

#[test]
fn basic_spacing() {
    assert_eq!(
        fmt("function f(a,b)\n{\nx=a+b ;\nfoo (a,b);\n}\n"),
        "function f( a, b )\n{\n\tx = a + b;\n\tfoo( a, b );\n}\n"
    );
}
