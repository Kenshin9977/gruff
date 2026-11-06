//! The registry of hash families: one entry per (algorithm, canonicalisation, width) a game uses.

use std::borrow::Cow;
use std::fmt;

use crate::algo::{self, *};
use crate::game::Game;
use crate::lookup::mask;

/// How a string is canonicalised before it is hashed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Canon {
    /// The bytes as written.
    Raw,
    /// ASCII lower case (`A`-`Z` only, as C `tolower` in the "C" locale).
    Lower,
    /// ASCII lower case, and `\` folded to `/` (asset paths).
    LowerSlash,
    /// ASCII lower case, and `/` folded to `\` (BO4 SAB `sound_asset` names, CoD Online paths).
    LowerBackslash,
}

impl Canon {
    /// Applies the canonicalisation.
    pub fn apply(self, bytes: &[u8]) -> Cow<'_, [u8]> {
        let needs = |b: &u8| match self {
            Canon::Raw => false,
            Canon::Lower => b.is_ascii_uppercase(),
            Canon::LowerSlash => b.is_ascii_uppercase() || *b == b'\\',
            Canon::LowerBackslash => b.is_ascii_uppercase() || *b == b'/',
        };
        if !bytes.iter().any(needs) {
            return Cow::Borrowed(bytes);
        }
        Cow::Owned(
            bytes
                .iter()
                .map(|&b| match (self, b) {
                    (Canon::LowerSlash, b'\\') => b'/',
                    (Canon::LowerBackslash, b'/') => b'\\',
                    _ => b.to_ascii_lowercase(),
                })
                .collect(),
        )
    }

    /// A short description.
    pub fn describe(self) -> &'static str {
        match self {
            Canon::Raw => "as written",
            Canon::Lower => "lower case",
            Canon::LowerSlash => "lower case, '\\' -> '/'",
            Canon::LowerBackslash => "lower case, '/' -> '\\'",
        }
    }
}

/// A hash algorithm with its parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algo {
    /// FNV-1a 64.
    Fnv64 { basis: u64, prime: u64 },
    /// Salted FNV-1a 64, the salt inserted after the first byte.
    Secure {
        salt: &'static [u8],
        basis: u64,
        prime: u64,
    },
    /// FNV-1a 64 of the name followed by the salt.
    SaltAfter {
        salt: &'static [u8],
        basis: u64,
        prime: u64,
    },
    /// FNV-1a 32.
    Fnv32 { basis: u32 },
    /// The BO3 canonical hash (FNV-1a 32, seed 0x4B9ACE2F, one extra multiply).
    T7Canon,
    /// The BO4/CW script hash (acts' `HashT89Scr`).
    T89Scr,
    /// djb2, xor, every byte or-ed with 0x20, from a seed.
    Djb2XorNocase { seed: u32 },
    /// djb2, addition, from 5381.
    Djb2Add,
    /// `h * 31 + c` from 0.
    StringTable31,
    /// `Com_HashKey`.
    ComHashKey,
    /// SDBM from 5381.
    Sdbm,
    /// SDBM from 5381, a result of 0 becoming 1 (`SND_HashName`).
    SdbmNonZero,
    /// XXH3-64 with a seed.
    Xxh3 { seed: u64 },
    /// MPQ `HashString` with a crypt-table offset.
    MpqHashString { offset: usize },
    /// lookup3 `hashlittle2` with its two seeds, as `pc | pb << 32`.
    HashLittle2 { pc: u32, pb: u32 },
}

impl Algo {
    /// Runs the algorithm over canonical bytes. The result is not masked.
    pub fn run(self, bytes: &[u8]) -> u64 {
        match self {
            Algo::Fnv64 { basis, prime } => fnv1a64(bytes, basis, prime),
            Algo::Secure { salt, basis, prime } => secure_fnv64(bytes, salt, basis, prime),
            Algo::SaltAfter { salt, basis, prime } => salt_after_fnv64(bytes, salt, basis, prime),
            Algo::Fnv32 { basis } => u64::from(fnv1a32(bytes, basis)),
            Algo::T7Canon => u64::from(t7_canon(bytes)),
            Algo::T89Scr => u64::from(t89_scr(bytes)),
            Algo::Djb2XorNocase { seed } => u64::from(djb2_xor_nocase(bytes, seed)),
            Algo::Djb2Add => u64::from(djb2_add(bytes)),
            Algo::StringTable31 => u64::from(string_table31(bytes)),
            Algo::ComHashKey => u64::from(com_hash_key(bytes)),
            Algo::Sdbm => u64::from(sdbm(bytes)),
            Algo::SdbmNonZero => match sdbm(bytes) {
                0 if !bytes.is_empty() => 1,
                h => u64::from(h),
            },
            Algo::Xxh3 { seed } => xxh3_64(bytes, seed),
            Algo::MpqHashString { offset } => u64::from(mpq_hash_string(bytes, offset)),
            Algo::HashLittle2 { pc, pb } => {
                let (c, b) = algo::hashlittle2(bytes, pc, pb);
                u64::from(c) | (u64::from(b) << 32)
            }
        }
    }

    /// A one-line description with the parameters.
    pub fn describe(self) -> String {
        match self {
            Algo::Fnv64 { basis, prime } => {
                format!("FNV-1a 64, basis {basis:#018x}, prime {prime:#x}")
            }
            Algo::Secure { salt, basis, prime } => format!(
                "salted FNV-1a 64: name[0] + {:?} + name[1..], basis {basis:#018x}, prime {prime:#x}",
                String::from_utf8_lossy(salt)
            ),
            Algo::SaltAfter { salt, basis, prime } => format!(
                "FNV-1a 64 of name + {:?}, basis {basis:#018x}, prime {prime:#x}",
                String::from_utf8_lossy(salt)
            ),
            Algo::Fnv32 { basis } => format!("FNV-1a 32, basis {basis:#010x}, prime 0x1000193"),
            Algo::T7Canon => {
                "FNV-1a 32, seed 0x4b9ace2f, prime 0x1000193, then one extra multiply".into()
            }
            Algo::T89Scr => "shift/xor/add loop from seed 0x4b9ace2f, then \
                             0x8001 * ((9h) ^ ((9h) >> 11)) (acts HashT89Scr)"
                .into(),
            Algo::Djb2XorNocase { seed } => format!("djb2 xor, c | 0x20, seed {seed}"),
            Algo::Djb2Add => "djb2, h * 33 + c, seed 5381".into(),
            Algo::StringTable31 => "h * 31 + c, seed 0".into(),
            Algo::ComHashKey => "h += c * (0x77 + i); h ^ ((h ^ (h >> 10)) >> 10)".into(),
            Algo::Sdbm => "SDBM, h = c + h * 0x1003f, seed 5381".into(),
            Algo::SdbmNonZero => "SDBM, h = c + h * 0x1003f, seed 5381, 0 -> 1".into(),
            Algo::Xxh3 { seed } => format!("XXH3-64, seed {seed:#x}"),
            Algo::MpqHashString { offset } => format!("MPQ HashString, table offset {offset:#x}"),
            Algo::HashLittle2 { pc, pb } => {
                format!("lookup3 hashlittle2, seeds pc={pc}, pb={pb}, pc | pb << 32")
            }
        }
    }
}

/// How well a family is established.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Re-hashes 100 % of a local verified name database (see the integration tests).
    Verified,
    /// Checked against a handful of known pairs.
    Spot,
    /// Taken from working code (acts, GrayWolf, OpenAssetTools), not checked against names here.
    Documented,
}

impl Status {
    /// A short label.
    pub fn label(self) -> &'static str {
        match self {
            Status::Verified => "verified",
            Status::Spot => "spot-checked",
            Status::Documented => "documented",
        }
    }
}

/// A hash family: what a game hashes, how, and at which width.
#[derive(Clone, Copy, Debug)]
pub struct Family {
    /// Stable id, `<engine>.<name>` (`t7.canon`, `t8.fnv64`, `t10.function`).
    pub id: &'static str,
    /// The games that use it.
    pub games: &'static [Game],
    /// What is hashed with it.
    pub kinds: &'static str,
    /// The algorithm.
    pub algo: Algo,
    /// The input canonicalisation.
    pub canon: Canon,
    /// The width the game keeps (32, 60, 63 or 64).
    pub bits: u32,
    /// How decompilers spell an unknown hash of this family (`function_`, `var_`...).
    pub spellings: &'static [&'static str],
    /// How well it is established.
    pub status: Status,
}

impl Family {
    /// Hashes a string: canonicalise, run, mask to [`Family::bits`].
    pub fn hash(&self, s: &str) -> u64 {
        self.hash_bytes(s.as_bytes())
    }

    /// Hashes bytes: canonicalise, run, mask to [`Family::bits`].
    pub fn hash_bytes(&self, bytes: &[u8]) -> u64 {
        self.hash_unmasked(bytes) & mask(self.bits)
    }

    /// Canonicalises and runs the algorithm without masking (bit 63 included).
    pub fn hash_unmasked(&self, bytes: &[u8]) -> u64 {
        self.algo.run(&self.canon.apply(bytes))
    }

    /// True if `name` hashes to `value` at `bits` (pass the width the value was stored with; it is
    /// capped at the family's own width).
    pub fn verify(&self, name: &str, value: u64, bits: u32) -> bool {
        let m = mask(bits.min(self.bits));
        (self.hash_unmasked(name.as_bytes()) & m) == (value & m)
    }

    /// True if the family is used by `game`.
    pub fn applies_to(&self, game: Game) -> bool {
        self.games.contains(&game)
    }

    /// Formats a value at the family's width, as hex without a prefix.
    pub fn format(&self, value: u64) -> String {
        if self.bits <= 32 {
            format!("{:08x}", value & mask(self.bits))
        } else {
            format!("{:x}", value & mask(self.bits))
        }
    }
}

/// Families are equal when their ids are.
impl PartialEq for Family {
    fn eq(&self, other: &Family) -> bool {
        self.id == other.id
    }
}

impl Eq for Family {}

impl fmt::Display for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id)
    }
}

const TA: u64 = FNV64_BASIS_TA;
const IW: u64 = FNV64_BASIS_IW;
const P: u64 = FNV64_PRIME;

const FNV64_TA: Algo = Algo::Fnv64 {
    basis: TA,
    prime: P,
};
const FNV64_IW: Algo = Algo::Fnv64 {
    basis: IW,
    prime: P,
};
const T10_SCR: Algo = Algo::Secure {
    salt: SALT_T10_SCR,
    basis: FNV64_BASIS_T10_SCR,
    prime: FNV64_PRIME_SECURE,
};

use Game::*;

const MODERN_IW: &[Game] = &[Iw9, S4, H2, Jup, T10, Bo7];
const BO6_BO7: &[Game] = &[T10, Bo7];
const BO4_CW: &[Game] = &[T8, T9];

/// Every family, grouped by era.
pub static FAMILIES: &[Family] = &[
    // --- CoD4 to Black Ops II: scripts in clear; renderer, table and sound hashes ---
    Family {
        id: "iw3.r_hash",
        games: &[Iw3, T4, Iw4, Iw5, T5],
        kinds: "material / techset names (R_HashString)",
        algo: Algo::Djb2XorNocase { seed: 0 },
        canon: Canon::Raw,
        bits: 32,
        spellings: &[],
        status: Status::Documented,
    },
    Family {
        id: "iw4.stringtable",
        games: &[Iw4, Iw5],
        kinds: "string table cells (StringTable_HashString)",
        algo: Algo::StringTable31,
        canon: Canon::Lower,
        bits: 32,
        spellings: &[],
        status: Status::Documented,
    },
    Family {
        id: "t5.com_hashstring",
        games: &[T5, T6],
        kinds: "string tables (Com_HashString)",
        algo: Algo::Djb2Add,
        canon: Canon::Lower,
        bits: 32,
        spellings: &[],
        status: Status::Documented,
    },
    Family {
        id: "t5.com_hashkey",
        games: &[T5, T6],
        kinds: "keys (Com_HashKey)",
        algo: Algo::ComHashKey,
        canon: Canon::Raw,
        bits: 32,
        spellings: &[],
        status: Status::Documented,
    },
    Family {
        id: "t5.snd",
        games: &[T5, T6, T7],
        kinds: "sound alias, duck and context names (SND_HashName)",
        algo: Algo::SdbmNonZero,
        canon: Canon::Lower,
        bits: 32,
        spellings: &[],
        status: Status::Documented,
    },
    Family {
        id: "t6.sab",
        games: &[T6, T7],
        kinds: ".sab sound file names",
        algo: Algo::Sdbm,
        canon: Canon::Lower,
        bits: 32,
        spellings: &[],
        status: Status::Verified,
    },
    // --- Black Ops III ---
    Family {
        id: "t7.canon",
        games: &[T7],
        kinds: "function, field/var, event and key identifiers in compiled GSC",
        algo: Algo::T7Canon,
        canon: Canon::Lower,
        bits: 32,
        spellings: &["function_", "var_", "namespace_", "hash_"],
        status: Status::Verified,
    },
    // --- Black Ops 4, Cold War ---
    Family {
        id: "t89.scr",
        games: BO4_CW,
        kinds: "function, namespace and var identifiers (32-bit script hash)",
        algo: Algo::T89Scr,
        canon: Canon::LowerSlash,
        bits: 32,
        spellings: &["function_", "var_", "namespace_", "hash_"],
        status: Status::Spot,
    },
    Family {
        id: "t8.fnv64",
        games: BO4_CW,
        kinds: "asset names, #\"...\" script strings, script_<hash> file names",
        algo: FNV64_TA,
        canon: Canon::LowerSlash,
        bits: 63,
        spellings: &["hash_", "script_"],
        status: Status::Verified,
    },
    Family {
        id: "t8.localize",
        games: BO4_CW,
        kinds: "localize keys",
        algo: FNV64_TA,
        canon: Canon::LowerSlash,
        bits: 60,
        spellings: &["localize_"],
        status: Status::Documented,
    },
    Family {
        id: "t9.strings",
        games: &[T9],
        kinds: "bones, notify and script strings (fnv1a_strings)",
        algo: FNV64_TA,
        canon: Canon::LowerSlash,
        bits: 60,
        spellings: &["hash_"],
        status: Status::Verified,
    },
    Family {
        id: "t8.sound_asset",
        games: &[T8],
        kinds: "SAB sound_asset names (backslashes kept)",
        algo: FNV64_TA,
        canon: Canon::LowerBackslash,
        bits: 63,
        spellings: &[],
        status: Status::Verified,
    },
    // --- MW2019 ---
    Family {
        id: "iw8.localize_ta",
        games: &[Iw8, Jup],
        kinds: "weapon display-name keys as GrayWolf's weapon stats hash them (TA basis)",
        algo: FNV64_TA,
        canon: Canon::Lower,
        bits: 63,
        spellings: &[],
        status: Status::Documented,
    },
    // --- MWII to Black Ops 7 (IW asset era) ---
    Family {
        id: "iw.asset",
        games: MODERN_IW,
        kinds: "asset names (HashIWAsset)",
        algo: FNV64_IW,
        canon: Canon::LowerSlash,
        bits: 63,
        spellings: &["hash_", "ximage_", "xmodel_"],
        status: Status::Verified,
    },
    Family {
        id: "iw.localize",
        games: &[Jup, T10, Bo7],
        kinds: "localize keys (the asset-name hash, IW basis)",
        algo: FNV64_IW,
        canon: Canon::LowerSlash,
        bits: 63,
        spellings: &["localize_"],
        status: Status::Verified,
    },
    Family {
        id: "iw.sound_alias",
        games: &[S4, Iw9, Jup, T10, Bo7],
        kinds: "sound alias names (TA basis, full 64 bits)",
        algo: FNV64_TA,
        canon: Canon::Lower,
        bits: 64,
        spellings: &[],
        status: Status::Documented,
    },
    Family {
        id: "iw9.bone",
        games: &[Iw9, Jup],
        kinds: "bone names (FNV-1a 32)",
        algo: Algo::Fnv32 { basis: FNV32_BASIS },
        canon: Canon::Lower,
        bits: 32,
        spellings: &["bone_"],
        status: Status::Verified,
    },
    Family {
        id: "iw9.scr",
        games: &[Iw9, Jup],
        kinds: "script identifiers (acts HashJupScr): functions, fields, locals, namespaces; MWIII entity field keys",
        algo: Algo::Fnv64 {
            basis: FNV64_BASIS_IW_SCR,
            prime: FNV64_PRIME_SECURE,
        },
        canon: Canon::LowerSlash,
        bits: 64,
        spellings: &["function_", "var_", "namespace_"],
        status: Status::Spot,
    },
    Family {
        id: "iw9.xhash",
        games: &[Iw9, Jup],
        kinds: "#\"hash_<hex>\" script hashes (full 64 bits)",
        algo: FNV64_TA,
        canon: Canon::LowerSlash,
        bits: 64,
        spellings: &["hash_"],
        status: Status::Spot,
    },
    Family {
        id: "iw.rawfile_ext",
        games: &[Jup, T10, Bo7],
        kinds: "rawfile extensions (.gsc, .csc, .json...)",
        algo: Algo::Fnv32 { basis: FNV32_BASIS },
        canon: Canon::Lower,
        bits: 32,
        spellings: &[],
        status: Status::Documented,
    },
    // --- Black Ops 6, Black Ops 7: the eleven GSC families ---
    Family {
        id: "t10.function",
        games: BO6_BO7,
        kinds: "function names (acts HashT10Scr)",
        algo: T10_SCR,
        canon: Canon::LowerSlash,
        bits: 60,
        spellings: &["function_"],
        status: Status::Verified,
    },
    Family {
        id: "t10.var",
        games: BO6_BO7,
        kinds: "fields and locals",
        algo: T10_SCR,
        canon: Canon::LowerSlash,
        bits: 60,
        spellings: &["var_"],
        status: Status::Verified,
    },
    Family {
        id: "t10.namespace",
        games: BO6_BO7,
        kinds: "namespaces",
        algo: T10_SCR,
        canon: Canon::LowerSlash,
        bits: 60,
        spellings: &["namespace_"],
        status: Status::Verified,
    },
    Family {
        id: "t10.event",
        games: BO6_BO7,
        kinds: "notify / endon / waittill events",
        algo: T10_SCR,
        canon: Canon::LowerSlash,
        bits: 60,
        spellings: &["event_"],
        status: Status::Verified,
    },
    Family {
        id: "t10.bundlevar",
        games: BO6_BO7,
        kinds: "bundle field names in script",
        algo: T10_SCR,
        canon: Canon::LowerSlash,
        bits: 60,
        spellings: &["var_"],
        status: Status::Verified,
    },
    Family {
        id: "t10.dvar",
        games: BO6_BO7,
        kinds: "dvar names, @\"hash_<hex>\" (acts HashIWDVar)",
        algo: Algo::Secure {
            salt: SALT_DVAR,
            basis: FNV64_BASIS_DVAR,
            prime: FNV64_PRIME_SECURE,
        },
        canon: Canon::LowerSlash,
        bits: 60,
        spellings: &["hash_"],
        status: Status::Verified,
    },
    Family {
        id: "t10.xhash",
        games: BO6_BO7,
        kinds: "#\"hash_<hex>\" script hashes",
        algo: FNV64_TA,
        canon: Canon::LowerSlash,
        bits: 60,
        spellings: &["hash_"],
        status: Status::Verified,
    },
    Family {
        id: "t10.case",
        games: BO6_BO7,
        kinds: "switch case constants (raw bytes, prime 0x10000000193)",
        algo: Algo::Fnv64 {
            basis: TA,
            prime: FNV64_PRIME_CASE,
        },
        canon: Canon::Raw,
        bits: 60,
        spellings: &["hash_"],
        status: Status::Verified,
    },
    Family {
        id: "t10.assetlit",
        games: BO6_BO7,
        kinds: "%\"hash_<hex>\" asset ids, over \"<type>:<name>\" (bare name for some)",
        algo: FNV64_IW,
        canon: Canon::LowerSlash,
        bits: 63,
        spellings: &["hash_"],
        status: Status::Verified,
    },
    Family {
        id: "t10.script",
        games: BO6_BO7,
        kinds: "script_<hex>.gsc file names, the path with .gsc and forward slashes",
        algo: FNV64_IW,
        canon: Canon::LowerSlash,
        bits: 63,
        spellings: &["script_"],
        status: Status::Verified,
    },
    Family {
        id: "t10.bundlekey",
        games: BO6_BO7,
        kinds: "scriptbundle field keys and hash values (as written, unmasked)",
        algo: FNV64_TA,
        canon: Canon::Raw,
        bits: 64,
        spellings: &["hash_"],
        status: Status::Verified,
    },
    // --- Black Ops 6, Black Ops 7: zone-side families ---
    Family {
        id: "t10.entity_key",
        games: BO6_BO7,
        kinds: "map entity keys (the script hash, 64 bits)",
        algo: T10_SCR,
        canon: Canon::Lower,
        bits: 64,
        spellings: &[],
        status: Status::Documented,
    },
    Family {
        id: "t10.scr_sp",
        games: &[T10],
        kinds: "BO6 campaign script hash and entity keys: FNV of name + salt (acts HashT10ScrSP)",
        algo: Algo::SaltAfter {
            salt: SALT_T10_SCR,
            basis: FNV64_BASIS_T10_SCR,
            prime: FNV64_PRIME_SECURE,
        },
        canon: Canon::LowerSlash,
        bits: 64,
        spellings: &["function_", "var_"],
        status: Status::Documented,
    },
    Family {
        id: "t10.omnvar",
        games: BO6_BO7,
        kinds: "omnvar names (acts HashT10OmnVar)",
        algo: Algo::Secure {
            salt: SALT_OMNVAR,
            basis: FNV64_BASIS_OMNVAR,
            prime: FNV64_PRIME_OMNVAR,
        },
        canon: Canon::LowerSlash,
        bits: 64,
        spellings: &[],
        status: Status::Documented,
    },
    Family {
        id: "t10.script_string",
        games: BO6_BO7,
        kinds: "zone script strings and notetracks (XXH3-64 of the decrypted text)",
        algo: Algo::Xxh3 { seed: P },
        canon: Canon::Raw,
        bits: 64,
        spellings: &[],
        status: Status::Spot,
    },
    Family {
        id: "t10.type_name",
        games: BO6_BO7,
        kinds: "asset type name beside an asset id; settings / bundle type names (TA basis)",
        algo: FNV64_TA,
        canon: Canon::Lower,
        bits: 63,
        spellings: &[],
        status: Status::Documented,
    },
    Family {
        id: "t10.bone",
        games: BO6_BO7,
        kinds: "bone names (TA basis, full 64 bits)",
        algo: FNV64_TA,
        canon: Canon::Raw,
        bits: 64,
        spellings: &["bone_"],
        status: Status::Documented,
    },
    Family {
        id: "t10.tunable_tag",
        games: BO6_BO7,
        kinds: "weapon tunables tags (TA basis, no lower-casing)",
        algo: FNV64_TA,
        canon: Canon::Raw,
        bits: 63,
        spellings: &[],
        status: Status::Documented,
    },
    Family {
        id: "t10.type_id",
        games: BO6_BO7,
        kinds: "asset type ids in the zone asset list (FNV-1a 32 of the type name)",
        algo: Algo::Fnv32 { basis: FNV32_BASIS },
        canon: Canon::Raw,
        bits: 32,
        spellings: &[],
        status: Status::Documented,
    },
    // --- Call of Duty Online: archive names, not script identifiers ---
    Family {
        id: "codol.ifs_name",
        games: &[Codol],
        kinds: ".ifs archive file names (lookup3 hashlittle2, seeds 2 and 1)",
        algo: Algo::HashLittle2 { pc: 2, pb: 1 },
        canon: Canon::LowerBackslash,
        bits: 64,
        spellings: &[],
        status: Status::Documented,
    },
    Family {
        id: "codol.mpq_key",
        games: &[Codol],
        kinds: ".ifs table and file encryption keys (MPQ HashString, offset 0x300)",
        algo: Algo::MpqHashString { offset: 0x300 },
        canon: Canon::Raw,
        bits: 32,
        spellings: &[],
        status: Status::Documented,
    },
];
