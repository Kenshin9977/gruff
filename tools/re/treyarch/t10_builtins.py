"""Extract the builtin catalogues of Black Ops 6 (T10) and Black Ops 7 (T11) from the decrypted,
flattened game executables, statically and under Unicorn. The games are never run.

    py -3.11 -I treyarch/t10_builtins.py extract --game t11      (or t10, t10sp)

What the engine looks like (both games; see README_t10.md):

1. The builtin tables are static: 0x18-byte entries {u64 hash, u64 handler, u8 type, u8 min,
   u8 max, 5 x 0} (BO6 MP stores min/max too, its linker does not check them). Each table is
   handed out as a span {ptr, count}: by a tiny getter (`lea rax, table; mov [rcx], rax;
   mov qword [rcx+8], N; ret`) or a span in .data (BO6 SP).
2. At init, "builtin set" objects are built: a builder clears the object and appends spans with
   one of two add functions (functions, methods) into two arrays {u64 hash, entry*} (counts at
   +C_f / +C_m), then sorts them (qsort import). There is one set per (VM, game mode): the VM
   struct holds sets[mode] (`vm->builtins = sets[GetMode()]`, read off the code).
3. The linker resolves an import with one of two lookups: bsearch over the function array
   (returns index + 1) or the method array (returns (index + 1) | 0x8000). It checks
   type == 1 (the "Dev only calls must be wrapped in a devblock" error) and, in BO7, the
   argument count against min..max ("Invalid number of params for call").

Measures: the builders are emulated (spans appended, objects sorted) and the result compared with
the live objects of the dump (counts, every (hash, entry) pair); each lookup is emulated with an
absent key (its bsearch arguments name the array and count it searches); every hash goes through
the emulated lookup again (round trip).
"""
import argparse
import collections
import json
import os
import re
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from common.image import Image  # noqa: E402
from common.emu import Emu, unicorn  # noqa: E402
from common import tables as T  # noqa: E402
from common import paths  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(os.path.dirname(HERE), "out")

LAYOUT = T.Layout(0x18, {"hash": (0, "Q"), "handler": (8, "Q"), "type": (0x10, "B"),
                         "min": (0x11, "B"), "max": (0x12, "B")}, zero=((0x13, 5),),
                  text="u64 hash (full 64-bit salted FNV-1a), u64 handler, u8 type (1: dev "
                       "only), u8 min_args, u8 max_args, 5 bytes 0 (0x18 bytes)")

LINKER_STR = 'ERROR:  **** Unresolved external "%s" with %d parameters in "%s" ****\n'
DEV_STR = 'ERROR:  **** Dev only calls must be wrapped in a devblock /#...#/ : "%s" in "%s"\n'
ARITY_STR = ('ERROR:  Invalid number of params for call (%d where valid range is (%d, %d): '
             '"%s" in "%s")\n')

# Engine anchors. Each is checked at run time (the check names what fails).
PROFILES = {
    "t11": {
        "label": "Black Ops 7 (cod.exe, Cordycep dump)",
        # {char *name[n_vm]; vm *ptr[n_vm]}: "server", "client"
        "vm_names": 0xdb17d40, "n_vm": 2,
        # GetMode(): byte at +0xbc of the current mode's info; the names table it indexes
        "mode_fn": 0xa03cd60, "mode_names": 0xda90890,
    },
    "t10": {
        "label": "Black Ops 6 multiplayer (cod24-cod.exe, "
                                                   "Cordycep dump)",
        "vm_names": 0xb27d678, "n_vm": 1,
        # GetMode(): byte at +0 of modeinfo[current] (stride 0x1a0), name prefix at +8
        "mode_fn": 0x7da1120, "mode_info": (0xb20e468, 0x1a0, 8),
    },
    "t10sp": {
        "label": "Black Ops 6 campaign (sp24-cod.exe, "
                                                      "Cordycep dump)",
        "vm_names": None,
    },
}


# ---------------------------------------------------------------------------------------------
def fail(msg):
    raise SystemExit("t10_builtins: " + msg)


def raw_str(img, va, n=300):
    """The bytes at va up to NUL, as latin-1 (error messages end with a newline)."""
    o = va - img.base
    return img.d[o:o + n].split(b"\0")[0].decode("latin1")


def target(ins):
    return int(ins.op_str, 16) if ins.op_str.startswith("0x") else None


def find_linker(img, log):
    refs = []
    for s in img.find_cstr(LINKER_STR):
        refs += img.code_refs(s)
    if len(refs) != 1:
        fail("linker: %d code refs to the unresolved-external error" % len(refs))
    head = img.head_of(refs[0])
    log.append("linker %#x (loads the unresolved-external error at %#x)"
               % (head - img.base, refs[0] - img.base))
    return head


def linker_code(img, ref, span=0x800):
    """The linker's instructions: .pdata splits it in chunks (and the error path that loads the
    string may sit before or after the lookups), so every chunk within `span` of the string's
    load is decoded from its own start."""
    out = []
    for b, e in img.pd:
        if b < ref - img.base + span and e > ref - img.base - span:
            out += img.disasm(img.base + b, e - b)
    return out


def find_lookups(img, linker, log):
    """The two lookups: the calls of the linker whose 16-bit result is tested (`movzx r8d, ax`).
    Each one's body gives its sorted flag, count and array offsets and whether it tags the id
    with 0x8000 (the method id space)."""
    ins = linker_code(img, linker)
    found = []
    for k, i in enumerate(ins[:-1]):
        if i.mnemonic == "call" and target(i) and ins[k + 1].mnemonic == "movzx" \
                and ins[k + 1].op_str.endswith(", ax"):
            found.append(target(i))
    found = list(dict.fromkeys(found))
    if len(found) != 2:
        fail("linker: %d lookups (calls with a 16-bit result), expected 2" % len(found))
    res = []
    for lk in found:
        body = img.body(lk)
        # `or ax, 0x8000` (or the constant through a register): the method id space; the round
        # trip checks every returned id carries it
        d = {"va": lk, "method": any(i.mnemonic == "or" and i.op_str.startswith("ax, ")
                                     for i in body) and any("0x8000" in i.op_str for i in body)}
        imports = [i for i in body if i.mnemonic == "call" and "qword ptr [rip" in i.op_str]
        if len(imports) != 2:
            fail("lookup %#x: %d import calls, expected qsort and bsearch" % (lk, len(imports)))
        d["qsort_slot"], d["bsearch_slot"] = (img.operand_addr(x) for x in imports)
        for i in body:
            m = re.match(r"\w+, dword ptr \[r(?:bx|cx) \+ (0x[0-9a-f]+)\]$", i.op_str)
            if i.mnemonic == "mov" and m and i.address > imports[0].address:
                d["count_off"] = int(m.group(1), 16)
                break
        m = [re.match(r"rdx, \[rbx \+ (0x[0-9a-f]+)\]$", i.op_str) for i in body
             if i.mnemonic == "lea"]
        m = [x for x in m if x]
        d["array_off"] = int(m[0].group(1), 16) if m else 0
        res.append(d)
        log.append("lookup %#x: %s, array +%#x, count +%#x, ids %s" % (
            lk - img.base, "method" if d["method"] else "function", d["array_off"],
            d["count_off"], "| 0x8000" if d["method"] else "plain"))
    if sorted(r["method"] for r in res) != [False, True]:
        fail("lookups: expected one function and one method lookup")
    return {("method" if r["method"] else "function"): r for r in res}


def linker_checks(img, linker):
    """What the linker checks on the resolved entry: dev flag (type == 1 then the devblock
    error) and, where present, min/max (the invalid-number-of-params error)."""
    ins = linker_code(img, linker)
    strs = {raw_str(img, a) for a in (img.operand_addr(i) for i in ins if i.mnemonic == "lea")
            if a is not None and img.in_image(a)}
    body = " ; ".join("%s %s" % (i.mnemonic, i.op_str) for i in ins)
    dev = DEV_STR in strs and "byte ptr [rax + 0x10], 1" in body
    arity = ARITY_STR in strs and "byte ptr [rax + 0x11]" in body and \
        "byte ptr [rax + 0x12]" in body
    if not dev:
        fail("linker: no dev-only check on entry byte 0x10")
    return {"dev_check": dev, "arity_check": arity}


# ---------------------------------------------------------------------------------------------
# builders
def find_add_functions(img, lookups, log):
    """The two append functions: `inc dword ptr [rcx + count]` for each lookup's count field;
    the function starts at its `mov r9, qword ptr [rdx + 8]` (span count)."""
    adds = {}
    for kind, lk in lookups.items():
        pat = b"\xff\x81" + struct.pack("<I", lk["count_off"])
        hits = [h for h in img.find_all(pat, 0, img.texts[0][1] - img.base) if img.is_code(h)]
        starts = set()
        for h in hits:
            st = img.d.rfind(b"\x4c\x8b\x4a\x08", h - img.base - 0x40, h - img.base)
            if st > 0:
                starts.add(img.base + st)
        starts = [s for s in starts if img.rel32_callers({s})[s]]
        if len(starts) != 1:
            fail("append function for %s count +%#x: %d candidates" % (kind, lk["count_off"],
                                                                     len(starts)))
        adds[kind] = starts[0]
        log.append("append %s spans: %#x" % (kind, starts[0] - img.base))
    return adds


def find_builders(img, adds, log):
    """Builders: the functions calling the append functions. Object: `lea rcx, [rip+O]` before
    the append call (inline builder), else before each call/jmp to the builder."""
    callers = img.rel32_callers(set(adds.values()))
    builders = {}
    for add, cs in callers.items():
        for v, k in cs:
            if img.func(v):  # an E8 byte outside any .pdata function is not a call
                builders.setdefault(img.head_of(v), []).append(v)
    out = []
    for b, sites in sorted(builders.items()):
        objs = set()
        for v in sites:
            pre = img.disasm(v - 0x30, 0x30 + 5)
            leas = [i for i in pre if i.mnemonic == "lea" and i.op_str.startswith("rcx, [rip")
                    and i.address < v]
            if leas and pre[-1].address == v:
                objs.add(img.operand_addr(leas[-1]))
        if not objs:
            for v, k in img.rel32_callers({b})[b]:
                pre = img.disasm(v - 7, 7)
                if pre and pre[0].mnemonic == "lea" and pre[0].op_str.startswith("rcx, [rip"):
                    objs.add(img.operand_addr(pre[0]))
        if len(objs) != 1:
            fail("builder %#x: objects %s" % (b - img.base, [hex(o - img.base) for o in objs]))
        out.append((b, objs.pop()))
        log.append("builder %#x -> set object %#x (%d append calls)" % (
            b - img.base, out[-1][1] - img.base, len(sites)))
    return out


class ImportStubs:
    """qsort and bsearch are imports (their IAT slots point into ucrtbase, not in the dump): the
    slots are pointed at a stub page and the calls done in Python. qsort orders the {u64 hash,
    ptr} pairs by unsigned hash, which is what the game's comparator does (checked by
    `check_comparator`); bsearch records its arguments (the measure of the array searched)."""

    def __init__(self, emu, qsort_slot, bsearch_slot):
        self.emu = emu
        uc = emu.uc
        self.page = emu.alloc(0x1000)
        uc.mem_write(self.page, b"\xc3" * 0x20)
        emu.wptr(qsort_slot, self.page)
        emu.wptr(bsearch_slot, self.page + 0x10)
        uc.hook_add(unicorn.UC_HOOK_CODE, self._qsort, begin=self.page, end=self.page)
        uc.hook_add(unicorn.UC_HOOK_CODE, self._bsearch, begin=self.page + 0x10,
                    end=self.page + 0x10)
        self.searches = []
        self.sorts = []
        self.skipped = collections.Counter()
        uc.hook_add(unicorn.UC_HOOK_MEM_FETCH_UNMAPPED, self._other_import)

    def _other_import(self, uc, access, addr, size, value, ud):
        """Any other import (the builders' memset of the object, which is zeroed beforehand):
        return to the caller, rax = rcx. Counted, by the calling site, in the log."""
        e = self.emu
        sp = uc.reg_read(e.sp_reg)
        ret = e.r64(sp)
        if not e.img.is_code(ret):
            return False
        self.skipped[ret - e.img.base] += 1
        page = addr & ~0xFFF
        try:
            uc.mem_map(page, 0x1000)
        except unicorn.UcError:
            pass
        uc.mem_write(addr, b"\xc3")
        uc.reg_write(e.ax_reg, e.arg(0))
        return True

    def _qsort(self, uc, addr, size, ud):
        e = self.emu
        base, num, sz = e.arg(0), e.arg(1) & 0xFFFFFFFF, e.arg(2)
        raw = bytes(uc.mem_read(base, num * sz)) if num else b""
        els = sorted((raw[k * sz:(k + 1) * sz] for k in range(num)),
                     key=lambda r: struct.unpack_from("<Q", r)[0])
        if num:
            uc.mem_write(base, b"".join(els))
        self.sorts.append((base, num, sz))
        e.ret(0)

    def _bsearch(self, uc, addr, size, ud):
        e = self.emu
        key, base, num, sz = e.arg(0), e.arg(1), e.arg(2) & 0xFFFFFFFF, e.arg(3)
        k = e.r64(key)
        self.searches.append((k, base, num, sz))
        lo, hi, found = 0, num, 0
        while lo < hi:
            mid = (lo + hi) // 2
            v = e.r64(base + mid * sz)
            if v == k:
                found = base + mid * sz
                break
            if v < k:
                lo = mid + 1
            else:
                hi = mid
        e.ret(found)


def check_comparator(img, lookups):
    """The lookups' comparator (the lea r9 / [rsp+0x20] function) orders u64 keys unsigned."""
    emu = Emu(img)
    cmps = set()
    for lk in lookups.values():
        for i in img.body(lk["va"]):
            if i.mnemonic == "lea" and i.op_str.startswith(("r9, [rip", "rax, [rip")):
                cmps.add(img.operand_addr(i))
    a, b = emu.alloc(8), emu.alloc(8)
    for c in cmps:
        for x, y in ((1, 2), (2, 1), (5, 5), (1 << 63, 1), (1, 1 << 63)):
            emu.uc.mem_write(a, struct.pack("<Q", x))
            emu.uc.mem_write(b, struct.pack("<Q", y))
            r, err = emu.run(c, (a, b))
            r = struct.unpack("<i", struct.pack("<I", r & 0xFFFFFFFF))[0] if r is not None else None
            want = -1 if x < y else (1 if x > y else 0)
            if err or r != want:
                fail("comparator %#x(%#x, %#x) = %s %s" % (c - img.base, x, y, r, err))
    return sorted(c - img.base for c in cmps)


def build_sets(img, builders, adds, lookups, log, arity_checked=True):
    """Emulate each builder on a zeroed object; record the spans appended (table, count, kind)."""
    emu = Emu(img)
    lk = lookups["function"]
    stubs = ImportStubs(emu, lk["qsort_slot"], lk["bsearch_slot"])
    kind_of = {v: k for k, v in adds.items()}
    spans = []

    def on_add(uc, addr, size, ud):
        ptr, n = emu.r64(emu.arg(1)), emu.r64(emu.arg(1) + 8)
        spans.append((kind_of[addr], ptr, n, emu.arg(0)))

    for a in adds.values():
        emu.uc.hook_add(unicorn.UC_HOOK_CODE, on_add, begin=a, end=a)
    # up to the counts only: the next object can start right after the flags
    size = max(lookups["function"]["count_off"], lookups["method"]["count_off"]) + 4
    sets = {}
    for b, obj in builders:
        emu.uc.mem_write(obj, b"\0" * size)
        spans.clear()
        r, err = emu.run(b, (obj,), count=50_000_000)
        if err:
            fail("builder %#x: %s" % (b - img.base, err))
        arrays = {}
        for kind, l in lookups.items():
            n = emu.r32(obj + l["count_off"])
            raw = bytes(emu.uc.mem_read(obj + l["array_off"], 16 * n))
            arrays[kind] = [struct.unpack_from("<QQ", raw, 16 * k) for k in range(n)]
            live = [struct.unpack_from("<QQ", img.d, obj - img.base + l["array_off"] + 16 * k)
                    for k in range(img.u32(obj + l["count_off"]))]
            if sorted(live) != sorted(arrays[kind]):
                fail("set %#x %s: emulated build (%d) differs from the dump's object (%d)"
                     % (obj - img.base, kind, n, len(live)))
            if [h for h, p in arrays[kind]] != sorted(h for h, p in arrays[kind]):
                fail("set %#x %s: not sorted after the build" % (obj - img.base, kind))
        tabs = [(kind, ptr, n) for kind, ptr, n, o in spans if o == obj]
        for kind, ptr, n in tabs:
            for k in range(n):
                e = LAYOUT.read(img, ptr + 0x18 * k)
                if not (LAYOUT.shaped(img, e) if arity_checked else (
                        e["hash"] and e["zero_ok"] and img.is_code(e["handler"]))):
                    fail("table %#x entry %d is not entry-shaped" % (ptr - img.base, k))
        sets[obj] = {"builder": b, "arrays": arrays, "spans": tabs}
        log.append("set %#x: built by %#x, %d function spans (%d entries), %d method spans (%d "
                   "entries); emulated build == dump object" % (
                       obj - img.base, b - img.base,
                       sum(1 for t in tabs if t[0] == "function"), len(arrays["function"]),
                       sum(1 for t in tabs if t[0] == "method"), len(arrays["method"])))
    log.append("builders: other imports returned at once (called from): %s" % (
        ", ".join("%#x" % a for a in sorted(stubs.skipped)) or "none"))
    return emu, stubs, sets


# ---------------------------------------------------------------------------------------------
# roles: which VM and game mode each set serves
def find_select(img, prof, log):
    """`vm->builtins = vm->sets[GetMode()]`: after the call to GetMode, `mov r, [vm + rax*8 + D]`
    then `mov [vm + 8], r`. Returns D (offset of sets[] in the VM)."""
    mf = img.base + prof["mode_fn"]
    ds = set()
    for v, k in img.rel32_callers({mf})[mf]:
        ins = img.disasm(v, 0x30)
        for a, b in zip(ins, ins[1:]):
            m = re.match(r"(\w+), qword ptr \[(\w+) \+ rax\*8 \+ (0x[0-9a-f]+)\]$", a.op_str)
            if a.mnemonic == "mov" and m and b.mnemonic == "mov" and \
                    b.op_str == "qword ptr [%s + 8], %s" % (m.group(2), m.group(1)):
                ds.add(int(m.group(3), 16))
    if len(ds) != 1:
        fail("set selection after GetMode %#x: %s" % (prof["mode_fn"], ds))
    d = ds.pop()
    log.append("set selection: vm->builtins (+8) = vm->sets[GetMode()] (+%#x + 8 * mode), "
               "GetMode %#x" % (d, prof["mode_fn"]))
    return d


def mode_names(img, prof, n):
    """Names of game-mode indices 0..n-1, from the game's own tables."""
    if "mode_names" in prof:  # BO7: char *names[] indexed by the same mode byte
        f = img.disasm(img.base + prof["mode_fn"], 0x40)
        if not any(img.operand_addr(i) is not None for i in f):
            fail("GetMode %#x does not read a global" % prof["mode_fn"])
        t = img.base + prof["mode_names"]
        names = [img.cstr(img.u64(t + 8 * k)) for k in range(n)]
        if names[0] != "none" or None in names:
            fail("mode names at %#x: %s" % (prof["mode_names"], names))
        return names
    base, stride, name_off = prof["mode_info"]  # BO6: modeinfo[] {u8 set index, char *prefix}
    f = img.disasm(img.base + prof["mode_fn"], 0x20)
    if not any(i.mnemonic == "imul" and i.op_str.endswith(hex(stride)) for i in f):
        fail("GetMode %#x: no modeinfo stride %#x" % (prof["mode_fn"], stride))
    names = collections.defaultdict(list)
    for k in range(8):
        a = img.base + base + k * stride
        p = img.u64(a + name_off)
        s = img.cstr(p) if img.in_image(p) else None
        if k and not s:
            break
        nm = (s or "none").rstrip("_")
        if nm not in names[img.u8(a)]:
            names[img.u8(a)].append(nm)
    return ["/".join(names.get(k, ["?"])) for k in range(n)]


def find_roles(img, prof, sets, log):
    d = find_select(img, prof, log)
    t = img.base + prof["vm_names"]
    nv = prof["n_vm"]
    roles = {}
    nmodes = None
    for v in range(nv):
        name = img.cstr(img.u64(t + 8 * v))
        vm = img.u64(t + 8 * (nv + v))
        if name not in ("server", "client"):
            fail("vm %d name %r" % (v, name))
        objs = []
        k = 0
        while True:
            o = img.u64(vm + d + 8 * k)
            if k and o not in sets:
                break
            objs.append(o)
            k += 1
        if objs[0] != 0:
            fail("vm %s: sets[0] is not null" % name)
        nmodes = max(nmodes or 0, len(objs))
        roles[name] = (vm, objs)
    names = mode_names(img, prof, nmodes)
    res = {}
    for name, (vm, objs) in roles.items():
        for k, o in enumerate(objs):
            if o:
                res[o] = {"vm": "gsc" if name == "server" else "csc", "vm_name": name,
                          "mode": k, "mode_name": names[k], "vm_va": vm}
                log.append("set %#x: %s VM (%#x), mode %d (%s)" % (
                    o - img.base, name, vm - img.base, k, names[k]))
    if set(res) - set(sets):
        fail("a VM selects a set no builder built: %s" % sorted(
            hex(o - img.base) for o in set(res) - set(sets)))
    for o in sorted(set(sets) - set(res)):
        # built, but no VM of the dump holds it: role not proven, kept apart (not dropped)
        res[o] = dict(UNASSIGNED)
        log.append("set %#x: built, held by no VM in the dump: role not proven" % (o - img.base))
    return res, d, names


UNASSIGNED = {"vm": None, "vm_name": None, "mode": None, "mode_name": None}


# ---------------------------------------------------------------------------------------------
def trace_and_round_trip(img, emu, stubs, sets, lookups, log):
    """Lookup with an absent key: bsearch must get exactly the set's array and count. Then every
    hash of the set through the lookup: the id must name an element with that hash."""
    stats = collections.Counter()
    for obj, s in sets.items():
        for kind, lk in lookups.items():
            stubs.searches.clear()
            r, err = emu.run(lk["va"], (obj, 0))
            if err or (r or 0) & 0xFFFF:
                fail("lookup %s on %#x, absent key: %s %s" % (kind, obj - img.base, err, r))
            if len(stubs.searches) != 1:
                fail("lookup %s: %d bsearch calls" % (kind, len(stubs.searches)))
            k, base, num, sz = stubs.searches[0]
            arr = s["arrays"][kind]
            if base != obj + lk["array_off"] or num != len(arr) or sz != 16:
                fail("lookup %s on %#x searches %#x x %d (size %d), expected %#x x %d" % (
                    kind, obj - img.base, base - img.base, num, sz,
                    obj + lk["array_off"] - img.base, len(arr)))
            s.setdefault("trace", {})[kind] = {"array_rva": base - img.base, "count": num}
            ok = 0
            s.setdefault("rt", {})[kind] = rt = []
            for idx, (h, p) in enumerate(arr):
                r, err = emu.run(lk["va"], (obj, h))
                rid = (r or 0) & 0xFFFF
                got = (rid & 0x7FFF) - 1
                good = (not err and bool(rid & 0x8000) == (kind == "method") and 0 <= got < len(arr)
                        and arr[got][0] == h)
                rt.append({"id": rid, "entry": arr[got][1] if good else None})
                ok += good
                if not good:
                    print("round trip: set %#x %s hash %#x -> id %#x err %s" % (
                        obj - img.base, kind, h, rid, err), file=sys.stderr)
            stats[kind] += ok
            log.append("set %#x %s: absent-key bsearch over +%#x x %d; round trip %d/%d" % (
                obj - img.base, kind, lk["array_off"], num, ok, len(arr)))
            if ok != len(arr):
                fail("round trip failed for %d entries" % (len(arr) - ok))
    return stats


# ---------------------------------------------------------------------------------------------
# handlers: parameter accessors and the handler runs
def neutralise_imports(img, emu):
    """Point every IAT slot (data directory 12) at `mov rax, rcx; ret`: the dump's slots hold
    ucrtbase/kernel addresses that are not mapped; an import call then returns at once."""
    e = struct.unpack_from("<I", img.d, 0x3C)[0]
    iat, size = struct.unpack_from("<II", img.d, e + 24 + 112 + 8 * 12)
    stub = emu.alloc(0x10)
    emu.uc.mem_write(stub, b"\x48\x89\xc8\xc3")
    for o in range(iat, iat + size, 8):
        if img.u64(img.base + o) and not img.in_image(img.u64(img.base + o)):
            emu.wptr(img.base + o, stub)
    # pointers into system DLLs outside the IAT (thunk slots of the protected code, function
    # tables): same stub, so a call through them returns instead of running unmapped memory
    import numpy as np
    n = 0
    for name, sva, vsz, ch in img.sections:
        if name not in (".rdata", ".data", ".idata", "_RDATA"):
            continue
        q = np.frombuffer(img.d, dtype=np.uint64, count=vsz // 8, offset=sva)
        hit = np.nonzero((q >= np.uint64(0x7FF800000000)) & (q < np.uint64(0x800000000000)) &
                         ((q < np.uint64(img.base)) | (q >= np.uint64(img.base + img.size))))[0]
        for i in hit:
            emu.wptr(img.base + sva + 8 * int(i), stub)
        n += len(hit)
    return size // 8 + n


IDX_REG = {"edx": 1, "r8d": 2, "r9d": 3}  # the index argument: (ctx, index) or (ctx, out, index)


def find_param_accessors(img, handlers, log):
    """Scr_GetNumParam: the handlers' most called callee that is `mov eax, [rcx + N]; ret` (N:
    outparamcount of the VM context). Accessors: callees whose first instructions bound-check
    their index (`cmp edx, [rcx + N]`); the top pointer is the `mov rax, [rcx + T]` after it.
    Scr_Error: the call that follows the message formatter in their error paths."""
    calls = collections.Counter()
    for h in handlers:
        for t in img.direct_calls(h):
            calls[t] += 1
    getnum = None
    for t, n in calls.most_common(50):
        b = img.disasm(t, 8)
        if len(b) >= 2 and b[1].mnemonic == "ret" and b[0].mnemonic == "mov" and \
                re.match(r"eax, dword ptr \[rcx \+ 0x[0-9a-f]+\]$", b[0].op_str):
            getnum = (t, int(b[0].op_str.split("+ ")[1].rstrip("]"), 16))
            break
    if not getnum:
        fail("no Scr_GetNumParam among the handlers' callees")
    opc = getnum[1]
    # the VM context the accessors take: the handler's VM plus D (`lea rcx, [rbx + D]` or
    # `add rcx, D` right before the calls to Scr_GetNumParam)
    ctxs = collections.Counter()
    for h in handlers:
        ins = img.body(h)
        for p, i in zip(ins, ins[1:]):
            if i.mnemonic == "call" and target(i) == getnum[0]:
                m = re.match(r"rcx, (?:\[r\w+ \+ )?(0x[0-9a-f]+)\]?$", p.op_str)
                if p.mnemonic in ("lea", "add") and m:
                    ctxs[int(m.group(1), 16)] += 1
    if not ctxs:
        fail("no VM context offset before the calls to Scr_GetNumParam")
    ctx_off = ctxs.most_common(1)[0][0]
    acc, tops = {}, collections.Counter()
    for t, n in calls.items():
        b = img.body(t)[:16]
        idx = [IDX_REG[i.op_str.split(",")[0]] for i in b if i.mnemonic == "cmp" and
               i.op_str.split(",")[0] in IDX_REG and
               i.op_str.endswith(", dword ptr [rcx + %#x]" % opc)]
        if idx:
            acc[t] = {"callers": n, "index_arg": idx[0]}
            for k, i in enumerate(b):
                m = re.match(r"(\w+), qword ptr \[rcx \+ (0x[0-9a-f]+)\]$", i.op_str)
                if i.mnemonic == "mov" and m:
                    # BO7: ctx holds a pointer to the top pointer (`mov rdx, [rax]` follows)
                    ind = any(j.mnemonic == "mov" and j.op_str.endswith(
                        "qword ptr [%s]" % m.group(1)) for j in b[k + 1:k + 5])
                    tops[(int(m.group(2), 16), ind)] += 1
                    break
    if not acc or not tops:
        fail("no bound-checked parameter accessor")
    top, indirect = tops.most_common(1)[0][0]
    err = find_scr_error(img, acc)
    log.append("Scr_GetNumParam %#x (outparamcount at ctx+%#x), %d accessors bound-checking "
               "against it, top %s ctx+%#x, Scr_Error %#x" % (
                   getnum[0] - img.base, opc, len(acc),
                   "pointer to the top pointer at" if indirect else "pointer at", top,
                   err - img.base))
    log.append("VM context = VM + %#x (%d of %d calls to Scr_GetNumParam)" % (
        ctx_off, ctxs[ctx_off], sum(ctxs.values())))
    return getnum[0], opc, (top, indirect), acc, err, ctx_off


def find_scr_error(img, acc):
    """Scr_Error: in the accessors' error paths the code is formatted ("DEV ERROR 0x%08X",
    `mov r8d, CODE`) then raised with the same code (`mov ecx, CODE; ...; call Scr_Error`)."""
    cnt = collections.Counter()
    for a in acc:
        ins = img.disasm(a, 0x200)
        codes = {i.op_str.split(", ")[1] for i in ins if i.mnemonic == "mov" and
                 i.op_str.startswith("r8d, 0x")}
        for k, i in enumerate(ins):
            if i.mnemonic == "mov" and i.op_str.startswith("ecx, 0x") and \
                    i.op_str.split(", ")[1] in codes:
                for j in ins[k + 1:k + 4]:
                    if j.mnemonic == "call" and target(j):
                        cnt[target(j)] += 1
                        break
    if not cnt:
        fail("Scr_Error: no `mov ecx, CODE; call` after a formatted DEV ERROR code")
    return cnt.most_common(1)[0][0]


def accessor_tags(img, acc, opc, top_ind, err, log, ntags=32):
    """Which value tags (byte +8 of a stack value {payload, u8 tag}) each accessor takes without
    raising Scr_Error, run with one parameter of each tag. The tags have no names in the
    executable (the type-name strings and the typed error messages are gone: errors are
    'DEV ERROR 0x%08X' codes), so an accessor is labelled by the tags it accepts."""
    emu = Emu(img, null_page=True)
    neutralise_imports(img, emu)
    emu.map_on_unmapped()
    uc = emu.uc
    st = {}

    def on_err(uc, addr, size, ud):
        st["err"] = True
        uc.emu_stop()
    uc.hook_add(unicorn.UC_HOOK_CODE, on_err, begin=err, end=err)
    top, indirect = top_ind
    csz = max(opc, top) + 0x100
    ctx, slot, pp, out = emu.alloc(csz), emu.alloc(0x100), emu.alloc(16), emu.alloc(64)
    for a in acc:
        ok = []
        for tag in range(ntags):
            uc.mem_write(ctx, b"\0" * csz)
            uc.mem_write(slot, b"\0" * 0x100)
            uc.mem_write(ctx + opc, struct.pack("<I", 1))
            if indirect:
                emu.wptr(ctx + top, pp)
                emu.wptr(pp, slot + 0x40)
            else:
                emu.wptr(ctx + top, slot + 0x40)
            uc.mem_write(slot + 0x40, struct.pack("<Q", 0x1234) + bytes([tag]))
            st.clear()
            emu.pages = 0
            args = [ctx, out, out + 32, out + 48]
            args[acc[a]["index_arg"]] = 0
            r, e = emu.run(a, tuple(args), count=200_000)
            if not st.get("err") and not e:
                ok.append(tag)
        if len(ok) == ntags:
            lab = "any"
        elif not ok:
            lab = "unknown"
        else:
            lab = "tag" + "x".join(str(t) for t in ok)
        acc[a]["label"] = lab
        acc[a]["tags"] = ok
    log.append("accessor labels (accepted value tags): %s" % dict(
        collections.Counter(a["label"] for a in acc.values())))


def run_handlers(img, entries, sets, log):
    from common import handlers as H
    handlers = sorted({img.base + e["handler_rva"] for e in entries})
    getnum, opc, top, acc, err, ctx_off = find_param_accessors(img, handlers, log)
    accessor_tags(img, acc, opc, top, err, log)
    run_acc = acc
    vm = {"vmpub": 0, "vmpub_top": ctx_off + top[0], "vmpub_outparamcount": ctx_off + opc,
          "index_arg": 1}
    def fresh():
        import gc
        gc.collect()  # the old runner (hooks hold it in a cycle) and its image copy
        r = H.HandlerRunner(img, vm, run_acc, {}, [err])
        neutralise_imports(img, r.emu)
        return r
    # a fresh runner every RENEW runs: after a few thousand runs (pages mapped on demand,
    # code the handlers wrote over) unicorn itself faults (an access violation in the host)
    RENEW = 300
    runner = fresh()
    server = [s["vm_rva"] for s in sets.values() if s["vm_name"] == "server"]
    done = {}
    faults = 0
    for i, e in enumerate(entries):
        vms = [sets[o]["vm_rva"] for o in e["sets"] if sets[o]["vm_rva"]] or server[:1]
        if not vms:
            continue
        vmrva = vms[0]
        method = e["kind"] == "method"
        vm["vmpub"] = vmrva
        vmva = img.base + vmrva
        vm["handler_args"] = (lambda va: (lambda inst, m, ref: (va, runner.blank) if m else (va,)))(vmva)
        mn, mx = (e["min"], e["max"]) if e["min"] <= e["max"] else (0, 16)
        key = (e["handler_rva"], vmrva, method, mn, mx)
        if key not in done:
            if len(done) % RENEW == RENEW - 1:
                del runner
                runner = fresh()
            try:
                done[key] = H.handler_profile(runner, img.base + e["handler_rva"], 0, method,
                                              mn, mx)
            except OSError as ex:  # the emulator's own fault: no measure for this handler
                faults += 1
                done[key] = None
                print("handler %#x: emulator fault %s" % (e["handler_rva"], ex), file=sys.stderr)
                del runner
                runner = fresh()
        e["handler"] = done[key]
        if i % 1000 == 0:
            print("handlers %d/%d" % (i, len(entries)), file=sys.stderr)
    log.append("handler runs: %d distinct (handler, VM, kind, min, max), %d emulator faults "
               "(no measure)" % (len(done), faults))
    return {"getnumparam_rva": getnum - img.base, "outparamcount_off": opc, "top_off": top[0],
            "top_indirect": top[1], "ctx_off": ctx_off,
            "scr_error_rva": err - img.base,
            "accessors": {"%x" % (a - img.base): {"label": v["label"], "tags": v["tags"],
                                                 "callers": v["callers"],
                                                 "index_arg": v["index_arg"]}
                          for a, v in acc.items()}}


def extract(game, exe, out_path, with_handlers=True):
    prof = PROFILES[game]
    img = Image(exe or paths.need("exe." + game))  # paths.local.json or GSC_RE_EXE_<GAME>
    log = [img.describe()]
    linker = find_linker(img, log)
    lookups = find_lookups(img, linker, log)
    checks = linker_checks(img, linker)
    log.append("linker checks: dev flag (entry +0x10 == 1): %s; min/max (+0x11/+0x12): %s"
               % (checks["dev_check"], checks["arity_check"]))
    cmps = check_comparator(img, lookups)
    adds = find_add_functions(img, lookups, log)
    builders = find_builders(img, adds, log)
    # BO6: min > max in some entries; its linker never reads them, so they do not decide shape
    emu, stubs, sets = build_sets(img, builders, adds, lookups, log, checks["arity_check"])
    if prof.get("vm_names"):
        roles, sets_off, modes = find_roles(img, prof, sets, log)
    else:
        roles, sets_off, modes = {o: dict(UNASSIGNED) for o in sets}, None, None
    trace_and_round_trip(img, emu, stubs, sets, lookups, log)
    # tables: every span, with the sets (vm, mode) that hold it
    tables = {}
    for obj, s in sets.items():
        for kind, ptr, n in s["spans"]:
            t = tables.setdefault((kind, ptr), {"kind": kind, "rva": ptr - img.base, "count": n,
                                                "sets": []})
            if t["count"] != n:
                fail("table %#x appended with two counts" % (ptr - img.base))
            t["sets"].append(obj - img.base)
    entries = []
    for (kind, ptr), t in sorted(tables.items(), key=lambda kv: kv[1]["rva"]):
        for k in range(t["count"]):
            va = ptr + 0x18 * k
            e = LAYOUT.read(img, va)
            ent = {"kind": kind, "table_rva": t["rva"], "index": k, "entry_rva": va - img.base,
                   "hash_full": e["hash"], "handler_rva": e["handler"] - img.base,
                   "type": e["type"], "min": e["min"], "max": e["max"], "sets": {}}
            for o in t["sets"]:
                s = sets[o + img.base]
                arr = s["arrays"][kind]
                pos = [i for i, (h, p) in enumerate(arr) if p == va]
                if len(pos) != 1:
                    fail("entry %#x in set %#x %d times" % (va - img.base, o, len(pos)))
                ret = s["rt"][kind][pos[0]]["entry"]
                ent["sets"]["%x" % o] = {"returned": ret == va}
            entries.append(ent)
    result = {
        "game": game, "exe": os.path.basename(img.path), "pe_timestamp": img.timestamp,
        "image_base": img.base, "linker_rva": linker - img.base, "checks": checks,
        "comparators": cmps, "sets_offset": sets_off, "modes": modes,
        "lookups": {k: {"rva": v["va"] - img.base, "array_off": v["array_off"],
                        "count_off": v["count_off"]} for k, v in lookups.items()},
        "append": {k: v - img.base for k, v in adds.items()},
        "sets": {"%x" % (o - img.base): dict(
            {k: v for k, v in roles[o].items() if k != "vm_va"},
            vm_rva=(roles[o]["vm_va"] - img.base) if roles[o].get("vm_va") else None,
            builder_rva=s["builder"] - img.base, functions=len(s["arrays"]["function"]),
            methods=len(s["arrays"]["method"]), trace=s["trace"]) for o, s in sets.items()},
        "tables": [dict(t, sets=["%x" % o for o in t["sets"]]) for t in tables.values()],
        "entries": entries,
    }
    del emu, stubs  # the handler runs map their own image copy
    if with_handlers:
        result["handler_runs"] = run_handlers(img, entries, result["sets"], log)
    result["log"] = log
    if out_path is None:
        out_path = os.path.join(OUT, "%s_extract.json" % game)
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(result, f, indent=1)
    print("\n".join(log))
    print("%d entries -> %s" % (len(entries), out_path))
    return result, img


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    x = sub.add_parser("extract")
    x.add_argument("--game", choices=sorted(PROFILES), required=True)
    x.add_argument("--exe")
    x.add_argument("--out")
    x.add_argument("--no-handlers", action="store_true")
    a = ap.parse_args()
    extract(a.game, a.exe, a.out, not a.no_handlers)


if __name__ == "__main__":
    main()
