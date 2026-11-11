//! The promotion gate: a rule may only report as an error (be stable rather than preview) when
//! the measurements in `docs/rules/precision.toml` support it. See that file's header.

use std::path::PathBuf;

use gsc_check::{Severity, RULES};

fn precision_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/rules/precision.toml")
        .canonicalize()
        .expect("docs/rules/precision.toml")
}

fn table() -> toml::Table {
    let text = std::fs::read_to_string(precision_file()).expect("read precision.toml");
    text.parse().expect("precision.toml is not valid TOML")
}

#[test]
fn records_name_rules_that_exist() {
    let t = table();
    for code in t.keys().filter(|k| *k != "gate") {
        assert!(
            RULES.iter().any(|r| r.code == code),
            "precision.toml has a record for `{code}`, which is not a rule"
        );
    }
}

#[test]
fn a_record_below_the_gate_keeps_its_rule_in_preview() {
    let t = table();
    let gate = t["gate"].as_table().expect("[gate]");
    let min_precision = gate["min_precision"].as_float().expect("min_precision");
    let min_reports = gate["min_reports"].as_integer().expect("min_reports");

    for (code, v) in t.iter().filter(|(k, _)| *k != "gate") {
        let rec = v
            .as_table()
            .unwrap_or_else(|| panic!("[{code}] is not a table"));
        let get = |k: &str| {
            rec.get(k)
                .and_then(toml::Value::as_integer)
                .unwrap_or_else(|| panic!("[{code}] has no integer `{k}`"))
        };
        let (reports, t_count, f_count) = (get("reports"), get("true"), get("false"));
        assert!(
            t_count + f_count <= reports,
            "[{code}]: {t_count} true + {f_count} false is more than {reports} reports"
        );
        let judged = t_count + f_count;
        let precision = if judged == 0 {
            0.0
        } else {
            t_count as f64 / judged as f64
        };
        let passes = judged >= min_reports && precision >= min_precision;
        let severity = Severity::of(code);
        if !passes {
            assert!(
                severity.is_hint(),
                "[{code}] reports as an error, but its record does not meet the gate: \
                 {t_count} true of {judged} judged ({:.0}%), {reports} reports, and the gate is \
                 {:.0}% over at least {min_reports}. Either measure it again or put it back in \
                 preview.",
                precision * 100.0,
                min_precision * 100.0,
            );
        }
    }
}

/// Every rule that reports as an error is stable, and every hint is a preview rule: the two
/// notions are one thing, so that `--preview` can never fail a build.
#[test]
fn hints_are_exactly_the_preview_rules() {
    for r in RULES {
        assert_eq!(
            Severity::of(r.code).is_hint(),
            r.preview,
            "{} ({}) is preview = {} but reports as {:?}",
            r.code,
            r.name,
            r.preview,
            Severity::of(r.code)
        );
    }
}
