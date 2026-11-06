"""Builtin tables that still carry their names (IW1 gamex86.dll, IW3): runs of
{const char *name, handler, int type} whose name is a lower-case identifier string, the handler
an address in code and type 0 or 1. Returns each run with its entries and the code that loads
its address (the lookups).
"""
import os
import re
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from image import Image  # noqa: E402

IDENT = re.compile(rb"^[a-z_][a-z0-9_]*$")


def find_tables(im, min_run=3):
    """Runs of {name, handler, type 0/1} (12 bytes) and {name, handler} (8 bytes, CoD1's method
    tables). Each entry: (offset, name, handler, type or None); run.stride says which."""
    out = []
    for stride in (12, 8):
        for name, sva, vsz, ch in im.sections:
            if ch & 0x20000000 and name != ".text":
                continue
            o, end = sva, sva + vsz - stride
            while o < end:
                run = []
                p = o
                while p < end:
                    nm, fn = struct.unpack_from("<II", im.mem, p)
                    ty = struct.unpack_from("<I", im.mem, p + 8)[0] if stride == 12 else None
                    if (ty is not None and ty > 1) or not im.is_code(fn) or not im.in_image(nm):
                        break
                    s_ = im.cstr(nm, 80)
                    if not s_ or not IDENT.match(s_):
                        break
                    run.append((p, s_.decode(), fn, ty if ty is not None else 0))
                    p += stride
                if len(run) >= min_run:
                    if not any(r.stride == 12 and r[0][0] <= run[0][0] <= r[-1][0] for r in out):
                        out.append(Run(run, stride))
                    o = p
                else:
                    o += 4
    return out


class Run(list):
    def __init__(self, items, stride):
        super().__init__(items)
        self.stride = stride


def code_refs_x86(im, va):
    """Absolute 4-byte references to va from code (x86 push/mov/lea imm32)."""
    pat = struct.pack("<I", va)
    out = []
    i = im.mem.find(pat)
    while i >= 0:
        if im.is_code(im.base + i):
            out.append(im.base + i)
        i = im.mem.find(pat, i + 1)
    return out


if __name__ == "__main__":
    im = Image(sys.argv[1])
    print(im.describe())
    for run in find_tables(im):
        p0 = run[0][0]
        refs = code_refs_x86(im, im.base + p0) if not im.x64 else []
        refs2 = code_refs_x86(im, im.base + p0 + 4) if not im.x64 else []
        print("table 0x%x: %d entries, dev %d, first %s last %s, refs %s %s" % (
            im.base + p0, len(run), sum(r[3] for r in run), run[0][1], run[-1][1],
            [hex(r) for r in refs], [hex(r) for r in refs2]))
