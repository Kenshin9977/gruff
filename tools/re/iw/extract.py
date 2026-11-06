"""Extract one IW-engine game's builtin catalogue from its executable, to out/<game>.json
(addresses and measures only; not committed, regenerated in minutes).

    py -3.11 -I extract.py <game> [--exe PATH] [--limit N]

Steps (each logged): the tables and their roles (named.py for the builds that keep names,
idreg.py for the registration builds), then the VM's parameter interface and every handler
run (vm.py). Games and their dumps: GAMES below.
"""
import argparse
import collections
import json
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from image import Image  # noqa: E402
import typenames  # noqa: E402
import vm as vmmod  # noqa: E402
sys.path.insert(0, os.path.dirname(HERE))
from common import paths  # noqa: E402

GAMES = {
    # game: (method, note); the executable is exe.<game> in tools/re/paths.local.json
    "iw1": ("named",
            "CoD1 game module (the script builtins live in gamex86.dll; CoDMP.exe is packed)"),
    "iw3": ("named",
            "Steam iw3mp.exe, x86, plain .text (iw3sp.exe is SteamStub-encrypted)"),
    "iw4": ("plain",
            "Steam iw4mp.exe, the 2026 x64 rebuild, plain .text"),
    "iw5": ("id",
            "Steam iw5mp.exe, the 2026 x64 rebuild, plain .text"),
    "iw6": ("id",
            "Ghosts SP dump (iw6sp64_ship.exe); no MP dump exists (iw6mp64_ship.exe is encrypted)"),
    "s1": ("id", "Cordycep dump of s1_mp64_ship.exe"),
    "h1": ("id", "Cordycep dump of h1_mp64_ship.exe"),
    "iw7": ("id", "Cordycep dump of iw7_ship.exe"),
    "s2": ("static", "Cordycep dump of s2_mp64_ship.exe"),
    "iw8": ("desc", "Cordycep dump of ModernWarfare.exe, flattened"),
    "jup": ("hash", "Cordycep dump of cod23-cod.exe, flattened"),
    # single-player executables, merged into the game's catalogue by build.py
    "iw5_sp": ("id",
               "Steam iw5sp.exe, the 2026 x64 rebuild, plain .text"),
    "s1_sp": ("id", "Cordycep dump of s1_sp64_ship.exe"),
    "h1_sp": ("id", "Cordycep dump of h1_sp64_ship.exe"),
    "s2_sp": ("static", "Cordycep dump of s2_sp64_ship.exe"),
    "jup_sp": ("hash", "Cordycep dump of sp23-cod.exe, flattened"),
}


def tables_named(im, log):
    import named
    tables, chains = named.extract(im, log)
    # roles: the method chain calls several lookups in a row; the function chain one; a
    # lookup reached from no chain with a static bound is not a script table (client commands)
    roles = {}
    # of the chains calling several lookups, the method chain is the one covering the most
    # entries (IW3 has two more two-lookup chains, over console and menu tables)
    count = {t["lookup"]: t["count"] for t in tables}
    multis = sorted((f for f, lst in chains.items() if len(lst) > 1),
                    key=lambda f: -sum(count.get(lk, 0) for lk in chains[f]))
    for f in multis[1:]:
        log("  chain 0x%x (%d lookups, %d entries): not the method chain, left out" % (
            f, len(chains[f]), sum(count.get(lk, 0) for lk in chains[f])))
    keep = set(multis[:1]) | {f for f, lst in chains.items() if len(lst) == 1}
    for f, lst in chains.items():
        if f not in keep:
            continue
        for lk in lst:
            roles.setdefault(lk, []).append((f, len(lst)))
    dropped = {lk for f in multis[1:] for lk in chains[f]}
    multi = [f for f, lst in chains.items() if len(lst) > 1]
    single = [f for f, lst in chains.items() if len(lst) == 1]
    ents = []
    for t in tables:
        r = roles.get(t["lookup"], [])
        if not r and t["lookup"] in dropped:
            continue
        if not r:
            # a lookup nobody calls directly is handed to the engine by pointer (CoD1's game
            # module exports Scr_GetFunction/Scr_GetMethod): its role by the table's shape,
            # {name, handler, developer} the function table, {name, handler} a method table
            if t["static_bound"] is None:
                log("  lookup 0x%x: called by no chain and no loop bound, left out" % t["lookup"])
                continue
            f, kind = 0, "function" if t["stride"] == 12 else "method"
            log("  lookup 0x%x: called by no chain (passed by pointer): %s table by its %d-byte "
                "entries" % (t["lookup"], kind, t["stride"]))
        else:
            f, n = r[0]
            kind = "method" if n > 1 else "function"
        if kind == "function" and r and (t["static_bound"] is None or t["stride"] != 12):
            # a single-lookup chain with no loop bound (IW3's client commands) or 8-byte
            # entries (CoD1's spawn functions by classname) is not the builtin table
            log("  lookup 0x%x (chain 0x%x): not the function table (bound %s, %d-byte entries), "
                "left out" % (t["lookup"], f, t["static_bound"], t["stride"]))
            continue
        for i, (va, nm, h, ty) in enumerate(t["entries"]):
            ents.append({"name": nm, "kind": kind, "handler": h, "type": ty,
                         "where": "table 0x%x[%d] (lookup 0x%x, chain 0x%x)" % (
                             t["table"], i, t["lookup"], f),
                         "round_trip": nm not in {b[0] for b in t["round_trip_failed"]}})
    info = {"tables": [{"lookup": hex(t["lookup"]), "table": hex(t["table"]), "count": t["count"],
                        "static_bound": t["static_bound"], "round_trip_failed": len(t["round_trip_failed"])}
                       for t in tables],
            "chains": {hex(f): [hex(x) for x in lst] for f, lst in chains.items()}}
    return ents, info


def tables_reg(im, log):
    import idreg
    r = idreg.extract(im, log)
    hi, ii, ti = r["args"]
    ents = {}
    if ii:
        bases = collections.Counter(e["slot"] - 8 * e["id"] for e in r["entries"] if e["slot"])
        arrays = [b for b, _ in bases.most_common(2)]
        log("registrar arrays (slot - 8*id): %s" % ", ".join("0x%x (%d)" % (b, n) for b, n in bases.most_common(4)))
        # the function array takes the small ids, the method array those >= 0x8000
        for e in r["entries"]:
            if not e["slot"]:
                continue
            b = e["slot"] - 8 * e["id"]
            arr = arrays.index(b) if b in arrays else None
            key = (arr, e["id"])
            ents.setdefault(key, {"id": e["id"], "array": arr, "handler": e["handler"],
                                  "type": e["type"], "callers": set(), "conflict": False})
            if ents[key]["handler"] != e["handler"]:
                ents[key]["conflict"] = True
            ents[key]["callers"].add(e["caller"])
        lows = {a: min(k[1] for k in ents if k[0] == a) for a in set(k[0] for k in ents)}
        meth_arr = max(lows, key=lambda a: lows[a])
        out = []
        for (arr, i), e in sorted(ents.items(), key=lambda kv: (kv[0][1])):
            kind = "method" if arr == meth_arr and lows[meth_arr] >= 0x8000 else "function"
            out.append({"id": i, "kind": kind, "handler": e["handler"], "type": e["type"],
                        "where": "registrar 0x%x into array %d, from %s" % (
                            r["registrar"], arr, ",".join("0x%x" % c for c in sorted(e["callers"]))),
                        "conflict": e["conflict"]})
    else:
        # IW4: register(handler, 0) appends: the index is the registration order; the
        # function and method inits are told apart by their caller chain
        seq = []
        for e in r["entries"]:
            seq.append(e)
        out = []
        for i, e in enumerate(seq):
            out.append({"index": i, "kind": None, "handler": e["handler"], "type": e["type"],
                        "caller": e["caller"], "where": "registrar 0x%x call %d from 0x%x" % (
                            r["registrar"], i, e["caller"])})
    info = {"registrar": hex(r["registrar"]), "args": r["args"],
            "callers": [hex(c) for c in r["callers"]],
            "loops": sorted({(hex(a), hex(b), c, d) for a, b, c, d in r["loops"]})}
    return out, info


def tables_plain(im, log):
    """IW4 x64: register(handler, type) appends to one array (no name, no id): the bytecode's
    builtin index is the registration order. The loops are called from two roots (the
    function init and the method init), both called by one parent: the parent is run with the
    registrar hooked, and a block hook says which root each registration is under."""
    import idreg
    from iwemu import Emu
    import unicorn
    from unicorn import x86_const as X
    reg, loops = idreg.find_registrar(im, log, shape="plain")
    sites = idreg.callers(im, reg)
    loopfns = sorted(set(f for f in (idreg.pdata_start(im, s) for s in sites) if f))
    callers_of = collections.defaultdict(set)
    for lf in loopfns:
        for s in idreg.callers(im, lf):
            f = idreg.pdata_start(im, s)
            if f:
                callers_of[f].add(lf)
    roots = sorted(callers_of, key=lambda f: -len(callers_of[f]))[:2]
    parents = collections.Counter()
    for r in roots:
        for s in idreg.callers(im, r):
            f = idreg.pdata_start(im, s)
            if f:
                parents[f] += 1
    parent = [f for f, n in parents.items() if n == len(roots)]
    log("registrar 0x%x; %d loop functions; roots %s (loops called: %s); parent %s" % (
        reg, len(loopfns), [hex(r) for r in roots], [len(callers_of[r]) for r in roots],
        [hex(p) for p in parent]))
    emu = Emu(im)
    emu.install_import_stubs()
    uc = emu.uc
    calls = []
    cur = [None]

    def on_reg(uc_, addr, size, data):
        calls.append((cur[0], uc_.reg_read(X.UC_X86_REG_RCX), uc_.reg_read(X.UC_X86_REG_RDX)))
        emu.do_return(0)

    def on_root(uc_, addr, size, data):
        cur[0] = addr
    uc.hook_add(unicorn.UC_HOOK_CODE, on_reg, begin=reg, end=reg)
    for r in roots:
        uc.hook_add(unicorn.UC_HOOK_CODE, on_root, begin=r, end=r)
    uc.ctl_flush_tb()
    start = parent[0] if parent else None
    if start:
        try:
            emu.call(start, (0, 0, 0, 0), count=5000000)
        except unicorn.UcError as e:
            log("  parent run: %s" % e)
    log("registrations under the parent: %d (%s)" % (len(calls), dict(collections.Counter(
        hex(c[0]) if c[0] else None for c in calls))))
    # which root is the method init: its handlers take an entref (checked against the corpus
    # by the build); the order of the roots in the parent is the order of the index
    out = []
    for i, (root, h, t) in enumerate(calls):
        ri = roots.index(root) if root in roots else None
        out.append({"index": i, "kind": None, "root": ri, "handler": h, "type": t & 0xFFFFFFFF,
                    "where": "registrar 0x%x call %d under root 0x%x (parent 0x%x)" % (
                        reg, i, root or 0, start or 0)})
    # the method init: the root whose handlers read their first argument (the entref, in
    # ecx) before writing it; a function handler takes no argument
    import capstone
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = True
    rcx_ids = {capstone.x86.X86_REG_RCX, capstone.x86.X86_REG_ECX, capstone.x86.X86_REG_CX,
               capstone.x86.X86_REG_CL}

    def reads_rcx(h):
        o = h - im.base
        for ins in md.disasm(im.mem[o:o + 0x80], h):
            rr, ww = ins.regs_access()
            if any(r in rcx_ids for r in rr) and not (ins.mnemonic == "xor" and "ecx, ecx" in ins.op_str):
                return True
            if any(r in rcx_ids for r in ww) or ins.mnemonic in ("ret", "call", "jmp"):
                return False
        return False
    frac = {}
    for ri in range(len(roots)):
        hs = [e["handler"] for e in out if e["root"] == ri and e["handler"] != 0]
        frac[ri] = sum(1 for h in hs if reads_rcx(h)) / max(1, len(hs))
    meth = max(frac, key=lambda r: frac[r]) if frac else None
    log("handlers reading their first argument, per root: %s -> method init: root %s" % (
        {hex(roots[r]): round(v, 2) for r, v in frac.items()}, meth))
    for e in out:
        e["kind"] = "method" if e["root"] == meth else "function"
        e["where"] += "; %s init (first-argument reads %.0f%%)" % (e["kind"], 100 * frac.get(e["root"], 0))
    return out, {"registrar": hex(reg), "roots": [hex(r) for r in roots],
                 "parent": hex(start) if start else None, "rcx_read_fraction": frac}


def tables_static(im, log):
    """S2: the {handler, id} tables are plain static data, but no code references them by a
    usable address (the obfuscated dump computes them): no registrar, no lookup to emulate.
    The tables are taken from the static scan alone, each entry once (overlapping scan phases
    deduplicated by (id, handler)); the role from the id range the other id builds' registrars
    use (functions below 0x8000, methods from 0x8000). Weaker than every other game's: said so
    in each entry's evidence."""
    import idreg
    runs = [r for r in idreg.entry_runs(im) if r[3] == "id"]
    seen = {}
    for rva, stride, n, shape, ho, io in runs:
        for i in range(n):
            e = im.base + rva + stride * i
            h, eid = im.u64(e + ho), im.u64(e + io)
            if not im.is_code(h) or eid == 0 or eid >= 0x10000:
                continue
            seen.setdefault((eid, h), (e, stride, rva))
    by_id = collections.defaultdict(set)
    for (eid, h) in seen:
        by_id[eid].add(h)
    out = []
    for (eid, h), (e, stride, rva) in sorted(seen.items()):
        if len(by_id[eid]) > 1:
            continue  # two handlers for one id: not a builtin table (or ambiguous)
        out.append({"id": eid, "kind": "method" if eid >= 0x8000 else "function", "handler": h,
                    "type": None,
                    "where": "static table 0x%x (stride %d), entry 0x%x; no registrar found, role "
                             "by id range" % (im.base + rva, stride, e)})
    log("static id tables: %d runs, %d entries (ids with two handlers dropped: %d)" % (
        len(runs), len(out), sum(1 for v in by_id.values() if len(v) > 1)))
    return out, {"runs": len(runs)}


def tables_desc(im, log):
    import descreg
    r = descreg.extract(im, log)
    groups = collections.defaultdict(list)
    for e in r["entries"]:
        if e["slot"]:
            groups[e["slot"] - 8 * e["id"]].append(e)
    # the function array: the one whose ids start at the context's function base (small ids)
    lows = {b: min(x["id"] for x in v) for b, v in groups.items()}
    meth = max(lows, key=lambda b: lows[b])
    out, seen = [], {}
    for b, v in groups.items():
        kind = "method" if b == meth and len(groups) > 1 else "function"
        for e in v:
            key = (kind, e["id"])
            if key in seen:
                if seen[key]["handler"] != e["handler"]:
                    seen[key]["conflict"] = True
                continue
            d = {"id": e["id"], "kind": kind, "handler": e["handler"], "type": e["type"],
                 "where": "descriptor 0x%x entry 0x%x, stored by registrar 0x%x into the %s array" % (
                     e["desc"], e["entry"], r["registrar"], kind), "conflict": False}
            seen[key] = d
            out.append(d)
    unstored = sum(1 for e in r["entries"] if not e["slot"])
    log("entries stored: %d unique (%d table rows never stored)" % (len(out), unstored))
    return out, {"registrar": hex(r["registrar"]), "init": hex(r["init"]), "context": r["context"],
                 "descriptors": r["descriptors"]}


def tables_hash(im, log):
    import descreg
    r = descreg.extract_hash(im, log)
    out, seen = [], {}
    for e in r["entries"]:
        kind = "function" if e["list"] == 0 else "method"
        key = (kind, e["hash"])
        if key in seen:
            continue
        d = {"hash": "0x%016x" % e["hash"], "kind": kind, "handler": e["handler"], "type": e["type"],
             "stored_min": e["min"], "stored_max": e["max"],
             "where": "entry 0x%x, appended by chain 0x%x to context list %d (split +0x%x)" % (
                 e["entry"], e["chain_fn"], e["list"], e["list_cut"] or 0)}
        seen[key] = d
        out.append(d)
    return out, {"chains": r["chains"], "descriptors": r["descriptors"]}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("game", choices=sorted(GAMES))
    ap.add_argument("--exe")
    ap.add_argument("--limit", type=int)
    ap.add_argument("--out")
    a = ap.parse_args()
    method, note = GAMES[a.game]
    exe = a.exe or paths.need("exe." + a.game)  # tools/re/paths.local.json or GSC_RE_EXE_<GAME>
    out = a.out or os.path.join(HERE, "out", a.game + ".json")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    lines = []

    def log(*x):
        s = " ".join(str(v) for v in x)
        lines.append(s)
        print(s, flush=True)
    t0 = time.time()
    im = Image(exe)
    log("exe %s: %s" % (os.path.basename(exe), im.describe()))
    if method == "named":
        ents, info = tables_named(im, log)
    elif method == "desc":
        ents, info = tables_desc(im, log)
    elif method == "hash":
        ents, info = tables_hash(im, log)
    elif method == "plain":
        ents, info = tables_plain(im, log)
    elif method == "static":
        ents, info = tables_static(im, log)
    else:
        ents, info = tables_reg(im, log)
    kinds = collections.Counter(e["kind"] for e in ents)
    log("entries: %d %s" % (len(ents), dict(kinds)))
    if a.game == "iw1":
        # CoD1's script VM (the accessors, the parameter count) is in CoDMP.exe, which is
        # packed; gamex86.dll reaches it only through the syscall pointer the engine passes at
        # load (0 in the file): no handler run can measure anything here
        import iw1vm
        V1 = iw1vm.IW1VM(im, log)
        hs = sorted(set((e["handler"], e["kind"]) for e in ents))
        res = {}
        for h, k in hs:
            res["%x:%s" % (h, k)] = V1.measure(h, k)
        log("handler runs through the engine slots: %d handlers, %d with an arity" % (
            len(res), sum(1 for r in res.values() if r.get("arity"))))
        stops = collections.Counter(r["runs"][iw1vm.NMAX][1] for r in res.values() if not r.get("arity"))
        log("no arity, why (the N=16 run): %s" % stops.most_common(6))
        doc = {"game": a.game, "exe": exe, "note": note, "method": method,
               "pe_timestamp": im.timestamp, "image_base": hex(im.base), "x64": im.x64,
               "info": info, "entries": ents,
               "runs": {k: {kk: (vv if kk != "runs" else {str(n): list(t) for n, t in vv.items()})
                            for kk, vv in r.items()} for k, r in res.items()},
               "vm": {"G": "0x0", "T": "0x0", "switches": [], "errors": [], "type_names": [],
                      "type_table": None, "accessors": {},
                      "slots": {"count": hex(V1.nump or 0), "accessors": [hex(s) for s in V1.acc],
                                "error": hex(V1.err or 0)}},
               "log": lines, "seconds": round(time.time() - t0)}
        for e in doc["entries"]:
            if isinstance(e.get("handler"), int):
                e["handler"] = hex(e["handler"])
        with open(out, "w") as f:
            json.dump(doc, f, indent=1)
        log("wrote %s" % out)
        return
    tva, tn = typenames.find(im)
    log("value type names: %s" % ("table 0x%x, %d names" % (tva, len(tn)) if tva else
                                  "no table in this build: types 0-6 labelled as in the other builds"))
    hs = sorted(set((e["handler"], e["kind"] or "function") for e in ents))
    V = vmmod.VM(im, hs, log, ctx_mode=method in ("desc", "hash"))
    V.type_names = dict(enumerate(tn)) if tn else dict(enumerate(
        ["undefined", "object", "string", "localized string", "vector", "float", "int"]))
    top = V.find_globals()
    acc = V.probe_accessors(top)
    V.find_raise_switch(acc)
    V.type_acceptance(acc)
    for c, x in acc.items():
        lab = V.label(x)
        x["label"] = lab
        x["neutral"] = 0 if lab in ("int", "float", "any") else V.zero_buf
        x["numeric"] = lab in ("int", "float")
        log("  accessor 0x%x: %d handlers call it, index in %s, accepts %s -> %s" % (
            c, V.callee_freq.get(c, 0), x["loc"], x.get("accepts"), lab))
    V.setup_runner(acc)
    res = {}
    t1 = time.time()
    for i, (h, k) in enumerate(hs[:a.limit] if a.limit else hs):
        res["%x:%s" % (h, k)] = V.measure(h, k)
        if i % 200 == 199:
            log("  handlers run: %d (%.0f s)" % (i + 1, time.time() - t1))
    ok = sum(1 for r in res.values() if r.get("arity"))
    log("handler runs: %d handlers, %d with an arity (%d strict, %d lenient), %.0f s" % (
        len(res), ok, sum(1 for r in res.values() if r.get("mode") == "strict" and r.get("arity")),
        sum(1 for r in res.values() if r.get("mode") == "lenient"), time.time() - t1))
    stops = collections.Counter(r["runs"][vmmod.NMAX][1] for r in res.values() if not r.get("arity"))
    log("no arity, why (the N=%d run): %s" % (vmmod.NMAX, stops.most_common(8)))
    doc = {"game": a.game, "exe": exe, "note": note, "method": method,
           "pe_timestamp": im.timestamp, "image_base": hex(im.base), "x64": im.x64,
           "info": info, "entries": ents,
           "vm": {"G": hex(V.G), "T": hex(V.T), "ctx": hex(V.ctx) if V.ctx else None, "switches": [(hex(g), s) for g, s in V.switches],
                  "errors": [hex(e) for e in V.errors],
                  "type_names": tn, "type_table": hex(tva) if tva else None,
                  "accessors": {hex(c): {"loc": x["loc"], "accepts": x.get("accepts"),
                                         "label": x["label"], "callers": V.callee_freq.get(c, 0)}
                                for c, x in acc.items()}},
           "runs": {k: {kk: (vv if kk != "runs" else {str(n): list(t) for n, t in vv.items()})
                        for kk, vv in r.items() if kk != "types"} | {
                       "types": {str(i): [hex(x) for x in v] for i, v in r.get("types", {}).items()}}
                    for k, r in res.items()},
           "log": lines, "seconds": round(time.time() - t0)}
    for e in doc["entries"]:
        for k in ("handler", "caller"):
            if isinstance(e.get(k), int):
                e[k] = hex(e[k])
    with open(out, "w") as f:
        json.dump(doc, f, indent=1, default=lambda o: sorted(o) if isinstance(o, set) else str(o))
    log("wrote %s (%.0f s)" % (out, time.time() - t0))


if __name__ == "__main__":
    main()
