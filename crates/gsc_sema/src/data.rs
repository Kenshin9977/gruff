//! Engine data from gruff-data: the builtin catalogue (`builtins/<game>.toml`) and the field
//! catalogue (`fields/<game>.toml`) of one game, both looked up by hash so that entries whose
//! name is unknown still count.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::vfs::Vm;

/// How a catalogue keys names: the game's hash family, or (without one) FNV-1a of the name.
#[derive(Clone, Copy, Debug, Default)]
struct NameKey(Option<&'static gsc_hash::Family>);

impl NameKey {
    fn key(self, name: &str) -> u64 {
        let lower = name.to_ascii_lowercase();
        match self.0 {
            Some(f) => f.hash(&lower),
            None => lower.bytes().fold(0xcbf29ce484222325u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
            }),
        }
    }
}

/// The parts every catalogue file shares: `game`, `hash_family` and its `[[<entries>]]` array.
struct Header<'a> {
    game: String,
    key: NameKey,
    entries: &'a [toml::Value],
}

fn header<'a>(doc: &'a toml::Table, entries: &str) -> Result<Header<'a>, String> {
    let game = doc
        .get("game")
        .and_then(|v| v.as_str())
        .ok_or("no `game`")?
        .to_owned();
    let key = match doc.get("hash_family").and_then(|v| v.as_str()) {
        Some(id) => NameKey(Some(
            gsc_hash::family(id).ok_or(format!("unknown hash family {id}"))?,
        )),
        None => NameKey(None),
    };
    let entries = doc
        .get(entries)
        .and_then(|v| v.as_array())
        .ok_or(format!("no [[{entries}]] entries"))?;
    Ok(Header { game, key, entries })
}

/// The `hash` of an entry (`"0x1a2b"`), if any.
fn entry_hash(t: &toml::Table) -> Option<u64> {
    t.get("hash")
        .and_then(|v| v.as_str())
        .and_then(|h| u64::from_str_radix(h.trim_start_matches("0x"), 16).ok())
}

/// Typed reads of a catalogue entry's keys (absent or of another type: None / false / empty).
pub mod toml_get {
    pub fn int(t: &toml::Table, key: &str) -> Option<i64> {
        t.get(key).and_then(|v| v.as_integer())
    }

    pub fn str<'a>(t: &'a toml::Table, key: &str) -> Option<&'a str> {
        t.get(key).and_then(|v| v.as_str())
    }

    /// `key = true`.
    pub fn flag(t: &toml::Table, key: &str) -> bool {
        t.get(key).and_then(|v| v.as_bool()) == Some(true)
    }

    /// The strings of an array of strings (other items skipped).
    pub fn strs(t: &toml::Table, key: &str) -> Vec<String> {
        t.get(key)
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|x| x.as_str().map(str::to_owned))
            .collect()
    }
}

use toml_get::{flag, int};

fn read(path: &Path) -> Result<toml::Table, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.parse()
        .map_err(|e| format!("{}: invalid TOML: {e}", path.display()))
}

/// Function (`foo()`) or method (`ent foo()`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Function,
    Method,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Function => "function",
            Kind::Method => "method",
        }
    }
}

/// One catalogue entry.
#[derive(Clone, Debug)]
pub struct Builtin {
    pub name: Option<String>,
    pub vm: Vm,
    pub kind: Kind,
    /// `None` when the engine records no arity, or the catalogue marks it `arity_unknown`.
    pub arity: Option<(u32, u32)>,
    /// The engine rejects more than the maximum. False with `max_checked = false`: the handler
    /// ignores extra arguments, so only the minimum is a rule.
    pub max_enforced: bool,
    /// Debug builtin: the linker rejects it outside a dev block.
    pub dev_only: bool,
}

/// `0 to 2 arguments`, `1 argument`, `at least 1 argument`.
fn describe(arity: Option<(u32, u32)>) -> Option<String> {
    let plural = |n: u32| if n == 1 { "" } else { "s" };
    arity.map(|(lo, hi)| match (lo, hi) {
        (lo, hi) if lo == hi => format!("{lo} argument{}", plural(lo)),
        (lo, hi) if hi >= 1 << 20 => format!("at least {lo} argument{}", plural(lo)),
        (lo, hi) => format!("{lo} to {hi} arguments"),
    })
}

/// `min_args`/`max_args` of an entry, unless it has none or is marked unknown or unchecked.
fn entry_arity(t: &toml::Table) -> Option<(u32, u32)> {
    if flag(t, "arity_unchecked") || flag(t, "arity_unknown") {
        return None;
    }
    match (int(t, "min_args"), int(t, "max_args")) {
        (Some(lo), Some(hi)) if lo >= 0 && hi >= lo => {
            Some((lo as u32, hi.min(u32::MAX as i64) as u32))
        }
        _ => None,
    }
}

impl Builtin {
    /// `0 to 2 arguments`, `1 argument`, `at least 1 argument`.
    pub fn describe_arity(&self) -> Option<String> {
        describe(self.arity)
    }

    /// Does a call with `argc` arguments fit? True when no arity is recorded.
    pub fn accepts(&self, argc: u32) -> bool {
        self.arity
            .is_none_or(|(lo, hi)| argc >= lo && (argc <= hi || !self.max_enforced))
    }
}

/// A call the compiler turns into one VM opcode (`isdefined`, `vectorscale`, `waittill` in the
/// later games): no table lookup resolves it, so it is known whatever the tables hold. Read from
/// the catalogue's `[[intrinsic]]` list, the single source of truth: a profile's
/// `opcode_builtins` documents the dialect and is not read by the tools.
#[derive(Clone, Debug)]
pub struct Intrinsic {
    pub name: String,
    /// `None`: both VMs.
    pub vm: Option<Vm>,
    /// From `min_args`/`max_args`; `None` when the catalogue gives none.
    pub arity: Option<(u32, u32)>,
}

impl Intrinsic {
    pub fn describe_arity(&self) -> Option<String> {
        describe(self.arity)
    }

    /// Does a call with `argc` arguments fit? True when no arity is recorded.
    pub fn accepts(&self, argc: u32) -> bool {
        self.arity.is_none_or(|(lo, hi)| (lo..=hi).contains(&argc))
    }
}

/// The builtin catalogue of one game.
#[derive(Clone, Debug, Default)]
pub struct Catalogue {
    pub game: String,
    key: NameKey,
    by_hash: HashMap<u64, Vec<Builtin>>,
    /// Named entries, for suggestions and completion.
    names: Vec<(String, Vm, Kind)>,
    /// Calls the compiler turns into opcodes (`[[intrinsic]]`).
    intrinsics: Vec<Intrinsic>,
    /// Some name the game can call may be missing: `coverage.complete = false` (an executable of
    /// the game was not extracted), or an entry has neither a name nor a hash.
    partial: bool,
}

impl Catalogue {
    pub fn load(path: &Path) -> Result<Catalogue, String> {
        Catalogue::from_table(&read(path)?).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn from_toml_str(text: &str) -> Result<Catalogue, String> {
        let doc: toml::Table = text.parse().map_err(|e| format!("invalid TOML: {e}"))?;
        Catalogue::from_table(&doc)
    }

    fn from_table(doc: &toml::Table) -> Result<Catalogue, String> {
        let h = header(doc, "builtin")?;
        let mut c = Catalogue {
            game: h.game,
            key: h.key,
            partial: doc
                .get("coverage")
                .and_then(|v| v.get("complete"))
                .and_then(|v| v.as_bool())
                == Some(false),
            ..Catalogue::default()
        };
        for e in h.entries {
            let Some(t) = e.as_table() else { continue };
            if flag(t, "shadowed") {
                continue; // the engine never returns it
            }
            let name = t.get("name").and_then(|v| v.as_str()).map(str::to_owned);
            let key = match (entry_hash(t), &name) {
                (Some(h), _) => h,
                (None, Some(n)) => c.key.key(n),
                (None, None) => {
                    c.partial = true; // a builtin nobody can name
                    continue;
                }
            };
            let vm = match t.get("vm").and_then(|v| v.as_str()) {
                Some("csc") => Vm::Csc,
                _ => Vm::Gsc,
            };
            let kind = match t.get("kind").and_then(|v| v.as_str()) {
                Some("method") => Kind::Method,
                _ => Kind::Function,
            };
            let arity = entry_arity(t);
            let max_enforced = t.get("max_checked").and_then(|v| v.as_bool()) != Some(false);
            let dev_only = int(t, "type_flags") == Some(1);
            if let Some(n) = &name {
                c.names.push((n.clone(), vm, kind));
            }
            c.by_hash.entry(key).or_default().push(Builtin {
                name,
                vm,
                kind,
                arity,
                max_enforced,
                dev_only,
            });
        }
        let intrinsics = doc.get("intrinsic").and_then(|v| v.as_array());
        for t in intrinsics
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_table())
        {
            let Some(name) = t.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            let vm = match t.get("vm").and_then(|v| v.as_str()) {
                Some("csc") => Some(Vm::Csc),
                Some("gsc") => Some(Vm::Gsc),
                _ => None,
            };
            c.intrinsics.push(Intrinsic {
                name: name.to_ascii_lowercase(),
                vm,
                arity: entry_arity(t),
            });
        }
        Ok(c)
    }

    /// Add intrinsics by name (both VMs, no arity), as a test catalogue needs.
    pub fn with_intrinsics(mut self, names: impl IntoIterator<Item = String>) -> Catalogue {
        self.intrinsics.extend(names.into_iter().map(|n| Intrinsic {
            name: n.to_ascii_lowercase(),
            vm: None,
            arity: None,
        }));
        self
    }

    /// Every entry for `name` (any VM, any kind). A decompiler's `function_<hex>` spelling looks
    /// the hash up, in a catalogue keyed by a hash family.
    pub fn lookup(&self, name: &str) -> &[Builtin] {
        let key = match self.key.0 {
            Some(f) => gsc_hash::parse_spelling(name, "function_")
                .map(|v| v & gsc_hash::mask(f.bits))
                .unwrap_or_else(|| self.key.key(name)),
            None => self.key.key(name),
        };
        self.by_hash
            .get(&key)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// Every entry keyed `hash` (a decompiler's `function_<hex>` spelling of an unnamed call).
    pub fn lookup_hash(&self, hash: u64) -> &[Builtin] {
        self.by_hash
            .get(&hash)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// The intrinsic `name`, if the compiler turns it into an opcode.
    pub fn intrinsic(&self, name: &str) -> Option<&Intrinsic> {
        self.intrinsics
            .iter()
            .find(|o| o.name.eq_ignore_ascii_case(name))
    }

    /// Is `name` an intrinsic (any VM)?
    pub fn is_intrinsic(&self, name: &str) -> bool {
        self.intrinsic(name).is_some()
    }

    /// Does the catalogue hold every name the game can call? A name it lacks is then unknown to
    /// the engine; otherwise it may be an entry the catalogue cannot name.
    pub fn is_complete(&self) -> bool {
        !self.partial
    }

    /// Is `name` a builtin of any kind in any VM (intrinsics included)?
    pub fn is_known(&self, name: &str) -> bool {
        self.is_intrinsic(name) || !self.lookup(name).is_empty()
    }

    /// Is `name` a builtin of `vm` (a header counts as GSC)? Both call forms are accepted, since
    /// scripts call a few functions as methods.
    pub fn contains(&self, vm: Option<Vm>, name: &str) -> bool {
        let vm = vm.unwrap_or(Vm::Gsc);
        self.intrinsic(name)
            .is_some_and(|i| i.vm.is_none_or(|v| v == vm))
            || self.lookup(name).iter().any(|b| b.vm == vm)
    }

    /// The named entries of `vm` and `kind` closest to `name` (edit distance at most 2).
    pub fn suggest(&self, name: &str, vm: Option<Vm>, kind: Kind) -> Vec<&str> {
        let lower = name.to_ascii_lowercase();
        let max = if lower.len() <= 4 { 1 } else { 2 };
        let mut best: Vec<(usize, &str)> = self
            .names
            .iter()
            .filter(|(_, v, k)| *k == kind && vm.is_none_or(|vm| vm == *v))
            .filter_map(|(n, _, _)| {
                let d = levenshtein(&lower, n, max)?;
                Some((d, n.as_str()))
            })
            .collect();
        best.sort();
        best.dedup_by_key(|(_, n)| *n);
        let Some(&(top, _)) = best.first() else {
            return Vec::new();
        };
        best.into_iter()
            .take_while(|(d, _)| *d == top)
            .map(|(_, n)| n)
            .collect()
    }

    /// The named entries: `(name, vm, kind)`, in file order.
    pub fn names(&self) -> &[(String, Vm, Kind)] {
        &self.names
    }

    /// The intrinsics, names lower case.
    pub fn intrinsics(&self) -> &[Intrinsic] {
        &self.intrinsics
    }

    pub fn len(&self) -> usize {
        self.by_hash.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.by_hash.is_empty()
    }
}

/// Every field the engine registers, on any object kind and in either VM.
#[derive(Clone, Debug, Default)]
pub struct FieldCatalogue {
    pub game: String,
    key: NameKey,
    hashes: HashSet<u64>,
    /// Named entries, sorted, for suggestions and completion.
    pub names: Vec<String>,
}

impl FieldCatalogue {
    pub fn load(path: &Path) -> Result<FieldCatalogue, String> {
        FieldCatalogue::from_table(&read(path)?).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn from_toml_str(text: &str) -> Result<FieldCatalogue, String> {
        let doc: toml::Table = text.parse().map_err(|e| format!("invalid TOML: {e}"))?;
        FieldCatalogue::from_table(&doc)
    }

    fn from_table(doc: &toml::Table) -> Result<FieldCatalogue, String> {
        let h = header(doc, "field")?;
        let mut c = FieldCatalogue {
            game: h.game,
            key: h.key,
            ..FieldCatalogue::default()
        };
        for e in h.entries {
            let Some(t) = e.as_table() else { continue };
            let name = t.get("name").and_then(|v| v.as_str());
            let key = match (entry_hash(t), name) {
                (Some(h), _) => h,
                (None, Some(n)) => c.key.key(n),
                (None, None) => continue,
            };
            c.hashes.insert(key);
            if let Some(n) = name {
                c.names.push(n.to_owned());
            }
        }
        c.names.sort();
        c.names.dedup();
        Ok(c)
    }

    /// Does the engine define a field `name` on some object kind?
    pub fn contains(&self, name: &str) -> bool {
        self.hashes.contains(&self.key.key(name))
    }

    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }
}

/// `<gruff-data>/<dir>/<game>.toml` next to a `profiles` directory, when it exists.
pub fn data_file(profiles_dir: &Path, dir: &str, game: &str) -> Option<PathBuf> {
    let path = profiles_dir
        .parent()?
        .join(dir)
        .join(format!("{game}.toml"));
    path.is_file().then_some(path)
}

/// The builtin catalogue for `game` as the user spelled it: a variant catalogue of the profile
/// (`builtins/jup-sp.toml` for `--game jup-sp` or `jup_sp`, read with the `jup` profile) when
/// one exists, else `builtins/<dialect id>.toml`.
pub fn catalogue_file(
    profiles_dir: &Path,
    dialect_id: &str,
    game: Option<&str>,
) -> Option<PathBuf> {
    let variant = game.map(|g| g.to_ascii_lowercase().replace('_', "-"));
    variant
        .filter(|v| v != dialect_id && v.starts_with(dialect_id))
        .and_then(|v| data_file(profiles_dir, "builtins", &v))
        .or_else(|| data_file(profiles_dir, "builtins", dialect_id))
}

/// Edit distance between `a` and `b` (an adjacent transposition counts as one edit), or `None`
/// when it exceeds `max`.
pub fn levenshtein(a: &str, b: &str, max: usize) -> Option<usize> {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len().abs_diff(b.len()) > max {
        return None;
    }
    let mut prev2: Vec<usize> = vec![0; b.len() + 1];
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    let mut prev_min = 0;
    for i in 1..=a.len() {
        cur[0] = i;
        let mut row_min = cur[0];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(prev2[j - 2] + 1);
            }
            cur[j] = v;
            row_min = row_min.min(v);
        }
        // A transposition reaches back two rows, so stop only when both exceed `max`.
        if row_min > max && prev_min > max {
            return None;
        }
        prev_min = row_min;
        std::mem::swap(&mut prev2, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    (prev[b.len()] <= max).then_some(prev[b.len()])
}

/// The names of the data folder discovery looks for, preferred first (`gsc-data` is the old name).
pub const DATA_DIR_NAMES: &[&str] = &["gruff-data", "gsc-data"];

/// The environment variables naming the data folder, preferred first (`GSC_DATA` is the old
/// name).
pub const DATA_ENV_VARS: &[&str] = &["GRUFF_DATA", "GSC_DATA"];

/// The data folder named by `$GRUFF_DATA`, else `$GSC_DATA`.
pub fn data_dir_from_env() -> Option<PathBuf> {
    DATA_ENV_VARS
        .iter()
        .find_map(|v| std::env::var_os(v).filter(|d| !d.is_empty()))
        .map(PathBuf::from)
}

/// The `profiles` directory of gruff-data: `explicit` (a data folder or its `profiles`), then
/// `$GRUFF_DATA` or `$GSC_DATA`, then a `gruff-data` or `gsc-data` folder next to one of
/// `folders`, the working directory or the executable, or above them.
pub fn find_profiles_dir(explicit: Option<&Path>, folders: &[PathBuf]) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(d) = explicit {
        candidates.push(d.to_path_buf());
    }
    if let Some(d) = data_dir_from_env() {
        candidates.push(d);
    }
    let mut starts: Vec<PathBuf> = folders.to_vec();
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }
    if let Ok(exe) = std::env::current_exe() {
        starts.push(exe);
    }
    for s in starts {
        for a in s.ancestors() {
            candidates.extend(DATA_DIR_NAMES.iter().map(|n| a.join(n)));
        }
    }
    candidates.into_iter().find_map(|c| {
        if c.join("profiles").is_dir() {
            Some(c.join("profiles"))
        } else if c.file_name().is_some_and(|n| n == "profiles") && c.is_dir() {
            Some(c)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
schema_version = 1
game = "t7"
license = "CC0-1.0"
source = "test"
hash_family = "t7.canon"

[[builtin]]
name = "spawn"
hash = "0x0"
vm = "gsc"
kind = "function"
min_args = 1
max_args = 5
evidence = ""

[[builtin]]
name = "line"
vm = "gsc"
kind = "function"
min_args = 2
max_args = 6
type_flags = 1
evidence = ""
"#;

    #[test]
    fn looks_up_by_name_hash() {
        let spawn_hash = gsc_hash::hash_id("t7.canon", "spawn").unwrap();
        let text = SAMPLE.replace("0x0", &format!("0x{spawn_hash:08x}"));
        let c = Catalogue::from_toml_str(&text).unwrap();
        assert_eq!(c.lookup("SPAWN").len(), 1);
        assert_eq!(c.lookup("spawn")[0].arity, Some((1, 5)));
        assert_eq!(
            c.lookup("spawn")[0].describe_arity().as_deref(),
            Some("1 to 5 arguments")
        );
        assert!(c.lookup("line")[0].dev_only);
        assert!(c.lookup("nope").is_empty());
        assert!(c.contains(None, "spawn"));
        assert!(!c.contains(Some(Vm::Csc), "spawn"));
        assert_eq!(
            c.suggest("spwan", Some(Vm::Gsc), Kind::Function),
            vec!["spawn"]
        );
    }

    #[test]
    fn edit_distance() {
        assert_eq!(levenshtein("kitten", "sitting", 3), Some(3));
        assert_eq!(levenshtein("kitten", "sitting", 2), None);
        assert_eq!(levenshtein("abc", "abc", 0), Some(0));
        assert_eq!(levenshtein("helath", "health", 1), Some(1));
        assert_eq!(levenshtein("ab", "ba", 1), Some(1));
    }

    #[test]
    fn fields_by_name() {
        let c = FieldCatalogue::from_toml_str(
            "game = \"t7\"\nhash_family = \"t7.canon\"\n[[field]]\nname = \"health\"\nkind = \"entity\"\n",
        )
        .unwrap();
        assert!(c.contains("Health"));
        assert!(!c.contains("helath"));
    }

    const IW: &str = r#"
game = "iw6"
[[builtin]]
name = "getentarray"
vm = "gsc"
kind = "function"
min_args = 0
max_args = 0
arity_unknown = true
[[builtin]]
name = "playsound"
vm = "gsc"
kind = "method"
min_args = 1
max_args = 2
max_checked = false
[[builtin]]
id = 7
vm = "gsc"
kind = "function"
min_args = 1
max_args = 1
[[intrinsic]]
name = "isdefined"
min_args = 1
max_args = 1
[[intrinsic]]
name = "waittill"
vm = "csc"
"#;

    #[test]
    fn intrinsics_coverage_and_arity_flags() {
        let c = Catalogue::from_toml_str(IW).unwrap();
        let i = c.intrinsic("IsDefined").unwrap();
        assert_eq!(i.arity, Some((1, 1)));
        assert!(i.accepts(1) && !i.accepts(2));
        assert!(c.is_known("isdefined") && c.contains(Some(Vm::Gsc), "isdefined"));
        assert!(c.intrinsic("waittill").unwrap().accepts(5));
        assert!(!c.contains(Some(Vm::Gsc), "waittill"));
        assert!(c.contains(Some(Vm::Csc), "waittill"));
        // arity_unknown: no arity, any count fits
        assert_eq!(c.lookup("getentarray")[0].arity, None);
        assert!(c.lookup("getentarray")[0].accepts(2));
        // max_checked = false: only the minimum is a rule
        let ps = &c.lookup("playsound")[0];
        assert!(ps.accepts(5) && !ps.accepts(0));
        // an entry with neither a name nor a hash makes the catalogue partial
        assert!(!c.is_complete());
        let full = Catalogue::from_toml_str(&IW.replace("id = 7\n", "name = \"x\"\n")).unwrap();
        assert!(full.is_complete());
        let declared = format!("{IW}\n[coverage]\ncomplete = false\nnote = \"mp only\"\n")
            .replace("id = 7\n", "name = \"x\"\n");
        assert!(!Catalogue::from_toml_str(&declared).unwrap().is_complete());
    }

    #[test]
    fn variant_catalogue_file() {
        let dir = std::env::temp_dir().join(format!("gsc_sema_variant_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("profiles")).unwrap();
        std::fs::create_dir_all(dir.join("builtins")).unwrap();
        std::fs::write(dir.join("builtins/jup.toml"), "").unwrap();
        std::fs::write(dir.join("builtins/jup-sp.toml"), "").unwrap();
        let p = dir.join("profiles");
        let name = |g: Option<&str>| {
            catalogue_file(&p, "jup", g)
                .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
        };
        assert_eq!(name(Some("jup_sp")).as_deref(), Some("jup-sp.toml"));
        assert_eq!(name(Some("JUP-SP")).as_deref(), Some("jup-sp.toml"));
        assert_eq!(name(Some("mw23")).as_deref(), Some("jup.toml"));
        assert_eq!(name(None).as_deref(), Some("jup.toml"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_gruff_data_before_gsc_data() {
        if data_dir_from_env().is_some() {
            return; // an explicit data folder wins over discovery
        }
        let root = std::env::temp_dir().join(format!("gsc_sema_data_{}", std::process::id()));
        let ws = root.join("mod").join("scripts");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::create_dir_all(root.join("gsc-data").join("profiles")).unwrap();
        let found = find_profiles_dir(None, std::slice::from_ref(&ws)).unwrap();
        assert!(
            found.starts_with(root.join("gsc-data")),
            "{}",
            found.display()
        );
        std::fs::create_dir_all(root.join("gruff-data").join("profiles")).unwrap();
        let found = find_profiles_dir(None, std::slice::from_ref(&ws)).unwrap();
        assert!(
            found.starts_with(root.join("gruff-data")),
            "{}",
            found.display()
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
