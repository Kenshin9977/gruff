"""Builtin registration of IW8 (and its successors): a registrar R(vm, ?, ?, desc) walks one
table descriptor {entries, count} of {u64 id, handler, u64 type} entries and stores each
handler in the VM context's function array (ids from vm.base_f) or method array; the context's
bases, counts and array pointers are set by a small init. The engine calls R once per
descriptor, in a chain.

1. Descriptors: {pointer to an id-entry run, count <= the run's length} in data.
2. R: the call that follows the most `lea r9, [descriptor]` sites.
3. The init: the function that stores immediates into the context fields R reads.
4. Emulation: a heap context, the init run on it, then R run on every descriptor with a write
   hook: each store of an entry's handler gives its array (function or method) and slot.
"""
import collections
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from image import user_site  # noqa: E402
from iwemu import Emu  # noqa: E402
from idreg import entry_runs, pdata_start  # noqa: E402
from xdis import riprel_index  # noqa: E402
from common.image import rel32_branches  # noqa: E402

user_site()
import capstone  # noqa: E402
import unicorn  # noqa: E402


def descriptors(im, runs):
    """{desc va: (entries va, count)} for qword pairs pointing at an id-entry run start."""
    starts = {}
    for rva, stride, n, shape, ho, _io in runs:
        if shape != "id" or stride != 24:
            continue
        # entry start: the id field precedes the handler
        st = im.base + rva + ho - 8
        starts[st] = max(starts.get(st, 0), n)
    return entry_count_pairs(im, starts)


def entry_count_pairs(im, starts):
    """{va: (entries va, count)}: aligned qword pairs in data whose first is a key of `starts`
    (entry run start -> its length) and whose second a count up to that length + 2."""
    import numpy as np
    out = {}
    for name, sva, vsz, ch in im.sections:
        if ch & 0x20000000 or name in (".pdata", ".rsrc", ".reloc"):
            continue
        q = np.frombuffer(im.mem, dtype="<u8", count=vsz // 8, offset=sva)
        hit = np.nonzero(np.isin(q, np.array(sorted(starts), dtype=np.uint64)))[0]
        for i in hit:
            if i + 1 >= len(q):
                continue
            p, n = int(q[i]), int(q[i + 1])
            if 0 < n <= starts[p] + 2:
                out[im.base + sva + 8 * int(i)] = (p, n)
    return out


def hash_runs(im, min_run=3):
    """JUP entries: {u64 name hash, u64 handler, u8 type, u8 min, u8 max, 5 bytes 0} (24 bytes).
    [(entries va, count)]"""
    import numpy as np
    code = [(im.base + s[1], im.base + s[1] + s[2]) for s in im.sections if s[3] & 0x20000000]
    out = []
    for name, sva, vsz, ch in im.sections:
        if ch & 0x20000000 or name in (".pdata", ".rsrc", ".reloc") or not vsz:
            continue
        n8 = vsz // 8
        q = np.frombuffer(im.mem, dtype="<u8", count=n8, offset=sva)
        isc = np.zeros(n8, bool)
        for a, b in code:
            isc |= (q >= a) & (q < b)
        mn = (q >> np.uint64(8)) & np.uint64(0xFF)
        mx = (q >> np.uint64(16)) & np.uint64(0xFF)
        packed = (q < 0x1000000) & (mn <= mx) & ((q & np.uint64(0xFF)) < 8)
        big = q > 0xFFFFFFFF
        for ph in range(3):
            m = (n8 - ph) // 3 - 1
            if m < min_run:
                continue
            ok = (big[ph:ph + 3 * m:3] & isc[ph + 1:ph + 1 + 3 * m:3] & packed[ph + 2:ph + 2 + 3 * m:3])
            d = np.diff(np.concatenate(([0], ok.astype(np.int8), [0])))
            for a, b in zip(np.nonzero(d == 1)[0], np.nonzero(d == -1)[0]):
                if b - a >= min_run:
                    out.append((im.base + sva + 8 * (ph + 3 * int(a)), int(b - a)))
    return out


def method_list_base(im, chain):
    """The method adder appends at slot `count + K` of the context (`add rdx, K` before the
    store): K * 16 is where the method list starts. Read off the adders the chain calls."""
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    o = chain - im.base
    targets = []
    for ins in md.disasm(im.mem[o:o + 0x600], chain):
        if ins.mnemonic == "call" and ins.op_str.startswith("0x"):
            targets.append(int(ins.op_str, 16))
        if ins.mnemonic == "ret":
            break
    bases = []
    for t in set(targets):
        p = t - im.base
        for ins in md.disasm(im.mem[p:p + 0x40], t):
            if ins.mnemonic == "add" and ins.op_str.startswith("rdx, 0x"):
                k = int(ins.op_str.split(", ")[1], 16)
                if 0x100 <= k <= 0x10000:
                    bases.append(k * 16)
            if ins.mnemonic == "ret":
                break
    return min(bases) if bases else None


def extract_hash(im, log=print):
    """JUP: chain functions call init(ctx) then add(ctx, &descriptor copy) once per table; two
    adders (two lists in the VM context: the function list and the method list), each
    appending {hash, &entry}. Emulated with a heap context and a write hook: every entry
    pointer stored names its entry, the chain it came from names its list."""
    runs = hash_runs(im)
    starts = {}
    for p, n in runs:
        starts[p] = max(starts.get(p, 0), n)
    desc = entry_count_pairs(im, starts)
    log("hash-entry runs %d; descriptors {entries, count} %d (%d entries)" % (
        len(runs), len(desc), sum(n for _, n in desc.values())))
    idx = riprel_index(im, [(d, d + 1) for d in desc])
    chains = collections.defaultdict(set)
    for d, refs in idx.items():
        for r in refs:
            f = pdata_start(im, r)
            if f:
                chains[f].add(d)
    log("chain functions (load descriptors): %s" % {hex(f): len(v) for f, v in chains.items()})
    emu = Emu(im)
    emu.install_import_stubs()
    uc = emu.uc
    entry_set = set()
    for d, (p, n) in desc.items():
        for i in range(n):
            entry_set.add(p + 24 * i)
    ents = {}
    writes = []

    def on_w(uc_, access, addr, size, value, data):
        if size == 8 and (value & 0xFFFFFFFFFFFFFFFF) in entry_set:
            writes.append((value & 0xFFFFFFFFFFFFFFFF, addr))
    h = uc.hook_add(unicorn.UC_HOOK_MEM_WRITE, on_w)
    uc.ctl_flush_tb()
    order = []
    for f in sorted(chains):
        writes.clear()
        emu.restore()
        ctx = emu.alloc(0x40000, 0x1000)
        try:
            emu.call(f, (ctx, 0, 0, 0), count=2000000)
            st = "returned" if emu.returned else "limit"
        except unicorn.UcError as e:
            st = str(e).split(" (")[0]
        # the lists: the stores' offsets in the context, split at the largest gap
        writes[:] = [(e, a) for e, a in writes if ctx <= a < ctx + 0x40000]
        # the lists: each adder appends at its own base, one 16-byte slot per entry; the
        # function list is the contiguous run of slots from the context's start, the method
        # list the slots past its end
        offs = sorted(set(a - ctx for _, a in writes))
        cut = method_list_base(im, f)
        if cut is None and offs:
            # (a slot or two missing inside a list is an entry of a table the scan did not
            # see; the method list starts past the first gap of more than 16 slots)
            k = 0
            while k + 1 < len(offs) and offs[k + 1] - offs[k] <= 0x100:
                k += 1
            cut = offs[k] + 1 if k + 1 < len(offs) else None
        got = {}
        for e, a in writes:
            got.setdefault(e, 0 if cut is None or a - ctx < cut else 1)
        log("  chain 0x%x: %s, %d entries stored (list split at context +0x%x: %d / %d)" % (
            f, st, len(got), cut or 0, sum(1 for v in got.values() if v == 0),
            sum(1 for v in got.values() if v == 1)))
        order.append((f, got, cut))
    uc.hook_del(h)
    uc.ctl_flush_tb()
    out = []
    for ci, (f, got, cut) in enumerate(order):
        for e, lst in got.items():
            q2 = im.u64(e + 16)
            out.append({"chain": ci, "chain_fn": f, "list": lst, "list_cut": cut, "entry": e,
                        "hash": im.u64(e),
                        "handler": im.u64(e + 8), "type": q2 & 0xFF, "min": (q2 >> 8) & 0xFF,
                        "max": (q2 >> 16) & 0xFF})
    return {"chains": [hex(f) for f, _, _ in order], "descriptors": len(desc), "entries": out}


def extract(im, log=print):
    runs = entry_runs(im)
    desc = descriptors(im, runs)
    log("descriptors {entries, count} pointing at id-entry runs: %d (%d entries)" % (
        len(desc), sum(n for _, n in desc.values())))
    if not desc:
        raise SystemExit("no descriptor")
    idx = riprel_index(im, [(d, d + 1) for d in desc])
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    votes = collections.Counter()
    sites = []
    for d, refs in idx.items():
        for r in refs:
            o = r - im.base
            for k in range(o, o + 0x30):
                if im.mem[k] == 0xE8:
                    t = im.base + k + 5 + struct.unpack_from("<i", im.mem, k + 1)[0]
                    votes[t] += 1
                    sites.append((r, d, t))
                    break
    reg, nv = votes.most_common(1)[0]
    log("registrar 0x%x: the call after %d of the %d descriptor loads" % (reg, nv, sum(votes.values())))
    # the context fields R reads: [reg + disp] with disp >= 0x100
    fields = set()
    o = reg - im.base
    for ins in md.disasm(im.mem[o:o + 0x100], reg):
        s = ins.op_str
        if "[" in s and "+ 0x" in s and "rip" not in s:
            try:
                disp = int(s[s.index("+ 0x") + 2:s.index("]")], 16)
            except ValueError:
                continue
            if disp >= 0x100:
                fields.add(disp)
        if ins.mnemonic == "ret":
            break
    log("registrar reads context fields %s" % sorted(hex(f) for f in fields))
    # the init: code storing immediates/pointers into most of those fields, relative to rcx
    stores = []
    for f in fields:
        for pat in (b"\xc7\x81" + struct.pack("<I", f), b"\x48\x89\x81" + struct.pack("<I", f)):
            i = im.mem.find(pat)
            while i >= 0:
                if im.is_code(im.base + i):
                    stores.append((im.base + i, f))
                i = im.mem.find(pat, i + 1)
    stores.sort()
    # the init: a window of 0x80 bytes storing the most distinct fields
    best = (0, None)
    for k, (a, f) in enumerate(stores):
        win = {ff for aa, ff in stores[k:] if aa - a < 0x80}
        if len(win) > best[0]:
            best = (len(win), a)
    ni, first = best
    # its start: the nearest direct call/jmp target at or before the first store
    tg = set()
    for sva, vsz, op, t in rel32_branches(im.mem, im.base, im.sections, tail=5):
        hit = ((op == 0xE8) | (op == 0xE9)) & (t <= first) & (t > first - 0x100)
        tg.update(int(x) for x in t[hit])
    init = max(tg) if tg else first
    log("context init 0x%x (stores %d of the fields within 0x80 bytes from 0x%x)" % (init, ni, first))
    emu = Emu(im)
    emu.install_import_stubs()
    uc = emu.uc
    ctx = emu.alloc(0x10000, 0x1000)
    emu.call(init, (ctx,), count=10000)
    fvals = {f: emu.r32(ctx + f) for f in sorted(fields)}
    fptrs = {f: emu.rptr(ctx + f) for f in sorted(fields)}
    log("context after init: %s" % {hex(f): hex(v) for f, v in fvals.items()})
    writes = []

    def on_w(uc_, access, addr, size, value, data):
        writes.append((addr, value & 0xFFFFFFFFFFFFFFFF))
    h = uc.hook_add(unicorn.UC_HOOK_MEM_WRITE, on_w)
    uc.ctl_flush_tb()
    ents = []
    for d, (p, n) in sorted(desc.items()):
        writes.clear()
        snap = bytes(uc.mem_read(ctx, 0x2000))
        try:
            emu.call(reg, (ctx, 0, 0, d), count=50000)
        except unicorn.UcError:
            pass
        uc.mem_write(ctx, snap)
        w = {v: a for a, v in writes}
        for i in range(n):
            e = p + 24 * i
            eid, hnd, typ = im.u64(e), im.u64(e + 8), im.u64(e + 16)
            ents.append({"desc": d, "entry": e, "id": eid & 0xFFFFFFFF, "handler": hnd,
                         "type": typ & 0xFFFFFFFF, "slot": w.get(hnd)})
    uc.hook_del(h)
    uc.ctl_flush_tb()
    arrays = collections.Counter(e["slot"] - 8 * e["id"] for e in ents if e["slot"])
    log("stores: %d of %d entries; arrays (slot - 8*id): %s" % (
        sum(1 for e in ents if e["slot"]), len(ents),
        ", ".join("0x%x (%d)" % (b, c) for b, c in arrays.most_common(3))))
    return {"registrar": reg, "init": init, "context": {hex(f): hex(v) for f, v in fvals.items()},
            "descriptors": {hex(d): (hex(p), n) for d, (p, n) in desc.items()},
            "entries": ents, "array_bases": [b for b, _ in arrays.most_common(2)]}
