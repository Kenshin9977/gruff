//! The builtin catalogue as written (`gruff-data/builtins/<game>.toml`): what the checker's
//! [`gsc_sema::Catalogue`] drops and an agent wants to see, i.e. where each fact comes from.

use std::collections::HashMap;
use std::path::Path;

/// One `[[builtin]]` entry, as facts to print.
#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub name: String,
    pub vm: String,
    pub kind: String,
    pub min: Option<i64>,
    pub max: Option<i64>,
    pub param_types: Vec<String>,
    pub dev_only: bool,
    /// `max_checked = false`: extra arguments are ignored.
    pub max_unchecked: bool,
    /// `arity_unknown` or `arity_unchecked`.
    pub arity_flag: Option<&'static str>,
    pub arity_mode: Option<String>,
    pub arity_source: Option<String>,
    pub name_source: Option<String>,
}

/// The catalogue file's entries by lower-case name, plus its header facts.
#[derive(Debug, Default)]
pub struct Raw {
    pub file: String,
    pub source: Option<String>,
    pub arity_source: Option<String>,
    pub complete: bool,
    pub variadic_max: Option<i64>,
    pub by_name: HashMap<String, Vec<Entry>>,
}

use gsc_sema::data::toml_get::{flag, strs};

fn s(t: &toml::Table, k: &str) -> Option<String> {
    gsc_sema::data::toml_get::str(t, k).map(str::to_owned)
}

impl Raw {
    pub fn load(path: &Path) -> Result<Raw, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let doc: toml::Table = text
            .parse()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let ext = doc.get("extraction").and_then(|v| v.as_table());
        let mut raw = Raw {
            file: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            source: s(&doc, "source"),
            arity_source: ext.and_then(|e| s(e, "arity_source")),
            complete: doc
                .get("coverage")
                .and_then(|c| c.get("complete"))
                .and_then(|v| v.as_bool())
                != Some(false),
            variadic_max: ext
                .and_then(|e| e.get("variadic_max"))
                .and_then(|v| v.as_integer()),
            by_name: HashMap::new(),
        };
        let list = doc.get("builtin").and_then(|v| v.as_array());
        for t in list.into_iter().flatten().filter_map(|v| v.as_table()) {
            let Some(name) = s(t, "name") else { continue };
            if flag(t, "shadowed") {
                continue;
            }
            let e = Entry {
                vm: s(t, "vm").unwrap_or_else(|| "gsc".to_owned()),
                kind: s(t, "kind").unwrap_or_else(|| "function".to_owned()),
                min: t.get("min_args").and_then(|v| v.as_integer()),
                max: t.get("max_args").and_then(|v| v.as_integer()),
                param_types: strs(t, "param_types"),
                dev_only: flag(t, "dev_only")
                    || t.get("type_flags").and_then(|v| v.as_integer()) == Some(1),
                max_unchecked: t.get("max_checked").and_then(|v| v.as_bool()) == Some(false),
                arity_flag: if flag(t, "arity_unknown") {
                    Some("arity_unknown")
                } else if flag(t, "arity_unchecked") {
                    Some("arity_unchecked")
                } else {
                    None
                },
                arity_mode: s(t, "arity_mode"),
                arity_source: s(t, "arity_source"),
                name_source: s(t, "name_source"),
                name: name.clone(),
            };
            raw.by_name
                .entry(name.to_ascii_lowercase())
                .or_default()
                .push(e);
        }
        Ok(raw)
    }

    /// Every name, for hash lookups and suggestions.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.by_name.values().flatten().map(|e| e.name.as_str())
    }
}

impl Entry {
    /// `1 argument`, `0 to 2 arguments`, `at least 1 argument`, or why it is unknown.
    pub fn arity(&self, variadic: Option<i64>) -> String {
        if let Some(f) = self.arity_flag {
            return format!("arity not checked ({f})");
        }
        let plural = |n: i64| if n == 1 { "" } else { "s" };
        match (self.min, self.max) {
            (Some(lo), Some(hi)) if lo == hi => format!("{lo} argument{}", plural(lo)),
            (Some(lo), Some(hi)) if variadic.is_some_and(|v| hi >= v) || hi >= 1 << 20 => {
                format!("at least {lo} argument{}", plural(lo))
            }
            (Some(lo), Some(hi)) => format!("{lo} to {hi} arguments"),
            _ => "no arity recorded".to_owned(),
        }
    }

    /// How far the arity can be trusted, in one phrase.
    pub fn verification(&self, raw: &Raw) -> String {
        let mut parts = Vec::new();
        if let Some(m) = &self.arity_mode {
            parts.push(format!("arity_mode={m}"));
        }
        match (&self.arity_source, &raw.arity_source) {
            (Some(a), _) => parts.push(format!("arity from {a}")),
            (None, Some(a)) => parts.push(format!("arity from {}", short(a))),
            (None, None) => {
                if let Some(src) = &raw.source {
                    parts.push(format!("arity from the engine table ({src})"));
                }
            }
        }
        if self.max_unchecked {
            parts.push("extra arguments ignored by the engine".to_owned());
        }
        if let Some(n) = &self.name_source {
            parts.push(format!("name from {n}"));
        }
        if !raw.complete {
            parts.push("partial catalogue".to_owned());
        }
        parts.join("; ")
    }
}

/// The part of a long description before its first `:` (the method, not its details).
fn short(s: &str) -> &str {
    s.split(':').next().unwrap_or(s).trim()
}
