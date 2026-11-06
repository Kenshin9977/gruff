"""From t89_builtins.py's extraction (out/<game>_extract.json): names, validation against the
decompiled corpus, gruff-data/builtins/<game>.toml and reports/<game>_builtins.md, for T8 and T9.

    py -3.11 -I treyarch/build_t89.py --game t8 [--extract out/t8_extract.json] [--data ../../../gruff-data]

Names: a hash is named only by a string whose t89.scr hash equals it (the claim of a list is
never trusted). Candidate strings, in this order of attribution: the T7 catalogue's names (with
the T7 entry's own name_source), the identifiers of the game's decompiled corpus, the BO4
glossary, GrayWolf's name indexes (.wni), cod-hash-recovery, the local hash lists (`hash_lists`). Every
source is also counted alone (how many entries it would name).
"""
import argparse
import collections
import glob
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
RE = os.path.dirname(HERE)
sys.path.insert(0, RE)
from common.hashes import t89_scr  # noqa: E402
from common import corpus  # noqa: E402
from common import handlers as H  # noqa: E402
from common import report as R  # noqa: E402
from common import paths  # noqa: E402
from common.names import files_under, words_of  # noqa: E402

TOOLCHAIN = os.path.normpath(os.path.join(RE, "..", "..", ".."))
VARIADIC = 0x7FFFFFFF
GAMES = {
    "t8": {"title": "Black Ops 4", "exe": "BlackOps4.exe, Cordycep dump (BlackOps4_dump.exe, flat)",
           "corpus": os.path.join(TOOLCHAIN, "corpus", "t8", "decompiled")},
    "t9": {"title": "Black Ops Cold War",
           "exe": "BlackOpsColdWar.exe, Cordycep dump (BlackOpsColdWar_dump.exe, flat)",
           "corpus": os.path.join(TOOLCHAIN, "corpus", "t9", "decompiled")},
}
# Candidate opcode builtins (the T7 list, measured again here): a name is an intrinsic of this
# game when the decompiled corpus calls it by name (acts names opcodes, never table builtins,
# which it writes function_<hash>) and its hash has no entry in the lookup of that kind.
INTRINSIC_CANDIDATES = [
    "isdefined", "vectorscale", "anglestoup", "anglestoright", "anglestoforward", "angleclamp180",
    "vectortoangles", "abs", "gettime", "getdvar", "getdvarint", "getdvarfloat", "getdvarvector",
    "getdvarcolorred", "getdvarcolorgreen", "getdvarcolorblue", "getdvarcoloralpha",
    "getfirstarraykey", "getnextarraykey", "profilestart", "profilestop", "waitrealtime",
    "isarray", "isint", "isfloat", "isstring", "isvec", "isfunctionptr", "isentity", "isstruct",
    "getdvarstring", "getdvarbool", "getdvarhash", "realwait", "waitframe", "hash", "vectordot",
    "length", "distance", "distancesquared", "getrealtime", "waittilltimeout",
    "endoncallback", "waittillmatchtimeout", "int", "float", "string", "istring", "wait",
]


# ---------------------------------------------------------------------------------------------
# name sources
def t7_names():
    """{name: name_source} of the T7 catalogue."""
    path = os.path.join(paths.get("gsc_data"), "builtins", "t7.toml")
    out = {}
    name = None
    with open(path, encoding="utf-8") as f:
        for line in f:
            m = re.match(r'^name = "([a-z0-9_]+)"', line)
            if m:
                name = m.group(1)
            m = re.match(r'^name_source = "([a-z0-9-]+)"', line)
            if m and name:
                out.setdefault(name, m.group(1))
                name = None
    return out


def sources(game):
    # this machine's name sources: tools/re/paths.local.json (or GSC_RE_<KEY>)
    gw = paths.get("graywolf_package_index") or ""
    chr_ = paths.get("cod_hash_recovery") or ""
    tmp = paths.get_list("hash_lists")
    glossary = [g for p in paths.get_list("bo4_glossary") for g in (glob.glob(p) if "*" in p else [p])]
    return [
        ("corpus-scripts", lambda: words_of(files_under(GAMES[game]["corpus"], ("gsc", "csc")))),
        ("bo4-glossary", lambda: words_of(glossary)),
        ("graywolf-name-index", lambda: words_of(glob.glob(os.path.join(gw, "*.wni")))),
        ("cod-hash-recovery", lambda: words_of(list(files_under(os.path.join(chr_, "data"))) +
                                               list(files_under(os.path.join(chr_, "tools"))))),
        ("hash-list", lambda: words_of(p for root in tmp for p in files_under(root))),
    ]


def load_names(game, hashes, log):
    """({hash: name}, {hash: source}, {source: hashes it names alone}, {source: candidates})."""
    alone = {}
    ncand = {}
    hits = {}
    t7 = t7_names()
    lab_t7 = {}
    for n, src in t7.items():
        h = t89_scr(n)
        if h in hashes:
            hits.setdefault(h, n)
            lab_t7[h] = src
    alone["t7-catalogue"] = set(lab_t7)
    ncand["t7-catalogue"] = len(t7)
    order = [(h, lab_t7[h]) for h in lab_t7]
    names = {h: hits[h] for h in lab_t7}
    src = dict(lab_t7)
    for label, get in sources(game):
        words = get()
        ncand[label] = len(words)
        mine = {}
        for w in words:
            if len(w) < 2:
                continue
            h = t89_scr(w)
            if h in hashes:
                mine.setdefault(h, set()).add(w)
        alone[label] = set(mine)
        for h, ws in mine.items():
            if h not in names:
                names[h] = sorted(ws)[0]
                src[h] = label
        log.append("names: %s: %d candidate words, names %d hashes alone" % (label, len(words), len(mine)))
    return names, src, alone, ncand


# ---------------------------------------------------------------------------------------------
def build(game, extract_path, data_dir, report_path):
    G = GAMES[game]
    with open(extract_path, encoding="utf-8") as f:
        ex = json.load(f)
    E = ex["entries"]
    hashes = {e["canon"] for e in E}
    blog = []
    names, nsrc, alone, ncand = load_names(game, hashes, blog)
    calls, defined, nfiles = corpus.scan(G["corpus"])
    R.effective_entries(E, names)
    by_key = R.by_key(E)
    key_of = R.hashed_key(t89_scr)

    # intrinsics: called by name in the decompiled corpus and absent from the lookup
    named_calls = collections.Counter()
    for (n, kind, vm, a), c in calls.items():
        if corpus.hashed_name(n) is None and n not in defined:
            named_calls[(n, kind)] += c
    intrinsics = []
    intr_names = []
    for name in sorted(set(INTRINSIC_CANDIDATES)):
        h = t89_scr(name)
        tab = sorted("%s_%s" % (vm, k) for (hh, vm, k) in by_key if hh == h)
        sites = sum(c for (n, k, vm, a), c in calls.items() if n == name)
        args = sorted({a for (n, k, vm, a) in calls if n == name})
        if not sites:
            continue
        kinds = sorted({k for (n, k, vm, a) in calls if n == name})
        if any("_" + k in " ".join(tab) for k in kinds):
            continue  # a table builtin of the kind it is called as: not an opcode
        intr_names.append(name)
        intrinsics.append({
            "name": name, "hash": "0x%08x" % h, "also_builtin": tab, "corpus_args": args,
            "evidence": "called by name in the acts-decompiled corpus (acts writes opcodes by name, "
                        "almost every table builtin as function_<hash>), no table entry of the "
                        "kind it is called as: %d call sites; t89.scr hash in the "
                        "four lookups' tables: %s" % (sites, ", ".join(tab) or "none")})
    cs = R.validate_calls(E, calls, defined, key_of, intr_names)

    write_toml(game, G, ex, E, nsrc, intrinsics, os.path.join(data_dir, "builtins", "%s.toml" % game))
    write_report(game, G, ex, E, dict(cs=cs, nfiles=nfiles, alone=alone, ncand=ncand, nsrc=nsrc,
                                      intrinsics=intrinsics, blog=blog, named_calls=named_calls),
                 report_path)


def normalised(e, nsrc):
    hp = e.get("handler") or {}
    pt = H.param_types(hp) if hp else None
    if pt:
        pt = [t if re.match(r"^(\?|[a-z0-9]+(\|[a-z0-9]+)*)$", t) else "?" for t in pt]
    return R.lookup_entry(e, nsrc, "static data", VARIADIC, pt)


def write_toml(game, G, ex, E, nsrc, intrinsics, path):
    R.write_builtins_toml(
        path,
        ["%s (%s) engine builtins: every entry of the four lookups the VM uses" % (G["title"], game.upper()),
         "(GSC/CSC x function/method), extracted from the decrypted game executable by",
         "gruff/tools/re/treyarch/t89_builtins.py (static tables; lookups and handlers run under Unicorn).",
         "[[intrinsic]]: names the decompiled corpus calls as opcodes, which no lookup of that kind resolves.",
         "Schema: ../schema/builtins.schema.json. Facts only; no documentation text."],
        [("game", game), ("license", "CC0-1.0"), ("source", "extracted from game executable"),
         ("hash_family", "t89.scr")],
        [("tool", R.toml_str("gruff/tools/re/treyarch/t89_builtins.py + build_t89.py")),
         ("executable", R.toml_str(G["exe"])),
         ("pe_timestamp", "%d" % ex["pe_timestamp"]),
         ("entry_layout", R.toml_str("u32 hash (t89.scr), u32 min_args, u32 max_args, u32 pad, "
                                     "u64 handler, u32 type, u32 pad (0x20 bytes)")),
         ("variadic_max", "%d" % VARIADIC),
         ("counts", R.vm_kind_counts(E))],
        R.lookups_of(ex),
        [normalised(e, nsrc) for e in E], intrinsics=intrinsics)


def write_report(game, G, ex, E, s, path):
    live = [e for e in E if not e["shadowed"]]
    cnt = collections.Counter((e["vm"], e["kind"]) for e in live)
    named = [e for e in live if e["name"]]
    src = collections.Counter(s["nsrc"][e["canon"]] for e in named)
    L = []
    w = L.append
    w("# %s (%s) builtin catalogue: extraction and validation" % (game.upper(), G["title"]))
    w("")
    w("Generated by `treyarch/build_t89.py --game %s` from `treyarch/t89_builtins.py extract --game "
      "%s`. Executable: `%s`, PE timestamp %d, image base 0x%x." % (
          game, game, os.path.basename(ex["exe"]), ex["pe_timestamp"], ex["image_base"]))
    w("")
    w("## Extraction log")
    w("")
    w("```")
    L.extend(ex["log"])
    L.extend(s["blog"])
    w("```")
    w("")
    w("## Counts")
    w("")
    w("| VM | kind | entries | tables (bsearch) |")
    w("|---|---|---|---|")
    for role in R.VM_ORDER:
        vm, kind = role.split("_")
        tb = ex["lookups"][role]["tables"]
        w("| %s | %s | %d | %d (%d) |" % (vm, kind, cnt[(vm, kind)], len(tb),
                                          sum(1 for t in tb if t["scan"] == "bsearch")))
    w("")
    w("%d table entries, %d effective (%d shadowed by the same hash in an earlier table of the "
      "same lookup), %d distinct hashes. The BG tables are shared by the GSC and CSC function "
      "lookups." % (len(E), len(live), len(E) - len(live), len({e["canon"] for e in E})))
    w("")
    dev = collections.Counter(e["eff_type"] for e in live)
    w("Type flags (as the lookup returns them): %s (1: dev-only)." % ", ".join(
        "%d: %d" % kv for kv in sorted(dev.items())))
    w("")
    w("## Tables")
    w("")
    w("| lookup | table | entries | scan | static bound | shaped run at the address |")
    w("|---|---|---|---|---|---|")
    for role in R.VM_ORDER:
        for t in ex["lookups"][role]["tables"]:
            w("| %s | 0x%x | %d | %s | %s | %d%s |" % (
                role, t["rva"], t["count"], t["scan"] + (
                    " (index 0x%x)" % t["index_array_rva"] if t["index_array_rva"] else ""),
                ",".join(map(str, t["static_bounds"])) or "-", t["shaped_run"],
                "" if t["run_agrees"] else " **disagrees**"))
    w("")
    w("A shaped run longer than the count is the next table, adjacent in .rdata (another getter's).")
    w("")
    w("## Names")
    w("")
    w("Named: %d/%d effective entries (%s); by attributed source: %s. Unnamed: %d (hash only)."
      % (len(named), len(live), R.pct(len(named), len(live)),
         ", ".join("%s %d" % kv for kv in src.most_common()), len(live) - len(named)))
    w("")
    w("| source | candidate strings | distinct hashes it names alone |")
    w("|---|---|---|")
    for lab, hs in s["alone"].items():
        w("| %s | %d | %d |" % (lab, s["ncand"][lab], len(hs)))
    w("")
    nc = sum(s["ncand"].values())
    w("A 32-bit hash: with about %d candidate strings and %d distinct hashes, about %.1f names "
      "are expected to match by chance (a string that hashes to an entry is the only proof)."
      % (nc, len({e["canon"] for e in E}), nc * len({e["canon"] for e in E}) / 2 ** 32))
    w("")
    L.extend(R.calls_section(s["cs"], "%s, %d files" % (os.path.relpath(G["corpus"], TOOLCHAIN).replace("\\", "/"),
                                                         s["nfiles"]), VARIADIC))
    called, live_n = R.coverage(E, s["cs"])
    w("Coverage: %d/%d effective entries are called at least once by the corpus (%s). Named-call "
      "coverage: %d of the called entries have a name." % (
          called, live_n, R.pct(called, live_n),
          sum(1 for e in live if id(e) in s["cs"]["called"] and e["name"])))
    w("")
    w("## Compiler intrinsics (opcode builtins)")
    w("")
    w("acts writes the opcodes by name and almost every table builtin as `function_<hash>` (a "
      "few methods, delete, hide, linkto..., by name). Listed: the candidate opcode names (T7's "
      "list and the T8/T9 wait/cast forms) that the corpus calls and that have no table entry "
      "of the kind they are called as; an entry of the other kind is shown.")
    w("")
    w("| name | table entries | call sites | corpus arg counts |")
    w("|---|---|---|---|")
    for it in s["intrinsics"]:
        n = it["name"]
        sites = sum(c for (nn, k), c in s["named_calls"].items() if nn == n)
        w("| %s | %s | %d | %s |" % (n, ", ".join(it["also_builtin"]) or "**none**", sites,
                                    ",".join(map(str, it["corpus_args"])) or "-"))
    w("")
    hl, hst = R.handler_section(E, VARIADIC)
    L.extend(hl)
    acc = ex.get("accessors", {})
    if acc:
        w("Accessors (label = the VM types they accept, measured by running each alone): %s."
          % ", ".join("%s %d" % kv for kv in collections.Counter(v["label"] for v in acc.values()).most_common()))
        w("")
    w("## Open issues")
    w("")
    w("- %d effective entries have no name: no candidate string hashes to them." % (len(live) - len(named)))
    w("- The bsearch index arrays are zero in the dump; they are rebuilt as the hash order of "
      "their table (the only order the comparator accepts). The round trip shows the engine's "
      "lookup then returns every entry, but the game's own filler was not run.")
    w("- Parameter types are the weakest measure (one handler run with N parameters, accessors "
      "hooked); error texts are hashed in this engine, so accessors are recognised by their "
      "outparamcount bound check and typed by emulation, not by their messages.")
    w("")
    R.write_md(path, L)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--game", choices=sorted(GAMES), required=True)
    ap.add_argument("--extract")
    ap.add_argument("--data", default=paths.get("gsc_data"))
    ap.add_argument("--report")
    a = ap.parse_args()
    build(a.game, a.extract or os.path.join(RE, "out", "%s_extract.json" % a.game), a.data,
          a.report or os.path.join(RE, "reports", "%s_builtins.md" % a.game))


if __name__ == "__main__":
    main()
