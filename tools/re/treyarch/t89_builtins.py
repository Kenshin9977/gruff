"""Extract the Black Ops 4 (T8) and Black Ops Cold War (T9) builtin catalogues from the decrypted,
flat Cordycep dumps, statically and under Unicorn. The game is never run.

    py -3.11 -I treyarch/t89_builtins.py extract --game t8 [--exe ...] [--out out/t8_extract.json]
    py -3.11 -I treyarch/t89_builtins.py extract --game t9 [--exe ...]

What differs from T7 (t7_builtins.py), see README_t89.md:

1. The tables are static data (0x20-byte entries as T7, hash = t89.scr): no init to emulate.
   The large tables are not scanned linearly: the lookup bsearch()es an int index array sorted
   by hash. Those index arrays are zero in the dumps (filled at run time; T9's filler is a qsort
   initialiser, T8's was not found): the bsearch is hooked and, when its array is all zeros, the
   array is filled with the permutation that sorts the table by hash (what the comparator
   requires); an array already filled must be that permutation, or the extraction stops.
2. Roles: the VM value reader, exactly as T7 (is-function byte, then the hash; inst 0 GSC, 1 CSC):
   t7_builtins.find_roles is reused.
3. A lookup is traced with an absent hash (0): the linear getters' 4-byte reads at the 0x20
   stride give their tables; the bsearch calls give the others (comparator -> table, count). The
   static loop bound, and the shaped run at the table address, are the second measure; the round
   trip (each hash looked up again through the engine) the third.
4. Error texts are hashed in T8/T9, so accessors are not found by their strings: scrVmPub is read
   off the animtree accessor (the one function that still loads "expected a animtree name, got
   %s, param %d"), an accessor is a handler callee that bound-checks its index against
   outparamcount and reads the slot, and its type is measured by running it with a value of each
   VM type in the slot (accepted types named by the game's own type-name table).
   Handlers are void(inst) and void(inst, scr_entref_t *).
"""
import argparse
import collections
import json
import os
import re
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
sys.path.insert(0, HERE)
from common.image import Image  # noqa: E402
from common.emu import Emu, unicorn, X  # noqa: E402
from common import tables as T  # noqa: E402
from common import handlers as H  # noqa: E402
from common import paths  # noqa: E402
import t7_builtins  # noqa: E402  (find_roles, round_trip, lookup_args: same conventions)

OUT = os.path.join(os.path.dirname(HERE), "out")
LAYOUT = T.LAYOUT_T7
ANIMTREE_MSG = "expected a animtree name, got %s, param %d"

# Engine anchors (RVAs). Each is a hypothesis checked at run time.
PROFILES = {
    "t8": {
        "value_reader": 0x2754F50,   # case "builtin pointer": read u8 flag, u32 hash, 4 lookups
        "reader_read": 0x3D0A220,    # read(reader, size, dst)
        "bsearch": 0x485D08C,        # CRT bsearch(key, base, n, size, cmp)
    },
    "t9": {
        "value_reader": 0x1B735B0,
        "reader_read": 0xC97FB10,
        "bsearch": 0xD47FA44,
    },
}
COMMON = {
    "handler_args": lambda inst, method, self_ref: (inst, self_ref) if method else (inst,),
    "index_arg": 1, "out_arg": 2,
}


# ---------------------------------------------------------------------------------------------
# scrVmPub, the type names: read off the animtree accessor
def read_vmpub(img, log):
    """The animtree accessor: `imul r, inst, STRIDE`, then either `cmp edx, [r + base + OPC]`
    and `mov r, [r + base + TOP]` (T8: scrVmPub[inst] in the image), or `mov r, [r + base + P]`,
    `cmp edx, [r + OPC]`, `mov r, [r + TOP]` (T9: a pointer to it); the type-name table is the
    `mov rdx, [base + type*8 + NAMES]` that feeds the message."""
    refs = [v for a in img.find_cstr(ANIMTREE_MSG) for v in img.code_refs(a)]
    if len(refs) != 1:
        raise SystemExit("animtree accessor: %d references to %r" % (len(refs), ANIMTREE_MSG))
    f = img.head_of(refs[0])
    body = img.body(f)
    disp = re.compile(r"\[([a-z0-9]+) \+ ([a-z0-9]+)(?:\*\d)? \+ (0x[0-9a-f]+|\d+)\]|\[([a-z0-9]+) \+ (0x[0-9a-f]+|\d+)\]")
    v = {}
    for k, ins in enumerate(body[:40]):
        if ins.mnemonic == "imul" and "stride" not in v:
            v["stride"] = int(ins.op_str.split(", ")[-1], 0)
        m = disp.search(ins.op_str)
        if not m:
            continue
        two = m.group(3) is not None
        d = int(m.group(3) or m.group(5), 0)
        if ins.mnemonic == "cmp" and ins.op_str.startswith("edx,") and "opc" not in v:
            v["opc"], v["opc_abs"] = d, two
            if not two:  # through a pointer: the load before it is the pointer slot
                v["ptr"] = v.pop("pre")
        elif ins.mnemonic == "mov" and ins.op_str.startswith("r") and "opc" not in v and two:
            v["pre"] = d
        elif ins.mnemonic == "mov" and "opc" in v and "top" not in v:
            v["top"] = d
    for ins in body:
        if ins.address > refs[0]:
            break
        m = re.search(r"\[r\w+ \+ rax\*8 \+ (0x[0-9a-f]+)\]", ins.op_str)
        if ins.mnemonic == "mov" and m:
            v["names"] = int(m.group(1), 16)
    need = {"stride", "opc", "top", "names"}
    if not need <= set(v):
        raise SystemExit("animtree accessor %#x: could not read %s" % (f - img.base, need - set(v)))
    names = []
    for i in range(64):
        p = img.u64(img.base + v["names"] + 8 * i)
        s = img.cstr(p) if img.in_image(p) else None
        if not s:
            break
        names.append(s)
    if names[:4] != ["undefined", "pointer", "string", "vector"]:
        raise SystemExit("type-name table %#x: %s" % (v["names"], names[:6]))
    vm = {"stride": v["stride"], "type_names": names, "type_names_rva": v["names"],
          "animtree_accessor": f - img.base}
    if v["opc_abs"]:  # T8: scrVmPub[inst] is image data
        vm.update(direct=True, vmpub=v["top"], vmpub_top=0, vmpub_outparamcount=v["opc"] - v["top"])
    else:
        vm.update(direct=False, vmpub_ptr=v["ptr"], vmpub_top=v["top"], vmpub_outparamcount=v["opc"])
    log.append("scrVmPub (read off the animtree accessor %#x): %s; %d VM type names at %#x (%s)"
               % (f - img.base, {k: (hex(x) if isinstance(x, int) else x) for k, x in vm.items()
                                 if k not in ("type_names",)}, len(names), v["names"],
                  ", ".join(names)))
    return vm


# ---------------------------------------------------------------------------------------------
# tables of a lookup, bsearch-aware
class Lookups:
    """Lookup tracing and round trips on one emulator. The bsearch is hooked: its comparator
    names the table, its count the size; a zero index array is filled with the hash-sorted
    permutation of its table (the game fills it at run time)."""

    def __init__(self, img, prof, log):
        self.img, self.log = img, log
        self.emu = Emu(img)
        self.bs = img.base + prof["bsearch"]
        f = img.func(self.bs)
        if not f or f[0] != self.bs:
            raise SystemExit("bsearch %#x is not a function start" % prof["bsearch"])
        self.bs_range = f
        self.arrays = {}   # index array va -> (table va, n, filled_by_us)
        self.events = []
        self.cmp_ranges = []
        self.emu.uc.hook_add(unicorn.UC_HOOK_CODE, self._on_bsearch, begin=self.bs, end=self.bs)

    def comparator_table(self, cmp):
        for ins in self.img.md.disasm(self.img.d[cmp - self.img.base:cmp - self.img.base + 0x60], cmp):
            if ins.mnemonic == "lea":
                t = self.img.operand_addr(ins)
                if t is not None:
                    return t
        raise SystemExit("bsearch comparator %#x: no table lea" % (cmp - self.img.base))

    def _on_bsearch(self, uc, addr, size, ud):
        e = self.emu
        arr, n, sz = e.arg(1), e.arg(2), e.arg(3)
        cmp = e.arg(4)
        if sz != 4 or not self.img.is_code(cmp):
            raise SystemExit("bsearch call with size %d, comparator %#x: not an index search" % (sz, cmp))
        tab = self.comparator_table(cmp)
        cf = self.img.func(cmp)
        self.cmp_ranges.append(cf if cf and cf[0] == cmp else (cmp, cmp + 0x40))
        if arr not in self.arrays:
            ents = [LAYOUT.read(self.img, tab + 0x20 * k) for k in range(n)]
            if not all(LAYOUT.shaped(self.img, x) for x in ents):
                raise SystemExit("bsearch table %#x (%d entries) is not entry-shaped" % (tab, n))
            perm = sorted(range(n), key=lambda k: (ents[k]["hash"], k))
            cur = list(struct.unpack("<%di" % n, bytes(uc.mem_read(arr, 4 * n))))
            if any(cur):
                if cur != perm:
                    raise SystemExit("index array %#x is filled but not the hash order of %#x"
                                     % (arr - self.img.base, tab - self.img.base))
                mine = False
            else:
                uc.mem_write(arr, struct.pack("<%di" % n, *perm))
                mine = True
            self.arrays[arr] = (tab, n, mine)
        self.events.append(("bsearch", tab, n, arr))

    def trace(self, va):
        """Tables of the lookup at va, in lookup order: [(table va, n, how, index array)]."""
        img, uc = self.img, self.emu.uc
        reads = []
        self.events = []

        def hr(uc, acc, addr, size, val, ud):
            if size == 4 and img.base <= addr < img.base + img.size:
                pc = uc.reg_read(X.UC_X86_REG_RIP)
                reads.append((addr, pc, len(self.events)))

        h = uc.hook_add(unicorn.UC_HOOK_MEM_READ, hr)
        out, args = t7_builtins.lookup_args(self.emu, 0)
        r, err = self.emu.run(va, args)
        uc.hook_del(h)
        if err or r:
            raise SystemExit("lookup %#x with an absent key: %s %s" % (va - img.base, err, r))

        def skip(pc):
            return (self.bs_range[0] <= pc < self.bs_range[1] or
                    any(a <= pc < b for a, b in self.cmp_ranges))
        lin = [(a, k) for a, pc, k in reads if not skip(pc)]
        st = LAYOUT.size
        # runs in time order: a scan reads table[0], table[1]... one after the other, so two
        # adjacent tables scanned by two getters are two runs, not one
        runs, cur, inrun = [], None, set()
        for a, k in lin:
            if cur and a == cur[2] + st:
                cur[2] = a
                inrun.add(a)
                continue
            if a in inrun:
                continue
            cur = [a, k, a]
            inrun.add(a)
            runs.append(cur)
        found = []  # (order key, table, n, how, arr)
        for a, k, last in runs:
            n = (last - a) // st + 1
            ents = [LAYOUT.read(img, a + st * j) for j in range(n)]
            p = 0
            while p < n and LAYOUT.shaped(img, ents[p]):
                p += 1
            if p == n - 1 and p and img.u32(a + st * p) == p:
                # a table with its count stored right after it (the getter reads the count at
                # the next stride): {entries[p], u32 p}
                n, ents = p, ents[:p]
            if not all(LAYOUT.shaped(img, x) for x in ents):
                if n > 1:
                    raise SystemExit("lookup %#x reads a %d-entry run at %#x that is not a "
                                     "builtin table" % (va - img.base, n, a - img.base))
                continue
            found.append((k, a, n, "linear", None))
        for i, (_, tab, n, arr) in enumerate(self.events):
            found.append((i + 0.5, tab, n, "bsearch", arr))
        found.sort(key=lambda x: x[0])
        return [(a, n, how, arr) for k, a, n, how, arr in found]

    def round_trip(self, va, h):
        return t7_builtins.round_trip(self.emu, va, h)


def shaped_run(img, va, limit=4096):
    n = 0
    while n < limit and LAYOUT.shaped(img, LAYOUT.read(img, va + 0x20 * n)):
        n += 1
    return n


# ---------------------------------------------------------------------------------------------
# accessors, by their bound check, typed by emulation
class VmRunner(H.HandlerRunner):
    """HandlerRunner whose scrVmPub[inst] is either image data (T8) or behind a pointer (T9: the
    pointer slot is set to a fake structure)."""

    def __init__(self, img, vm, *a, **k):
        super().__init__(img, vm, *a, **k)
        self.fake = {}
        if not vm["direct"]:
            for inst in (0, 1):
                s = self.emu.alloc(0x20000)
                self.fake[inst] = s
                self.emu.wptr(img.base + vm["vmpub_ptr"] + inst * vm["stride"], s)
        self.entref = self.emu.alloc(0x40)

    def pub(self, inst):
        vm = self.vm
        if vm["direct"]:
            return self.img.base + vm["vmpub"] + inst * vm["stride"]
        self.emu.wptr(self.img.base + vm["vmpub_ptr"] + inst * vm["stride"], self.fake[inst])
        return self.fake[inst]

    def run(self, handler, inst, method, nparams, count=300_000, slot_type=None, args=None):
        uc = self.emu.uc
        vm = self.vm

        def fill(top):  # every slot: a value of that type pointing at blank memory
            for i in range(max(nparams, 1)):
                uc.mem_write(top - 16 * i, struct.pack("<QI", self.blank, slot_type))
        self.start_run(inst, nparams, fill if slot_type is not None else None)
        uc.mem_write(self.entref, b"\0" * 0x40)
        if args is None:
            args = vm["handler_args"](inst, method, self.entref)
        r, err = self.emu.run(handler, args, count=count)
        if not (self.stop_reason or err) and self.emu.ip() != self.emu.retaddr:
            err = "instruction limit at %#x" % self.emu.ip()
        return list(self.events), self.stop_reason or err or "returned"


def bound_checks(img, va, vm, n=48):
    """Does the function at va compare its index (edx) with outparamcount and read the slot type
    within its first instructions?"""
    opc = vm["vmpub_outparamcount"] + (vm["vmpub"] if vm["direct"] else 0)
    pat = "%#x]" % opc
    body = img.body(va)[:n]
    return any(i.mnemonic == "cmp" and i.op_str.startswith("edx,") and i.op_str.endswith(pat)
               for i in body)


def find_errors(img, accs):
    """Error calls: in the accessors (decoded linearly over 0x300 bytes: their error paths sit in
    other .pdata chunks), a direct call preceded by `mov ecx|edx, imm32` (a hashed message id:
    the message-text lookup, Scr_Error, the parameter-type error). Returned with the number of
    accessors making each."""
    cnt = collections.Counter()
    for a in accs:
        o = a - img.base
        body = list(img.md.disasm(img.d[o:o + 0x300], a))
        hit = set()
        for k, ins in enumerate(body):
            if ins.mnemonic == "call" and ins.op_str.startswith("0x"):
                prev = body[max(0, k - 3):k]
                cookie = any(p.mnemonic == "xor" and p.op_str in ("rcx, rsp", "rcx, rbp")
                             for p in prev)  # __security_check_cookie
                if not cookie and any(p.mnemonic == "mov" and re.match(
                        r"e[cd]x, 0x[0-9a-f]{5,8}$", p.op_str) for p in prev):
                    hit.add(int(ins.op_str, 16))
        cnt.update(hit)
    return cnt


def find_accessors(img, handlers, vm, log):
    calls = H.handler_callees(img, handlers)
    acc = {}
    for t, n in calls.items():
        if img.in_image(t) and img.is_code(t) and bound_checks(img, t, vm):
            acc[t] = {"callers": n}
    for t, n in calls.items():  # short wrappers whose first call or jmp is an accessor
        if t in acc or not img.is_code(t):
            continue
        body = img.body(t)[:6]
        first = [i for i in body if i.mnemonic in ("call", "jmp") and i.op_str.startswith("0x")]
        if first and int(first[0].op_str, 16) in acc and img.end_of(t) - t <= 0x40:
            acc[t] = {"callers": n, "wrapper_of": int(first[0].op_str, 16)}
    errs = find_errors(img, [a for a in acc if "wrapper_of" not in acc[a]])
    stops = [e for e, k in errs.items() if k >= 3]
    if not stops:
        raise SystemExit("no Scr_Error candidate in the %d accessors" % len(acc))
    log.append("accessors (bound check on outparamcount): %d, %d of them wrappers; script-error "
               "calls (hashed message id in ecx, made by >= 3 accessors): %s"
               % (len(acc), sum(1 for a in acc.values() if "wrapper_of" in a),
                  ", ".join("%#x (%d)" % (e - img.base, k) for e, k in errs.most_common() if k >= 3)))
    return acc, stops


def type_accessors(img, vm, acc, stops, log):
    """Run every accessor alone with outparamcount 1 and a value of each VM type in slot 0: the
    types it returns on are the ones it accepts. Label: the accepted type names."""
    names = vm["type_names"]
    r = VmRunner(img, dict(vm, **COMMON), {}, {}, stops)
    obuf = r.emu.alloc(0x100)
    out = {}
    for a, info in acc.items():
        ok = []
        for t in range(len(names)):
            ev, why = r.run(a, 0, False, 1, count=100_000, slot_type=t, args=(0, 0, obuf, obuf))
            if why == "returned":
                ok.append(t)
            # self-reference: accessors are run without their own hooks
        info["accepts"] = ok
        if len(ok) == len(names) or len(ok) > 6:  # a type-agnostic read (Scr_GetType...)
            lab = "any"
        elif not ok:
            lab = "none"
        else:
            lab = "|".join(names[t].replace(" ", "").replace("_", "") for t in ok)
        info["label"] = lab
        info["label_by"] = "emulation"
        out[a] = info
    for a, info in out.items():  # none accepted when run alone (a value it converts or
        if info["label"] != "none":  # resolves must be real): the inner accessor, or its text
            continue
        if "wrapper_of" in info and out[info["wrapper_of"]]["label"] != "none":
            info["label"], info["label_by"] = out[info["wrapper_of"]]["label"], "wrapper"
            continue
        strs = img.strings_loaded(a, depth=1)
        for frag, lab in H.ACCESSOR_LABELS:
            if any(frag in s for s in strs):
                info["label"], info["label_by"] = lab, "message text"
                break
        else:
            info["label"], info["label_by"] = "any", "unmeasured"
    log.append("accessor types (accepted VM types, by emulation): %s" % dict(
        collections.Counter(i["label"] for i in out.values()).most_common()))
    return out


def find_resolvers(img, method_handlers, acc, min_callers=20):
    """The entref -> object functions: the first direct call of many method handlers, not an
    accessor."""
    cnt = collections.Counter()
    for h in method_handlers:
        for ins in img.body(h)[:12]:
            if ins.mnemonic == "call" and ins.op_str.startswith("0x"):
                cnt[int(ins.op_str, 16)] += 1
                break
    return {t: {"label": "self", "callers": n} for t, n in cnt.items()
            if n >= min_callers and t not in acc}


# ---------------------------------------------------------------------------------------------
def extract(game, exe, out_path, with_handlers=True):
    prof = PROFILES[game]
    img = Image(exe)
    log = ["exe %s, image base %#x, PE timestamp %d, size %d" % (os.path.basename(exe), img.base, img.timestamp,
                                                                 img.size)]
    roles = t7_builtins.find_roles(img, Emu(img), prof, log)
    L = Lookups(img, prof, log)
    result = {"game": game, "exe": os.path.basename(exe), "pe_timestamp": img.timestamp, "image_base": img.base,
              "lookups": {}, "entries": []}
    known = set()
    traced = {}
    for role in ("gsc_function", "gsc_method", "csc_function", "csc_method"):
        traced[role] = L.trace(roles[role])
        known |= {a for a, n, how, arr in traced[role]}
    for role in ("gsc_function", "gsc_method", "csc_function", "csc_method"):
        va = roles[role]
        tables = traced[role]
        sbs = T.static_bounds(img, [a for a, n, how, arr in tables])
        tinfo = []
        for ti, (a, n, how, arr) in enumerate(tables):
            run = shaped_run(img, a)
            nxt = a + 0x20 * n
            run_ok = run == n or (run > n and nxt in known)
            tinfo.append({"rva": a - img.base, "count": n, "scan": how,
                          "index_array_rva": (arr - img.base) if arr else None,
                          "index_filled_here": L.arrays[arr][2] if arr else None,
                          "static_bounds": sorted(sbs[a]),
                          "static_agrees": (not sbs[a]) or (n in sbs[a]),
                          "shaped_run": run, "run_agrees": run_ok})
            for k in range(n):
                e = LAYOUT.read(img, a + 0x20 * k)
                r, rt, rmn, rmx, err = L.round_trip(va, e["hash"])
                result["entries"].append({
                    "role": role, "table": ti, "index": k, "canon": e["hash"],
                    "min": e["min"], "max": e["max"], "type_field": e["type"],
                    "handler_rva": e["handler"] - img.base,
                    "lookup_handler_rva": (r - img.base) if r else None,
                    "lookup_type": rt if rt != 0xEEEEEEEE else None,
                    "lookup_min": rmn, "lookup_max": rmx, "lookup_err": err,
                })
        result["lookups"][role] = {"rva": va - img.base, "tables": tinfo}
        log.append("%s: %d tables (%d bsearch), %d entries; static bound agrees on %d/%d tables "
                   "with one; shaped run at the address agrees on %d/%d"
                   % (role, len(tinfo), sum(1 for t in tinfo if t["scan"] == "bsearch"),
                      sum(t["count"] for t in tinfo),
                      sum(1 for t in tinfo if t["static_bounds"] and t["static_agrees"]),
                      sum(1 for t in tinfo if t["static_bounds"]),
                      sum(1 for t in tinfo if t["run_agrees"]), len(tinfo)))
    filled = [(a, v) for a, v in L.arrays.items()]
    log.append("bsearch index arrays: %d, zero in the dump and filled with the hash order: %d"
               % (len(filled), sum(1 for a, v in filled if v[2])))
    E = result["entries"]
    rt_ok = sum(1 for e in E if e["lookup_handler_rva"] == e["handler_rva"] and
                e["lookup_min"] == e["min"] and e["lookup_max"] == e["max"])
    log.append("round trip: %d/%d entries returned by their lookup with the same handler, min, max"
               % (rt_ok, len(E)))
    if with_handlers:
        vm = read_vmpub(img, log)
        result["vm"] = {k: v for k, v in vm.items()}
        handlers = sorted({e["handler_rva"] + img.base for e in E})
        acc, stops = find_accessors(img, handlers, vm, log)
        acc = type_accessors(img, vm, acc, stops, log)
        mh = sorted({e["handler_rva"] + img.base for e in E if e["role"].endswith("method")})
        res = find_resolvers(img, mh, acc)
        log.append("entref resolvers (first call of >= 20 method handlers): %s"
                   % ", ".join("%#x (%d)" % (a - img.base, v["callers"]) for a, v in res.items()))
        result["accessors"] = {"%#x" % (a - img.base): {"label": v["label"], "accepts": v["accepts"],
                                                       "callers": v["callers"]}
                               for a, v in acc.items()}
        result["stops"] = ["%#x" % (s - img.base) for s in stops]
        result["resolvers"] = ["%#x" % (a - img.base) for a in res]
    result["log"] = log
    os.makedirs(os.path.dirname(out_path), exist_ok=True)
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(result, f, indent=1)
    if with_handlers:
        del img, L, result
        run_handlers(out_path, log)
    print("\n".join(log))


# ---------------------------------------------------------------------------------------------
# handler runs, in child processes: unicorn crashes the host process on a few handlers (an
# access violation inside the emulator); the run in flight is recorded as crashed and a new
# child goes on from the next one.
def handler_key(e):
    return (e["handler_rva"], 1 if e["role"].startswith("csc") else 0,
            e["role"].endswith("method"), e["min"], e["max"])


def handlers_child(ex_path, prog_path):
    with open(ex_path, encoding="utf-8") as f:
        ex = json.load(f)
    done = set()
    if os.path.exists(prog_path):
        with open(prog_path, encoding="utf-8") as f:
            for line in f:
                done.add(tuple(json.loads(line)["key"]))
    # the extraction records the executable's file name only: find it again by game key
    exe = ex["exe"] if os.path.isfile(ex["exe"]) else paths.need("exe." + ex["game"])
    img = Image(exe)
    vm = dict(ex["vm"], **COMMON)
    acc = {img.base + int(a, 16): v for a, v in ex["accessors"].items()}
    res = {img.base + int(a, 16): {"label": "self"} for a in ex["resolvers"]}
    stops = [img.base + int(a, 16) for a in ex["stops"]]
    runner = VmRunner(img, vm, acc, res, stops)
    keys = []
    for e in ex["entries"]:
        k = handler_key(e)
        if k not in done and k not in keys:
            keys.append(k)
    with open(prog_path, "a", encoding="utf-8") as out:
        for i, k in enumerate(keys):
            out.write(json.dumps({"key": list(k), "start": True}) + "\n")
            out.flush()
            hp = H.handler_profile(runner, k[0] + img.base, k[1], k[2], k[3], k[4])
            out.write(json.dumps({"key": list(k), "profile": hp}) + "\n")
            out.flush()
            if i % 500 == 0:
                print("handlers %d/%d" % (i, len(keys)), file=sys.stderr)


CRASHED = {"n_max": 0, "read_at_max": [], "stop_at_max": "emulator crash", "n_min": 0,
           "read_at_min": [], "stop_at_min": "emulator crash", "types": {}, "resolves_self": False}


def run_handlers(ex_path, log):
    import subprocess
    prog = ex_path + ".handlers.jsonl"
    if os.path.exists(prog):
        os.remove(prog)
    crashed = []
    for attempt in range(500):
        rc = subprocess.call([sys.executable, "-I", os.path.abspath(__file__), "handlers-child",
                              "--extract", ex_path, "--progress", prog])
        last = None
        if os.path.exists(prog):
            with open(prog, encoding="utf-8") as f:
                for line in f:
                    last = json.loads(line)
        if rc == 0:
            break
        if not last or "start" not in last:
            raise SystemExit("handler child failed (%d) outside a handler run" % rc)
        crashed.append(last["key"])
        with open(prog, "a", encoding="utf-8") as f:
            f.write(json.dumps({"key": last["key"], "profile": CRASHED}) + "\n")
    else:
        raise SystemExit("handler runs: too many crashes")
    prof = {}
    with open(prog, encoding="utf-8") as f:
        for line in f:
            r = json.loads(line)
            if "profile" in r:
                prof[tuple(r["key"])] = r["profile"]
    with open(ex_path, encoding="utf-8") as f:
        ex = json.load(f)
    for e in ex["entries"]:
        e["handler"] = prof.get(handler_key(e))
    ex["log"].append("handler runs: %d distinct (handler, VM, kind, min, max); the emulator "
                     "crashed the process on %d (recorded without types): %s" % (
                         len(prof), len(crashed), ", ".join("%#x" % k[0] for k in crashed)))
    with open(ex_path, "w", encoding="utf-8") as f:
        json.dump(ex, f, indent=1)
    log[:] = ex["log"]


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    x = sub.add_parser("extract")
    x.add_argument("--game", choices=sorted(PROFILES), required=True)
    x.add_argument("--exe")
    x.add_argument("--out")
    x.add_argument("--no-handlers", action="store_true")
    c = sub.add_parser("handlers-child")
    c.add_argument("--extract", required=True)
    c.add_argument("--progress", required=True)
    a = ap.parse_args()
    if a.cmd == "handlers-child":
        handlers_child(a.extract, a.progress)
        return
    exe = a.exe or paths.need("exe." + a.game)  # paths.local.json or GSC_RE_EXE_<GAME>
    out = a.out or os.path.join(OUT, "%s_extract.json" % a.game)
    extract(a.game, exe, out, not a.no_handlers)


if __name__ == "__main__":
    main()
