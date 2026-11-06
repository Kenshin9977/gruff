"""Find arrays of function pointers in an image's data: runs of fixed-stride entries whose field
at one offset is always a function start (x64: a .pdata begin; x86: an address in code). The
builtin tables of every IW engine are such arrays ({name, handler, dev}, {handler, dev}, {id,
handler, ...}); the scan finds them with no name string, for the builds that strip the names.

    py -3.11 -I tablescan.py <exe> [--min 40]
"""
import argparse
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from image import Image, user_site  # noqa: E402

user_site()
import numpy as np  # noqa: E402


def fn_starts(im):
    if im.x64 and im.pdata:
        return np.array(sorted({im.base + b for b, _ in im.pdata}), dtype=np.uint64)
    return None


def scan(im, min_run=40, strides=(8, 12, 16, 24, 32, 40, 48), rva_mode=False):
    starts = fn_starts(im)
    w = im.ptr
    out = []
    for name, sva, vsz, ch in im.sections:
        if ch & 0x20000000 or not vsz or name in (".pdata", ".rsrc", ".reloc"):
            continue
        if rva_mode:
            vals = np.frombuffer(im.mem, dtype="<u4", count=vsz // 4, offset=sva).astype(np.uint64)
            vals = vals + np.uint64(im.base)
        elif w == 8:
            # every 4-aligned u64
            lo = np.frombuffer(im.mem, dtype="<u4", count=vsz // 4, offset=sva).astype(np.uint64)
            vals = lo[:-1] | (lo[1:] << np.uint64(32))
        else:
            vals = np.frombuffer(im.mem, dtype="<u4", count=vsz // 4, offset=sva).astype(np.uint64)
        if True:
            code = [(im.base + s[1], im.base + s[1] + s[2]) for s in im.sections
                    if s[3] & 0x20000000]
            ok = np.zeros(len(vals), bool)
            for a, b in code:
                ok |= (vals >= a) & (vals < b)
        # ok[i]: the 4-aligned slot i holds a function pointer
        used = set()
        for stride in strides:
            if stride % 4:
                continue
            k = stride // 4
            for phase in range(k):
                sel = ok[phase::k]
                # runs of True in sel
                d = np.diff(np.concatenate(([0], sel.astype(np.int8), [0])))
                rs, re_ = np.nonzero(d == 1)[0], np.nonzero(d == -1)[0]
                for a, b in zip(rs, re_):
                    if b - a >= min_run:
                        slot = phase + a * k
                        if any(abs(slot - u) < 4 and abs(cnt - (b - a)) < 4 for u, cnt in used):
                            continue
                        out.append((sva + slot * 4, stride, int(b - a)))
                        used.add((slot, int(b - a)))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("exe")
    ap.add_argument("--min", type=int, default=40)
    ap.add_argument("--rva", action="store_true", help="u32 RVAs instead of pointers")
    a = ap.parse_args()
    im = Image(a.exe)
    print(im.describe())
    res = scan(im, a.min, rva_mode=a.rva)
    # keep the smallest stride for one start (a stride-16 run is also a stride-32 run)
    res.sort(key=lambda r: (r[0], r[1]))
    seen = set()
    for rva, stride, n in res:
        if any(abs(rva - s) < 64 for s in seen):
            continue
        seen.add(rva)
        w = im.ptr
        row = []
        for e in range(2):
            ent = im.mem[rva + e * stride - 0: rva + e * stride + stride]
            row.append(ent.hex())
        print("0x%x stride %d count %d  sec %s  %s" % (im.base + rva, stride, n,
              im.section_of(im.base + rva)[0], " | ".join(row)))


if __name__ == "__main__":
    main()
