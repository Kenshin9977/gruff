//! Embed a small T7 (Black Ops III) data set when gruff-data is at hand: the profile, the event
//! catalogue and the builtin catalogue reduced to what `check` reads (names, hashes, arities).
//!
//! gruff-data is looked up in `GRUFF_DATA`, then `GSC_DATA`, then next to the repository
//! (`../gruff-data`, or the older `../gsc-data`). Without it
//! the crate builds with no embedded game: callers pass profile and catalogue TOML themselves.

use std::path::{Path, PathBuf};

const BUILTIN_KEYS: &[&str] = &[
    "name",
    "hash",
    "vm",
    "kind",
    "min_args",
    "max_args",
    "max_checked",
    "type_flags",
    "shadowed",
    "arity_unchecked",
    "arity_unknown",
];

fn data_dir() -> Option<PathBuf> {
    if let Some(d) = ["GRUFF_DATA", "GSC_DATA"]
        .iter()
        .find_map(|v| std::env::var_os(v).filter(|d| !d.is_empty()))
    {
        let d = PathBuf::from(d);
        return d.join("profiles").is_dir().then_some(d);
    }
    let up = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR")?).join("../../..");
    ["gruff-data", "gsc-data"]
        .iter()
        .map(|n| up.join(n))
        .find(|d| d.join("profiles").is_dir())
}

/// Keep the keys `check` reads, drop provenance and evidence.
fn slim_entries(doc: &toml::Table, key: &str) -> toml::Value {
    let entries = doc
        .get(key)
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let slim = entries
        .into_iter()
        .filter_map(|e| {
            let t = e.as_table()?;
            let kept: toml::Table = t
                .iter()
                .filter(|(k, _)| BUILTIN_KEYS.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            Some(toml::Value::Table(kept))
        })
        .collect();
    toml::Value::Array(slim)
}

fn slim_builtins(text: &str) -> Result<String, String> {
    let doc: toml::Table = text.parse().map_err(|e| format!("builtins: {e}"))?;
    let mut out = toml::Table::new();
    for k in ["schema_version", "game", "hash_family", "coverage"] {
        if let Some(v) = doc.get(k) {
            out.insert(k.to_owned(), v.clone());
        }
    }
    out.insert("builtin".to_owned(), slim_entries(&doc, "builtin"));
    out.insert("intrinsic".to_owned(), slim_entries(&doc, "intrinsic"));
    toml::to_string(&out).map_err(|e| format!("builtins: {e}"))
}

fn copy(src: &Path, dst: &Path, slim: bool) -> Result<(), String> {
    println!("cargo:rerun-if-changed={}", src.display());
    let text = std::fs::read_to_string(src).map_err(|e| format!("{}: {e}", src.display()))?;
    let text = if slim { slim_builtins(&text)? } else { text };
    std::fs::write(dst, text).map_err(|e| format!("{}: {e}", dst.display()))
}

fn main() {
    println!("cargo:rerun-if-env-changed=GRUFF_DATA");
    println!("cargo:rerun-if-env-changed=GSC_DATA");
    println!("cargo::rustc-check-cfg=cfg(gsc_embedded_t7)");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let Some(data) = data_dir() else {
        return;
    };
    let files = [
        ("profiles/t7.toml", "t7_profile.toml", false),
        ("events/t7.toml", "t7_events.toml", false),
        ("builtins/t7.toml", "t7_builtins.toml", true),
    ];
    for (src, dst, slim) in files {
        if let Err(e) = copy(&data.join(src), &out.join(dst), slim) {
            println!("cargo:warning={e}: no game is embedded");
            return;
        }
    }
    println!("cargo:rustc-cfg=gsc_embedded_t7");
}
