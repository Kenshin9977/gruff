//! Readers for name databases: `hash,name` CSV and GrayWolf/Wraith `.wni` indexes.
//!
//! The readers take bytes or text, never paths: the caller owns the file system. No database is
//! shipped with this crate.

use std::fmt;

/// A database read error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbError {
    /// Not a WNI file (bad magic).
    BadMagic,
    /// The file ends early.
    Truncated,
    /// The LZ4 block is malformed.
    Lz4(&'static str),
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbError::BadMagic => f.write_str("not a WNI file"),
            DbError::Truncated => f.write_str("truncated file"),
            DbError::Lz4(why) => write!(f, "bad LZ4 block: {why}"),
        }
    }
}

impl std::error::Error for DbError {}

/// Parses a hash written as hex, with or without `0x` or `hash_`.
pub fn parse_hex(s: &str) -> Option<u64> {
    let s = s.trim();
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .or_else(|| s.strip_prefix("hash_"))
        .unwrap_or(s);
    if s.is_empty() || s.len() > 16 {
        return None;
    }
    u64::from_str_radix(s, 16).ok()
}

/// Reads `hash,name` lines. The name is everything after the first comma (names can hold commas).
/// Lines whose first field is not hex (a header) are skipped, as are empty lines.
pub fn read_csv_pairs(text: &str) -> impl Iterator<Item = (u64, &str)> {
    text.lines().filter_map(|line| {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let (h, name) = line.split_once(',')?;
        Some((parse_hex(h)?, name))
    })
}

/// WNI magic, `"WNI "` little endian.
pub const WNI_MAGIC: u32 = 0x2049_4E57;

/// Reads a `.wni` index: `u32 magic, u16 version, u32 count, u32 packed, u32 unpacked`, then one
/// LZ4 block of `{u64 key, name NUL}` records. Keys are returned unmasked.
pub fn read_wni(data: &[u8]) -> Result<Vec<(u64, String)>, DbError> {
    let u32_at = |at: usize| -> Result<u32, DbError> {
        data.get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or(DbError::Truncated)
    };
    if u32_at(0)? != WNI_MAGIC {
        return Err(DbError::BadMagic);
    }
    let count = u32_at(6)? as usize;
    let packed = u32_at(10)? as usize;
    let unpacked = u32_at(14)? as usize;
    let block = data.get(18..18 + packed).ok_or(DbError::Truncated)?;
    let raw = lz4_block_decompress(block, unpacked)?;
    let mut out = Vec::with_capacity(count);
    let mut at = 0usize;
    for _ in 0..count {
        let key = raw.get(at..at + 8).ok_or(DbError::Truncated)?;
        let key = u64::from_le_bytes(key.try_into().map_err(|_| DbError::Truncated)?);
        at += 8;
        let len = raw[at..]
            .iter()
            .position(|&b| b == 0)
            .ok_or(DbError::Truncated)?;
        out.push((
            key,
            String::from_utf8_lossy(&raw[at..at + len]).into_owned(),
        ));
        at += len + 1;
    }
    Ok(out)
}

/// Decompresses one raw LZ4 block (no frame) of known decompressed size.
pub fn lz4_block_decompress(src: &[u8], size: usize) -> Result<Vec<u8>, DbError> {
    let mut out: Vec<u8> = Vec::with_capacity(size);
    let mut i = 0usize;
    let byte = |i: usize| src.get(i).copied().ok_or(DbError::Lz4("truncated"));
    let read_len = |i: &mut usize, mut len: usize| -> Result<usize, DbError> {
        if len == 15 {
            loop {
                let b = byte(*i)?;
                *i += 1;
                len += usize::from(b);
                if b != 255 {
                    break;
                }
            }
        }
        Ok(len)
    };
    while i < src.len() {
        let token = byte(i)?;
        i += 1;
        let lit = read_len(&mut i, usize::from(token >> 4))?;
        let lits = src
            .get(i..i + lit)
            .ok_or(DbError::Lz4("literals past end"))?;
        out.extend_from_slice(lits);
        i += lit;
        if i >= src.len() {
            break; // the last sequence has literals only
        }
        let offset = usize::from(byte(i)?) | (usize::from(byte(i + 1)?) << 8);
        i += 2;
        if offset == 0 || offset > out.len() {
            return Err(DbError::Lz4("bad offset"));
        }
        let mlen = read_len(&mut i, usize::from(token & 0x0F))? + 4;
        let start = out.len() - offset;
        for k in 0..mlen {
            let b = out[start + k];
            out.push(b);
        }
        if out.len() > size {
            return Err(DbError::Lz4("output larger than declared"));
        }
    }
    if out.len() != size {
        return Err(DbError::Lz4("output size differs from the header"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_forms() {
        assert_eq!(parse_hex("0x1A"), Some(0x1a));
        assert_eq!(parse_hex("hash_ff"), Some(0xff));
        assert_eq!(parse_hex("0ec28edb5ed47500"), Some(0x0ec2_8edb_5ed4_7500));
        assert_eq!(parse_hex("hash"), None);
    }

    #[test]
    fn csv_keeps_commas_in_names() {
        let v: Vec<_> = read_csv_pairs("hash,name\n0000d38e,a, b\r\n\n").collect();
        assert_eq!(v, vec![(0xd38e, "a, b")]);
    }

    #[test]
    fn lz4_literals_and_overlapping_match() {
        // "abc" then a match of offset 3, length 9 -> "abcabcabcabc", then literal "x".
        let block = [0x35, b'a', b'b', b'c', 3, 0, 0x10, b'x'];
        let out = lz4_block_decompress(&block, 13).unwrap();
        assert_eq!(out, b"abcabcabcabcx");
    }

    #[test]
    fn wni_roundtrip() {
        let mut raw = Vec::new();
        raw.extend_from_slice(&0x1234u64.to_le_bytes());
        raw.extend_from_slice(b"name\0");
        let mut block = vec![(raw.len() as u8) << 4];
        block.extend_from_slice(&raw);
        let mut file = Vec::new();
        file.extend_from_slice(&WNI_MAGIC.to_le_bytes());
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&1u32.to_le_bytes());
        file.extend_from_slice(&(block.len() as u32).to_le_bytes());
        file.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        file.extend_from_slice(&block);
        assert_eq!(read_wni(&file).unwrap(), vec![(0x1234, "name".to_string())]);
    }
}
