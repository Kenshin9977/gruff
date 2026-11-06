#!/usr/bin/env python3
"""Count the hidden foreach locals the retail T6 compiler left in BO2 bytecode (TSV on stdout).

Treyarch's compiler lowers `foreach ( v in a )` to a getfirstarraykey/getnextarraykey loop over
two hidden locals named `_a<N>` (array copy) and `_k<N>` (key); the key/value form names only
`_a<N>`. T6 bytecode keeps local names as plain strings, so these names show which loops were
written as foreach in the retail source. A hand-written for loop would carry the author's names.
Reads .gscc/.cscc files (GrayWolf `--type script` without `--decompile`); prints counts only.

    python -I t6_foreach_locals.py <bytecode dir> [<decompiled dir>]
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

HIDDEN = re.compile(rb"(?<![\x21-\x7e])_([ak])([0-9]+)(?=\x00)")
FOREACH_SRC = re.compile(r"\bforeach\s*\(")
FOREACH_KV = re.compile(r"\bforeach\s*\(\s*\w+\s*,\s*\w+\s+in\b")


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    files = scripts = with_a = n_a = n_k = a_only = 0
    for p in Path(sys.argv[1]).rglob("*"):
        if p.suffix.lower() not in (".gscc", ".cscc") or not p.is_file():
            continue
        files += 1
        d = p.read_bytes()
        if not d.startswith(b"\x80GSC"):
            continue
        scripts += 1
        seen = {(m.group(1), int(m.group(2))) for m in HIDDEN.finditer(d)}
        a = {n for k, n in seen if k == b"a"}
        k = {n for kk, n in seen if kk == b"k"}
        if a:
            with_a += 1
        n_a += len(a)
        n_k += len(k)
        a_only += len(a - k)
    rows = [("bytecode_scripts", scripts), ("scripts_with_hidden_array_local", with_a),
            ("hidden_array_locals_aN", n_a), ("hidden_key_locals_kN", n_k),
            ("aN_without_kN_key_value_form", a_only)]
    if len(sys.argv) > 2:
        fe = files_fe = kv = 0
        for p in Path(sys.argv[2]).rglob("*"):
            if p.suffix.lower() in (".gsc", ".csc") and p.is_file():
                text = p.read_text("latin-1")
                c = len(FOREACH_SRC.findall(text))
                fe += c
                files_fe += c > 0
                kv += len(FOREACH_KV.findall(text))
        rows += [("decompiled_foreach", fe), ("decompiled_files_with_foreach", files_fe),
                 ("decompiled_foreach_key_value", kv)]
    print("measure\tcount")
    for k, v in rows:
        print(f"{k}\t{v}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
