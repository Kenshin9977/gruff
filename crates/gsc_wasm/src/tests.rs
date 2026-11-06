use super::*;
use gsc_syntax::TextSize;

const SOURCE: &str = "function f()\n{\n\tx = 1;\n\tif ( x )\n\t\treturn;\n}\n";

fn superset() -> Engine {
    Engine::load(None, None, None, None).unwrap()
}

#[test]
fn hashes_with_a_family() {
    assert_eq!(
        hash_hex("t10.function", "0exp1j983").unwrap(),
        "ec28edb5ed47500"
    );
    assert!(hash_hex("no.such", "x").is_err());
    assert!(hash_families().contains("\"t7.canon\""));
}

#[test]
fn parse_reports_errors_with_positions() {
    let out = superset().parse_json("f()\n{\n\tx = é;\n", false);
    assert!(
        out.starts_with("{\"decompiled\":false,\"errors\":[{"),
        "{out}"
    );
    assert!(out.contains("\"line\":3"), "{out}");
    assert!(out.contains("\"kind\":\"SOURCE_FILE\""), "{out}");
    let clean = superset().parse_json(SOURCE, true);
    assert!(clean.contains("\"errors\":[]"), "{clean}");
    assert!(clean.contains("\"text\":\"return\""), "{clean}");
}

#[test]
fn utf16_offsets_count_code_units() {
    let p = Positions::new("aé😀b");
    assert_eq!(p.utf16(TextSize::from(1)), 1);
    assert_eq!(p.utf16(TextSize::from(3)), 2);
    assert_eq!(p.utf16(TextSize::from(7)), 4);
    assert_eq!(p.utf16(TextSize::from(8)), 5);
}

#[test]
fn check_runs_the_rules_on_one_file() {
    let out = superset()
        .check_json("f()\n{\n\twhile ( 1 )\n\t{\n\t}\n}\n", None, None, false)
        .unwrap();
    assert!(out.contains("\"code\":\"L001\""), "{out}");
    let none = superset()
        .check_json(SOURCE, Some("scripts/a.csc"), Some("L001"), false)
        .unwrap();
    assert_eq!(none, "[]");
    assert!(superset()
        .check_json(SOURCE, None, Some("NOPE!"), false)
        .is_err());
}

#[test]
fn format_returns_text_or_error() {
    let out = superset().format_json("f(){x=1;}", None);
    assert!(
        out.starts_with("{\"ok\":true,\"changed\":true,\"text\":"),
        "{out}"
    );
    let bad = superset().format_json("f( {", None);
    assert!(bad.starts_with("{\"ok\":false"), "{bad}");
}

#[test]
fn unknown_games_need_a_profile() {
    assert!(Engine::load(Some("t9"), None, None, None).is_err());
    let profile = "id = \"demo\"\n[keywords]\nwords = [\"if\", \"return\"]\n";
    let e = Engine::load(Some("t9"), Some(profile), None, None).unwrap();
    assert_eq!(e.game(), "demo");
}

#[cfg(gsc_embedded_t7)]
#[test]
fn embedded_t7_knows_its_builtins() {
    let e = Engine::load(Some("t7"), None, None, None).unwrap();
    assert!(e.has_builtins());
    let out = e
        .check_json(
            "function f()\n{\n\tx = abs( 1, 2, 3 );\n}\n",
            None,
            None,
            false,
        )
        .unwrap();
    assert!(out.contains("\"code\":\"B002\""), "{out}");
}
