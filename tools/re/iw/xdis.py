"""Exploration helper: rip-relative / absolute references to an address, and a disassembly.

    py -3.11 -I xdis.py <exe> xref <va>
    py -3.11 -I xdis.py <exe> dis <va> [count]
"""
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from image import Image, user_site  # noqa: E402
from common.image import code_sections, le_s32, section_bytes  # noqa: E402

user_site()
import capstone  # noqa: E402
import numpy as np  # noqa: E402


def riprel_refs(im, target):
    """Every rip-relative disp32 whose end + disp == target (any instruction; confirm by decode)."""
    out = []
    for sva, vsz in code_sections(im.sections):
        b = section_bytes(im.mem, sva, vsz)
        n = len(b) - 4
        disp = le_s32(b, 0, n)
        pos = im.base + sva + np.arange(n, dtype=np.int64)
        for extra in (0, 1, 4):  # imm8 / imm32 after the disp
            hit = np.nonzero(pos + 4 + extra + disp == target)[0]
            out += [int(im.base + sva + i) for i in hit]
    return sorted(set(out))


def riprel_index(im, ranges, chunk=1 << 24):
    """{target: [disp32 va]} for every rip-relative disp32 in code whose target falls in one
    of `ranges` [(lo, hi)] (end of disp + 0/1/4 for a trailing immediate)."""
    out = {}
    for sva, vsz in code_sections(im.sections):
        for c0 in range(0, vsz - 4, chunk):
            n = min(chunk, vsz - 4 - c0)
            b = section_bytes(im.mem, sva + c0, n + 4)
            disp = le_s32(b, 0, n)
            pos = im.base + sva + c0 + np.arange(n, dtype=np.int64)
            for extra in (0, 1, 4):
                t = pos + 4 + extra + disp
                m = np.zeros(n, bool)
                for lo, hi in ranges:
                    m |= (t >= lo) & (t < hi)
                for i in np.nonzero(m)[0]:
                    out.setdefault(int(t[i]), []).append(int(pos[i]))
    return out


def abs_refs(im, target):
    pat = struct.pack("<Q" if im.x64 else "<I", target)
    out, i = [], im.mem.find(pat)
    while i >= 0:
        out.append(im.base + i)
        i = im.mem.find(pat, i + 1)
    return out


def capstone_for(im, detail=True):
    """A capstone decoder for the image's mode."""
    m = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64 if im.x64 else capstone.CS_MODE_32)
    m.detail = detail
    return m


md = capstone_for


def insn_at(im, m, disp_va):
    """The instruction containing the disp32 at disp_va: try starts up to 10 bytes before."""
    for back in range(1, 11):
        st = disp_va - back
        try:
            ins = next(m.disasm(im.mem[st - im.base:st - im.base + 16], st, 1))
        except StopIteration:
            continue
        if ins.address + ins.size > disp_va + 3 and ins.address < disp_va:
            if "rip" in ins.op_str:
                return ins
    return None


def dis(im, va, count=40):
    m = md(im)
    o = va - im.base
    for ins in m.disasm(im.mem[o:o + count * 15], va, count):
        print("  %x: %-24s %s %s" % (ins.address, ins.bytes.hex(), ins.mnemonic, ins.op_str))


def main():
    im = Image(sys.argv[1])
    cmd, va = sys.argv[2], int(sys.argv[3], 16)
    if cmd == "xref":
        m = md(im)
        if im.x64:
            for r in riprel_refs(im, va):
                ins = insn_at(im, m, r)
                if ins:
                    print("%x: %s %s" % (ins.address, ins.mnemonic, ins.op_str))
        for r in abs_refs(im, va):
            print("abs %x (%s)" % (r, im.section_of(r)[0]))
    else:
        dis(im, va, int(sys.argv[4]) if len(sys.argv) > 4 else 40)


if __name__ == "__main__":
    main()
