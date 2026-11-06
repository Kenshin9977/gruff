//! Masks, and name lookup with the 63-then-60-bit fallback.

use std::collections::{BTreeMap, HashMap};

use crate::family::Family;

/// 32-bit mask.
pub const MASK32: u64 = 0xFFFF_FFFF;
/// 60-bit mask (the width GrayWolf's name indexes key on, and the BO7 script families).
pub const MASK60: u64 = 0x0FFF_FFFF_FFFF_FFFF;
/// 63-bit mask (bit 63 of an IW/Treyarch 64-bit id is an engine flag).
pub const MASK63: u64 = 0x7FFF_FFFF_FFFF_FFFF;

/// The mask of a width in bits (1 to 64).
pub const fn mask(bits: u32) -> u64 {
    if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    }
}

/// The widths a value of `bits` is looked up at, widest first: 64 -> 64, 63, 60; 63 -> 63, 60;
/// 32 or 60 -> itself.
pub fn fallback_widths(bits: u32) -> &'static [u32] {
    match bits {
        64.. => &[64, 63, 60],
        63 => &[63, 60],
        60 => &[60],
        32 => &[32],
        _ => &[],
    }
}

/// Looks `value` up at each fallback width of `bits` (63, then 60, for a 63-bit value) and returns
/// the first hit with the width it matched at.
pub fn lookup_with_fallback<T>(
    value: u64,
    bits: u32,
    mut get: impl FnMut(u64, u32) -> Option<T>,
) -> Option<(T, u32)> {
    fallback_widths(bits)
        .iter()
        .find_map(|&w| get(value & mask(w), w).map(|t| (t, w)))
}

/// A name store keyed by hash, holding sources of several widths (a 63-bit id table beside a
/// GrayWolf index keyed on 60 bits). A lookup tries the widest table first.
#[derive(Debug, Default, Clone)]
pub struct NameTable {
    by_width: BTreeMap<u32, HashMap<u64, String>>,
}

impl NameTable {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a pair stored at `bits`. The key is masked to that width. Returns false if a
    /// different name already held the key (the first one is kept).
    pub fn insert(&mut self, value: u64, bits: u32, name: impl Into<String>) -> bool {
        let name = name.into();
        let map = self.by_width.entry(bits).or_default();
        match map.entry(value & mask(bits)) {
            std::collections::hash_map::Entry::Occupied(e) => e.get() == &name,
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(name);
                true
            }
        }
    }

    /// Hashes `name` with `family` and stores it at the family's width.
    pub fn insert_name(&mut self, family: &Family, name: &str) -> u64 {
        let value = family.hash(name);
        self.insert(value, family.bits, name);
        value
    }

    /// Inserts a stored pair only if `name` re-hashes to `value` with `family` (verify on load).
    pub fn insert_verified(&mut self, family: &Family, value: u64, bits: u32, name: &str) -> bool {
        family.verify(name, value, bits) && self.insert(value, bits.min(family.bits), name)
    }

    /// Looks a value of `bits` up: at each width the table holds that the value can be cut to,
    /// widest first (64, 63, then 60).
    pub fn lookup(&self, value: u64, bits: u32) -> Option<(&str, u32)> {
        self.by_width
            .iter()
            .rev()
            .filter(|(&w, _)| w <= bits)
            .find_map(|(&w, map)| map.get(&(value & mask(w))).map(|n| (n.as_str(), w)))
    }

    /// Number of pairs.
    pub fn len(&self) -> usize {
        self.by_width.values().map(HashMap::len).sum()
    }

    /// True if empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
