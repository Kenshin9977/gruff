#!/usr/bin/env python3
"""Print the export and import tables of a compiled BO3 (T7) script (.gscc/.cscc) as JSON.

acts' disassembly shows each call's target name but not the namespace the import is linked
against nor the import flags; this reads them straight from the GSCOBJ header (layout as in acts'
gsc_data_t7.hpp). Hashes are named when they match a word given with --names (fnv1a32-t7).

    python -I gscc_imports.py file.gscc --names zz_probe spawnstruct main
"""

from __future__ import annotations

import argparse
import json
import struct
import sys

IMPORT_KINDS = {1: "func_ref", 2: "function", 3: "function_thread", 4: "method", 5: "method_thread"}
EXPORT_FLAGS = {1: "linked", 2: "autoexec", 4: "private", 0x20: "ve"}


def t7_hash(s: str) -> int:
    h = 0x4B9ACE2F
    for b in s.lower().encode():
        h = ((h ^ b) * 0x01000193) & 0xFFFFFFFF
    return (h * 0x01000193) & 0xFFFFFFFF


def read_tables(d: bytes, words=()) -> dict:
    """Header name, exports and imports of a T7 GSCOBJ, hashes named from words (plus sys)."""
    names = {t7_hash(n): n for n in list(words) + ["sys"]}

    def nm(h: int) -> str:
        return names.get(h, f"0x{h:08x}")

    (src_crc, _inc, _anim, _cseg, _str, _dstr, exp_off, imp_off, _fix, _prof, _cseg_size,
     name_off) = struct.unpack_from("<12I", d, 8)
    _str_n, exp_n, imp_n, _fix_n, _prof_n, _dstr_n = struct.unpack_from("<6H", d, 56)

    exports = []
    for i in range(exp_n):
        _crc, addr, name, ns, params, fl = struct.unpack_from("<4IBB", d, exp_off + i * 20)
        exports.append({"name": nm(name), "namespace": nm(ns), "params": params,
                        "flags": [v for k, v in EXPORT_FLAGS.items() if fl & k], "address": addr})

    imports, off = [], imp_off
    for _ in range(imp_n):
        name, ns, n_addr, params, fl = struct.unpack_from("<IIHBB", d, off)
        off += 12
        addrs = list(struct.unpack_from(f"<{n_addr}I", d, off))
        off += 4 * n_addr
        imports.append({"name": nm(name), "namespace": nm(ns), "params": params,
                        "kind": IMPORT_KINDS.get(fl & 0xF, f"0x{fl & 0xF:x}"),
                        "dev_call": bool(fl & 0x10), "get_call": bool(fl & 0x20),
                        "flags": f"0x{fl:02x}", "refs": [f"0x{a:x}" for a in addrs]})
    end = d.index(0, name_off)
    return {"name": d[name_off:end].decode("latin-1"), "source_crc": f"0x{src_crc:08x}",
            "exports": exports, "imports": imports}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("gscc")
    ap.add_argument("--names", nargs="*", default=[], help="words to resolve hashes with")
    args = ap.parse_args()
    with open(args.gscc, "rb") as f:
        json.dump(read_tables(f.read(), args.names), sys.stdout, indent=1)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
