//! The deprecated `gsc` alias runs the same program as `gruff`, with a notice on stderr.

use std::process::Command;

#[test]
fn gsc_alias_runs_gruff_with_a_notice() {
    let gruff = Command::new(env!("CARGO_BIN_EXE_gruff"))
        .args(["hash", "t7.canon", "main"])
        .output()
        .unwrap();
    let gsc = Command::new(env!("CARGO_BIN_EXE_gsc"))
        .args(["hash", "t7.canon", "main"])
        .output()
        .unwrap();
    assert!(gruff.status.success() && gsc.status.success());
    assert_eq!(gruff.stdout, gsc.stdout);
    assert!(gruff.stderr.is_empty());
    let notice = String::from_utf8_lossy(&gsc.stderr);
    assert!(notice.contains("deprecated alias of `gruff`"), "{notice}");
}

#[test]
fn help_names_gruff() {
    let out = Command::new(env!("CARGO_BIN_EXE_gruff"))
        .arg("--help")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Gruff: GSC language tools"), "{text}");
    assert!(text.contains("Usage: gruff"), "{text}");
}
