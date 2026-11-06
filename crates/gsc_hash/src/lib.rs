//! Hash families of the GSC scripting language, CoD1 (2003) to Black Ops 7.
//!
//! Each [`Family`] has a stable id (`t7.canon`, `t8.fnv64`, `t10.function`), its games, its input
//! canonicalisation and its width. 64-bit lookups fall back from 63 to 60 bits
//! ([`lookup_with_fallback`], [`NameTable`]). See `docs/hash-families.md` for sources.
//!
//! ```
//! let f = gsc_hash::family("t10.function").unwrap();
//! assert_eq!(gsc_hash::hash(f, "0exp1j983"), 0x0ec2_8edb_5ed4_7500);
//! ```

pub mod algo;
pub mod db;
mod family;
mod game;
mod lookup;
pub mod package;
pub mod scheme;

pub use family::{Algo, Canon, Family, Status, FAMILIES};
pub use game::Game;
pub use lookup::{fallback_widths, lookup_with_fallback, mask, NameTable, MASK32, MASK60, MASK63};
pub use package::{Dictionary, LoadStats, Package, PackageError};
pub use scheme::{parse_any_spelling, parse_spelling, Role, Scheme};

/// Every registered family.
pub fn families() -> &'static [Family] {
    FAMILIES
}

/// The family with this id (case-insensitive).
pub fn family(id: &str) -> Option<&'static Family> {
    FAMILIES.iter().find(|f| f.id.eq_ignore_ascii_case(id))
}

/// The families a game uses.
pub fn families_for(game: Game) -> impl Iterator<Item = &'static Family> {
    FAMILIES.iter().filter(move |f| f.applies_to(game))
}

/// Hashes `s` with `family`, masked to the family's width.
pub fn hash(family: &Family, s: &str) -> u64 {
    family.hash(s)
}

/// Hashes `s` with the family of id `id`, or `None` for an unknown id.
pub fn hash_id(id: &str, s: &str) -> Option<u64> {
    family(id).map(|f| f.hash(s))
}

#[cfg(test)]
mod tests;
