//! Re-hash local name databases. Ignored by default: the databases are not in the repository.
//!
//! ```text
//! GSC_BO7_NAMES_DIR=.../bo7-gsc-archive/names            (all_names.tsv, must be 100 %)
//! GSC_BO3_NAMES_CSV=.../bo3_names_and_labels.csv          (real names, must be 100 %)
//! GSC_WNI_DIR=.../package_index                           (GrayWolf .wni, reported only)
//! GSC_T89_PAIRS=.../getters.txt                           (BO4 exe field table, must be 100 %)
//! GSC_LOCALIZE_CSV=.../fnv1a_localize.csv                 (IW localize keys, must be 100 %)
//! cargo test -p gsc_hash --test name_databases -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;

use gsc_hash::{db, family, mask, Family, FAMILIES};

fn env_path(var: &str) -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os(var)?);
    if p.exists() {
        Some(p)
    } else {
        eprintln!("{var}={} does not exist, skipped", p.display());
        None
    }
}

fn read(path: &std::path::Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    String::from_utf8_lossy(&bytes).into_owned()
}

#[derive(Default)]
struct Score {
    ok: usize,
    total: usize,
    bad: Vec<String>,
}

impl Score {
    fn add(&mut self, pass: bool, what: impl FnOnce() -> String) {
        self.total += 1;
        if pass {
            self.ok += 1;
        } else if self.bad.len() < 5 {
            self.bad.push(what());
        }
    }
    fn pct(&self) -> f64 {
        100.0 * self.ok as f64 / self.total.max(1) as f64
    }
}

fn report(title: &str, scores: &BTreeMap<String, Score>) {
    println!("\n{title}");
    for (k, s) in scores {
        println!("  {k:<22} {:>7} / {:<7} {:>7.3} %", s.ok, s.total, s.pct());
        for b in &s.bad {
            println!("      mismatch: {b}");
        }
    }
}

fn bo7_family(tsv: &str) -> &'static Family {
    let id = match tsv {
        "dvarhash" => "t10.dvar".to_string(),
        other => format!("t10.{other}"),
    };
    family(&id).unwrap_or_else(|| panic!("no family for {tsv}"))
}

#[test]
#[ignore]
fn bo7_gsc_archive_names() {
    let Some(dir) = env_path("GSC_BO7_NAMES_DIR") else {
        return;
    };
    let text = read(&dir.join("all_names.tsv"));
    let mut lines = text.lines();
    let header: Vec<_> = lines.next().unwrap().split('\t').collect();
    let col = |n: &str| header.iter().position(|h| *h == n).unwrap();
    let (cf, cb, ch, cn) = (col("family"), col("bits"), col("hash"), col("name"));
    let mut scores: BTreeMap<String, Score> = BTreeMap::new();
    for line in lines.filter(|l| !l.is_empty()) {
        let row: Vec<_> = line.split('\t').collect();
        let f = bo7_family(row[cf]);
        let bits: u32 = row[cb].parse().unwrap();
        let value = db::parse_hex(row[ch]).unwrap();
        let name = row[cn];
        assert!(bits <= f.bits, "{} stored at {bits} bits", f.id);
        let pass = (f.hash(name) & mask(bits)) == (value & mask(bits));
        scores
            .entry(f.id.to_string())
            .or_default()
            .add(pass, || format!("{value:x} {name}"));
    }
    report("bo7-gsc-archive/names/all_names.tsv", &scores);
    let total: usize = scores.values().map(|s| s.total).sum();
    println!("  {} pairs", total);
    for (k, s) in &scores {
        assert_eq!(s.ok, s.total, "{k} below 100 %");
    }
}

#[test]
#[ignore]
fn bo3_names_csv() {
    let Some(path) = env_path("GSC_BO3_NAMES_CSV") else {
        return;
    };
    let f = family("t7.canon").unwrap();
    let text = read(&path);
    let (mut labels, mut lossy) = (0usize, 0usize);
    let mut s = Score::default();
    for (value, name) in db::read_csv_pairs(&text) {
        if name.starts_with("x_") {
            labels += 1;
            continue;
        }
        // U+FFFD: the CSV lost the original bytes of the name; it cannot be re-hashed.
        if name.contains('\u{FFFD}') {
            lossy += 1;
            continue;
        }
        s.add(f.verify(name, value, 32), || format!("{value:08x} {name}"));
    }
    let mut scores = BTreeMap::new();
    scores.insert("t7.canon".to_string(), s);
    report("BO3 names CSV", &scores);
    println!("  skipped: {labels} x_* labels, {lossy} names holding U+FFFD");
    let s = &scores["t7.canon"];
    assert_eq!(s.ok, s.total, "t7.canon below 100 %");
}

#[test]
#[ignore]
fn bo4_t89_field_table() {
    let Some(path) = env_path("GSC_T89_PAIRS") else {
        return;
    };
    let f = family("t89.scr").unwrap();
    let mut s = Score::default();
    for line in read(&path).lines() {
        let mut it = line.split_whitespace();
        let (Some(_addr), Some(h), Some(name)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        if h.len() != 8 || !name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
            continue;
        }
        let Some(value) = db::parse_hex(h) else {
            continue;
        };
        if name == "acc" {
            continue;
        }
        s.add(f.verify(name, value, 32), || format!("{value:08x} {name}"));
    }
    let mut scores = BTreeMap::new();
    scores.insert("t89.scr".to_string(), s);
    report("BO4 field table (T89)", &scores);
    let s = &scores["t89.scr"];
    assert!(s.total > 0);
    assert_eq!(s.ok, s.total);
}

#[test]
#[ignore]
fn iw_localize_csv() {
    let Some(path) = env_path("GSC_LOCALIZE_CSV") else {
        return;
    };
    let iw = family("iw.localize").unwrap();
    let ta = family("iw8.localize_ta").unwrap();
    let text = read(&path);
    let (mut s, mut t) = (Score::default(), Score::default());
    for (value, name) in db::read_csv_pairs(&text) {
        s.add(iw.verify(name, value, 60), || format!("{value:x} {name}"));
        t.add(ta.verify(name, value, 60), String::new);
    }
    println!(
        "\nlocalize CSV: IW basis {} / {}, TA basis {} / {}",
        s.ok, s.total, t.ok, t.total
    );
    assert_eq!(s.ok, s.total);
}

/// The families a GrayWolf index holds, by file stem.
fn wni_families(stem: &str) -> Vec<&'static Family> {
    let ids: &[&str] = match stem {
        "bo3_script_hashes" => &["t7.canon"],
        "bo2_sab" | "bo3_sab" => &["t6.sab"],
        "bo2_ipak" => &[], // native ipak keys, not a name hash
        "fnv1a_localize" => &["iw.localize"],
        "fnv1a_bones" => &["iw9.bone"],
        // a few keys there are hashed as written (bundle keys), not lower-cased
        "fnv1a_string" => &["t9.strings", "t10.bundlekey"],
        "fnv1a_xsounds" => &["t8.fnv64", "t8.sound_asset"],
        s if s.starts_with("fnv1a_v2_") || s == "fnv1a_ximages_mw5" => &["iw.asset"],
        s if s.starts_with("fnv1a_") => &["t8.fnv64"],
        s if s.starts_with("bo6_") || s.starts_with("bo7_") => {
            return FAMILIES
                .iter()
                .filter(|f| f.id.starts_with("t10.") || f.id.starts_with("iw."))
                .collect();
        }
        _ => return FAMILIES.iter().collect(),
    };
    ids.iter().map(|id| family(id).unwrap()).collect()
}

#[test]
#[ignore]
fn graywolf_wni_indexes() {
    let Some(dir) = env_path("GSC_WNI_DIR") else {
        return;
    };
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "wni"))
        .collect();
    files.sort();
    let mut scores: BTreeMap<String, Score> = BTreeMap::new();
    let mut by_family: BTreeMap<String, Score> = BTreeMap::new();
    for path in files {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let fams = wni_families(&stem);
        if fams.is_empty() {
            println!("  {stem}: skipped (not a name hash)");
            continue;
        }
        let data = std::fs::read(&path).unwrap();
        let pairs = db::read_wni(&data).unwrap_or_else(|e| panic!("{stem}: {e}"));
        let s = scores.entry(stem.clone()).or_default();
        for (key, name) in &pairs {
            if stem == "bo3_script_hashes" && name.starts_with("x_") {
                continue; // labels of unresolved hashes, not names
            }
            let hit = fams.iter().find(|f| {
                let w = if f.bits <= 32 { 32 } else { 60 };
                f.verify(name, *key, w)
            });
            if let Some(f) = hit {
                by_family
                    .entry(f.id.to_string())
                    .or_default()
                    .add(true, String::new);
            }
            s.add(hit.is_some(), || format!("{key:x} {name}"));
        }
    }
    report("GrayWolf package_index (.wni), any listed family", &scores);
    report("  matched pairs by family", &by_family);
}
