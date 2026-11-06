"""The name hashes of the builtin tables (Treyarch VMs and JUP), in Python. Each mirrors a crates/gsc_hash family (same
id) and is checked against `gruff hash <family> <name>` by `py -3.11 -I -m common.hashes` (run from
tools/re), which prints both for a few names.

    t7.canon       FNV-1a 32 from 0x4B9ACE2F over the lower-cased name, then one more multiply
    t89.scr        shift/xor/add loop from 0x4B9ACE2F, then 0x8001 * ((9h) ^ ((9h) >> 11))
    t8.fnv64       FNV-1a 64 (lower case), masked to 63 bits
    t10.function   salted FNV-1a 64: name[0] + salt + name[1..], masked to 60 bits
    iw9.scr        FNV-1a 64 from 0x79D6530B0BB9B5D1, prime 0x10000000233 (the JUP builtin tables)
    iw9.xhash      FNV-1a 64 (lower case), full 64 bits: the script hashes of MWII/MWIII and the
                   event hashes the BO7 engine passes to its notify (measured, events.py t11)
"""
FNV32_PRIME = 0x01000193
T7_SEED = 0x4B9ACE2F
M32 = 0xFFFFFFFF
M64 = (1 << 64) - 1


def _norm(name):
    return name.lower().replace("\\", "/").encode("latin1")


def t7_canon(name):
    h = T7_SEED
    for b in name.lower().encode("latin1"):
        h = ((h ^ b) * FNV32_PRIME) & M32
    return (h * FNV32_PRIME) & M32


def t89_scr(name):
    h = T7_SEED
    for b in _norm(name):
        x = (b + h) & M32
        x = (x ^ (x << 10)) & M32
        h = (x + (x >> 6)) & M32
    h = (h * 9) & M32
    return ((h ^ (h >> 11)) * 0x8001) & M32


def fnv1a64(data, basis=0xCBF29CE484222325, prime=0x100000001B3):
    h = basis
    for b in data:
        h = ((h ^ b) * prime) & M64
    return h


def t8_fnv64(name):
    return fnv1a64(_norm(name)) & 0x7FFFFFFFFFFFFFFF


T10_SALT = b"zt@f3yp(d[kkd=_@"


def t10_function(name):
    b = _norm(name)
    if not b:
        return 0
    return fnv1a64(b[:1] + T10_SALT + b[1:], 0x1C2F2E3C8A257D07, 0x10000000233) & 0x0FFFFFFFFFFFFFFF


def iw9_scr(name):
    """The name hash of the JUP (MWIII) builtin tables and of the bytecode's builtin references:
    64 bits, not masked."""
    return fnv1a64(_norm(name), 0x79D6530B0BB9B5D1, 0x10000000233)


def iw9_xhash(name):
    """FNV-1a 64 of the lower-cased name, not masked."""
    return fnv1a64(_norm(name))


FAMILIES = {"t7.canon": t7_canon, "t89.scr": t89_scr, "t8.fnv64": t8_fnv64,
            "t10.function": t10_function, "iw9.scr": iw9_scr, "iw9.xhash": iw9_xhash}

if __name__ == "__main__":
    for fam, f in FAMILIES.items():
        for n in ("getentarray", "main", "vectorscale"):
            print(fam, n, "%#x" % f(n))
