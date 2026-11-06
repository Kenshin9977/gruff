"""Extract the Black Ops III (T7) builtin function and method catalogue from the decrypted, flattened
game executable, statically and under Unicorn. The game is never run.

    py -3.11 -I treyarch/t7_builtins.py extract [--exe PATH (default: exe.t7 in tools/re/paths.local.json)] [--out out/t7_extract.json]

Steps (each one checked by a second, independent measure; see README.md):

1. Tables are built at static-init time: the dump holds zeros where the 0x20-byte entries
   {u32 canon, u32 min, u32 max, u32 pad, u64 handler, u32 type, u32 pad} will be. Every
   function that multiplies by the FNV prime 0x01000193 is emulated once (they hash the names
   at init); the writes give the tables.
2. The four lookups (CSC/GSC x function/method) are found from the VM value reader that resolves
   a serialised builtin pointer: it is emulated for each (inst, is-function flag) and the lookup it
   calls is recorded. That gives each lookup its role from the game's own code.
3. Each lookup is emulated with an absent canon id (0): it scans every table it owns, so its
   4-byte reads at a 32-byte stride give each table's address and entry count in lookup order.
   Measure 2: the static loop bound (`cmp eax, N`) of each table scan, where one exists.
4. Every entry is looked up again through the engine (lookup(canon) under emulation): the
   handler, min, max and type the game returns must be the entry's (first match wins, so a
   shadowed duplicate is reported, not dropped).
5. Each handler is run under Unicorn with N parameters on a fake VM stack, with the parameter
   accessors (found by their own error strings) hooked: the accessor called for each index gives
   its type, the indices read with N = min and N = max show the optional ones. Unless --no-fake-self,
   a believable `self` (t7_self.py) lets handlers that check their entity get past the check.

The engine-neutral pieces are in ../common; this file is the T7 profile and what is truly T7's
(the init-time tables, the value reader's roles, the lookup convention).
"""
import argparse
import json
import os
import re
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common.image import Image  # noqa: E402
from common.emu import Emu, unicorn, X  # noqa: E402
from common import tables as T  # noqa: E402
from common import handlers as H  # noqa: E402
from common import paths  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(os.path.dirname(HERE), "out")
FNV_IMUL = re.compile(rb"\x69[\xc0-\xff]\x93\x01\x00\x01")
LAYOUT = T.LAYOUT_T7

# Engine-specific anchors. Each is a hypothesis checked at run time (the check names what fails).
T7 = {
    # VM value reader: case "builtin pointer" reads a 1-byte is-function flag, then the canon id,
    # then calls one of four lookups chosen by (inst == 1, flag == 1).
    "value_reader": 0x7FF626E55CD0 - 0x7FF625B80000,
    "reader_read": 0x7FF627DF78B0 - 0x7FF625B80000,
    # scrVmPub[inst]: top at +0, outparamcount at +0x18, stride 0x8a40 (read off Scr_GetInt).
    "vmpub": 0x5124860, "vmpub_stride": 0x8A40, "vmpub_top": 0x0, "vmpub_outparamcount": 0x18,
    # handlers: void(inst) and void(inst, entref); accessors (inst, index[, out])
    "handler_args": lambda inst, method, self_ref: (inst, self_ref) if method else (inst,),
    "index_arg": 1, "out_arg": 2,
}


# ---------------------------------------------------------------------------------------------
# 2. roles from the value reader
def find_roles(img, emu, prof, log):
    reader = img.base + prof["value_reader"]
    read = img.base + prof["reader_read"]
    feed = []
    called = []
    sizes = []
    uc = emu.uc

    def on_read(uc, addr, size, ud):
        # read(reader, size, dst): write the next fed value, return.
        n = uc.reg_read(X.UC_X86_REG_EDX)
        dst = uc.reg_read(X.UC_X86_REG_R8)
        v = feed.pop(0) if feed else 0
        sizes.append(n)
        uc.mem_write(dst, v.to_bytes(8, "little")[:n])
        emu.ret()

    def on_call(uc, addr, size, ud):
        ins = next(img.md.disasm(bytes(uc.mem_read(addr, 16)), addr))
        if ins.mnemonic == "call" and ins.op_str.startswith("0x"):
            called.append(int(ins.op_str, 16))

    f = img.func(reader)
    h1 = uc.hook_add(unicorn.UC_HOOK_CODE, on_read, begin=read, end=read)
    h2 = uc.hook_add(unicorn.UC_HOOK_CODE, on_call, begin=f[0], end=f[1])
    out = emu.alloc(64)
    roles = {}
    for vtype in range(1, 27):
        for inst in (0, 1):
            for flag in (0, 1):
                feed[:] = [flag, 0]
                called.clear()
                sizes.clear()
                uc.mem_write(out, b"\0" * 64)
                uc.mem_write(out + 8, struct.pack("<I", vtype))
                emu.run(reader, (inst, emu.alloc(64), out), count=200_000)
                if sizes == [1, 4]:  # this value type reads a flag byte, then the canon id
                    roles.setdefault((inst, flag), set()).add(
                        (vtype, tuple(c for c in called if c != read)))
    uc.hook_del(h1)
    uc.hook_del(h2)
    # keep the calls that differ between the four (inst, flag) cases: the lookups. A call made in
    # every case (the stack cookie check) is not one.
    common = None
    for s in roles.values():
        for v, cs in s:
            common = set(cs) if common is None else common & set(cs)
    roles = {k: {(v, t) for v, cs in s for t in cs if t not in common} for k, s in roles.items()}
    res = {}
    for (inst, flag), s in sorted(roles.items()):
        name = ("csc" if inst == 1 else "gsc") + ("_function" if flag == 1 else "_method")
        targets = {t for v, t in s}
        if len(targets) != 1:
            raise SystemExit("value reader: %s reaches %s, expected one lookup" % (name, targets))
        res[name] = targets.pop()
        log.append("role %s: lookup %#x (value type %s, inst %d, is-function byte %d)"
                   % (name, res[name], sorted(v for v, t in s), inst, flag))
    if len(res) != 4 or len(set(res.values())) != 4:
        raise SystemExit("value reader: expected 4 distinct lookups, got %s" % res)
    return res


# ---------------------------------------------------------------------------------------------
# 4. lookup round trip: lookup(canon, &type, &min, &max) -> handler
def lookup_args(emu, canon):
    out = emu.alloc(32)
    emu.uc.mem_write(out, b"\xee" * 32)
    return out, (canon, out, out + 8, out + 16)


def round_trip(emu, va, canon):
    out, args = lookup_args(emu, canon)
    r, err = emu.run(va, args)
    t, mn, mx = struct.unpack("<I4xI4xI", bytes(emu.uc.mem_read(out, 20)))
    return r, t, mn, mx, err


# ---------------------------------------------------------------------------------------------
def extract(exe, out_path, with_handlers=True, fake_self=True):
    img = Image(exe)
    prof = T7
    log = ["exe %s, image base %#x, PE timestamp %d, size %d" % (os.path.basename(exe), img.base, img.timestamp,
                                                                 img.size)]
    emu = Emu(img)
    # roles on a pristine image: after the initialisers the reader takes other paths
    roles = find_roles(img, Emu(img), prof, log)
    written = T.fnv_initialisers(img, emu, FNV_IMUL, log)
    result = {"exe": os.path.basename(exe), "pe_timestamp": img.timestamp,
              "image_base": img.base, "lookups": {}, "entries": []}
    for role in ("gsc_function", "gsc_method", "csc_function", "csc_method"):
        va = roles[role]
        tables = T.trace_lookup(emu, va, LAYOUT, lookup_args(emu, 0)[1], written)
        sbs = T.static_bounds(img, [a for a, ents in tables])
        tinfo = []
        for ti, (a, ents) in enumerate(tables):
            sb = sbs[a]
            tinfo.append({"rva": a - img.base, "count": len(ents),
                          "static_bounds": sorted(sb),
                          "static_agrees": (not sb) or (len(ents) in sb)})
            for k, e in enumerate(ents):
                r, rt, rmn, rmx, err = round_trip(emu, va, e["hash"])
                result["entries"].append({
                    "role": role, "table": ti, "index": k, "canon": e["hash"],
                    "min": e["min"], "max": e["max"], "type_field": e["type"],
                    "handler_rva": e["handler"] - img.base,
                    "lookup_handler_rva": (r - img.base) if r else None,
                    "lookup_type": rt if rt != 0xEEEEEEEE else None,
                    "lookup_min": rmn, "lookup_max": rmx, "lookup_err": err,
                })
        result["lookups"][role] = {"rva": va - img.base, "tables": tinfo}
        log.append("%s: %d tables, %d entries, static bound agrees on %d/%d tables with one"
                   % (role, len(tables), sum(t["count"] for t in tinfo),
                      sum(1 for t in tinfo if t["static_bounds"] and t["static_agrees"]),
                      sum(1 for t in tinfo if t["static_bounds"])))
    if with_handlers:
        handlers = sorted({e["handler_rva"] + img.base for e in result["entries"]})
        acc, resolvers = H.find_accessors(img, handlers, log, vm=prof)
        result["accessors"] = {"%#x" % (a - img.base): v["label"] for a, v in acc.items()}
        result["entref_resolvers"] = ["%#x" % (a - img.base) for a in resolvers]
        result["accessor_index_args"] = {"%#x" % (a - img.base): v["index_arg"]
                                         for a, v in acc.items() if "index_arg" in v}
        stops = H.scr_error_candidates(img, acc)
        log.append("stops (Scr_Error): %s" % [hex(s - img.base) for s in stops])
        fs = None
        if fake_self:
            import t7_self
            fs = t7_self.FakeSelf(img, log)
        runner = H.HandlerRunner(img, prof, acc, resolvers, stops,
                                 self_obj=fs.provide if fs else None)
        if fs:
            fs.attach(runner)
        done = {}
        for i, e in enumerate(result["entries"]):
            inst = 1 if e["role"].startswith("csc") else 0
            method = e["role"].endswith("method")
            key = (e["handler_rva"], inst, method, e["min"], e["max"])
            if key not in done:
                def prof_fn(e=e, inst=inst, method=method):
                    return H.handler_profile(runner, e["handler_rva"] + img.base, inst, method,
                                             e["min"], e["max"])
                done[key] = fs.best_profile(prof_fn, method) if fs else prof_fn()
            e["handler"] = done[key]
            if i % 500 == 0:
                print("handlers %d/%d" % (i, len(result["entries"])), file=sys.stderr)
        if fs:
            log.extend(fs.summary())
    result["log"] = log
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(result, f, indent=1)
    print("\n".join(log))


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    x = sub.add_parser("extract")
    x.add_argument("--exe", default=paths.get("exe.t7"))
    x.add_argument("--out", default=os.path.join(OUT, "t7_extract.json"))
    x.add_argument("--no-handlers", action="store_true")
    x.add_argument("--no-fake-self", action="store_true")
    a = ap.parse_args()
    if a.cmd == "extract":
        extract(a.exe, a.out, not a.no_handlers, not a.no_fake_self)


if __name__ == "__main__":
    main()
