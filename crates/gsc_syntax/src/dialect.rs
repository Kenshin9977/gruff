//! Dialects: the feature set of one game, read at run time from a gruff-data profile.
//!
//! `keywords.words` decides which words lex as keywords; `features` decides what the dialect
//! check reports. Unknown values, unresolved entries and missing features keep the check silent.
//! `"n/a"` (the feature does not apply to the game, such as `hash_equality` without hash
//! literals) reads as off: a known answer, never a guess.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::SyntaxKind;

/// A feature value as the profile spells it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeatureValue {
    Bool(bool),
    Text(String),
    List(Vec<String>),
    /// `"n/a"`: the feature does not apply to this game (the construct it is about cannot
    /// occur). Gates read it as off.
    NotApplicable,
    Unknown,
}

/// The answer of a gate query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// The dialect has the feature.
    On,
    /// The dialect lacks the feature: report it.
    Off,
    /// The profile does not know (unknown value, unresolved entry, or no such feature).
    Unknown,
}

/// Which toolchain the scripts target.
///
/// A profile's `value` always describes the retail compiler. `community.value`, when present,
/// describes a community compiler or client (gsc-tool, Plutonium...): BO2's preprocessor and
/// `.gsh` headers exist only there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mode {
    /// The game's own compiler (the default).
    #[default]
    Retail,
    /// A community compiler or client: read `features.<f>.community.value` when present.
    Community,
}

impl Mode {
    /// `retail` or `community`.
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Retail => "retail",
            Mode::Community => "community",
        }
    }
}

impl std::str::FromStr for Mode {
    type Err = ProfileError;
    fn from_str(s: &str) -> Result<Mode, ProfileError> {
        match s.to_ascii_lowercase().as_str() {
            "retail" => Ok(Mode::Retail),
            "community" => Ok(Mode::Community),
            _ => Err(ProfileError(format!(
                "unknown mode {s:?}; expected retail or community"
            ))),
        }
    }
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The feature set of one game.
#[derive(Clone, Debug)]
pub struct Dialect {
    /// Engine id (`t7`), or `superset`.
    pub id: String,
    /// Display name (`Call of Duty: Black Ops III`).
    pub name: String,
    keywords: Vec<bool>,
    case_insensitive: bool,
    features: HashMap<String, (FeatureValue, bool)>,
    /// `features.<f>.community.value`, for the features that have one.
    community: HashMap<String, FeatureValue>,
    mode: Mode,
    gating: bool,
    decompiled: bool,
}

/// An error while reading a profile.
#[derive(Debug)]
pub struct ProfileError(pub String);

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ProfileError {}

fn keyword_index(kind: SyntaxKind) -> usize {
    SyntaxKind::KEYWORDS
        .iter()
        .position(|(_, k)| *k == kind)
        .unwrap_or(usize::MAX)
}

impl Dialect {
    /// The superset dialect: every keyword on, no dialect diagnostics. Needs no data.
    pub fn superset() -> Dialect {
        Dialect {
            id: "superset".to_owned(),
            name: "every dialect".to_owned(),
            keywords: vec![true; SyntaxKind::KEYWORDS.len()],
            case_insensitive: true,
            features: HashMap::new(),
            community: HashMap::new(),
            mode: Mode::Retail,
            gating: false,
            decompiled: false,
        }
    }

    /// Parse a profile (the text of `gruff-data/profiles/<id>.toml`).
    pub fn from_toml_str(text: &str) -> Result<Dialect, ProfileError> {
        let doc: toml::Table = text
            .parse()
            .map_err(|e| ProfileError(format!("invalid TOML: {e}")))?;
        let id = doc
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProfileError("profile has no `id`".to_owned()))?
            .to_owned();
        let name = doc
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&id)
            .to_owned();

        let mut keywords = vec![false; SyntaxKind::KEYWORDS.len()];
        let words = doc
            .get("keywords")
            .and_then(|k| k.get("words"))
            .and_then(|w| w.as_array())
            .ok_or_else(|| ProfileError(format!("profile {id} has no keywords.words")))?;
        for w in words {
            if let Some(w) = w.as_str() {
                let w = w.to_ascii_lowercase();
                if let Some(i) = SyntaxKind::KEYWORDS.iter().position(|(t, _)| *t == w) {
                    keywords[i] = true;
                }
            }
        }

        let mut features = HashMap::new();
        let mut community = HashMap::new();
        if let Some(table) = doc.get("features").and_then(|f| f.as_table()) {
            for (key, entry) in table {
                let (value, unresolved) = match entry {
                    toml::Value::Table(t) => {
                        let unresolved =
                            t.get("status").and_then(|s| s.as_str()) == Some("unresolved");
                        let value = t.get("value").map(convert).unwrap_or(FeatureValue::Unknown);
                        if let Some(c) = t.get("community").and_then(|c| c.get("value")) {
                            community.insert(key.clone(), convert(c));
                        }
                        (value, unresolved)
                    }
                    other => (convert(other), false),
                };
                features.insert(key.clone(), (value, unresolved));
            }
        }
        let mut d = Dialect {
            id,
            name,
            keywords,
            case_insensitive: true,
            features,
            community,
            mode: Mode::Retail,
            gating: true,
            decompiled: false,
        };
        d.case_insensitive = d.compute_case_insensitive();
        Ok(d)
    }

    /// Read a profile file.
    pub fn load(path: &Path) -> Result<Dialect, ProfileError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| ProfileError(format!("cannot read {}: {e}", path.display())))?;
        Dialect::from_toml_str(&text)
            .map_err(|e| ProfileError(format!("{}: {}", path.display(), e.0)))
    }

    /// Find the profile of `game` (an id such as `t7` or an alias such as `bo3`) in a gruff-data
    /// `profiles` directory.
    pub fn find(profiles_dir: &Path, game: &str) -> Result<Dialect, ProfileError> {
        let direct = profiles_dir.join(format!("{game}.toml"));
        if direct.is_file() {
            return Dialect::load(&direct);
        }
        let entries = std::fs::read_dir(profiles_dir)
            .map_err(|e| ProfileError(format!("cannot read {}: {e}", profiles_dir.display())))?;
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "toml"))
            .collect();
        paths.sort();
        for p in &paths {
            let Ok(text) = std::fs::read_to_string(p) else {
                continue;
            };
            let Ok(doc) = text.parse::<toml::Table>() else {
                continue;
            };
            let matches = doc
                .get("aliases")
                .and_then(|a| a.as_array())
                .is_some_and(|a| a.iter().any(|x| x.as_str() == Some(game)));
            if matches {
                return Dialect::from_toml_str(&text)
                    .map_err(|e| ProfileError(format!("{}: {}", p.display(), e.0)));
            }
        }
        Err(ProfileError(format!(
            "no profile for game {game:?} in {}",
            profiles_dir.display()
        )))
    }

    /// The same dialect for decompiler output, whose names may hold `/`, `\`, `.` or a leading
    /// digit (hash lookups that resolved to asset paths).
    pub fn with_decompiled(mut self, decompiled: bool) -> Dialect {
        self.decompiled = decompiled;
        self
    }

    /// The same dialect in `mode`. Community mode reads `features.<f>.community.value` where the
    /// profile has one and `value` elsewhere.
    pub fn with_mode(mut self, mode: Mode) -> Dialect {
        self.mode = mode;
        self.case_insensitive = self.compute_case_insensitive();
        self
    }

    /// The active mode (retail unless [`Dialect::with_mode`] said otherwise).
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Does any feature of the profile have a `community` value (that is, does community mode
    /// differ from retail)?
    pub fn has_community_values(&self) -> bool {
        !self.community.is_empty()
    }

    /// The names of the features whose community value differs from retail, sorted.
    pub fn community_features(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self
            .community
            .iter()
            .filter(|(k, c)| self.features.get(*k).map(|(v, _)| v) != Some(*c))
            .map(|(k, _)| k.as_str())
            .collect();
        v.sort_unstable();
        v
    }

    /// The raw value of a feature in `mode`, whatever the active mode.
    pub fn feature_in(&self, name: &str, mode: Mode) -> Option<&FeatureValue> {
        if mode == Mode::Community {
            if let Some(v) = self.community.get(name) {
                return Some(v);
            }
        }
        self.features.get(name).map(|(v, _)| v)
    }

    /// Does the dialect read files with this extension (`gsc`, `csc`, `gsh`), per `file_kinds`?
    pub fn file_kind(&self, extension: &str) -> Gate {
        self.has("file_kinds", extension)
    }

    fn compute_case_insensitive(&self) -> bool {
        matches!(
            self.feature_in("case_insensitive_identifiers", self.mode),
            Some(FeatureValue::Bool(true)) | None
        )
    }

    /// Is the input decompiler output?
    pub fn decompiled(&self) -> bool {
        self.decompiled
    }

    /// Does this dialect report dialect diagnostics at all? The superset does not.
    pub fn is_gating(&self) -> bool {
        self.gating
    }

    /// Do identifiers (and keywords) compare case-insensitively?
    pub fn case_insensitive(&self) -> bool {
        self.case_insensitive
    }

    /// The keyword kind of `word` in this dialect, if it is a keyword here.
    pub fn keyword(&self, word: &str) -> Option<SyntaxKind> {
        let i = superset_keyword_index(word, self.case_insensitive)?;
        self.keywords[i].then_some(SyntaxKind::KEYWORDS[i].1)
    }

    /// Is `kind` a keyword of this dialect?
    pub fn has_keyword(&self, kind: SyntaxKind) -> bool {
        self.keywords
            .get(keyword_index(kind))
            .copied()
            .unwrap_or(false)
    }

    /// The raw value of a feature in the active mode.
    pub fn feature(&self, name: &str) -> Option<&FeatureValue> {
        self.feature_in(name, self.mode)
    }

    fn known(&self, name: &str) -> Option<&FeatureValue> {
        if !self.gating {
            return None;
        }
        if self.mode == Mode::Community {
            // A community value is its own claim: the retail entry's `unresolved` does not apply.
            match self.community.get(name) {
                Some(FeatureValue::Unknown) => return None,
                Some(v) => return Some(v),
                None => {}
            }
        }
        match self.features.get(name) {
            Some((_, true)) | None => None,
            Some((FeatureValue::Unknown, _)) => None,
            Some((v, false)) => Some(v),
        }
    }

    /// `gate: f`: is the boolean feature `f` on?
    pub fn flag(&self, name: &str) -> Gate {
        match self.known(name) {
            Some(FeatureValue::Bool(true)) => Gate::On,
            Some(FeatureValue::Bool(false) | FeatureValue::NotApplicable) => Gate::Off,
            _ => Gate::Unknown,
        }
    }

    /// `gate: f = v`: does the enum feature `f` equal `v`?
    pub fn is(&self, name: &str, value: &str) -> Gate {
        match self.known(name) {
            Some(FeatureValue::Text(t)) if t == value => Gate::On,
            Some(FeatureValue::Text(_) | FeatureValue::NotApplicable) => Gate::Off,
            _ => Gate::Unknown,
        }
    }

    /// `gate: f has v`: does the list feature `f` contain `v`?
    pub fn has(&self, name: &str, value: &str) -> Gate {
        match self.known(name) {
            Some(FeatureValue::List(l)) if l.iter().any(|x| x.eq_ignore_ascii_case(value)) => {
                Gate::On
            }
            Some(FeatureValue::List(_) | FeatureValue::NotApplicable) => Gate::Off,
            _ => Gate::Unknown,
        }
    }

    /// Does the profile say the feature does not apply to this game (`"n/a"`)?
    pub fn not_applicable(&self, name: &str) -> bool {
        matches!(self.known(name), Some(FeatureValue::NotApplicable))
    }

    /// The text value of an enum feature, if known and applicable.
    pub fn text(&self, name: &str) -> Option<&str> {
        match self.known(name) {
            Some(FeatureValue::Text(t)) => Some(t),
            _ => None,
        }
    }
}

/// Index of `word` in [`SyntaxKind::KEYWORDS`], whatever the dialect.
pub(crate) fn superset_keyword_index(word: &str, case_insensitive: bool) -> Option<usize> {
    if word.len() > 16 {
        return None;
    }
    let mut buf = [0u8; 16];
    let bytes = word.as_bytes();
    buf[..bytes.len()].copy_from_slice(bytes);
    if case_insensitive {
        buf[..bytes.len()].make_ascii_lowercase();
    }
    let w = std::str::from_utf8(&buf[..bytes.len()]).ok()?;
    SyntaxKind::keyword_index(w)
}

fn convert(v: &toml::Value) -> FeatureValue {
    match v {
        toml::Value::Boolean(b) => FeatureValue::Bool(*b),
        toml::Value::String(s) if s == "unknown" => FeatureValue::Unknown,
        toml::Value::String(s) if s == "n/a" => FeatureValue::NotApplicable,
        toml::Value::String(s) => FeatureValue::Text(s.clone()),
        toml::Value::Array(a) => FeatureValue::List(
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect(),
        ),
        _ => FeatureValue::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINI: &str = r#"
id = "tx"
name = "Test game"
[keywords]
words = ["if", "Function", "class"]
[features]
classes = { value = true, confidence = "documented", source = [] }
waitframe = { value = false, confidence = "documented", source = [] }
childthread = { value = false, confidence = "documented", source = [], status = "unresolved" }
import_directive = { value = "using", confidence = "documented", source = [] }
profiler_keywords = { value = ["profilestart"], confidence = "documented", source = [] }
array_literal = { value = "unknown", confidence = "documented", source = [] }
hash_equality = { value = "n/a", confidence = "documented", source = [] }
preprocessor_define = { value = false, confidence = "assumed", source = [], status = "unresolved", community = { value = true, tools = ["gsc-tool"], confidence = "documented", source = [] } }
header_directive = { value = "none", confidence = "assumed", source = [], community = { value = "inline", tools = ["gsc-tool"], confidence = "documented", source = [] } }
file_kinds = { value = ["gsc", "csc"], confidence = "documented", source = [], community = { value = ["gsc", "csc", "gsh"], tools = ["gsc-tool"], confidence = "documented", source = [] } }
"#;

    #[test]
    fn reads_a_profile() {
        let d = Dialect::from_toml_str(MINI).unwrap();
        assert_eq!(d.id, "tx");
        assert_eq!(d.keyword("if"), Some(SyntaxKind::IF_KW));
        assert_eq!(d.keyword("FUNCTION"), Some(SyntaxKind::FUNCTION_KW));
        assert_eq!(d.keyword("while"), None);
        assert_eq!(d.flag("classes"), Gate::On);
        assert_eq!(d.flag("waitframe"), Gate::Off);
        assert_eq!(d.flag("childthread"), Gate::Unknown);
        assert_eq!(d.flag("array_literal"), Gate::Unknown);
        assert_eq!(d.flag("no_such_feature"), Gate::Unknown);
        assert_eq!(d.is("import_directive", "using"), Gate::On);
        assert_eq!(d.is("import_directive", "include"), Gate::Off);
        assert_eq!(d.has("profiler_keywords", "profilestart"), Gate::On);
        assert_eq!(d.has("profiler_keywords", "prof_begin"), Gate::Off);
    }

    #[test]
    fn not_applicable_reads_as_off() {
        let d = Dialect::from_toml_str(MINI).unwrap();
        assert!(d.not_applicable("hash_equality"));
        assert!(!d.not_applicable("array_literal"));
        assert_eq!(
            d.feature("hash_equality"),
            Some(&FeatureValue::NotApplicable)
        );
        assert_eq!(d.is("hash_equality", "script-error"), Gate::Off);
        assert_eq!(d.flag("hash_equality"), Gate::Off);
        assert_eq!(d.text("hash_equality"), None);
        // unknown stays unknown
        assert_eq!(d.is("array_literal", "x"), Gate::Unknown);
    }

    #[test]
    fn community_mode_reads_the_community_value() {
        let retail = Dialect::from_toml_str(MINI).unwrap();
        assert_eq!(retail.mode(), Mode::Retail);
        assert_eq!(retail.flag("preprocessor_define"), Gate::Unknown);
        assert_eq!(retail.is("header_directive", "inline"), Gate::Off);
        assert_eq!(retail.file_kind("gsh"), Gate::Off);
        assert_eq!(
            retail.community_features(),
            ["file_kinds", "header_directive", "preprocessor_define"]
        );

        let community = retail.clone().with_mode(Mode::Community);
        assert_eq!(community.mode(), Mode::Community);
        assert_eq!(community.flag("preprocessor_define"), Gate::On);
        assert_eq!(community.is("header_directive", "inline"), Gate::On);
        assert_eq!(community.file_kind("gsh"), Gate::On);
        // Features without a community value read `value`.
        assert_eq!(community.flag("classes"), Gate::On);
        assert_eq!(community.flag("childthread"), Gate::Unknown);
        assert_eq!(
            community.feature_in("header_directive", Mode::Retail),
            Some(&FeatureValue::Text("none".to_owned()))
        );
        assert_eq!("Community".parse::<Mode>().unwrap(), Mode::Community);
        assert!("modded".parse::<Mode>().is_err());
    }

    #[test]
    fn superset_is_silent() {
        let d = Dialect::superset();
        assert_eq!(d.keyword("childthread"), Some(SyntaxKind::CHILDTHREAD_KW));
        assert_eq!(d.flag("classes"), Gate::Unknown);
    }
}
