"""Extract the Black Ops III (T7) script-visible engine fields of every object kind from the
decrypted, flattened game executable, statically and under Unicorn. The game is never run.

    py -3.11 -I treyarch/t7_fields.py extract [--exe C:\\tmp\\t7\\bo3_flat.exe] [--out out\\t7_fields_extract.json]

Same method as t7_builtins.py (README.md, "Entity fields"):

1. The field tables are built at static-init time like the builtin tables (the name hashes are
   computed by dynamic initialisers): every initialiser that multiplies by the FNV prime is
   emulated (common.tables.fnv_initialisers) and its writes recorded.
2. Registration, from the game's own code: every function that calls Scr_AddClassField(inst,
   classnum, canon, offset) is emulated with that call hooked. Each call gives the VM (inst), the
   object class, the field's canon hash and the offset the VM will hand back to the class's field
   getter and setter. The 4-byte read that produced the canon gives the entry's address.
3. Second measure of each table: the entries of one (inst, class, offset >> 13) group sit at a
   constant stride, the offset's low bits are the entry index, and the entry after the last one
   has canon 0 (the registration loop stops on it). Any break stops the extraction.
4. Read-only, measured twice: (a) every entry's setter function is run alone; one that raises a
   "... is read-only" error before reading a parameter makes the field read-only, one that reads
   its parameter first (VM stack empty: "parameter %d does not exist") makes it writable; (b) the
   VM's object-field setter dispatch (Scr_SetObjectField, found by its "weapon is read-only"
   case) is run for each (class, offset): the error it reaches, if any, is the second verdict.
5. The owner kind of each group is named by the read-only message its setters raise ("actor
   field %s is read-only" -> actor); a group with no such message keeps its class number.

Addresses are image-relative (RVA) and checked at run time; nothing here is copied from a
community table.
"""
import argparse
import collections
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import t7_builtins as TB  # noqa: E402
from common.image import Image  # noqa: E402
from common.emu import Emu, unicorn, X  # noqa: E402
from common import tables as T  # noqa: E402
from common import paths  # noqa: E402

# Engine anchors (RVA). Each is a hypothesis checked by check_anchors(); how each was found is in
# README.md ("Entity fields").
T7F = {
    "add_class_field": 0x12D8E40,   # Scr_AddClassField(inst, classnum, canon, offset)
    "class_init": 0x12DF390,        # per-class field map init, called by the CSC registration
    "hash_name": 0x12CBBB0,         # canon -> debug name, "%X" fallback (used by error messages)
    "va": 0x227D160,                # message formatter
    "scr_error": 0x12EA450,         # Scr_Error (same as t7_builtins' stop)
}
READONLY_WORDS = ("read-only", "read only")
PARAM_MISSING = "parameter %d does not exist"
# read-only message fragment -> owner kind
KIND_BY_MESSAGE = [
    ("Tried to set a read only entity field", "entity"),
    ("sentient property", "sentient"),
    ("actor field", "actor"),
    ("player field", "player"),
    ("vehicle node field", "vehiclenode"),
    ("vehicle field", "vehicle"),
    ("pathnode property", "pathnode"),
    ("weapon is read-only", "weapon"),
    ("centity field", "centity"),
    ("dynent field", "dynent"),
]


def check_anchors(img, a):
    """Each anchor must look like what it is said to be; stop otherwise."""
    def body(rva, n=0x200):
        va = img.base + rva
        f = img.func(va)
        end = f[1] if f else va + n
        return list(img.md.disasm(img.d[rva:end - img.base], va))
    ins = body(a["add_class_field"])
    ops = [(i.mnemonic, i.op_str) for i in ins]
    if ("mov", "dword ptr [rax + 8], 0xa") not in ops or not any(
            m == "mov" and o.startswith("dword ptr [rax], r") for m, o in ops):
        raise SystemExit("add_class_field: no 'type = int (0xa), value = offset' store")
    s = img.strings_loaded(img.base + a["hash_name"], depth=0)
    if "%X" not in s:
        raise SystemExit("hash_name: no '%X' fallback")
    hits = img.rel32_callers([img.base + a["scr_error"]])[img.base + a["scr_error"]]
    if len(hits) < 100:
        raise SystemExit("scr_error: only %d callers" % len(hits))


# ---------------------------------------------------------------------------------------------
# 2. registration
def registrations(img, emu, a, written, log):
    add = img.base + a["add_class_field"]
    cinit = img.base + a["class_init"]
    calls = img.rel32_callers([add])[add]
    heads = sorted({img.head_of(c) for c, op in calls})
    rec, reads = [], []
    uc = emu.uc

    def on_code(uc, addr, size, ud):
        if addr == add:
            r = uc.reg_read
            cn = r(X.UC_X86_REG_R8D)
            src = next((x for x in reversed(reads[-64:])
                        if struct.unpack("<I", uc.mem_read(x, 4))[0] == cn), None)
            rec.append({"inst": r(X.UC_X86_REG_ECX) & 0xFFFFFFFF,
                        "cls": r(X.UC_X86_REG_EDX) & 0xFFFFFFFF, "canon": cn,
                        "offset": r(X.UC_X86_REG_R9D), "entry": src})
        emu.ret()

    def on_read(uc, acc, addr, size, val, ud):
        if size == 4 and img.base <= addr < img.base + len(img.d):
            reads.append(addr)

    hs = [uc.hook_add(unicorn.UC_HOOK_CODE, on_code, begin=add, end=add),
          uc.hook_add(unicorn.UC_HOOK_CODE, on_code, begin=cinit, end=cinit),
          uc.hook_add(unicorn.UC_HOOK_MEM_READ, on_read)]
    out = {}
    for h in heads:
        rec.clear()
        reads.clear()
        r, err = emu.run(h)
        n_new = 0
        for x in rec:
            if x["entry"] is None:
                raise SystemExit("registration %#x: canon %#x not read from memory" % (
                    h - img.base, x["canon"]))
            x["init_written"] = (x["entry"] & ~7) in written
            k = (x["inst"], x["cls"], x["offset"])
            if k in out:
                if out[k]["canon"] != x["canon"]:
                    raise SystemExit("(inst, class, offset) %s registered twice with two hashes" % (k,))
                out[k]["registered_by"].append(h - img.base)
                continue
            n_new += 1
            out[k] = dict(x, registered_by=[h - img.base])
        log.append("registration %#x: %d calls (%d new)%s" % (
            h - img.base, len(rec), n_new, "" if err is None else ", stopped: " + err))
    for h in hs:
        uc.hook_del(h)
    return out, [h - img.base for h in heads]


# ---------------------------------------------------------------------------------------------
# 3. tables
SLOTS = {  # stride -> (type offset, setter slot, getter slot, writable byte); checked below
    0x20: (0xC, 0x10, 0x18, None),
    0x28: (0xC, 0x18, 0x20, None),
    0x14: (0xC, None, None, 0x10),
}


def tables(img, emu, regs, log):
    groups = collections.defaultdict(list)
    for (inst, cls, off), x in regs.items():
        groups[(inst, cls, off >> 13)].append(x)
    res = []
    for key, xs in sorted(groups.items()):
        xs.sort(key=lambda x: x["offset"])
        idx = [x["offset"] & 0x1FFF for x in xs]
        if idx != list(range(len(xs))):
            raise SystemExit("group %s: offsets are not 0..n-1" % (key,))
        diffs = {b["entry"] - a["entry"] for a, b in zip(xs, xs[1:])}
        if len(diffs) > 1:
            raise SystemExit("group %s: entries not at one stride: %s" % (key, diffs))
        stride = diffs.pop() if diffs else 0x20
        base = xs[0]["entry"]
        term = struct.unpack("<I", emu.uc.mem_read(base + stride * len(xs), 4))[0]
        if stride not in SLOTS:
            raise SystemExit("group %s: stride %#x has no known layout" % (key, stride))
        ty_o, set_o, get_o, wr_o = SLOTS[stride]
        ents = []
        for i, x in enumerate(xs):
            raw = bytes(emu.uc.mem_read(base + stride * i, stride))
            e = {"inst": key[0], "cls": key[1], "group": key[2], "index": i,
                 "offset": x["offset"], "canon": x["canon"], "entry_rva": base + stride * i - img.base,
                 "registered_by": x["registered_by"], "init_written": x["init_written"],
                 "member_offset": struct.unpack_from("<I", raw, 4)[0],
                 "type": struct.unpack_from("<I", raw, ty_o)[0]}
            if set_o is not None:
                e["setter"] = struct.unpack_from("<Q", raw, set_o)[0]
                e["getter"] = struct.unpack_from("<Q", raw, get_o)[0]
            if wr_o is not None:
                e["writable_byte"] = raw[wr_o]
            ents.append(e)
        # a slot is a function-pointer slot only if every value is 0 or a code address
        if set_o is not None:
            okp = all(v == 0 or img.is_code(v) for e in ents for v in (e["setter"], e["getter"]))
            if not okp:
                for e in ents:
                    e.pop("setter")
                    e.pop("getter")
            for e in ents:
                for k in ("setter", "getter"):
                    if k in e:
                        e[k] = (e[k] - img.base) if e[k] else 0
        res.append({"inst": key[0], "cls": key[1], "group": key[2], "rva": base - img.base,
                    "stride": stride, "count": len(xs), "terminator_canon": term,
                    "pointer_slots": set_o is not None and "setter" in ents[0], "entries": ents})
        log.append("table inst %d class %d group %d: rva %#x, %d entries, stride %#x, next canon %#x%s"
                   % (key[0], key[1], key[2], base - img.base, len(xs), stride, term,
                      "" if term == 0 else " (NOT a terminator)"))
    return res


# ---------------------------------------------------------------------------------------------
# 4. read-only
class SetterRunner:
    """Runs a setter or the dispatcher on the post-init image with one value on the VM stack:
    the parameter accessors (t7_builtins' accessor discovery, out/t7_extract.json) are hooked at
    entry and return a neutral value, so a setter that writes reads its value and goes on. Records
    the messages formatted (va) or raised (Scr_Error's message argument), the canon named
    (hash_name) and the parameters read."""

    def __init__(self, img, a, emu, accessors):
        self.img = img
        self.emu = emu
        self.uc = emu.uc
        try:
            self.uc.mem_map(0, 0x100000)  # null plus an offset reads zeros
        except unicorn.UcError:
            pass
        self.blank = self.emu.alloc(0x10000)
        self.ev = []
        self.addr = {img.base + a["hash_name"]: ("name", None), img.base + a["va"]: ("fmt", None),
                     img.base + a["scr_error"]: ("error", None)}
        for rva, lab in accessors.items():
            self.addr[img.base + rva] = ("param", lab)
        for v in self.addr:
            self.uc.hook_add(unicorn.UC_HOOK_CODE, self._on, begin=v, end=v)
        self.emu.map_on_unmapped()

    def _on(self, uc, addr, size, ud):
        k, lab = self.addr[addr]
        if k == "name":
            self.ev.append(("name", uc.reg_read(X.UC_X86_REG_ECX) & 0xFFFFFFFF))
            self.emu.ret(self.blank)
        elif k == "fmt":
            self.ev.append(("msg", self.img.cstr(uc.reg_read(X.UC_X86_REG_RCX))))
            self.emu.ret(self.blank)
        elif k == "param":
            self.ev.append(("param", lab))
            if lab == "vector":
                try:
                    uc.mem_write(uc.reg_read(X.UC_X86_REG_R8), b"\0" * 12)
                except unicorn.UcError:
                    pass
            self.emu.ret(0 if lab in ("int", "float", "uint64", "any") else self.blank)
        else:
            s = self.img.cstr(uc.reg_read(X.UC_X86_REG_RDX))
            if s:
                self.ev.append(("msg", s))
            self.ev.append(("error", None))
            uc.emu_stop()

    def run(self, va, args):
        self.ev = []
        self.emu.pages = 0
        self.uc.mem_write(self.blank, b"\0" * 0x10000)
        r, err = self.emu.run(va, args, count=500_000)
        return list(self.ev), err


def verdict(ev):
    """(read_only, message). A read-only message anywhere: read-only. Otherwise, the new value
    read (an accessor called, or the empty-stack error): writable. Otherwise undecided."""
    msgs = [v for k, v in ev if k == "msg" and v]
    for v in msgs:
        if any(w in v for w in READONLY_WORDS):
            return True, v
    if any(k == "param" for k, v in ev) or any(PARAM_MISSING in v for v in msgs):
        return False, msgs[0] if msgs else "value read"
    return None, msgs[0] if msgs else None


def find_dispatchers(img):
    """Scr_SetObjectField for each VM: the functions that load "weapon is read-only" (the weapon
    class case of the dispatch). The server one switches over classes 0..5, the client one over
    0, 4 and 5; inst is told apart by the Scr_Error inst argument (ecx = 0 or 1) on that path."""
    s = img.d.find(b"weapon is read-only\0")
    tgt = img.base + s
    out = {}
    for ref in img.riprel_xrefs({tgt})[tgt]:
        f = img.func(ref)
        if not f:
            continue
        ins = list(img.md.disasm(img.d[ref - 3 - img.base:f[1] - img.base], ref - 3))
        inst = None
        for i in ins[:8]:
            if i.op_str in ("ecx, ecx",) and i.mnemonic == "xor":
                inst = 0
            if i.mnemonic == "lea" and i.op_str == "ecx, [r8 + 1]":
                inst = 1
        if inst is not None:
            out[inst] = f[0]
    if set(out) != {0, 1}:
        raise SystemExit("dispatchers: found %s" % {k: hex(v) for k, v in out.items()})
    return out


def load_accessors(path):
    if not os.path.exists(path):
        raise SystemExit("%s missing: run `t7_builtins.py extract` first (it finds the parameter "
                         "accessors)" % path)
    with open(path, encoding="utf-8") as f:
        return {int(k, 16): v for k, v in json.load(f)["accessors"].items()}


def read_only(img, a, emu, tabs, accessors, log):
    sr = SetterRunner(img, a, emu, accessors)
    disp = find_dispatchers(img)
    for t in tabs:
        for e in t["entries"]:
            # (a) the entry's own setter
            if e.get("setter"):
                ev, err = sr.run(img.base + e["setter"], (sr.blank, img.base + e["entry_rva"], 0, 0))
                ro, msg = verdict(ev)
                named = [v for k, v in ev if k == "name"]
                e["setter_run"] = {"read_only": ro, "message": msg,
                                   "names_canon": bool(named) and named[0] == e["canon"],
                                   "stop": err or ("script error" if any(k == "error" for k, v in ev)
                                                   else "returned")}
            elif "setter" in e:
                e["setter_run"] = {"read_only": None, "message": None, "names_canon": False,
                                   "stop": "no setter (generic field write)"}
            # (b) the VM dispatch, entity number 0
            ev, err = sr.run(disp[e["inst"]], (e["cls"], 0, e["offset"], 0))
            ro, msg = verdict(ev)
            named = [v for k, v in ev if k == "name"]
            e["dispatch_run"] = {"read_only": ro, "message": msg,
                                 "names_canon": bool(named) and named[0] == e["canon"],
                                 "reached": bool(ev)}
            if e.get("writable_byte") is not None:
                e["writable_flag"] = bool(e["writable_byte"])
    log.append("dispatchers: gsc %#x, csc %#x" % (disp[0] - img.base, disp[1] - img.base))
    return {k: v - img.base for k, v in disp.items()}




def final_read_only(e):
    """Combine: the setter run decides when it decided; else the dispatch; else a null setter is
    the generic write (writable); else unknown. Returns (value or None, evidence)."""
    # A dispatch run that only read the value (no read-only message) is weak: some dispatchers
    # read the value before they look the entry up, and stop on entity 0 before the setter.
    s, d = e.get("setter_run") or {}, e.get("dispatch_run") or {}
    if s.get("read_only") is not None:
        if d.get("read_only") is None:
            dv = "dispatch did not reach the field"
        elif d["read_only"] == s["read_only"]:
            dv = "dispatch agrees"
        elif s["read_only"] and not d["read_only"]:
            dv = "dispatch read the value and stopped before the setter"
        else:
            dv = "DISPATCH DISAGREES"
        return s["read_only"], "setter run; " + dv
    if d.get("read_only") is not None:
        ev = "dispatch run" + ("" if d["read_only"] else " (value read, no read-only error)")
        if "writable_flag" in e:
            ev += "; writable flag agrees" if e["writable_flag"] == (not d["read_only"]) \
                else "; WRITABLE FLAG DISAGREES"
        return d["read_only"], ev
    if s.get("stop") == "no setter (generic field write)":
        return False, "no setter: generic field write (dispatch not reached)"
    return None, "not measured"


def kind_names(tabs):
    """Owner kind per (inst, class, group), from the read-only messages its entries raise."""
    names = {}
    for t in tabs:
        msgs = collections.Counter()
        for e in t["entries"]:
            for r in ("setter_run", "dispatch_run"):
                m = (e.get(r) or {}).get("message")
                if m and any(w in m for w in READONLY_WORDS):
                    msgs[m] += 1
        kind = None
        for m in msgs:
            for frag, k in KIND_BY_MESSAGE:
                if frag in m:
                    kind = kind or k
                    break
        names[(t["inst"], t["cls"], t["group"])] = (kind, sorted(msgs))
    return names


def extract(exe, out_path, acc_path):
    img = Image(exe)
    a = T7F
    check_anchors(img, a)
    log = ["exe %s, image base %#x, PE timestamp %d" % (os.path.basename(exe), img.base, img.timestamp)]
    emu = Emu(img)
    written = T.fnv_initialisers(img, emu, TB.FNV_IMUL, log)
    regs, heads = registrations(img, emu, a, written, log)
    tabs = tables(img, emu, regs, log)
    disp = read_only(img, a, emu, tabs, load_accessors(acc_path), log)
    kinds = kind_names(tabs)
    for t in tabs:
        k, msgs = kinds[(t["inst"], t["cls"], t["group"])]
        t["kind_from_messages"] = k
        t["readonly_messages"] = msgs
        for e in t["entries"]:
            e["read_only"], e["read_only_evidence"] = final_read_only(e)
    res = {"exe": os.path.basename(exe), "pe_timestamp": img.timestamp, "image_base": img.base,
           "anchors": {k: "%#x" % v for k, v in a.items()},
           "registration_functions": ["%#x" % h for h in heads],
           "dispatchers": {("gsc" if k == 0 else "csc"): "%#x" % v for k, v in disp.items()},
           "tables": tabs, "log": log}
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(res, f, indent=1)
    print("\n".join(log))


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    x = sub.add_parser("extract")
    x.add_argument("--exe", default=paths.get("exe.t7"))
    x.add_argument("--out", default=os.path.join(TB.OUT, "t7_fields_extract.json"))
    x.add_argument("--accessors", default=os.path.join(TB.OUT, "t7_extract.json"))
    ar = ap.parse_args()
    if ar.cmd == "extract":
        extract(ar.exe, ar.out, ar.accessors)


if __name__ == "__main__":
    main()
