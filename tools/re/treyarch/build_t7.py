"""From t7_builtins.py's extraction (out/t7_extract.json): resolve names, check against the official
doc and the mod-tools call sites, write gruff-data/builtins/t7.toml and reports/t7_builtins.md.

    py -3.11 -I treyarch/build_t7.py [--extract out/t7_extract.json] [--data ../../../gruff-data]

Names: a canon hash is named only by a string whose t7.canon hash equals it, taken from (in
order) the official doc's names, the identifiers of the mod-tools scripts, the BO3 hash list
(bo3_names_and_labels.csv). Nothing else from the doc is written: no description, no
parameter name, no example; only counts are compared and summarised in the report.
"""
import argparse
import collections
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
RE = os.path.dirname(HERE)
sys.path.insert(0, RE)
sys.path.insert(0, HERE)
from common.hashes import t7_canon  # noqa: E402
from common import corpus  # noqa: E402
from common import handlers as H  # noqa: E402
from common import names as N  # noqa: E402
from common import report as R  # noqa: E402
import officialdoc  # noqa: E402
from common import paths  # noqa: E402

HASH_CSV = paths.get("bo3_hash_csv")  # tools/re/paths.local.json or GSC_RE_BO3_HASH_CSV
CORPUS = os.path.join(paths.get("corpus"), "t7", "source-modtools")
VARIADIC = 0x7FFFFFFF
ROLE_ORDER = R.VM_ORDER
# The arc compiler's opcode builtins (one VM opcode each, no table lookup): gruff-data
# profiles/t7.toml opcode_builtins, plus isdefined and the profiler and real-time wait opcodes.
# Which of them also have a table entry, per VM, is measured below.
INTRINSICS = [
    ("isdefined", "IsDefined"), ("vectorscale", "VectorScale"), ("anglestoup", "AnglesToUp"),
    ("anglestoright", "AnglesToRight"), ("anglestoforward", "AnglesToForward"),
    ("angleclamp180", "AngleClamp180"), ("vectortoangles", "VectorToAngles"), ("abs", "Abs"),
    ("gettime", "GetTime"), ("getdvar", "GetDvar"), ("getdvarint", "GetDvarInt"),
    ("getdvarfloat", "GetDvarFloat"), ("getdvarvector", "GetDvarVector"),
    ("getdvarcolorred", "GetDvarColorRed"), ("getdvarcolorgreen", "GetDvarColorGreen"),
    ("getdvarcolorblue", "GetDvarColorBlue"), ("getdvarcoloralpha", "GetDvarColorAlpha"),
    ("getfirstarraykey", "FirstArrayKey"), ("getnextarraykey", "NextArrayKey"),
    ("profilestart", "ProfileStart"), ("profilestop", "ProfileStop"),
    ("waitrealtime", "RealWait"),
]


def load_names(hashes, doc, calls, defined):
    def hash_list():
        for row in N.csv_column(HASH_CSV, 1):
            yield row
    names, src = N.name_hashes(hashes, [
        ("official-doc", (d["name"] for d in doc)),
        ("mod-tools-scripts", sorted({k[0] for k in calls} | defined)),
        ("bo3-hash-list", hash_list()),
    ], t7_canon)
    return names, src


def build(extract_path, data_dir, report_path):
    with open(extract_path, encoding="utf-8") as f:
        ex = json.load(f)
    E = ex["entries"]
    doc = officialdoc.parse()
    hashes = {e["canon"] for e in E}
    calls, defined, nfiles = corpus.scan(CORPUS)
    names, nsrc = load_names(hashes, doc, calls, defined)

    # effective entries: what the engine's lookup returns (a shadowed duplicate is kept, flagged)
    # min/max are signed: 0xFFFFFFFF/0xFFFFFFFF (two entries) is -1/-1, no arity recorded
    R.effective_entries(E, names)

    by_key = {}
    for e in E:
        if not e["shadowed"]:
            by_key[(e["canon"], e["vm"], e["kind"])] = e

    # ---------------------------------------------------------------- doc oracle
    doc_rows = []
    for d in doc:
        h = t7_canon(d["name"])
        sides = {"Server": ["gsc"], "Client": ["csc"], "Both": ["gsc", "csc"]}.get(d["side"], [])
        kind = "method" if d["method"] else "function"
        any_role = [k for k in by_key if k[0] == h]
        for vm in sides:
            e = by_key.get((h, vm, kind))
            other = by_key.get((h, vm, "function" if kind == "method" else "method"))
            doc_rows.append((d, vm, kind, e, other, any_role))
    n_doc = len(doc)
    found = sum(1 for d in doc if any(k[0] == t7_canon(d["name"]) for k in by_key))
    pairs = [r for r in doc_rows if r[3] is not None]
    kind_mismatch = [r for r in doc_rows if r[3] is None and r[4] is not None]
    side_missing = [r for r in doc_rows if r[3] is None and r[4] is None and r[5]]
    absent = [d for d in doc if not any(k[0] == t7_canon(d["name"]) for k in by_key)]

    def arity_ok(d, e, head):
        if e["unchecked"]:
            return False
        req = d["head_req"] if head else d["req"]
        opt = d["head_opt"] if head else d["opt"]
        if e["max"] >= VARIADIC:
            return e["min"] == req or e["min"] <= req
        return e["min"] == req and e["max"] == req + opt

    ok_head = [r for r in pairs if arity_ok(r[0], r[3], True)]
    ok_body = [r for r in pairs if arity_ok(r[0], r[3], False)]
    ok_either = [r for r in pairs if arity_ok(r[0], r[3], True) or arity_ok(r[0], r[3], False)]
    ok_min = [r for r in pairs if r[3]["min"] in (r[0]["head_req"], r[0]["req"])]
    ok_max = [r for r in pairs if r[3]["max"] >= VARIADIC or r[3]["max"] in (
        r[0]["head_req"] + r[0]["head_opt"], r[0]["req"] + r[0]["opt"])]

    # ---------------------------------------------------------------- corpus call sites
    cs = R.validate_calls(E, calls, defined, t7_canon, [n for n, _ in INTRINSICS])
    usage = collections.defaultdict(collections.Counter)
    for (name, kind, vm, nargs), n in calls.items():
        if name not in defined:
            usage[name][(kind, vm, nargs)] += n

    # ---------------------------------------------------------------- intrinsics
    intrinsics = []
    for name, op in INTRINSICS:
        h = t7_canon(name)
        tab = sorted("%s_%s" % (vm, k) for (hh, vm, k) in by_key if hh == h)
        args = sorted({a for (n, k, vm, a) in calls if n == name})
        sites = sum(c for (n, k, vm, a), c in calls.items() if n == name)
        intrinsics.append({
            "name": name, "hash": "0x%08x" % h, "opcode": "OP_" + op,
            "also_builtin": tab, "corpus_args": args,
            "evidence": "compiled to the opcode by the arc compiler (gsc-tool, acts: OP_%s); "
                        "t7.canon hash %s in the four lookups' tables: %s; mod-tools call "
                        "sites %d" % (op, "found" if tab else "absent",
                                      ", ".join(tab) if tab else "none", sites)})

    write_toml(ex, E, nsrc, intrinsics, os.path.join(data_dir, "builtins", "t7.toml"))
    stats = dict(n_doc=n_doc, found=found, pairs=pairs, ok_head=ok_head, ok_body=ok_body,
                 ok_either=ok_either, ok_min=ok_min, ok_max=ok_max, kind_mismatch=kind_mismatch,
                 side_missing=side_missing, absent=absent, cs=cs, doc_rows=doc_rows, nsrc=nsrc,
                 usage=usage, intrinsics=intrinsics, nfiles=nfiles)
    write_report(ex, E, stats, report_path)


def normalised(e, nsrc):
    hp = e.get("handler") or {}
    return R.lookup_entry(
        e, nsrc, "init emulated", VARIADIC, H.param_types(hp) if hp else None,
        ", self = fake %s" % hp["self_variant"] if e["kind"] == "method"
        and hp.get("self_variant") else "")


def write_toml(ex, E, nsrc, intrinsics, path):
    R.write_builtins_toml(
        path,
        ["Black Ops III (T7) engine builtins: every entry of the four lookups the VM uses",
         "(GSC/CSC x function/method), extracted from the decrypted game executable by",
         "gruff/tools/re/treyarch/t7_builtins.py (static init and handlers run under Unicorn).",
         "[[intrinsic]]: the compiler's opcode builtins, which no lookup resolves.",
         "Schema: ../schema/builtins.schema.json. Facts only; no documentation text."],
        [("game", "t7"), ("license", "CC0-1.0"), ("source", "extracted from game executable"),
         ("hash_family", "t7.canon")],
        [("tool", R.toml_str("gruff/tools/re/treyarch/t7_builtins.py + build_t7.py")),
         ("executable", R.toml_str("BlackOps3.exe, Cordycep dump flattened (bo3_flat.exe)")),
         ("pe_timestamp", "%d" % ex["pe_timestamp"]),
         ("entry_layout", R.toml_str("u32 canon, u32 min_args, u32 max_args, u32 pad, u64 handler, "
                                     "u32 type, u32 pad (0x20 bytes)")),
         ("variadic_max", "%d" % VARIADIC),
         ("counts", R.vm_kind_counts(E))],
        R.lookups_of(ex, ROLE_ORDER),
        [normalised(e, nsrc) for e in E], intrinsics=intrinsics)


def write_report(ex, E, s, path):
    live = [e for e in E if not e["shadowed"]]
    cnt = collections.Counter((e["vm"], e["kind"]) for e in live)
    named = sum(1 for e in live if e["name"])
    src = collections.Counter(s["nsrc"][e["canon"]] for e in live if e["name"])
    L = []
    w = L.append
    w("# T7 (Black Ops III) builtin catalogue: extraction and validation")
    w("")
    w("Generated by `treyarch/build_t7.py` from `treyarch/t7_builtins.py extract`. Executable: "
      "Cordycep dump of `BlackOps3.exe`, flattened (`bo3_flat.exe`), PE timestamp %d, image base "
      "0x%x." % (ex["pe_timestamp"], ex["image_base"]))
    w("")
    w("## Extraction log")
    w("")
    w("```")
    L.extend(ex["log"])
    w("```")
    w("")
    w("## Counts")
    w("")
    w("| VM | kind | entries | tables |")
    w("|---|---|---|---|")
    for role in ROLE_ORDER:
        vm, kind = role.split("_")
        w("| %s | %s | %d | %d |" % (vm, kind, cnt[(vm, kind)], len(ex["lookups"][role]["tables"])))
    w("")
    shadowed = [e for e in E if e["shadowed"]]
    w("%d table entries, %d effective (%d shadowed by the same hash in an earlier table of the "
      "same lookup: the engine's first match wins). %d distinct hashes. Shared common tables "
      "(the BG ones) appear in both the GSC and the CSC function lookup."
      % (len(E), len(live), len(shadowed), len({e["canon"] for e in E})))
    w("")
    w("Names: %d/%d effective entries named (%s), %d unnamed (hash only)."
      % (named, len(live), ", ".join("%s %d" % kv for kv in src.most_common()),
         len(live) - named))
    w("")
    dev = collections.Counter(e["eff_type"] for e in live)
    w("Type flags (as the lookup returns them): %s." % ", ".join(
        "%d: %d" % kv for kv in sorted(dev.items())))
    w("")
    w("## Independent measures of the tables")
    w("")
    w("- Table bounds: the lookup's reads (emulated with an absent hash) vs the static loop "
      "bound `cmp reg, N` where the scan has one: see the log line per lookup.")
    w("- Every entry looked up again through the engine: handler, min, max and type agree for "
      "every non-shadowed entry; the type differs only where a table's getter does not return "
      "one (the lookup forces 0).")
    w("- Roles (VM, function/method) come from the VM's value reader, emulated for each "
      "(inst, is-function byte).")
    w("")
    w("## Official doc (bo3_scriptapifunctions.htm, read locally, facts only)")
    w("")
    w("- Doc entries: %d. Names found in the executable (any lookup): %d (%s)."
      % (s["n_doc"], s["found"], R.pct(s["found"], s["n_doc"])))
    w("- Doc (name, side, kind) pairs matched to a lookup entry: %d of %d."
      % (len(s["pairs"]), len(s["doc_rows"])))
    w("- Arity agreement on matched pairs (min == mandatory, max == mandatory + optional; a "
      "variadic entry agrees when min <= mandatory):")
    for lab, k in (("with the signature line", "ok_head"),
                   ("with the [MANDATORY]/[OPTIONAL] list", "ok_body"), ("with either", "ok_either")):
        w("  - %s: %d/%d (%s)" % (lab, len(s[k]), len(s["pairs"]),
                                 R.pct(len(s[k]), len(s["pairs"]))))
    w("  - min alone: %s; max alone: %s" % (R.pct(len(s["ok_min"]), len(s["pairs"])),
                                            R.pct(len(s["ok_max"]), len(s["pairs"]))))
    w("- Kind disagreements (doc says method, the executable has only a function, or the "
      "reverse, in that VM): %d" % len(s["kind_mismatch"]))
    w("- Side disagreements (the name exists, not in the doc's VM): %d" % len(s["side_missing"]))
    w("- Doc names absent from the executable: %d" % len(s["absent"]))
    w("")
    w("### Doc names absent from the executable")
    w("")
    w(", ".join(sorted(d["name"].lower() for d in s["absent"])) or "none")
    w("")
    w("### Kind disagreements")
    w("")
    usage = s["usage"]
    verdicts = collections.Counter()
    rows = []
    for d, vm, kind, e, other, _ in sorted(s["kind_mismatch"], key=lambda r: r[0]["name"].lower()):
        u = usage.get(d["name"].lower(), {})
        as_doc = sum(n for (k, ft, a), n in u.items() if k == kind and ft in (vm, "gsh"))
        as_exe = sum(n for (k, ft, a), n in u.items() if k == other["kind"] and ft in (vm, "gsh"))
        v = "executable" if as_exe and not as_doc else "doc" if as_doc and not as_exe else \
            "both" if as_doc else "unused"
        verdicts[v] += 1
        rows.append("| %s | %s | %s | %s %d..%s | %d | %d | %s |" % (
            d["name"].lower(), vm, kind, other["kind"], other["min"], fmt_max(other["max"]),
            as_doc, as_exe, v))
    w("Corpus arbitration (call sites in the doc's kind / in the executable's kind): %s."
      % ", ".join("%s %d" % kv for kv in verdicts.most_common()))
    w("")
    w("| name | VM | doc kind | executable | corpus sites, doc kind | corpus sites, exe kind | corpus sides with |")
    w("|---|---|---|---|---|---|---|")
    L.extend(rows)
    w("")
    w("### Side disagreements")
    w("")
    w("| name | doc side | executable has |")
    w("|---|---|---|")
    for d, vm, kind, e, other, roles in sorted(s["side_missing"],
                                               key=lambda r: r[0]["name"].lower()):
        w("| %s | %s (%s) | %s |" % (d["name"].lower(), d["side"], kind,
                                     ", ".join(sorted("%s %s" % (k[1], k[2]) for k in roles))))
    w("")
    w("### Arity disagreements (neither doc count matches)")
    w("")
    bad = [r for r in s["pairs"] if r not in s["ok_either"]]
    verdicts = collections.Counter()
    rows = []
    for d, vm, kind, e, other, _ in sorted(bad, key=lambda r: r[0]["name"].lower()):
        u = usage.get(d["name"].lower(), {})
        seen = sorted({a for (k, ft, a), n in u.items() if k == kind and ft == vm})
        lo, hi = d["req"], d["req"] + d["opt"]
        lo2, hi2 = d["head_req"], d["head_req"] + d["head_opt"]
        if not seen:
            v = "unused"
        elif e["unchecked"]:
            v = "n/a"
        elif all(e["min"] <= a <= e["max"] for a in seen) and any(
                not (lo <= a <= hi) and not (lo2 <= a <= hi2) for a in seen):
            v = "executable"
        elif any(not (e["min"] <= a <= e["max"]) for a in seen):
            v = "doc"
        else:
            v = "neither (corpus fits both)"
        verdicts[v] += 1
        rows.append("| %s | %s | %s | %d..%s | %d+%d | %d+%d | %s | %s |" % (
            d["name"].lower(), vm, kind, e["min"], fmt_max(e["max"]), d["head_req"],
            d["head_opt"], d["req"], d["opt"], ",".join(map(str, seen)) or "-", v))
    w("Corpus arbitration (argument counts used by the scripts, in the doc's kind): %s."
      % ", ".join("%s %d" % kv for kv in verdicts.most_common()))
    w("")
    w("| name | VM | kind | executable min..max | doc signature req+opt | doc list req+opt | corpus arg counts | corpus sides with |")
    w("|---|---|---|---|---|---|---|---|")
    L.extend(rows)
    w("")
    L.extend(R.calls_section(s["cs"], "corpus/t7/source-modtools, %d files" % s["nfiles"], VARIADIC))
    called, live_n = R.coverage(E, s["cs"])
    w("Coverage: %d/%d effective entries are called at least once by the mod-tools scripts (%s)."
      % (called, live_n, R.pct(called, live_n)))
    w("")
    w("## Compiler intrinsics (opcode builtins)")
    w("")
    w("These names compile to one VM opcode each (the arc compiler: gsc-tool and acts name the "
      "opcodes), so the VM never looks them up: a call site is not a table builtin. Measured "
      "here: whether the name's t7.canon hash has a table entry in a VM anyway (then the "
      "builtin exists too, e.g. for a function pointer or a call the compiler does not fold).")
    w("")
    w("| name | opcode | table entries | corpus arg counts |")
    w("|---|---|---|---|")
    for it in s["intrinsics"]:
        w("| %s | %s | %s | %s |" % (it["name"], it["opcode"], ", ".join(it["also_builtin"]) or
                                    "**none**", ",".join(map(str, it["corpus_args"])) or "-"))
    w("")
    hl, hst = R.handler_section(E, VARIADIC)
    L.extend(hl)
    unnamed = [e for e in live if not e["name"]]
    w("## Open issues")
    w("")
    w("- %d effective entries have no name: no string from the doc, the mod-tools scripts or "
      "the hash list hashes to them. Their hashes, arities and handlers are in the data." %
      len(unnamed))
    w("- Parameter types are the weakest measure: a handler that validates a value (a dvar or an "
      "asset that must exist) raises a script error before its later parameters; one that calls "
      "into another DLL stops there. 'any' means the handler read the slot through a type-agnostic "
      "accessor (Scr_GetType, a generic value) and branches on it.")
    w("- The handlers that read an index >= max (listed above) need a look by hand: a handler "
      "shared by two builtins, or a guard on a count other than outparamcount.")
    w("- The two -1/-1 entries (no arity recorded) need their linker path read.")
    w("- The dump is the current Steam build (PE timestamp %d); the mod-tools doc is from 2016. "
      "Doc names absent from the executable may be builtins removed since, or never shipped."
      % ex["pe_timestamp"])
    w("")
    R.write_md(path, L)


def fmt_max(m):
    return R.fmt_max(m, VARIADIC)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--extract", default=os.path.join(RE, "out", "t7_extract.json"))
    ap.add_argument("--data", default=paths.get("gsc_data"))
    ap.add_argument("--report", default=os.path.join(RE, "reports", "t7_builtins.md"))
    a = ap.parse_args()
    build(a.extract, a.data, a.report)


if __name__ == "__main__":
    main()
