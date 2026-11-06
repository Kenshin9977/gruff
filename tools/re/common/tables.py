"""Builtin tables: the entry layout, the static scan, a lookup's reads, the lookup round trip and
the static loop bound. Engine-neutral: the entry layout and the lookup's calling convention come
from the engine profile.

An `Layout` names the fields of one table entry: hash (u32 or u64), min, max, handler, type and
the bytes that must be zero. `decode` returns a dict with those keys; `shaped` says whether the
bytes look like an entry (hash set, handler in code, min <= max or the -1/-1 marker).
"""
import collections
import re
import struct

from .emu import unicorn

CMP_IMM = re.compile(r"^(e\w\w|r\d+d|r\w\w|r\d+), (0x[0-9a-f]+|\d+)$")


class Layout:
    """fields: {name: (offset, struct code)} with names hash, min, max, handler, type (optional);
    zero: offsets of bytes that are always zero (padding), as (offset, size) pairs."""

    def __init__(self, size, fields, zero=(), text=""):
        self.size = size
        self.fields = fields
        self.zero = zero
        self.text = text
        self.hash_size = struct.calcsize(fields["hash"][1])

    def decode(self, raw):
        e = {k: struct.unpack_from("<" + c, raw, o)[0] for k, (o, c) in self.fields.items()}
        e["zero_ok"] = all(not any(raw[o:o + n]) for o, n in self.zero)
        return e

    def shaped(self, img, e):
        mn, mx = e["min"], e["max"]
        arity = mn <= mx or (mn == mx)
        return bool(e["hash"]) and e["zero_ok"] and img.is_code(e["handler"]) and arity

    def read(self, img, va, mem=None):
        raw = (mem(va, self.size) if mem else img.d[va - img.base:va - img.base + self.size])
        return self.decode(bytes(raw))


# T7, T8 and T9: {u32 hash, u32 min, u32 max, u32 0, u64 handler, u32 type, u32 0}
LAYOUT_T7 = Layout(0x20, {"hash": (0, "I"), "min": (4, "I"), "max": (8, "I"),
                          "handler": (0x10, "Q"), "type": (0x18, "I")},
                   zero=((0xC, 4), (0x1C, 4)),
                   text="u32 canon, u32 min_args, u32 max_args, u32 pad, u64 handler, u32 type, "
                        "u32 pad (0x20 bytes)")


def scan_static(img, layout, min_run=2, sections=(".rdata", ".data")):
    """Runs of consecutive shaped entries in the image's data, at the layout's stride, aligned on
    8: [(va of the first entry, [entries])]. A plain scan: for engines whose tables are static."""
    runs = []
    for name, sva, vsz, ch in img.sections:
        if name not in sections:
            continue
        lo, hi = img.base + sva, img.base + sva + vsz - layout.size
        va = lo
        while va <= hi:
            ents = []
            v = va
            while v <= hi:
                e = layout.read(img, v)
                if not layout.shaped(img, e):
                    break
                ents.append(e)
                v += layout.size
            if len(ents) >= min_run:
                runs.append((va, ents))
                va = v
            else:
                va += 8
    return runs


def trace_lookup(emu, va, layout, args, written=None, strict=True):
    """Run a lookup with an absent key (`args`, built by the engine profile): it scans every
    table it owns, so its reads of the hash field at the layout's stride give each table's
    address and entry count, in lookup order. A run of reads that is not entry-shaped stops
    (strict), or, with `written` given, a table no initialiser wrote."""
    img = emu.img
    reads = []
    hs = layout.hash_size

    def hr(uc, acc, addr, size, val, ud):
        if size == hs and img.base <= addr < img.base + img.size:
            reads.append(addr)

    h = emu.uc.hook_add(unicorn.UC_HOOK_MEM_READ, hr)
    r, err = emu.run(va, args)
    emu.uc.hook_del(h)
    if err or r:
        raise SystemExit("lookup %#x with an absent key: %s %s" % (va, err, r))
    order, seen = [], set()
    for a in reads:
        if a not in seen:
            seen.add(a)
            order.append(a)
    st = layout.size
    tables = []
    for a in order:
        if a - st in seen:
            continue
        n = 1
        while a + st * n in seen:
            n += 1
        ents = [layout.read(img, a + st * k, lambda v, n_: emu.uc.mem_read(v, n_))
                for k in range(n)]
        shaped = all(layout.shaped(img, e) for e in ents)
        if not shaped:
            if n > 1 and strict:
                raise SystemExit("lookup %#x reads a %d-entry run at %#x that is not a builtin "
                                 "table (or its init did not run)" % (va, n, a))
            continue  # a scalar the lookup reads, not a table
        if written is not None and not all((a + st * k) in written for k in range(n)):
            raise SystemExit("table %#x: entries not written by any initialiser" % a)
        tables.append((a, ents))
    return tables


def static_bounds(img, tables):
    """Second measure of a table's size: the `cmp reg, N` bound of each scan loop whose base is
    `lea [rip+table]` (decoded from the lea forward, up to the first ret/jmp). {table va: {N}}."""
    x = img.riprel_xrefs(set(tables))
    out = {}
    for t in tables:
        bounds = set()
        for v in x[t]:
            o = v - 3 - img.base
            ins0 = next(img.md.disasm(img.d[o:o + 16], img.base + o), None)
            if not ins0 or ins0.mnemonic != "lea":
                continue
            for ins in img.md.disasm(img.d[o:o + 64], img.base + o):
                if ins.mnemonic == "cmp" and CMP_IMM.match(ins.op_str):
                    bounds.add(int(ins.op_str.split(", ")[1], 0))
                    break
                if ins.mnemonic in ("ret", "jmp"):
                    break
        out[t] = bounds
    return out


def fnv_initialisers(img, emu, pattern, log, count=5_000_000):
    """Emulate every .pdata function containing `pattern` (the static initialisers that hash the
    names: T7's FNV multiply) once, recording every write into the image. {slot (8-aligned):
    function}."""
    cnt = collections.Counter()
    for m in pattern.finditer(img.d, 0, img.texts[0][1] - img.base):
        f = img.func(img.base + m.start())
        if f:
            cnt[f[0]] += 1
    written = {}
    cur = [0]

    def hw(uc, acc, addr, size, val, ud):
        if img.base <= addr < img.base + img.size:
            written[addr & ~7] = cur[0]

    h = emu.uc.hook_add(unicorn.UC_HOOK_MEM_WRITE, hw)
    ok = 0
    for f in sorted(cnt):
        cur[0] = f
        r, err = emu.run(f, count=count)
        ok += err is None
    emu.uc.hook_del(h)
    log.append("init: %d functions match the initialiser pattern, %d ran to their return, %d "
               "slots written" % (len(cnt), ok, len(written)))
    return written
