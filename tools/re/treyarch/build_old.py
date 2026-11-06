"""From old_builtins.py's extractions (out/<id>_extract.json): arity (T4/T5: from the handler runs),
call-site validation, gruff-data/builtins/<game>.toml and reports/<game>_builtins.md, for the
32-bit Treyarch games T4, T5 and T6.

    py -3.11 -I treyarch/build_old.py --game t4|t5|t6 [--data ../../../gruff-data]

A game's catalogue is its primary executable's (T4: the SP exe; T5: BlackOps.exe; T6: t6mp);
the sibling executables' extractions (T5: BlackOpsMP.exe; T6: t6zm, t6sp) add the (name, VM,
kind) they have and the primary lacks, marked with `table` = that executable. Differences
between the builds are listed in the report.

T4/T5 arity (arity_source "handler emulation"): see arity_from_runs. T6 arity is the table's
(the linker checks it: "Unresolved external" below min, a too-many-parameters error above max).
"""
import argparse
import collections
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
RE = os.path.dirname(HERE)
sys.path.insert(0, RE)
from common import corpus  # noqa: E402
from common import report as R  # noqa: E402
from common import paths  # noqa: E402

CORPUS = paths.get("corpus")
VARIADIC = 0x7FFFFFFF
GAMES = {
    "t4": {"title": "World at War (T4)", "extracts": ["t4"],
           "corpus": ["t4/source"]},
    "t5": {"title": "Black Ops (T5)", "extracts": ["t5", "t5mp"],
           "corpus": ["t5/source", "t5/source-modtools"]},
    "t6": {"title": "Black Ops II (T6)", "extracts": ["t6", "t6zm", "t6sp"],
           "corpus": ["t6/decompiled"]},
}
EXE_LABEL = {"t4": "CoDWaW LanFixed.exe (SP)", "t5": "BlackOps.exe", "t5mp": "BlackOpsMP.exe",
             "t6": "t6mp.exe", "t6zm": "t6zm.exe", "t6sp": "t6sp.exe"}
# gruff-data profiles/t6.toml opcode_builtins (gsc-tool's arc syntax); measured against the tables
T6_OPCODES = ["abs", "angleclamp180", "anglestoforward", "anglestoright", "anglestoup", "getdvar",
              "getdvarcoloralpha", "getdvarcolorblue", "getdvarcolorgreen", "getdvarcolorred",
              "getdvarfloat", "getdvarint", "getdvarvector", "getfirstarraykey",
              "getnextarraykey", "gettime", "vectorscale", "vectortoangles", "isdefined"]
# hand-coded argument-count errors (messages of the handlers' own checks, as the runs show them)
ARITY_MSG = re.compile(r"(?i)(number of (param|arg)|too (many|few) (param|arg)|illegal call|"
                       r"invalid call|takes .*(param|arg|playback id)|wrong params|usage|"
                       r"with parameters|wrong number|requires \w+ param|expects? \w+ param)")


def arity_from_runs(runs):
    """T4/T5 min/max from the runs with N = 0..8, 12, 16, 24 parameters.

    A run is rejected when a parameter at index >= N was asked for ("parameter %d does not exist")
    or the handler raised one of its own argument-count errors (ARITY_MSG). min = the smallest
    accepted N. max: when N = 24 is accepted and that run reads an index >= 16, variadic; when it
    is accepted and reads less, the handler ignores extra arguments: max = 1 + the largest index
    any accepted run read (at least min); otherwise the largest accepted N (between two tested N,
    the lower one: flagged). Returns (min, max, note) or (None, None, why)."""
    by_n = {r["n"]: r for r in runs}
    ns = sorted(by_n)

    def rejected(r):
        return r["missing"] or any(ARITY_MSG.search(m) for m in r["errors"])
    acc = [n for n in ns if not rejected(by_n[n])]
    if not acc:
        return None, None, "no N accepted"
    mn = acc[0]
    notes = []
    gaps = [n for n in ns if mn < n < acc[-1] and n not in acc]
    if gaps:
        notes.append("rejected inside the accepted range at N=%s" % gaps)
    top = ns[-1]
    if top in acc:
        if any(i >= 16 for i in by_n[top]["reads"]):
            return mn, VARIADIC, "; ".join(notes + ["N=%d reads index %d" % (
                top, max(by_n[top]["reads"]))])
        hi = max([i + 1 for n in acc for i in by_n[n]["reads"]] + [mn])
        notes.append("extra arguments ignored (N=%d accepted), max = last index read + 1" % top)
        return mn, hi, "; ".join(notes)
    mx = acc[-1]
    nxt = [n for n in ns if n > mx]
    if nxt and nxt[0] != mx + 1:
        notes.append("max between %d and %d (untested)" % (mx, nxt[0] - 1))
    return mn, mx, "; ".join(notes)


DEF = re.compile(r"^[ \t]*(?:private[ \t]+|autoexec[ \t]+)?([A-Za-z_]\w*)[ \t]*\([^()]*\)\s*\{",
                 re.M)


def definitions(root):
    """Names the scripts define: these old dialects define `name( params ) {` at the start of a
    line, without the `function` keyword common.corpus looks for."""
    out = set()
    for dp, dn, fn in os.walk(root):
        for f in fn:
            if f.rsplit(".", 1)[-1].lower() not in ("gsc", "csc", "gsh"):
                continue
            with open(os.path.join(dp, f), "rb") as fh:
                src = corpus.blank(fh.read().decode("latin1"))
            for m in DEF.finditer(src):
                n = m.group(1).lower()
                if n not in corpus.KEYWORDS:
                    out.add(n)
    return out


def load(xid):
    with open(os.path.join(RE, "out", "%s_extract.json" % xid), encoding="utf-8") as f:
        return json.load(f)


def entries_of(ex, xid):
    out = []
    base = ex["base"]
    for role, ro in ex["roles"].items():
        vm, kind = role.split("_")
        for ti, t in enumerate(ro["tables"]):
            for e in t["entries"]:
                rt = e["round_trip"]
                d = {"name": e["name"].lower(), "vm": vm, "kind": kind, "role": role,
                     "exe": xid, "table_i": ti, "table_va": t["va"], "index": e["index"],
                     "handler_rva": e["handler"] - base, "shadowed": bool(e.get("shadowed")),
                     "dev_field": e["dev"], "dev_eff": rt["dev"] if rt["dev"] is not None else 0,
                     "dev_returned": rt["dev"] is not None, "runs": e.get("runs"),
                     "slots": e.get("slots") or {}, "id": e.get("id")}
                if ex["layout"] == "t6":
                    mn, mx = e["min"], e["max"]
                    if mn == 0xFFFFFFFF:
                        mn = mx = -1
                    d.update(min=mn, max=mx, arity_source=None, arity_note="")
                else:
                    mn, mx, note = arity_from_runs(e["runs"]) if e.get("runs") else (None, None,
                                                                                    "not run")
                    d.update(min=mn, max=mx, arity_source="handler emulation", arity_note=note)
                d["arg_read"] = any(r["arg_read"] for r in (e.get("runs") or []))
                out.append(d)
    return out


def param_types(d):
    slots = {int(k): v for k, v in d["slots"].items()}
    if d["runs"]:
        for r in d["runs"]:
            for k, v in r["types"].items():
                slots.setdefault(int(k), v[0])
    if not slots:
        return None
    lim = d["max"] if d["max"] is not None and 0 <= d["max"] < VARIADIC else 16
    idx = [i for i in slots if i < lim]
    if not idx:
        return None
    return [slots.get(i, "?") for i in range(max(idx) + 1)]


def evidence(d, ex):
    ev = ["%s %s table %d (0x%x) entry %d, static data" % (
        EXE_LABEL[d["exe"]], d["role"], d["table_i"], d["table_va"] - ex["base"], d["index"])]
    ev.append("lookup round trip %s" % ("shadowed by an earlier entry" if d["shadowed"] else "ok"))
    if d["dev_field"] != d["dev_eff"]:
        ev.append("table developer field %d, the lookup returns %d" % (d["dev_field"],
                                                                         d["dev_eff"]))
    if d["arity_source"]:
        acc = [r["n"] for r in d["runs"] or [] if not r["missing"]
               and not any(ARITY_MSG.search(m) for m in r["errors"])]
        top = d["runs"][-1] if d["runs"] else None
        ev.append("handler runs N=0..8,12,16,24: accepted N=%s%s%s" % (
            ",".join(map(str, acc)) or "none",
            ", N=%d read %s" % (top["n"], top["reads"][:20]) if top else "",
            (" (%s)" % d["arity_note"]) if d["arity_note"] else ""))
    elif d["runs"]:
        top = d["runs"][-1]
        ev.append("table min/max (linker-checked); handler run N=%d read %s" % (
            top["n"], top["reads"][:20]))
    if d["id"] is not None:
        ev.append("entry u32 at +4: %d" % d["id"])
    return "; ".join(ev)


def toml_entry(d, ex, nsrc="executable"):
    L = ["[[builtin]]", "name = %s" % R.toml_str(d["name"]),
         "name_source = %s" % R.toml_str(nsrc), 'vm = "%s"' % d["vm"], 'kind = "%s"' % d["kind"]]
    mn, mx = d["min"], d["max"]
    L.append("min_args = %d" % mn)
    L.append("max_args = %d" % mx)
    if mx is not None and mx >= VARIADIC:
        L.append("variadic = true")
    if mn < 0:
        L.append("arity_unchecked = true")
    L.append("dev_only = %s" % ("true" if d["dev_eff"] == 1 else "false"))
    if d["arity_source"]:
        L.append("arity_source = %s" % R.toml_str(d["arity_source"]))
    if d["exe"] != d["primary"]:
        L.append("table = %s" % R.toml_str("%s only" % EXE_LABEL[d["exe"]]))
    pt = param_types(d)
    if pt:
        L.append("param_types = [%s]" % ", ".join(R.toml_str(t) for t in pt))
    if d["shadowed"]:
        L.append("shadowed = true")
    if d["exe"] == d["primary"]:
        L.append('handler_rva = "0x%x"' % d["handler_rva"])
    L.append("evidence = %s" % R.toml_str(evidence(d, ex)))
    L.append("")
    return L


def build(game, data_dir, report_path):
    cfg = GAMES[game]
    exs = {x: load(x) for x in cfg["extracts"]}
    prim = cfg["extracts"][0]
    E = {x: entries_of(exs[x], x) for x in cfg["extracts"]}
    for x in E:
        for d in E[x]:
            d["primary"] = prim
    cat = list(E[prim])
    have = {(d["name"], d["vm"], d["kind"]) for d in cat}
    extra = collections.defaultdict(list)
    diffs = []
    for x in cfg["extracts"][1:]:
        pk = {(d["name"], d["vm"], d["kind"]): d for d in E[prim] if not d["shadowed"]}
        for d in E[x]:
            k = (d["name"], d["vm"], d["kind"])
            if d["shadowed"]:
                continue
            if k not in have:
                have.add(k)
                cat.append(d)
                extra[x].append(d)
            elif k in pk and (pk[k]["min"], pk[k]["max"]) != (d["min"], d["max"]):
                diffs.append((x, d, pk[k]))
    unresolved = [d for d in cat if d["min"] is None]
    for d in unresolved:  # no N accepted: min = smallest N without a missing parameter
        runs = d["runs"] or []
        if not runs:  # the emulator crashed on this handler: no arity measured
            d["min"] = d["max"] = -1
            d["arity_note"] = "handler not run (the emulator crashed on it)"
            continue
        ok = [r["n"] for r in runs if not r["missing"]]
        d["min"] = ok[0] if ok else 0
        reads = [i + 1 for r in runs for i in r["reads"]]
        d["max"] = max(reads + [d["min"]]) if not (runs and any(
            i >= 16 for i in runs[-1]["reads"])) else VARIADIC
        d["arity_note"] = "no N accepted by the handler's own checks; min = smallest N without a " \
                          "missing parameter, max = last index read + 1"
    for d in cat:
        d["key"] = d["name"]

    # corpus
    calls = collections.Counter()
    defined = set()
    nfiles = 0
    for c in cfg["corpus"]:
        cl, df, nf = corpus.scan(os.path.join(CORPUS, c))
        calls.update(cl)
        defined |= df | definitions(os.path.join(CORPUS, c))
        nfiles += nf
    live = [d for d in cat if not d["shadowed"]]
    intr = T6_OPCODES if game == "t6" else []
    # intrinsics: names never in a table (any role, any build) can only be opcodes
    names_any = {d["name"] for x in E for d in E[x]}
    intrinsics = []
    for n in intr:
        roles = sorted({"%s_%s" % (d["vm"], d["kind"]) for d in live if d["name"] == n})
        args = sorted({a for (nm, k, vm, a) in calls if nm == n})
        sites = sum(c for (nm, k, vm, a), c in calls.items() if nm == n)
        intrinsics.append({"name": n, "also_builtin": roles, "corpus_args": args,
                           "evidence": "gruff-data profiles/t6.toml opcode_builtins; name %s the "
                                       "linker's lookups (%s); decompiled call sites %d" % (
                                           "in" if roles else "absent from",
                                           ", ".join(roles) or "none", sites)})
    intr_names = [i["name"] for i in intrinsics if not i["also_builtin"]]
    cs = R.validate_calls(cat, calls, defined, lambda n: n, intr_names)
    # MP / per-path split (T4: the MP exe is not readable; its scripts are checked against SP)
    mp_calls = collections.Counter()
    for c in cfg["corpus"]:
        root = os.path.join(CORPUS, c)
        for sub in ("maps/mp", "clientscripts/mp"):
            p = os.path.join(root, sub)
            if os.path.isdir(p):
                cl, df, nf = corpus.scan(p)
                mp_calls.update(cl)
    cs_mp = R.validate_calls(cat, mp_calls, defined, lambda n: n, intr_names)

    # kind measure: method handlers read their entref argument
    kind_meas = collections.defaultdict(lambda: [0, 0])
    for d in E[prim]:
        if d["runs"]:
            k = kind_meas[(d["role"], d["table_i"])]
            k[0] += d["arg_read"]
            k[1] += 1

    # TOML
    cnt = collections.Counter((d["vm"], d["kind"]) for d in live)
    ex = exs[prim]
    lay = ("char *name, handler, int developer (12 bytes)" if ex["layout"] == "old12" else
           "char *name, u32 id, u32 min_args, u32 max_args, handler, u32 developer (0x18 bytes)")
    L = ["# %s engine builtins: every entry of the four lookups (GSC/CSC x function/method)" %
         cfg["title"],
         "# the %s uses, read from the executable's static tables by" % (
             "compiler" if ex["layout"] == "old12" else "linker"),
         "# gruff/tools/re/treyarch/old_builtins.py + build_old.py (lookups and handlers run "
         "under Unicorn).",
         "# Names are the tables' own strings. Schema: ../schema/builtins.schema.json. Facts only.",
         "", "schema_version = 1", 'game = "%s"' % game, 'license = "CC0-1.0"',
         'source = "extracted from game executable"', "", "[extraction]",
         "tool = %s" % R.toml_str("gruff/tools/re/treyarch/old_builtins.py + build_old.py"),
         "executable = %s" % R.toml_str("; ".join(
             exs[x]["what"] for x in cfg["extracts"])),
         "pe_timestamp = %d" % ex["pe_timestamp"],
         "entry_layout = %s" % R.toml_str(lay), "variadic_max = %d" % VARIADIC,
         "counts = { %s }" % ", ".join("%s_%s = %d" % (vm, k, cnt[(vm, k)])
                                        for vm in ("gsc", "csc") for k in ("function", "method")),
         "", "[extraction.lookups]"]
    for role in R.VM_ORDER:
        ro = ex["roles"][role]
        L.append("%s = { rva = %s, tables = [%s] }" % (
            role, R.toml_str("0x%x" % (ro["lookups"][0] - ex["base"])),
            ", ".join("{ rva = %s, count = %d }" % (R.toml_str("0x%x" % (t["va"] - ex["base"])),
                                                    t["count"]) for t in ro["tables"])))
    L.append("")
    order = sorted(cat, key=lambda d: (R.VM_ORDER.index(d["role"]), d["name"],
                                       d["exe"] != prim, d["table_i"], d["index"]))
    for d in order:
        L += toml_entry(d, exs[d["exe"]])
    for it in intrinsics:
        L += ["[[intrinsic]]", "name = %s" % R.toml_str(it["name"])]
        if it["also_builtin"]:
            L.append("also_builtin = [%s]" % ", ".join(R.toml_str(r) for r in it["also_builtin"]))
        if it["corpus_args"]:
            L.append("corpus_args = [%s]" % ", ".join(map(str, it["corpus_args"])))
        L += ["evidence = %s" % R.toml_str(it["evidence"]), ""]
    path = os.path.join(data_dir, "builtins", "%s.toml" % game)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(L))

    write_report(game, cfg, exs, E, cat, live, extra, diffs, unresolved, cs, cs_mp, nfiles,
                 kind_meas, intrinsics, report_path)
    print("wrote", path, report_path)


def write_report(game, cfg, exs, E, cat, live, extra, diffs, unresolved, cs, cs_mp, nfiles,
                 kind_meas, intrinsics, path):
    prim = cfg["extracts"][0]
    ex = exs[prim]
    L = []
    w = L.append
    w("# %s builtin catalogue: extraction and validation" % cfg["title"])
    w("")
    w("Generated by `treyarch/build_old.py --game %s` from `treyarch/old_builtins.py extract`." % game)
    w("")
    for x in cfg["extracts"]:
        w("- %s: `%s` (%s), PE timestamp %d." % (EXE_LABEL[x], os.path.basename(exs[x]["exe"]), exs[x]["what"],
                                                 exs[x]["pe_timestamp"]))
    w("")
    for x in cfg["extracts"]:
        w("## Extraction log (%s)" % EXE_LABEL[x])
        w("")
        w("```")
        L.extend(exs[x]["log"])
        w("```")
        w("")
    w("## Counts")
    w("")
    w("| VM | kind | %s |" % " | ".join(EXE_LABEL[x] for x in cfg["extracts"]) +
      " catalogue |")
    w("|---|---|" + "---|" * (len(cfg["extracts"]) + 1))
    for role in R.VM_ORDER:
        vm, kind = role.split("_")
        w("| %s | %s | %s | %d |" % (vm, kind, " | ".join(
            str(sum(1 for d in E[x] if d["role"] == role and not d["shadowed"]))
            for x in cfg["extracts"]),
            sum(1 for d in live if d["role"] == role)))
    w("")
    w("Catalogue: %d entries (%d shadowed), every one named by its table's own string (100%%). "
      "Developer-only (as the lookup returns the flag): %d." % (
          len(cat), len(cat) - len(live), sum(1 for d in live if d["dev_eff"] == 1)))
    for x, ds in extra.items():
        w("")
        w("%d (name, VM, kind) only in %s, added with `table = \"%s only\"`: %s." % (
            len(ds), EXE_LABEL[x], EXE_LABEL[x], ", ".join(sorted("%s (%s %s)" % (
                d["name"], d["vm"], d["kind"]) for d in ds))))
    w("")
    if diffs:
        w("### Arity differences between the builds (catalogue keeps %s's)" % EXE_LABEL[prim])
        w("")
        w("| build | name | VM | kind | that build | %s |" % EXE_LABEL[prim])
        w("|---|---|---|---|---|---|")
        for x, d, p in sorted(diffs, key=lambda t: (t[0], t[1]["name"])):
            w("| %s | %s | %s | %s | %s..%s | %s..%s |" % (
                EXE_LABEL[x], d["name"], d["vm"], d["kind"], d["min"], fmt_max(d["max"]),
                p["min"], fmt_max(p["max"])))
        w("")
    w("## Independent measures")
    w("")
    w("- Tables: the lookups' reads with an absent name vs the static loop bound "
      "(`cmp esi, N*stride`): equal for every table (log).")
    w("- Round trip: every entry looked up again by name through its role's lookups returns its "
      "handler (and, T6, its min/max); a repeated name returns the first entry's (shadowed).")
    w("- Kind: entries whose handler reads its first stack argument (the entref a method "
      "handler receives), per table:")
    w("")
    w("| role | table | handlers reading the entref argument |")
    w("|---|---|---|")
    for (role, ti), (a, n) in sorted(kind_meas.items(), key=lambda kv: (
            R.VM_ORDER.index(kv[0][0]), kv[0][1])):
        w("| %s | %d | %d/%d |" % (role, ti, a, n))
    w("")
    L.extend(R.calls_section(cs, ", ".join("corpus/%s" % c for c in cfg["corpus"]) +
                             ", %d files" % nfiles, VARIADIC))
    w("### Most frequent call sites outside min..max (by sites)")
    w("")
    w("| name | kind | file type | args | sites | extracted min..max |")
    w("|---|---|---|---|---|---|")
    for (name, kind, vm, nargs, mn, mx), n in cs["bad"].most_common(30):
        w("| %s | %s | %s | %d | %d | %d..%s |" % (name, kind, vm, nargs, n, mn, fmt_max(mx)))
    w("")
    called, live_n = R.coverage(cat, cs)
    w("Coverage: %d/%d effective entries called at least once (%s)." % (
        called, live_n, R.pct(called, live_n)))
    w("")
    w("MP script paths alone (maps/mp, clientscripts/mp): within min..max %d/%d (%s)." % (
        cs_mp["ok"], cs_mp["builtin"], R.pct(cs_mp["ok"], cs_mp["builtin"])))
    w("")
    if ex["layout"] == "old12":
        w("## Arity from handler emulation")
        w("")
        w("T4/T5 tables store no arity, and the compiler does not check it: a call with any "
          "number of arguments compiles, and the handler reads what it wants. min/max here are "
          "measured by running each handler with N = 0..8, 12, 16, 24 parameters (the rule is "
          "`arity_from_runs` in build_old.py): a run is rejected when it asks for a parameter "
          "index >= N or raises its own count error; min is the smallest accepted N; max the "
          "largest accepted N, or 1 + the last index read when the handler accepts 24 (extra "
          "arguments ignored), or variadic when it reads them all.")
        w("")
        notes = collections.Counter(re.sub(r"\d+", "#", d["arity_note"]) for d in live
                                    if d["arity_note"])
        w("Notes on the rule's result: %s." % (", ".join(
            "%s: %d" % kv for kv in notes.most_common(12)) or "none"))
        w("")
        w("Entries where no N was accepted (fallback in the evidence): %d: %s." % (
            len(unresolved), ", ".join(sorted("%s (%s)" % (d["name"], d["role"])
                                              for d in unresolved))))
        w("")
    typed = sum(1 for d in live if param_types(d) and any(t != "?" for t in param_types(d)))
    ends = collections.Counter(d["runs"][-1]["end"].split(" (")[0] for d in live if d["runs"])
    w("## Parameter types")
    w("")
    w("Entries with at least one typed parameter: %d/%d (%s). How the last run ended: %s." % (
        typed, len(live), R.pct(typed, len(live)),
        ", ".join("%s %d" % kv for kv in ends.most_common())))
    w("")
    if ex["layout"] == "old12":
        have = {d["name"] for d in live}
        absent = [n for n in T6_OPCODES if n not in have]
        w("## Compiler intrinsics")
        w("")
        w("None listed: the compiler is in this executable and no call is compiled to an opcode "
          "without a lookup (both call emitters always go through the lookups). Of the arc "
          "opcode-builtin names (gruff-data profiles/t6.toml), in the tables: %s; absent: %s "
          "(the executable has no other code reference to these strings; the scripts define "
          "vectorscale themselves)." % (", ".join(n for n in T6_OPCODES if n in have),
                                         ", ".join(absent) or "none"))
        w("")
    if intrinsics:
        w("## Compiler intrinsics")
        w("")
        w("| name | in the lookups | corpus arg counts |")
        w("|---|---|---|")
        for it in intrinsics:
            w("| %s | %s | %s |" % (it["name"], ", ".join(it["also_builtin"]) or "**none**",
                                    ",".join(map(str, it["corpus_args"])) or "-"))
        w("")
    R.write_md(path, L)


def fmt_max(m):
    return R.fmt_max(m, VARIADIC) if m is not None else "?"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--game", required=True, choices=sorted(GAMES))
    ap.add_argument("--data", default=paths.get("gsc_data"))
    ap.add_argument("--report")
    a = ap.parse_args()
    build(a.game, a.data, a.report or os.path.join(RE, "reports", "%s_builtins.md" % a.game))


if __name__ == "__main__":
    main()
