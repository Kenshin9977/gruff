//! The raw hash functions, over bytes that are already canonicalised.
//!
//! Every function here is a short loop over constants. They are reimplemented from their
//! descriptions; the constants are facts about the games, not code taken from any project.
//! Bytes are treated as unsigned (`u8`) everywhere. Some C implementations hash a signed `char`,
//! which only changes the result for bytes `>= 0x80`; see the crate docs.

/// The standard FNV-1a 64 offset basis, the "TA" (Treyarch) basis.
pub const FNV64_BASIS_TA: u64 = 0xCBF2_9CE4_8422_2325;
/// The IW asset-name basis.
pub const FNV64_BASIS_IW: u64 = 0x47F5_817A_5EF9_61BA;
/// The IW script / entity-field basis (MWII, MWIII "XHashScr").
pub const FNV64_BASIS_IW_SCR: u64 = 0x79D6_530B_0BB9_B5D1;
/// The BO6/BO7 script offset (function, var, namespace, event, bundlevar, entity keys).
pub const FNV64_BASIS_T10_SCR: u64 = 0x1C2F_2E3C_8A25_7D07;
/// The dvar offset of the salted dvar hash.
pub const FNV64_BASIS_DVAR: u64 = 0xD86A_3B09_566E_BAAC;
/// The BO6/BO7 omnvar offset.
pub const FNV64_BASIS_OMNVAR: u64 = 0xCBF2_8CE5_9312_3345;

/// The standard FNV-1a 64 prime.
pub const FNV64_PRIME: u64 = 0x0000_0100_0000_01B3;
/// The prime of BO6/BO7 `switch` case constants.
pub const FNV64_PRIME_CASE: u64 = 0x0000_0100_0000_0193;
/// The prime of the salted ("secure") hashes and of the IW script hash.
pub const FNV64_PRIME_SECURE: u64 = 0x0000_0100_0000_0233;
/// The prime of the omnvar hash.
pub const FNV64_PRIME_OMNVAR: u64 = 0x0000_0100_0000_02C1;

/// The standard FNV-1a 32 offset basis.
pub const FNV32_BASIS: u32 = 0x811C_9DC5;
/// The standard FNV-1a 32 prime.
pub const FNV32_PRIME: u32 = 0x0100_0193;
/// The Treyarch 32-bit seed (BO3 canonical hash, BO4/CW script hash).
pub const T7_SEED: u32 = 0x4B9A_CE2F;

/// The salt of the BO6/BO7 script hash.
pub const SALT_T10_SCR: &[u8] = b"zt@f3yp(d[kkd=_@";
/// The salt of the dvar hash.
pub const SALT_DVAR: &[u8] = b"q6n-+7=tyytg94_*";
/// The salt of the omnvar hash.
pub const SALT_OMNVAR: &[u8] = b"gvbs9*vpm@mh@krh";

/// FNV-1a 64 from `basis` with `prime`, continuing over `bytes`.
#[inline]
pub fn fnv1a64(bytes: &[u8], basis: u64, prime: u64) -> u64 {
    bytes
        .iter()
        .fold(basis, |h, &b| (h ^ u64::from(b)).wrapping_mul(prime))
}

/// FNV-1a 32 from `basis` with the standard 32-bit prime.
#[inline]
pub fn fnv1a32(bytes: &[u8], basis: u32) -> u32 {
    bytes
        .iter()
        .fold(basis, |h, &b| (h ^ u32::from(b)).wrapping_mul(FNV32_PRIME))
}

/// The salted ("secure") FNV-1a 64: the salt is inserted after the first byte of the name,
/// `name[0] + salt + name[1..]`. An empty name hashes to 0.
pub fn secure_fnv64(bytes: &[u8], salt: &[u8], basis: u64, prime: u64) -> u64 {
    let Some((&first, rest)) = bytes.split_first() else {
        return 0;
    };
    let h = fnv1a64(&[first], basis, prime);
    let h = fnv1a64(salt, h, prime);
    fnv1a64(rest, h, prime)
}

/// FNV-1a 64 of `name + salt` (the BO6 campaign variant of the script hash).
pub fn salt_after_fnv64(bytes: &[u8], salt: &[u8], basis: u64, prime: u64) -> u64 {
    fnv1a64(salt, fnv1a64(bytes, basis, prime), prime)
}

/// The BO3 (T7) canonical hash: FNV-1a 32 from seed `0x4B9ACE2F`, then one extra multiply by the
/// prime after the last byte.
pub fn t7_canon(bytes: &[u8]) -> u32 {
    fnv1a32(bytes, T7_SEED).wrapping_mul(FNV32_PRIME)
}

/// The BO4/Cold War script hash (acts' `HashT89Scr`): a shift/xor/add loop seeded with
/// `0x4B9ACE2F`, then the finaliser `0x8001 * ((9h) ^ ((9h) >> 11))`.
pub fn t89_scr(bytes: &[u8]) -> u32 {
    let mut h = T7_SEED;
    for &b in bytes {
        let x = u32::from(b).wrapping_add(h);
        let x = x ^ (x << 10);
        h = x.wrapping_add(x >> 6);
    }
    let h = h.wrapping_mul(9);
    (h ^ (h >> 11)).wrapping_mul(0x8001)
}

/// djb2 with xor, starting from `seed`, each byte or-ed with `0x20` (case-insensitive for letters,
/// and it also changes some non-letters, as the game does). `R_HashString` uses seed 0.
pub fn djb2_xor_nocase(bytes: &[u8], seed: u32) -> u32 {
    bytes
        .iter()
        .fold(seed, |h, &b| h.wrapping_mul(33) ^ u32::from(b | 0x20))
}

/// djb2 with addition from 5381 over already-lowered bytes (`Com_HashString`).
pub fn djb2_add(bytes: &[u8]) -> u32 {
    mul_add(bytes, 5381, 33)
}

/// `h = h * 31 + c` from 0 (`StringTable_HashString`, over already-lowered bytes).
pub fn string_table31(bytes: &[u8]) -> u32 {
    mul_add(bytes, 0, 31)
}

/// `Com_HashKey` (BO1/BO2): `h += c * (0x77 + i)`, then `h ^ ((h ^ (h >> 10)) >> 10)`, in signed
/// 32-bit arithmetic. Case-sensitive. `max_len` is not modelled: the whole input is hashed.
pub fn com_hash_key(bytes: &[u8]) -> u32 {
    let mut h: i32 = 0;
    for (i, &b) in bytes.iter().enumerate() {
        // The game multiplies a C `char`; bytes >= 0x80 are rare in keys and taken unsigned here.
        let m = 0x77i32.wrapping_add(i as i32);
        h = h.wrapping_add(i32::from(b).wrapping_mul(m));
    }
    (h ^ ((h ^ (h >> 10)) >> 10)) as u32
}

/// SDBM from 5381: `h = c + h * 0x1003F` (= `c + (h << 6) + (h << 16) - h`), over lowered bytes.
pub fn sdbm(bytes: &[u8]) -> u32 {
    mul_add(bytes, 5381, 0x1003F)
}

/// `h = h * mul + c` from `seed`: the shape djb2 (add), the string-table hash and SDBM share.
fn mul_add(bytes: &[u8], seed: u32, mul: u32) -> u32 {
    bytes
        .iter()
        .fold(seed, |h, &b| h.wrapping_mul(mul).wrapping_add(u32::from(b)))
}

/// XXH3-64 with a seed, over the raw bytes.
pub fn xxh3_64(bytes: &[u8], seed: u64) -> u64 {
    xxhash_rust::xxh3::xxh3_64_with_seed(bytes, seed)
}

/// The MPQ crypt table (1280 entries), as Blizzard's MPQ format builds it.
fn mpq_crypt_table() -> &'static [u32; 0x500] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<[u32; 0x500]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0u32; 0x500];
        let mut seed: u32 = 0x0010_0001;
        for i in 0..0x100 {
            for j in 0..5 {
                seed = (seed * 125 + 3) % 0x2A_AAAB;
                let high = (seed & 0xFFFF) << 16;
                seed = (seed * 125 + 3) % 0x2A_AAAB;
                table[0x100 * j + i] = high | (seed & 0xFFFF);
            }
        }
        table
    })
}

/// MPQ `HashString` with a table offset (0, 0x100, 0x200 or 0x300): upper-case ASCII, bytes
/// `>= 0x80` as `?` (as CoD Online's IFS archives do).
pub fn mpq_hash_string(bytes: &[u8], offset: usize) -> u32 {
    let table = mpq_crypt_table();
    let (mut hash, mut seed) = (0x7FED_7FEDu32, 0xEEEE_EEEEu32);
    for &b in bytes {
        let mut b = if b >= 0x80 { b'?' } else { b };
        b = b.to_ascii_uppercase();
        hash = table[offset + usize::from(b)] ^ hash.wrapping_add(seed);
        seed = seed
            .wrapping_add(hash)
            .wrapping_add(seed << 5)
            .wrapping_add(u32::from(b))
            .wrapping_add(3);
    }
    hash
}

/// Bob Jenkins' lookup3 `hashlittle2`, returning `(pc, pb)`.
pub fn hashlittle2(key: &[u8], pc: u32, pb: u32) -> (u32, u32) {
    let word = |s: &[u8]| u32::from_le_bytes([s[0], s[1], s[2], s[3]]);
    let init = 0xDEAD_BEEFu32
        .wrapping_add(key.len() as u32)
        .wrapping_add(pc);
    let (mut a, mut b, mut c) = (init, init, init.wrapping_add(pb));
    let mut rest = key;
    while rest.len() > 12 {
        a = a.wrapping_add(word(&rest[0..4]));
        b = b.wrapping_add(word(&rest[4..8]));
        c = c.wrapping_add(word(&rest[8..12]));
        // mix
        a = a.wrapping_sub(c);
        a ^= c.rotate_left(4);
        c = c.wrapping_add(b);
        b = b.wrapping_sub(a);
        b ^= a.rotate_left(6);
        a = a.wrapping_add(c);
        c = c.wrapping_sub(b);
        c ^= b.rotate_left(8);
        b = b.wrapping_add(a);
        a = a.wrapping_sub(c);
        a ^= c.rotate_left(16);
        c = c.wrapping_add(b);
        b = b.wrapping_sub(a);
        b ^= a.rotate_left(19);
        a = a.wrapping_add(c);
        c = c.wrapping_sub(b);
        c ^= b.rotate_left(4);
        b = b.wrapping_add(a);
        rest = &rest[12..];
    }
    if rest.is_empty() {
        return (c, b);
    }
    let mut tail = [0u8; 12];
    tail[..rest.len()].copy_from_slice(rest);
    a = a.wrapping_add(word(&tail[0..4]));
    b = b.wrapping_add(word(&tail[4..8]));
    c = c.wrapping_add(word(&tail[8..12]));
    // final
    c ^= b;
    c = c.wrapping_sub(b.rotate_left(14));
    a ^= c;
    a = a.wrapping_sub(c.rotate_left(11));
    b ^= a;
    b = b.wrapping_sub(a.rotate_left(25));
    c ^= b;
    c = c.wrapping_sub(b.rotate_left(16));
    a ^= c;
    a = a.wrapping_sub(c.rotate_left(4));
    b ^= a;
    b = b.wrapping_sub(a.rotate_left(14));
    c ^= b;
    c = c.wrapping_sub(b.rotate_left(24));
    (c, b)
}
