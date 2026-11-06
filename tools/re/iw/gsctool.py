"""gsc-tool's per-game builtin tables (xensik/gsc-tool, GPL-3.0), read locally from its source
as a cross-check and a name source only. Nothing from it is written as CC0: an entry whose name
comes from here is labelled GPL-3.0-only in the data, with gsc-tool as its source.

    names(engine) -> ({id: function name}, {id: method name})

engine: the file prefix in src/gsc/engine (iw5_pc, iw6_pc, s1_pc, h1, iw7, s2, iw8, iw9).
Placeholder names (`_func_XXX`, `_meth_XXX`) are dropped: gsc-tool does not know them.
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from common import paths  # noqa: E402

SRC = paths.get("gsctool_src")  # gsc-tool's src/gsc/engine: paths.local.json or GSC_RE_GSCTOOL_SRC
ROW = re.compile(r'\{\s*0x([0-9A-Fa-f]+)\s*,\s*"([^"]+)"\s*\}')


def _read(path):
    out = {}
    with open(path, encoding="utf-8", errors="replace") as f:
        for m in ROW.finditer(f.read()):
            i, nm = int(m.group(1), 16), m.group(2)
            if nm.startswith(("_func_", "_meth_", "_id_")):
                continue
            out[i] = nm
    return out


def names(engine, src=SRC):
    f = os.path.join(src, engine + "_func.cpp")
    m = os.path.join(src, engine + "_meth.cpp")
    if not os.path.exists(f):
        return {}, {}
    return _read(f), _read(m)


def placeholder(nm):
    """gsc-tool's spelling of an unnamed builtin in decompiled output: _func_<hex> / _meth_<hex>
    -> ('function'|'method', id)."""
    m = re.match(r"_(func|meth)_([0-9a-fA-F]+)$", nm)
    if not m:
        return None
    return ("function" if m.group(1) == "func" else "method", int(m.group(2), 16))
