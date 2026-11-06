"""Builtin registration of the id builds (IW4/IW5 64-bit rebuilds, IW6, S1, H1, IW7, S2): the
engine registers every handler at init through one registrar, from loops over static tables
({u64 id, handler, u32 type} in the id builds, {handler, u32 type} in IW4's), plus a few
single calls.

1. The tables: `tablescan` runs of handler pointers; the code loading an address inside one is
   a registration loop, the function it calls inside the loop the registrar R.
2. Every caller of R is emulated from its function start with R hooked: the hook records each
   call's (handler, type, id) and returns. The loop bound (`mov reg, N`) is the second
   measure of each table's size.
3. Roles: R itself is run on each recorded call with a write hook. It stores the handler in
   one of two arrays: the function array and the method array (the VM's two builtin tables).
   Which array is which: the build step checks the corpus (method call syntax).
"""
import collections
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from image import user_site  # noqa: E402
from iwemu import Emu  # noqa: E402
from tablescan import scan  # noqa: E402
from xdis import riprel_index  # noqa: E402
from common.image import rel32_branches  # noqa: E402

user_site()
import capstone  # noqa: E402
import unicorn  # noqa: E402
from unicorn import x86_const as X  # noqa: E402


def pdata_start(im, va):
    import bisect
    if not hasattr(im, "_pd"):
        im._pd = sorted((im.base + b, im.base + e) for b, e in im.pdata)
        im._pds = [b for b, _ in im._pd]
    k = bisect.bisect_right(im._pds, va) - 1
    if k >= 0 and im._pd[k][0] <= va < im._pd[k][1]:
        return im._pd[k][0]
    return None


def callers(im, target):
    """Function starts of every direct call/jmp to target."""
    import numpy as np
    out = []
    for sva, vsz, op, t in rel32_branches(im.mem, im.base, im.sections, tail=5):
        hit = np.nonzero(((op == 0xE8) | (op == 0xE9)) & (t == target))[0]
        out += [im.base + sva + int(i) for i in hit]
    return out


def entry_runs(im, min_run=3):
    """Runs of registration entries, in any layout of 2 to 4 qwords holding a handler (an
    address in code) and an id (a qword < 0x10000, mostly distinct over the run): IW5/IW6
    {id, handler, type}, S1 {handler, id}... Without any, the IW4 shape {handler, type <= 1}.
    [(rva, stride, count, shape, handler offset)]"""
    import numpy as np
    code = [(im.base + s[1], im.base + s[1] + s[2]) for s in im.sections if s[3] & 0x20000000]
    out = []
    plain = []
    for name, sva, vsz, ch in im.sections:
        if ch & 0x20000000 or name in (".pdata", ".rsrc", ".reloc") or not vsz:
            continue
        n8 = vsz // 8
        q = np.frombuffer(im.mem, dtype="<u8", count=n8, offset=sva)
        isc = np.zeros(n8, bool)
        for a, b in code:
            isc |= (q >= a) & (q < b)
        small = q < 0x10000
        lo = q & np.uint64(0xFFFFFFFF)
        hi = q >> np.uint64(32)
        typ_ok = (lo <= 1) & (hi == 0)
        for k in (2, 3, 4):
            for ph in range(k):
                m = (n8 - ph) // k
                if m < min_run:
                    continue
                for hq in range(k):
                    for iq in range(k):
                        if iq == hq:
                            continue
                        hs = isc[ph + hq:ph + hq + k * m:k][:m]
                        ids = small[ph + iq:ph + iq + k * m:k][:m]
                        mm = min(len(hs), len(ids))
                        ok = hs[:mm] & ids[:mm]
                        d = np.diff(np.concatenate(([0], ok.astype(np.int8), [0])))
                        for a, b in zip(np.nonzero(d == 1)[0], np.nonzero(d == -1)[0]):
                            if b - a < min_run:
                                continue
                            vals = q[ph + iq + k * a:ph + iq + k * b:k]
                            if len(set(vals.tolist())) < 0.8 * (b - a) or not vals.any():
                                continue
                            out.append((sva + 8 * (ph + k * int(a)), 8 * k, int(b - a), "id", 8 * hq, 8 * iq))
                if k == 2:
                    for hq in range(2):
                        hs = isc[ph + hq:ph + hq + 2 * m:2][:m]
                        ty = typ_ok[ph + 1 - hq:ph + 1 - hq + 2 * m:2][:m]
                        mm = min(len(hs), len(ty))
                        ok = hs[:mm] & ty[:mm]
                        d = np.diff(np.concatenate(([0], ok.astype(np.int8), [0])))
                        for a, b in zip(np.nonzero(d == 1)[0], np.nonzero(d == -1)[0]):
                            if b - a >= min_run and hq == 0:
                                plain.append((sva + 8 * (ph + 2 * int(a)), 16, int(b - a), "plain", 0, 8))
    # one run per start: keep the smallest stride
    best = {}
    for r in out:
        key = r[0] + r[4]
        if key not in best or r[1] < best[key][1]:
            best[key] = r
    return sorted(best.values()) + plain


def find_registrar(im, log=print, shape=None):
    """The registrar: the most common call target inside the loops that load a table address."""
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    er = entry_runs(im)
    shapes = collections.Counter(r[3] for r in er)
    shape = shape or ("id" if shapes["id"] else "plain")
    # the plain shape matches many unrelated pointer tables: keep it only when no id table exists
    runs = [(r, s, n, ho) for r, s, n, sh, ho, _io in er if sh == shape]
    log("registration entry runs: %d of shape %s (%s)" % (len(runs), shape, dict(shapes)))
    votes = collections.Counter()
    loops = []
    ranges = [(im.base + rva - 16, im.base + rva + stride + 8) for rva, stride, n, ho in runs]
    index = riprel_index(im, ranges)
    for rva, stride, n, ho in runs:
        va = im.base + rva
        for k in range(-16, stride + 8, 8):
            for r in index.get(va + k, []):
                f = pdata_start(im, r)
                if f is None:
                    continue
                # the direct calls of the function holding the reference, from the reference on
                end = next(e for b, e in im._pd if b == f)
                targets = set()
                for o in range(r - im.base, min(end - im.base, r - im.base + 0x80) - 4):
                    if im.mem[o] == 0xE8:
                        t = im.base + o + 5 + struct.unpack_from("<i", im.mem, o + 1)[0]
                        if im.is_code(t):
                            targets.add(t)
                for t in targets:
                    votes[t] += 1
                loops.append((f, va + k, stride, n))
    if not votes:
        return None, loops
    # decide by running the loops: the registrar is the candidate whose calls receive the
    # tables' handler pointers as an argument
    handlers = set()
    for rva, stride, n, ho in runs:
        for i in range(n):
            handlers.add(im.u64(im.base + rva + stride * i + ho))
    cands = set(t for t, _ in votes.most_common(12))
    hits = collections.Counter()
    emu = Emu(im)
    emu.install_import_stubs()
    uc = emu.uc

    def on_block(uc_, addr, size, data):
        if addr in cands:
            args = [uc_.reg_read(r) for r in (X.UC_X86_REG_RCX, X.UC_X86_REG_RDX,
                                             X.UC_X86_REG_R8, X.UC_X86_REG_R9)]
            if any(a in handlers for a in args):
                hits[addr] += 1
            emu.do_return(0)
    h = uc.hook_add(unicorn.UC_HOOK_BLOCK, on_block)
    uc.ctl_flush_tb()
    for f in sorted(set(lp[0] for lp in loops)):
        emu.restore()
        try:
            emu.call(f, (0, 0, 0, 0), count=500000)
        except unicorn.UcError:
            pass
    uc.hook_del(h)
    uc.ctl_flush_tb()
    if not hits:
        return None, loops
    reg, nv = hits.most_common(1)[0]
    log("registrar 0x%x (receives a table handler in %d calls from the %d loop functions; "
        "next %s)" % (reg, nv, len(set(lp[0] for lp in loops)),
                      [(hex(a), b) for a, b in hits.most_common(3)[1:]]))
    return reg, loops


def record_calls(im, emu, reg, log=print):
    """Emulate every caller of the registrar with it hooked; [(caller, rcx, rdx, r8, r9)]."""
    uc = emu.uc
    calls = []
    cur = [None]

    def on_reg(uc_, addr, size, data):
        calls.append((cur[0], uc_.reg_read(X.UC_X86_REG_RCX), uc_.reg_read(X.UC_X86_REG_RDX),
                      uc_.reg_read(X.UC_X86_REG_R8), uc_.reg_read(X.UC_X86_REG_R9)))
        emu.do_return(0)
    h = uc.hook_add(unicorn.UC_HOOK_CODE, on_reg, begin=reg, end=reg)
    uc.ctl_flush_tb()
    sites = sorted(set(callers(im, reg)))
    fns = sorted(set(f for f in (pdata_start(im, s) for s in sites) if f))
    stops = collections.Counter()
    for f in fns:
        cur[0] = f
        emu.restore()
        n0 = len(calls)
        try:
            emu.call(f, (0, 0, 0, 0), count=2000000)
            st = "returned" if emu.returned else "limit"
        except unicorn.UcError as e:
            st = str(e).split(" (")[0]
        stops[st] += 1
        if st != "returned":
            log("  caller 0x%x: %s after %d registrations" % (f, st, len(calls) - n0))
    uc.hook_del(h)
    uc.ctl_flush_tb()
    log("registrar callers: %d call sites in %d functions (%s); %d registrations" % (
        len(sites), len(fns), dict(stops), len(calls)))
    return calls, fns


def classify_args(im, calls):
    """Which argument is the handler, the id, the type: by value shape over all calls."""
    names = ["rcx", "rdx", "r8", "r9"]
    code = [sum(1 for c in calls if im.is_code(c[1 + i])) for i in range(4)]
    h = max(range(4), key=lambda i: code[i])
    rest = [i for i in range(4) if i != h]
    small = {i: [c[1 + i] & 0xFFFFFFFF for c in calls] for i in rest}
    typ = [i for i in rest if set(small[i]) <= {0, 1, 2, 3}]
    ids = sorted((i for i in rest if i not in typ and len(set(small[i])) > max(8, len(calls) // 8)),
                 key=lambda i: -len(set(small[i])))
    return names[h], (names[ids[0]] if ids else None), (names[typ[0]] if typ else None)


def roles(im, emu, reg, calls, hi, ii, ti):
    """Run the registrar on each recorded call; the array its store lands in."""
    uc = emu.uc
    idx = {"rcx": 1, "rdx": 2, "r8": 3, "r9": 4}
    writes = []

    def on_w(uc_, access, addr, size, value, data):
        writes.append((addr, value & 0xFFFFFFFFFFFFFFFF))
    h = uc.hook_add(unicorn.UC_HOOK_MEM_WRITE, on_w)
    uc.ctl_flush_tb()
    out = []
    for c in calls:
        writes.clear()
        emu.restore()
        try:
            emu.call(reg, tuple(c[1:5]), count=100000)
        except unicorn.UcError:
            pass
        hv = c[idx[hi]]
        slot = [a for a, v in writes if v == hv and im.in_image(a)]
        out.append(slot[0] if slot else None)
    uc.hook_del(h)
    uc.ctl_flush_tb()
    return out


def extract(im, log=print):
    emu = Emu(im)
    emu.install_import_stubs()
    reg, loops = find_registrar(im, log)
    if reg is None:
        raise SystemExit("no registration loop found")
    calls, fns = record_calls(im, emu, reg, log)
    hi, ii, ti = classify_args(im, calls)
    log("registrar arguments: handler %s, id %s, type %s" % (hi, ii, ti))
    slots = roles(im, emu, reg, calls, hi, ii, ti)
    idx = {"rcx": 1, "rdx": 2, "r8": 3, "r9": 4}
    ents = []
    for c, s in zip(calls, slots):
        ents.append({"caller": c[0], "handler": c[idx[hi]],
                     "id": (c[idx[ii]] & 0xFFFFFFFF) if ii else None,
                     "type": (c[idx[ti]] & 0xFFFFFFFF) if ti else None, "slot": s})
    # arrays: cluster the slots by gaps
    ss = sorted(set(e["slot"] for e in ents if e["slot"]))
    arrays = []
    for s in ss:
        if arrays and s - arrays[-1][-1] <= 0x40:
            arrays[-1].append(s)
        else:
            arrays.append([s])
    log("registrar stores into %d arrays: %s" % (len(arrays), ", ".join(
        "0x%x..0x%x (%d)" % (a[0], a[-1], len(a)) for a in arrays)))
    for e in ents:
        e["array"] = next((k for k, a in enumerate(arrays) if e["slot"] and a[0] <= e["slot"] <= a[-1]), None)
    return {"registrar": reg, "callers": fns, "args": (hi, ii, ti), "arrays": [(a[0], a[-1], len(a)) for a in arrays],
            "entries": ents, "loops": loops}
