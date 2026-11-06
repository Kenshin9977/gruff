#!/usr/bin/env python3
"""Count the namespaces of the get-call imports in BO4 (T8, VM 0x36) and Cold War (T9, VM 0x38)
bytecode (TSV on stdout).

The compiler writes a bare call `f()` and a bare pointer `&f` as an import flagged get-call; a
qualified `ns::f()` is an import without the flag. Each import names the namespace the game's
linker looks in. This counts, per call type, whether that namespace is the script's own (the
namespace of its exports), empty, `sys`, or another one: a get-call import of another namespace
would mean a bare call can reach a `#using`'d file's namespace. Layouts after acts
(src/core/acts/tools/gsc/data/gsc_data_t8.hpp, gsc_data_t9.hpp). Reads .gscc/.cscc files
(GrayWolf `--type script` without `--decompile`); prints counts only.

    python -I import_namespaces.py <bytecode dir>
"""

from __future__ import annotations

import struct
import sys
from collections import Counter
from pathlib import Path

EMPTY, SYS = 0xC1243180, 0x222276A9  # the T8/T9 script hash of "" and of "sys"
MAGIC_T8, MAGIC_T9 = 0x36000A0D43534780, 0x38000A0D43534780

# (get-call bit, call types by the low nibble)
T8 = (0x20, {1: "func_method", 2: "function", 3: "function_thread", 4: "function_childthread",
             5: "method", 6: "method_thread", 7: "method_childthread"})
T9 = (0x10, {1: "method_childthread", 2: "method_thread", 3: "function_childthread",
             4: "function", 5: "func_method", 6: "function_thread", 7: "method"})


def parse(data: bytes):
    magic = struct.unpack_from("<Q", data, 0)[0]
    if magic == MAGIC_T8:
        exports_count, = struct.unpack_from("<H", data, 0x1E)
        imports_count, = struct.unpack_from("<H", data, 0x28)
        exports_at, = struct.unpack_from("<I", data, 0x30)
        imports_at, = struct.unpack_from("<I", data, 0x38)
        layout = T8
    elif magic == MAGIC_T9:
        exports_count, imports_count = struct.unpack_from("<HH", data, 0x1A)
        exports_at, imports_at = struct.unpack_from("<II", data, 0x38)
        layout = T9
    else:
        return None
    own = set()
    for i in range(exports_count):
        own.add(struct.unpack_from("<I", data, exports_at + 24 * i + 12)[0])
    imports = []
    at = imports_at
    for _ in range(imports_count):
        _, ns, num, _, flags = struct.unpack_from("<IIHBB", data, at)
        imports.append((ns, num, flags))
        at += 12 + 4 * num
    return layout, own, imports


def main() -> None:
    counts: Counter = Counter()
    scripts = 0
    for p in sorted(Path(sys.argv[1]).rglob("*")):
        if p.suffix not in (".gscc", ".cscc"):
            continue
        parsed = parse(p.read_bytes())
        if parsed is None:
            counts["other_vm_scripts"] += 1
            continue
        scripts += 1
        (get_bit, types), own, imports = parsed
        for ns, num, flags in imports:
            kind = types.get(flags & 0xF, "unknown")
            where = ("empty" if ns == EMPTY else "sys" if ns == SYS
                     else "own" if ns in own else "other")
            form = "get" if flags & get_bit else "qualified"
            counts[f"{form}_{kind}_{where}"] += num
    print("measure\tcount")
    print(f"bytecode_scripts\t{scripts}")
    for k in sorted(counts):
        print(f"{k}\t{counts[k]}")


if __name__ == "__main__":
    main()
