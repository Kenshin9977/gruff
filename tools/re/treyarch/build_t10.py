"""From t10_builtins.py's extractions (out/t11_extract.json, out/t10_extract.json, and
out/t10sp_extract.json for the report): names, validation, gruff-data/builtins/t11.toml and
t10.toml, reports/t10_builtins.md.

    py -3.11 -I treyarch/build_t10.py [--data ../../../gruff-data]

Names: a hash is named only by a string whose t10.function hash equals it (the table stores the
full 64-bit FNV value, the 60-bit family value is its low 60 bits), taken from (in order) the
BO7 GSC archive's name lists, GrayWolf's name indexes (.wni), cod-hash-recovery, the T7 builtin
names (gruff-data builtins/t7.toml, with their own source), the identifiers of the BO7 corpus.
Validation: the BO7 corpus call sites (T11); BO6 has no corpus (its scripts are server-side and
never shipped), so T10 is cross-checked against T11 (same hash, same VM and kind).
"""
import argparse
import collections
import json
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
RE = os.path.dirname(HERE)
sys.path.insert(0, RE)
from common.hashes import t10_function, fnv1a64, T10_SALT  # noqa: E402
from common.image import user_site  # noqa: E402
from common import corpus  # noqa: E402
from common import handlers as H  # noqa: E402
from common import names as N  # noqa: E402
from common import report as R  # noqa: E402
from common import paths  # noqa: E402

M60 = (1 << 60) - 1
VARIADIC = 255  # BO7's linker: u8 argument count <= max, so 255 admits any call
CORPUS = paths.get("bo7_corpus")  # tools/re/paths.local.json or GSC_RE_<KEY>
ARCHIVE = paths.get("bo7_archive_names")
WNI = paths.get("graywolf_package_index")
CHR = os.path.join(paths.get("cod_hash_recovery") or "", "data", "names", "bo7")
INTRINSICS = [
    "isdefined", "vectorscale", "anglestoup", "anglestoright", "anglestoforward",
    "angleclamp180", "vectortoangles", "abs", "gettime", "getdvar", "getdvarint",
    "getdvarfloat", "getdvarvector", "getdvarcolorred", "getdvarcolorgreen",
    "getdvarcolorblue", "getdvarcoloralpha", "getfirstarraykey", "getnextarraykey",
    "getlastarraykey", "getprevarraykey", "profilestart", "profilestop", "waitrealtime",
    "isarray", "isint", "isfloat", "isstring", "isvec", "isfunctionptr", "isentity", "isstruct",
    "ishash", "isbool", "issubstr", "tolower", "getthread", "waittillframeend", "waitframe",
    "istrue",
]
GAMES = {
    "t11": ("Black Ops 7 (T11)", "cod.exe (BO7), Cordycep dump flattened (bo7_flat.exe)"),
    "t10": ("Black Ops 6 (T10)", "cod24-cod.exe (BO6 multiplayer), Cordycep dump flattened "
                                 "(bo6_flat.exe)"),
}


def full_hash(name):
    b = name.lower().encode("latin1")
    return fnv1a64(b[:1] + T10_SALT + b[1:], 0x1C2F2E3C8A257D07, 0x10000000233)


def wni_names(path):
    """GrayWolf .wni: 'WNI ' u16 version, u32 count, u32 packed, u32 unpacked, LZ4 block of
    {u64 key, NUL-terminated name}. Only the names are used (re-hashed)."""
    if not os.path.exists(path):
        return
    user_site()
    import lz4.block
    with open(path, "rb") as f:
        d = f.read()
    if d[:4] != b"WNI ":
        return
    count, packed, unpacked = struct.unpack_from("<III", d, 6)
    raw = lz4.block.decompress(d[18:18 + packed], uncompressed_size=unpacked)
    o = 0
    while o + 8 < len(raw):
        e = raw.index(b"\0", o + 8)
        yield raw[o + 8:e].decode("latin1")
        o = e + 1


def chr_names():
    if os.path.isdir(CHR):
        for f in sorted(os.listdir(CHR)):
            for n in N.csv_column(os.path.join(CHR, f), 1):
                yield n.split(":")[-1]


def t7_names(data):
    """Names of the T7 catalogue with their own name_source (the string is what counts)."""
    import tomllib
    p = os.path.join(data, "builtins", "t7.toml")
    with open(p, "rb") as f:
        t = tomllib.load(f)
    out = collections.defaultdict(list)
    for b in t["builtin"]:
        if b.get("name"):
            out[b["name_source"]].append(b["name"])
    return out


def load_names(hashes, data, corpus_words, which):
    t7 = t7_names(data)
    sources = [
        ("bo7-gsc-archive", N.csv_column(os.path.join(ARCHIVE, "function.csv"), 1)),
        ("bo7-gsc-archive", N.csv_column(os.path.join(ARCHIVE, "all_names.tsv"), 4, "\t")),
        ("graywolf-name-index", wni_names(os.path.join(WNI, "%s_names.wni" % which))),
        ("graywolf-name-index", wni_names(os.path.join(WNI, "bo7_names.wni"))),
        ("graywolf-name-index", wni_names(os.path.join(WNI, "bo6_names.wni"))),
        ("cod-hash-recovery", chr_names()),
    ] + [(src, t7[src]) for src in ("official-doc", "mod-tools-scripts", "bo3-hash-list")] + [
        ("corpus-scripts", corpus_words),
    ]
    return N.name_hashes(hashes, sources, t10_function)


# ---------------------------------------------------------------------------------------------
def normalise(ex):
    """One record per (vm, kind, table entry) over the sets that hold it; entries of sets no VM
    selects are kept apart (`unassigned`)."""
    sets = ex["sets"]
    out, unassigned = [], []
    for e in ex["entries"]:
        held = [o for o in e["sets"] if sets[o]["vm"]]
        if not held:
            unassigned.append(e)
            continue
        by_vm = collections.defaultdict(list)
        for o in held:
            by_vm[sets[o]["vm"]].append(o)
        for vm, os_ in by_vm.items():
            r = dict(e)
            r["vm"], r["set_ids"] = vm, os_
            r["modes"] = sorted(sets[o]["mode_name"] for o in os_)
            r["shadowed"] = not any(e["sets"][o]["returned"] for o in os_)
            r["hash"] = e["hash_full"] & M60
            r["key"] = r["hash"]
            out.append(r)
    return out, unassigned


def build_game(game, ex, data, names, nsrc, arity_checked):
    E, unassigned = normalise(ex)
    for e in E:
        e["name"] = names.get(e["hash"])
        e["name_source"] = nsrc.get(e["hash"])
        if arity_checked:
            e["min_eff"], e["max_eff"] = e["min"], e["max"]
        else:
            e["min_eff"], e["max_eff"] = -1, -1
    return E, unassigned


def toml_entry(e, game, arity_checked):
    hp = e.get("handler") or {}
    ev = ["table 0x%x[%d] (%s; sets %s)" % (e["table_rva"], e["index"], e["kind"],
                                            "+".join(e["modes"]))]
    ev.append("builders emulated == dump sets; lookup round trip %s" % (
        "shadowed by a duplicate hash" if e["shadowed"] else "ok"))
    if not arity_checked:
        ev.append("entry bytes min %d max %d stored, never checked by this linker"
                  % (e["min"], e["max"]))
    if hp:
        ev.append("handler run N=%d read %s (%s), N=%d read %s" % (
            hp["n_max"], hp["read_at_max"], hp["stop_at_max"].split(" at ")[0], hp["n_min"],
            hp["read_at_min"]))
    return {
        "name": e["name"], "name_source": e["name_source"], "hash": e["hash"],
        "vm": e["vm"], "kind": e["kind"], "min": e["min_eff"], "max": e["max_eff"],
        "variadic_max": VARIADIC if arity_checked else None,
        "type_flags": e["type"], "dev_only": e["type"] == 1,
        "param_types": H.param_types(hp) if hp else None, "shadowed": e["shadowed"],
        "handler_rva": e["handler_rva"], "evidence": "; ".join(ev),
        "table": "0x%x %s" % (e["table_rva"], "+".join(e["modes"])),
        "index": e["index"],
    }


def write_toml(game, ex, E, intrinsics, path, arity_checked):
    label, exe = GAMES[game]
    head = ["%s engine builtins: every entry of the builtin sets the VMs select (per VM and game"
            % label,
            "mode: sp, mp), extracted from the decrypted game executable by",
            "gruff/tools/re/treyarch/t10_builtins.py (set builders and lookups run under "
            "Unicorn).",
            "hash: t10.function (60-bit; the table stores the full 64-bit salted FNV-1a, whose "
            "low 60 bits these are).",
            "[[intrinsic]]: names the decompiled corpus calls that no table holds.",
            "Schema: ../schema/builtins.schema.json. Facts only; no documentation text."]
    if game == "t10":
        head += ["No corpus: BO6's scripts run server-side and are not shipped, so no call site "
                 "validates this file;",
                 "it is cross-checked against t11.toml (same hash, VM and kind). The entries "
                 "store min/max bytes but",
                 "BO6's linker checks none (no 'Invalid number of params' path): arity is "
                 "written as unchecked (-1/-1)."]
    tops = collections.Counter(e["hash_full"] >> 60 for e in E)
    R.write_builtins_toml(
        path, head,
        [("game", game), ("license", "CC0-1.0"), ("source", "extracted from game executable"),
         ("hash_family", "t10.function")],
        [("tool", R.toml_str("gruff/tools/re/treyarch/t10_builtins.py + build_t10.py")),
         ("executable", R.toml_str(exe)),
         ("pe_timestamp", "%d" % ex["pe_timestamp"]),
         ("entry_layout", R.toml_str(
             "u64 hash (the full 64-bit t10.function FNV-1a before its 60-bit mask; top 4 bits "
             "vary: %d distinct values), u64 handler, u8 type (1: dev only), u8 min_args, u8 "
             "max_args, 5 bytes 0 (0x18 bytes)%s" % (
                 len(tops), "" if arity_checked else "; min/max stored but not checked"))),
         ] + ([("variadic_max", "%d" % VARIADIC)] if arity_checked else []) + [
         ("counts", R.vm_kind_counts(E))],
        lookups_of(ex),
        [toml_entry(e, game, arity_checked) for e in E], hash_digits=15, intrinsics=intrinsics)


def lookups_of(ex):
    """Per role, the lookup and the tables of the sets of that VM (informative)."""
    out = {}
    for kind in ("function", "method"):
        for vm in ("gsc", "csc"):
            tabs = [(t["rva"], t["count"]) for t in ex["tables"] if t["kind"] == kind and any(
                ex["sets"][o]["vm"] == vm for o in t["sets"])]
            if tabs:
                out["%s_%s" % (vm, kind)] = (ex["lookups"][kind]["rva"], sorted(tabs))
    return out


# ---------------------------------------------------------------------------------------------
def merged_for_calls(E):
    """For the call-site check: one entry per (hash, vm, kind), the union of the sets' ranges
    (a call is within range when some mode's entry admits it)."""
    m = {}
    for e in E:
        if e["shadowed"]:
            continue
        k = (e["key"], e["vm"], e["kind"])
        if k not in m:
            m[k] = dict(e, min=e["min_eff"], max=e["max_eff"], _all=[e])
        else:
            m[k]["min"] = min(m[k]["min"], e["min_eff"])
            m[k]["max"] = max(m[k]["max"], e["max_eff"])
            m[k]["_all"].append(e)
    return list(m.values())


def corpus_key(name):
    """The decompiled corpus writes unknown names as function_<hex> of the full 64-bit hash
    (function_83bf4b34fbbe124b is gettime's table hash); t10.function keeps its low 60 bits."""
    h = corpus.hashed_name(name)
    return (h & M60) if h is not None else t10_function(name)


def intrinsic_rows(E, calls, defined):
    keys = collections.defaultdict(set)
    for e in E:
        if not e["shadowed"]:
            keys[e["key"]].add("%s_%s" % (e["vm"], e["kind"]))
    rows = []
    for name in INTRINSICS:
        h = t10_function(name)
        args = sorted({a for (n, k, vm, a) in calls if n == name})
        sites = sum(c for (n, k, vm, a), c in calls.items() if n == name)
        rows.append({"name": name, "hash": "0x%015x" % h, "also_builtin": sorted(keys.get(h, ())),
                     "corpus_args": args, "sites": sites})
    return rows


# ---------------------------------------------------------------------------------------------
def counts_table(w, ex, E):
    w("| set | VM | mode | functions | methods | builder |")
    w("|---|---|---|---|---|---|")
    for o, s in sorted(ex["sets"].items(), key=lambda kv: (kv[1]["vm"] or "~", kv[1]["mode"] or 9)):
        w("| 0x%s | %s | %s | %d | %d | 0x%x |" % (o, s["vm_name"] or "**none (not held by a VM)**",
                                                  s["mode_name"] or "-", s["functions"],
                                                  s["methods"], s["builder_rva"]))
    w("")


def cross_t10_t11(r10, r11):
    """T10 effective entries vs T11 by (hash, vm, kind): presence, stored min/max agreement."""
    k11 = collections.defaultdict(list)
    for e in r11["E"]:
        if not e["shadowed"]:
            k11[(e["hash"], e["vm"], e["kind"])].append(e)
    kinds11 = collections.defaultdict(set)
    for (h, vm, k) in k11:
        kinds11[h].add((vm, k))
    st = collections.Counter()
    dis = []
    seen = set()
    for e in r10["E"]:
        if e["shadowed"]:
            continue
        k = (e["hash"], e["vm"], e["kind"])
        if k in seen:
            continue
        seen.add(k)
        st["t10"] += 1
        if k in k11:
            st["shared"] += 1
            if any(x["min"] == e["min"] and x["max"] == e["max"] for x in k11[k]):
                st["arity_same"] += 1
            else:
                dis.append((e, k11[k][0]))
            st["min_gt_max"] += e["min"] > e["max"]
        elif e["hash"] in kinds11:
            st["other_kind"] += 1
    return st, dis


def write_report(path, ex, res, nfiles):
    L = []
    w = L.append
    r11, r10 = res["t11"], res["t10"]
    w("# T10 (Black Ops 6) and T11 (Black Ops 7) builtin catalogues: extraction and validation")
    w("")
    w("Generated by `treyarch/build_t10.py` from `treyarch/t10_builtins.py extract --game t11 | "
      "t10 | t10sp`.")
    w("")
    for g, title in (("t11", "Black Ops 7"), ("t10", "Black Ops 6 multiplayer"),
                     ("t10sp", "Black Ops 6 campaign")):
        if g not in ex:
            continue
        x = ex[g]
        w("## %s: extraction log (%s, PE timestamp %d)" % (title, x["exe"], x["pe_timestamp"]))
        w("")
        w("```")
        L.extend(x["log"])
        w("```")
        w("")
        w("Sets:")
        w("")
        counts_table(w, x, None)
    for g, r in (("t11", r11), ("t10", r10)):
        E = r["E"]
        live = [e for e in E if not e["shadowed"]]
        keys = {(e["hash"], e["vm"], e["kind"]) for e in live}
        named = {k for k in keys if r["names"].get(k[0])}
        src = collections.Counter(r["nsrc"][k[0]] for k in named)
        cnt = collections.Counter((e["vm"], e["kind"]) for e in live)
        w("## %s catalogue (%s.toml)" % (g.upper(), g))
        w("")
        w("- Records: %d (one per table entry and VM; %d shadowed: a duplicate hash in the same "
          "set, the lookup returns the other). Effective: %s." % (
              len(E), len(E) - len(live), ", ".join("%s %s %d" % (vm, k, cnt[(vm, k)])
                                                    for vm in ("gsc", "csc")
                                                    for k in ("function", "method"))))
        w("- Distinct (hash, VM, kind): %d; named: %d (%s): %s." % (
            len(keys), len(named), R.pct(len(named), len(keys)),
            ", ".join("%s %d" % kv for kv in src.most_common())))
        nl = [e for e in live if e["name"]]
        w("- Stored hash = full 64-bit FNV-1a (t10.function before its mask) on %d/%d named "
          "entries; top nibble values: %d distinct." % (
              sum(full_hash(e["name"]) == e["hash_full"] for e in nl), len(nl),
              len({e["hash_full"] >> 60 for e in live})))
        w("- Type byte (+0x10): %s (1 is dev only: the linker's devblock error)." % ", ".join(
            "%d: %d" % kv for kv in sorted(collections.Counter(e["type"] for e in live).items())))
        modes = collections.Counter("+".join(e["modes"]) for e in live)
        w("- Entries by the sets holding them: %s." % ", ".join(
            "%s %d" % kv for kv in modes.most_common()))
        w("- Unassigned (sets no VM of the dump holds, not in the TOML): %d entries." %
          len(r["unassigned"]))
        w("")
        if any(e.get("handler") for e in E):
            hl, hst = R.handler_section(E, VARIADIC if r["arity"] else None)
            L.extend(hl)
            acc = ex[g].get("handler_runs", {}).get("accessors", {})
            w("Accessors (label = value tags accepted without Scr_Error): %s." % ", ".join(
                "0x%s %s (%d callers)" % (a, v["label"], v["callers"]) for a, v in
                sorted(acc.items(), key=lambda kv: -kv[1]["callers"])))
            w("")
    # T11 call sites
    cs = r11["cs"]
    L.extend(R.calls_section(cs, "bo7-gsc-archive-work/decrypted-v2, %d files" % nfiles, VARIADIC))
    called, live_n = R.coverage(r11["merged"], cs)
    nm = sum(1 for k in cs["called_names"] if r11["names"].get(k[0]))
    w("Coverage: %d/%d effective (hash, VM, kind) entries are called at least once by the "
      "corpus (%s); of the %d distinct builtins called, %d are named (%s)." % (
          called, live_n, R.pct(called, live_n), len(cs["called_names"]), nm,
          R.pct(nm, len(cs["called_names"]))))
    w("")
    w("### Unknown names called most (neither script-defined nor a builtin; 30)")
    w("")
    w(", ".join("%s (%s %s) %d" % (n, k, vm, c) for (n, k, vm), c in cs["unknown"].most_common(30)))
    w("")
    # T10 cross-check
    st, dis = cross_t10_t11(r10, r11)
    w("## T10 cross-check against T11 (no BO6 corpus)")
    w("")
    w("BO6's scripts run server-side and are not shipped: there are no call sites. Measures:")
    w("")
    w("- T10 effective (hash, VM, kind): %d; also in T11 with the same VM and kind: %d (%s); "
      "in T11 under another VM/kind only: %d." % (st["t10"], st["shared"],
                                                  R.pct(st["shared"], st["t10"]),
                                                  st["other_kind"]))
    w("- Stored min/max equal to T11's on shared entries: %d/%d (%s). Stored min > max: %d "
      "(BO6's linker reads neither byte, so the TOML writes arity unchecked)." % (
          st["arity_same"], st["shared"], R.pct(st["arity_same"], st["shared"]),
          st["min_gt_max"]))
    w("")
    w("| hash | name | VM | kind | T10 stored min..max | T11 min..max |")
    w("|---|---|---|---|---|---|")
    for e, f in dis[:40]:
        w("| 0x%015x | %s | %s | %s | %d..%d | %d..%s |" % (
            e["hash"], e["name"] or "", e["vm"], e["kind"], e["min"], e["max"], f["min"],
            R.fmt_max(f["max"], VARIADIC)))
    w("")
    # unassigned BO6 sets vs T11 client sets
    t11csc = {e["hash"] for e in r11["E"] if e["vm"] == "csc"}
    t11gsc = {e["hash"] for e in r11["E"] if e["vm"] == "gsc"}
    w("### BO6 sets no VM holds in the dump")
    w("")
    w("| set | functions | methods | hashes also in T11 csc sets | also in T11 gsc sets |")
    w("|---|---|---|---|---|")
    for o, s in ex["t10"]["sets"].items():
        if s["vm"]:
            continue
        hs = {e["hash_full"] & M60 for e in ex["t10"]["entries"] if o in e["sets"]}
        w("| 0x%s | %d | %d | %d | %d |" % (o, s["functions"], s["methods"], len(hs & t11csc),
                                            len(hs & t11gsc)))
    w("")
    if "t10sp" in ex:
        x = ex["t10sp"]
        w("## BO6 campaign executable (not written to t10.toml)")
        w("")
        w("Same engine (static 0x18-byte tables, set builders, bsearch lookups, no arity check), "
          "but its names hash as FNV-1a over name + salt (the multiplayer executable and BO7 put "
          "the salt after the first character: t10.function). gsc_hash has no family for the "
          "campaign variant, so its sets are reported here only: %s." % ", ".join(
              "0x%s: %d functions, %d methods" % (o, s["functions"], s["methods"])
              for o, s in x["sets"].items()))
        w("")
    w("## Compiler intrinsics")
    w("")
    w("| name | t10.function | T11 table entries | T10 table entries | BO7 corpus sites | args |")
    w("|---|---|---|---|---|---|")
    r10i = {r["name"]: r for r in r10["intr_rows"]}
    for r in r11["intr_rows"]:
        w("| %s | %s | %s | %s | %d | %s |" % (
            r["name"], r["hash"], ", ".join(r["also_builtin"]) or "**none**",
            ", ".join(r10i[r["name"]]["also_builtin"]) or "**none**", r["sites"],
            ",".join(map(str, r["corpus_args"])) or "-"))
    w("")
    R.write_md(path, L)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", default=paths.get("gsc_data"))
    ap.add_argument("--report", default=os.path.join(RE, "reports", "t10_builtins.md"))
    a = ap.parse_args()
    ex = {}
    for g in ("t11", "t10", "t10sp"):
        p = os.path.join(RE, "out", "%s_extract.json" % g)
        if os.path.exists(p):
            with open(p, encoding="utf-8") as f:
                ex[g] = json.load(f)
    calls, defined, nfiles = corpus.scan(CORPUS)  # intrinsics stay calls: counted apart
    words = sorted({k[0] for k in calls} | defined)
    results = {}
    for g in ("t11", "t10"):
        hashes = {e["hash_full"] & M60 for e in ex[g]["entries"]}
        names, nsrc = load_names(hashes, a.data, words, "bo7" if g == "t11" else "bo6")
        arity = ex[g]["checks"]["arity_check"]
        E, unassigned = build_game(g, ex[g], a.data, names, nsrc, arity)
        for e in E + unassigned:
            if e.get("handler") is None:
                e["handler"] = None
        results[g] = {"E": E, "unassigned": unassigned, "names": names, "nsrc": nsrc,
                      "arity": arity}
    # T11 call sites
    E11 = results["t11"]["E"]
    merged = merged_for_calls(E11)
    key_of = corpus_key
    cs = R.validate_calls(merged, calls, defined, key_of, INTRINSICS)
    results["t11"]["cs"] = cs
    results["t11"]["merged"] = merged
    for g in ("t11", "t10"):
        rows = intrinsic_rows(results[g]["E"], calls, defined)
        intr = []
        for r in rows:
            if r["also_builtin"] or not r["sites"]:
                continue  # a table builtin, or not called by the corpus: not listed
            intr.append({"name": r["name"], "hash": r["hash"], "corpus_args": r["corpus_args"],
                         "evidence": "no entry in any builtin set of %s (t10.function hash %s); "
                                     "the BO7 corpus calls it %d times (the decompiler writes "
                                     "the opcode's name)" % (g, r["hash"], r["sites"])})
        results[g]["intr_rows"] = rows
        results[g]["intrinsics"] = intr
        write_toml(g, ex[g], results[g]["E"], intr,
                   os.path.join(a.data, "builtins", "%s.toml" % g), results[g]["arity"])
    write_report(a.report, ex, results, nfiles)


if __name__ == "__main__":
    main()
