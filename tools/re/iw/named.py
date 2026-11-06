"""Builtin tables of the builds that keep names (IW3 iw3mp.exe, IW1 gamex86.dll): every
{name, handler, type} table, its exact bounds and its role, from the engine's lookups.

1. `nametables.find_tables` finds runs of name-shaped entries (adjacent tables merge into one
   run: the bounds do not come from the run).
2. The lookups: the code that loads the address of an entry inside a run. Each is emulated with
   an absent name; its reads at the entry stride give the table it scans and the exact count
   (first measure). The loop bound `cmp reg, N*12` it compares against is the second.
3. Roles: the lookups are called from two places, the function chain (Scr_GetFunction) and the
   method chain (Scr_GetMethod: several lookups in a row). Which chain is which: the method
   handlers read their entref argument (checked per entry by the handler runner), and the
   corpus calls agree (build step).
4. Round trip: every name is looked up again through its lookup; handler and type must come back.
"""
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from image import user_site  # noqa: E402
from iwemu import Emu  # noqa: E402
from nametables import find_tables  # noqa: E402
from common.image import rel32_branches  # noqa: E402

user_site()
import capstone  # noqa: E402
import unicorn  # noqa: E402
from unicorn import x86_const as X  # noqa: E402


_STARTS = {}


def call_targets(im):
    """Every direct call (E8 rel32) target in code: the function starts of an x86 image without
    .pdata (a function nobody calls directly is not found; the lookups all are)."""
    if id(im) in _STARTS:
        return _STARTS[id(im)]
    out = set()
    for sva, vsz, op, t in rel32_branches(im.mem, im.base, im.sections, tail=5):
        hit = (op == 0xE8) & (t >= im.base + sva) & (t < im.base + sva + vsz)
        out.update(int(x) for x in t[hit])
    s = sorted(out)
    _STARTS[id(im)] = s
    return s


def fn_start_x86(im, va, limit=0x2000):
    """Start of the function containing va: the later of the nearest direct-call target at or
    below it and the end of the nearest padding run (int3/nop) before it (a function passed by
    pointer, CoD1's lookups, is never called directly)."""
    import bisect
    s = call_targets(im)
    k = bisect.bisect_right(s, va) - 1
    c = s[k] if k >= 0 and va - s[k] <= limit else None
    o = va - im.base
    lo = max(0, o - limit)
    pad = None
    while o > lo:
        if im.mem[o - 1] in (0xCC, 0x90) and im.mem[o - 2] in (0xCC, 0x90, 0xC3):
            pad = im.base + o
            break
        o -= 1
    cands = [x for x in (c, pad) if x is not None]
    return max(cands) if cands else None


def refs_to_runs(im, runs):
    """{lookup function start: set of run indices} for code holding the address of any entry
    field (name, handler, type) of a run as an imm32/disp32."""
    starts = {}
    targets = {}
    for ri, run in enumerate(runs):
        for p, _, _, _ in run:
            for k in range(0, run.stride, 4):
                targets[im.base + p + k] = ri
    code = [(s[1], s[1] + s[2]) for s in im.sections if s[3] & 0x20000000]
    m = im.mem
    for a, b in code:
        for o in range(a, b - 4):
            v = struct.unpack_from("<I", m, o)[0]
            ri = targets.get(v)
            if ri is None:
                continue
            f = fn_start_x86(im, im.base + o)
            if f is not None:
                starts.setdefault(f, set()).add(ri)
    return starts


def trace_lookup(im, emu, fn, runs):
    """Emulate the lookup with an absent name; the reads that land on run entries at stride 12
    give (table start va, count)."""
    uc = emu.uc
    lo = min(im.base + r[0][0] for r in runs)
    hi = max(im.base + r[-1][0] + r.stride for r in runs)
    reads = []

    def on_read(uc_, access, addr, size, value, data):
        if lo <= addr < hi:
            reads.append(addr)
    h = uc.hook_add(unicorn.UC_HOOK_MEM_READ, on_read, begin=lo, end=hi)
    uc.ctl_flush_tb()
    name = emu.alloc(16)
    uc.mem_write(name, b"\x01\x02zz_absent\0")
    pname = emu.alloc(8)
    emu.w32(pname, name)
    ptype = emu.alloc(8)
    try:
        emu.call(fn, (pname, ptype), count=200000,
                 regs={X.UC_X86_REG_ESI: pname, X.UC_X86_REG_EDI: ptype,
                       X.UC_X86_REG_EAX: pname, X.UC_X86_REG_ECX: pname, X.UC_X86_REG_EDX: ptype})
        ok = True
    except unicorn.UcError as e:
        ok = str(e)
    finally:
        uc.hook_del(h)
        uc.ctl_flush_tb()
    ret = uc.reg_read(X.UC_X86_REG_EAX)
    # name-pointer reads: the entry starts; keep the 12-stride run
    starts = {im.base + e[0] for r in runs for e in r}
    ents = sorted(set(a for a in reads if a in starts))
    if not ents:
        return None, ok, ret
    # longest arithmetic run with step 12 starting at the first read
    first = ents[0]
    stride = next(r.stride for r in runs if any(im.base + e[0] == first for e in r))
    n = 0
    s = set(ents)
    while first + stride * n in s:
        n += 1
    return (first, n, stride), ok, ret


def static_bound(im, fn, n, stride=12):
    """Second measure: a `cmp reg, n*12` (or `cmp reg, n`) in the lookup's first 0x80 bytes."""
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    o = fn - im.base
    for ins in md.disasm(im.mem[o:o + 0x80], fn):
        if ins.mnemonic == "cmp":
            ops = ins.op_str.split(", ")
            if len(ops) == 2 and ops[1].startswith("0x"):
                v = int(ops[1], 16)
                if v in (n * stride, n):
                    return v
        if ins.mnemonic == "ret":
            break
    return None


def round_trip(im, emu, fn, entries):
    bad = []
    for va, nm, h, ty in entries:
        name = emu.alloc(len(nm) + 1)
        emu.uc.mem_write(name, nm.encode() + b"\0")
        pname = emu.alloc(8)
        emu.w32(pname, name)
        ptype = emu.alloc(8)
        emu.w32(ptype, 0xDEAD)
        try:
            r = emu.call(fn, (pname, ptype), count=200000,
                         regs={X.UC_X86_REG_ESI: pname, X.UC_X86_REG_EDI: ptype})
        except unicorn.UcError as e:
            bad.append((nm, "fault %s" % e))
            continue
        t = emu.r32(ptype)
        # a lookup of the method chain leaves *type to its caller (sentinel unchanged)
        if not emu.returned or r != h or t not in (ty, 0xDEAD):
            bad.append((nm, "returned 0x%x type %d" % (r, emu.r32(ptype))))
    return bad


def chains(im, lookups):
    """The callers of the lookups: {caller function start: [lookups it calls, in call order]}."""
    m = im.mem
    out = {}
    want = set(lookups)
    code = [(s[1], s[1] + s[2]) for s in im.sections if s[3] & 0x20000000]
    for a, b in code:
        for o in range(a, b - 5):
            if m[o] != 0xE8:
                continue
            t = im.base + o + 5 + struct.unpack_from("<i", m, o + 1)[0]
            if t in want:
                f = fn_start_x86(im, im.base + o, 0x2000)
                out.setdefault(f, []).append((im.base + o, t))
    return {f: [t for _, t in sorted(v)] for f, v in out.items()}


def extract(im, log=print):
    runs = find_tables(im)
    log("name-shaped runs: %d (%s)" % (len(runs), ", ".join(
        "0x%x:%d" % (im.base + r[0][0], len(r)) for r in runs)))
    by_va = {}
    for r in runs:
        for p, nm, h, ty in r:
            by_va[im.base + p] = (im.base + p, nm, h, ty)
    lk = refs_to_runs(im, runs)
    emu = Emu(im)
    tables = []
    for fn in sorted(lk):
        tr, ok, ret = trace_lookup(im, emu, fn, runs)
        if not tr or tr[1] < 1:
            log("  code 0x%x refs a run, not a lookup (no entry-stride reads, %s)" % (fn, ok))
            continue
        first, n, stride = tr
        ents = [by_va[first + stride * i] for i in range(n) if first + stride * i in by_va]
        sb = static_bound(im, fn, n, stride)
        rt = round_trip(im, emu, fn, ents)
        log("  lookup 0x%x: table 0x%x, %d entries (static bound %s), absent name -> %s %s, "
            "round trip %d/%d" % (fn, first, n, hex(sb) if sb else "none", hex(ret), ok,
                                  n - len(rt), n))
        for b in rt[:5]:
            log("     round trip failed: %s %s" % b)
        tables.append({"lookup": fn, "table": first, "count": n, "static_bound": sb, "stride": stride,
                       "round_trip_failed": rt, "entries": ents})
    ch = chains(im, [t["lookup"] for t in tables])
    for f, lst in ch.items():
        log("  chain 0x%x calls %s" % (f, " ".join("%x" % x for x in lst)))
    covered = sum(t["count"] for t in tables)
    total = sum(len(r) for r in runs)
    log("entries in a traced table: %d of %d name-shaped" % (covered, total))
    return tables, ch
