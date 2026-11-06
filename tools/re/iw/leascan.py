"""Find registration code: functions that load many code addresses with `lea r64, [rip+disp]`
(x64) or `push imm32` / `mov r, imm32` of code addresses (x86). A build whose builtin tables
are not static data registers each handler from code; this finds those registrars.

    py -3.11 -I leascan.py <exe> [--min 40]
"""
import argparse
import bisect
import collections
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from image import Image, user_site  # noqa: E402

user_site()
import numpy as np  # noqa: E402


def lea_code_refs(im):
    """[(insn va, target va)] for every lea r64,[rip+disp32] whose target is in code."""
    out = []
    code = [(s[1], s[1] + s[2]) for s in im.sections if s[3] & 0x20000000]
    for a, b in code:
        buf = np.frombuffer(im.mem, dtype=np.uint8, count=b - a, offset=a).astype(np.int64)
        n = len(buf) - 7
        rex = buf[:n]
        op = buf[1:n + 1]
        modrm = buf[2:n + 2]
        hit = ((rex == 0x48) | (rex == 0x4C)) & (op == 0x8D) & ((modrm & 0xC7) == 0x05)
        idx = np.nonzero(hit)[0]
        if not len(idx):
            continue
        d = (buf[idx + 3] | (buf[idx + 4] << 8) | (buf[idx + 5] << 16) | (buf[idx + 6] << 24))
        d = np.where(d >= 1 << 31, d - (1 << 32), d)
        tgt = a + idx + 7 + d
        for i, t in zip(idx, tgt):
            t = int(t)
            if any(x <= t < y for x, y in code):
                out.append((im.base + a + int(i), im.base + t))
    return out


def by_function(im, refs):
    starts = sorted(im.base + b for b, _ in im.pdata)
    ends = {im.base + b: im.base + e for b, e in im.pdata}
    groups = collections.defaultdict(list)
    for va, t in refs:
        k = bisect.bisect_right(starts, va) - 1
        f = starts[k] if k >= 0 and va < ends[starts[k]] else None
        groups[f].append((va, t))
    return groups


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("exe")
    ap.add_argument("--min", type=int, default=40)
    a = ap.parse_args()
    im = Image(a.exe)
    print(im.describe())
    refs = lea_code_refs(im)
    g = by_function(im, refs)
    rows = sorted(((len(v), f) for f, v in g.items() if f and len(v) >= a.min), reverse=True)
    for n, f in rows[:40]:
        print("fn 0x%x: %d code leas, first targets %s" % (
            f, n, " ".join("%x" % t for _, t in g[f][:4])))


if __name__ == "__main__":
    main()
