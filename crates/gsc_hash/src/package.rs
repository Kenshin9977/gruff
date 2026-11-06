//! Hash packages: name lists that turn a game's hashed identifiers back into names.
//!
//! A `# key: value` header (`gsc-hash-package`, `game`, `family`, `source`, `licence`), then one
//! name per line (hashed here with the game's families) or `hex<TAB>name` pairs. The format is
//! specified in `docs/hash-packages.md`. The reader takes text, never paths.

use std::collections::HashMap;
use std::fmt;

use crate::family::Family;
use crate::game::Game;
use crate::lookup::mask;
use crate::scheme::{Role, Scheme};

/// The format version this crate reads and writes.
pub const FORMAT_VERSION: u32 = 1;

/// A package read error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageError {
    /// 1-based line.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for PackageError {}

/// One run of entries under one `family` (or none).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Section {
    /// The family the entries belong to; `None`: hash every name with each family of the game.
    pub family: Option<&'static Family>,
    /// Plain names, hashed on load.
    pub names: Vec<String>,
    /// `(hash, name)` pairs as written.
    pub pairs: Vec<(u64, String)>,
}

/// A parsed package.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Package {
    /// The games it applies to (profile ids as written); empty: any game.
    pub games: Vec<String>,
    pub source: Option<String>,
    pub licence: Option<String>,
    /// Every `# key: value` header line, in order (the known keys included).
    pub meta: Vec<(String, String)>,
    pub sections: Vec<Section>,
}

impl Package {
    /// Parses package text.
    pub fn parse(text: &str) -> Result<Package, PackageError> {
        let mut p = Package::default();
        let mut cur = Section::default();
        let mut header = true;
        for (i, line) in text.lines().enumerate() {
            let line = line.strip_suffix('\r').unwrap_or(line);
            let line = if i == 0 {
                line.trim_start_matches('\u{feff}')
            } else {
                line
            };
            let err = |message: String| PackageError {
                line: i + 1,
                message,
            };
            if line.trim().is_empty() {
                continue;
            }
            if let Some(rest) = line.strip_prefix('#') {
                let Some((k, v)) = rest.split_once(':') else {
                    continue; // a comment
                };
                let key = k.trim().to_ascii_lowercase();
                let value = v.trim().to_owned();
                if key.contains(' ') {
                    continue; // prose with a colon
                }
                match key.as_str() {
                    "family" => {
                        let family = match value.as_str() {
                            "" | "*" | "names" => None,
                            id => Some(
                                crate::family(id)
                                    .ok_or_else(|| err(format!("unknown hash family `{id}`")))?,
                            ),
                        };
                        if !cur.names.is_empty() || !cur.pairs.is_empty() {
                            p.sections.push(std::mem::take(&mut cur));
                        }
                        cur.family = family;
                    }
                    "gsc-hash-package" if header => {
                        let v: u32 = value
                            .parse()
                            .map_err(|_| err(format!("bad format version `{value}`")))?;
                        if v > FORMAT_VERSION {
                            return Err(err(format!(
                                "format version {v} is newer than this reader ({FORMAT_VERSION})"
                            )));
                        }
                    }
                    "game" | "games" if header => {
                        p.games = value
                            .split(',')
                            .map(|g| g.trim().to_owned())
                            .filter(|g| !g.is_empty() && g != "*")
                            .collect();
                    }
                    "source" if header => p.source = Some(value.clone()),
                    "licence" | "license" if header => p.licence = Some(value.clone()),
                    _ => {}
                }
                if header {
                    p.meta.push((key, value));
                }
                continue;
            }
            header = false;
            match line.split_once('\t') {
                Some((h, name)) => {
                    let v = parse_value(h).ok_or_else(|| err(format!("bad hash `{h}`")))?;
                    let name = name.split('\t').next().unwrap_or(name);
                    if !name.is_empty() {
                        cur.pairs.push((v, name.to_owned()));
                    }
                }
                None => cur.names.push(line.to_owned()),
            }
        }
        p.sections.push(cur);
        p.sections
            .retain(|s| !s.names.is_empty() || !s.pairs.is_empty());
        Ok(p)
    }

    /// Does the package apply to `game`?
    pub fn applies_to(&self, game: Game) -> bool {
        self.games.is_empty()
            || self
                .games
                .iter()
                .any(|g| Game::from_profile_id(g) == Some(game))
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.sections
            .iter()
            .map(|s| s.names.len() + s.pairs.len())
            .sum()
    }

    /// True if no entry.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A hash as a package writes it: hex, with or without `0x` or a decompiler prefix
/// (`function_`, `hash_`...).
fn parse_value(s: &str) -> Option<u64> {
    let s = s.trim();
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    let hex = match s.rfind('_') {
        Some(i) if s[..i].bytes().all(|b| b.is_ascii_alphabetic()) => &s[i + 1..],
        _ => s,
    };
    if hex.is_empty() || hex.len() > 16 {
        return None;
    }
    u64::from_str_radix(hex, 16).ok()
}

/// The header of a package, as [`write_header`] prints it.
#[derive(Clone, Debug, Default)]
pub struct Header<'a> {
    pub games: &'a [&'a str],
    pub family: Option<&'a str>,
    pub source: &'a str,
    pub licence: &'a str,
    /// More `key: value` lines.
    pub extra: &'a [(&'a str, &'a str)],
}

/// The header lines of a package, each ending in `\n`.
pub fn write_header(h: &Header) -> String {
    let mut s = format!("# gsc-hash-package: {FORMAT_VERSION}\n");
    let games = if h.games.is_empty() {
        "*".to_owned()
    } else {
        h.games.join(", ")
    };
    s.push_str(&format!("# game: {games}\n"));
    if let Some(f) = h.family {
        s.push_str(&format!("# family: {f}\n"));
    }
    s.push_str(&format!("# source: {}\n", h.source.replace('\n', " ")));
    s.push_str(&format!("# licence: {}\n", h.licence.replace('\n', " ")));
    for (k, v) in h.extra {
        s.push_str(&format!("# {k}: {}\n", v.replace('\n', " ")));
    }
    s
}

/// What loading packages into a [`Dictionary`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadStats {
    /// Plain names read.
    pub names: usize,
    /// Pairs whose name re-hashes to the value with the section's family (or, without one, with
    /// a family of the game).
    pub verified: usize,
    /// Pairs kept as given although the name does not re-hash (a section with a family only).
    pub unverified: usize,
    /// Pairs dropped: no family given and none of the game's re-hashes the name.
    pub dropped: usize,
    /// Entries of sections whose family the game does not use, or of packages for other games.
    pub other_game: usize,
}

impl std::ops::AddAssign for LoadStats {
    fn add_assign(&mut self, o: LoadStats) {
        self.names += o.names;
        self.verified += o.verified;
        self.unverified += o.unverified;
        self.dropped += o.dropped;
        self.other_game += o.other_game;
    }
}

/// The width a dictionary keys a family on: its own, at most 60 bits.
fn key_bits(f: &Family) -> u32 {
    f.bits.min(60)
}

/// Does `name` hash to `value` with `f`, at the family's width or, for a value written narrower,
/// at 60 bits?
fn verify(f: &Family, name: &str, value: u64) -> bool {
    f.verify(name, value, f.bits) || (f.bits > 60 && f.verify(name, value, 60))
}

/// The names of one game, keyed per family: what the packages give, plus names added one by one.
#[derive(Clone, Debug)]
pub struct Dictionary {
    scheme: Scheme,
    names: Vec<Box<str>>,
    ids: HashMap<Box<str>, u32>,
    /// Family id to (value cut to [`key_bits`], name index).
    by_family: HashMap<&'static str, HashMap<u64, u32>>,
}

impl Dictionary {
    /// An empty dictionary for a game's scheme.
    pub fn new(scheme: Scheme) -> Dictionary {
        Dictionary {
            scheme,
            names: Vec::new(),
            ids: HashMap::new(),
            by_family: HashMap::new(),
        }
    }

    pub fn scheme(&self) -> &Scheme {
        &self.scheme
    }

    fn intern(&mut self, name: &str) -> u32 {
        if let Some(&i) = self.ids.get(name) {
            return i;
        }
        let i = self.names.len() as u32;
        self.names.push(name.into());
        self.ids.insert(name.into(), i);
        i
    }

    fn put(&mut self, f: &'static Family, value: u64, name: u32) {
        self.by_family
            .entry(f.id)
            .or_default()
            .entry(value & mask(key_bits(f)))
            .or_insert(name);
    }

    /// Hashes `name` with every family of the scheme and stores it (the first name of a value is
    /// kept).
    pub fn add_name(&mut self, name: &str) {
        let id = self.intern(name);
        for f in self.scheme.families() {
            self.put(f, f.hash(name), id);
        }
    }

    /// Hashes `name` with one family and stores it.
    pub fn add_name_with(&mut self, f: &'static Family, name: &str) {
        let id = self.intern(name);
        self.put(f, f.hash(name), id);
    }

    /// Stores a pair as given.
    pub fn add_pair(&mut self, f: &'static Family, value: u64, name: &str) {
        let id = self.intern(name);
        self.put(f, value, id);
    }

    /// Adds a package's entries that apply to the scheme's game.
    pub fn add_package(&mut self, p: &Package) -> LoadStats {
        let mut st = LoadStats::default();
        if !p.applies_to(self.scheme.game) {
            st.other_game = p.len();
            return st;
        }
        let families = self.scheme.families();
        for s in &p.sections {
            match s.family {
                Some(f) if !f.applies_to(self.scheme.game) && !families.contains(&f) => {
                    st.other_game += s.names.len() + s.pairs.len();
                }
                Some(f) => {
                    for n in &s.names {
                        self.add_name_with(f, n);
                    }
                    st.names += s.names.len();
                    for (v, n) in &s.pairs {
                        if verify(f, n, *v) {
                            st.verified += 1;
                        } else {
                            st.unverified += 1;
                        }
                        self.add_pair(f, *v, n);
                    }
                }
                None => {
                    for n in &s.names {
                        self.add_name(n);
                    }
                    st.names += s.names.len();
                    for (v, n) in &s.pairs {
                        let hit: Vec<_> = families
                            .iter()
                            .copied()
                            .filter(|f| verify(f, n, *v))
                            .collect();
                        if hit.is_empty() {
                            st.dropped += 1;
                        } else {
                            st.verified += 1;
                        }
                        for f in hit {
                            self.add_pair(f, *v, n);
                        }
                    }
                }
            }
        }
        st
    }

    /// The name stored for `value` under `family`. Values of the 63- and 64-bit families are
    /// compared on their low 60 bits, the width some tools print them at.
    pub fn lookup(&self, f: &Family, value: u64) -> Option<&str> {
        let t = self.by_family.get(f.id)?;
        t.get(&(value & mask(key_bits(f))))
            .map(|&i| &*self.names[i as usize])
    }

    /// The name of a hash of `role`.
    pub fn name(&self, role: Role, value: u64) -> Option<&str> {
        self.lookup(self.scheme.family(role)?, value)
    }

    /// The name of a decompiler spelling (`function_1a2b`, `hash_...`), if known.
    pub fn name_of_spelling(&self, role: Role, written: &str) -> Option<&str> {
        self.name(role, self.scheme.spelled(role, written)?)
    }

    /// Every family of the scheme that names `value`, with the name.
    pub fn reverse(&self, value: u64) -> Vec<(&'static Family, &str)> {
        self.scheme
            .families()
            .into_iter()
            .filter_map(|f| self.lookup(f, value).map(|n| (f, n)))
            .collect()
    }

    /// Distinct names.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// True if no name.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Stored values per family id.
    pub fn counts(&self) -> Vec<(&'static str, usize)> {
        let mut v: Vec<_> = self.by_family.iter().map(|(k, m)| (*k, m.len())).collect();
        v.sort();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PKG: &str = "\u{feff}# gsc-hash-package: 1\r\n# game: t8, t9\n# source: a test\n# licence: CC0-1.0\n# note: plain\nmy_mod_init\n\n# family: t89.scr\n0x1234\twrong_name\nmy_mod_main\n";

    #[test]
    fn reads_header_and_sections() {
        let p = Package::parse(PKG).unwrap();
        assert_eq!(p.games, vec!["t8", "t9"]);
        assert_eq!(p.source.as_deref(), Some("a test"));
        assert_eq!(p.licence.as_deref(), Some("CC0-1.0"));
        assert_eq!(p.sections.len(), 2);
        assert_eq!(p.sections[0].family, None);
        assert_eq!(p.sections[0].names, vec!["my_mod_init"]);
        assert_eq!(p.sections[1].family.unwrap().id, "t89.scr");
        assert_eq!(p.sections[1].pairs, vec![(0x1234, "wrong_name".to_owned())]);
        assert!(p.applies_to(Game::T9) && !p.applies_to(Game::Bo7));
        assert!(Package::parse("# family: nope\nx\n").is_err());
        assert!(Package::parse("zz\tname\n").is_err());
        assert!(Package::parse("# gsc-hash-package: 9\n").is_err());
    }

    #[test]
    fn dictionary_hashes_names_and_keeps_given_pairs() {
        let p = Package::parse(PKG).unwrap();
        let s = Scheme::for_game(Game::T8).unwrap();
        let mut d = Dictionary::new(s);
        let st = d.add_package(&p);
        assert_eq!((st.names, st.verified, st.unverified), (2, 0, 1));
        let h = s.key(Role::Function, "my_mod_init").unwrap();
        assert_eq!(d.name(Role::Function, h), Some("my_mod_init"));
        let spelled = format!("function_{h:x}");
        assert_eq!(
            d.name_of_spelling(Role::Function, &spelled),
            Some("my_mod_init")
        );
        assert_eq!(d.name(Role::Function, 0x1234), Some("wrong_name"));
        // Without a family, names are hashed with the string family too.
        let hs = s.key(Role::String, "my_mod_init").unwrap();
        assert_eq!(d.name(Role::String, hs), Some("my_mod_init"));
        assert!(!d.reverse(h).is_empty());
        // Other games skip it.
        let mut d7 = Dictionary::new(Scheme::for_game(Game::Bo7).unwrap());
        assert_eq!(d7.add_package(&p).other_game, 3);
        assert!(d7.is_empty());
    }

    #[test]
    fn pairs_without_family_are_verified() {
        let s = Scheme::for_game(Game::Bo7).unwrap();
        let v = s.key(Role::Var, "health").unwrap();
        let text = format!("{v:x}\thealth\n123\tnope\nvar_{v:x}\thealth\n");
        let mut d = Dictionary::new(s);
        let st = d.add_package(&Package::parse(&text).unwrap());
        assert_eq!((st.verified, st.dropped), (2, 1));
        assert_eq!(d.name(Role::Var, v), Some("health"));
    }

    #[test]
    fn header_round_trip() {
        let h = write_header(&Header {
            games: &["t11"],
            family: Some("t10.function"),
            source: "x",
            licence: "local only",
            extra: &[("count", "2")],
        });
        let p = Package::parse(&format!("{h}a\nb\n")).unwrap();
        assert_eq!(p.games, vec!["t11"]);
        assert_eq!(p.sections[0].family.unwrap().id, "t10.function");
        assert_eq!(p.len(), 2);
        assert!(p.meta.iter().any(|(k, v)| k == "count" && v == "2"));
    }
}
