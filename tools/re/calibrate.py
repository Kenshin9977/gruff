"""The corpus second measure of the builtin catalogues, run after any build step:

    py -3.11 -I calibrate.py <game>... [--gsc PATH] [--dry-run]     (from tools/re)

For each catalogue (gruff-data/builtins/<game>.toml) it

1. writes `[coverage]`: complete = false when an executable of the game was not extracted
   (COVERAGE below) or some entry has neither a name nor a hash (gruff check then keeps B001 off);
2. adds the language intrinsics the decompiled corpus calls but no lookup holds (`endon`,
   `notify`, `waittill`... in T8+, each checked absent from every table), and gives every
   `[[intrinsic]]` its arity from its `corpus_args` (the compiler checks an intrinsic's count, so
   the shipped scripts only hold accepted counts; four or more distinct counts make it variadic);
3. runs `gruff check --select B002` on the game's own corpus and, where the handler-measured arity
   (IW handler runs, T4/T5 emulation, or a table whose bounds the shipped bytecode exceeds: T6,
   JUP) is contradicted by the game's shipped scripts in at least two files or three call sites
   (any number for a measured 0..0), corrects the entry: a measured 0..0 (the handler rejected
   the neutral values of a value-checked parameter, e.g. getentarray's key) becomes
   arity_unknown; otherwise min/max widen to the counts the scripts use. The extraction's values
   stay in `measured_args`, the change in `corpus_arity` and the evidence line. A rarer
   contradiction is listed for review (table arities) or as a possible script bug.

The pass is idempotent: it first restores every entry it changed before (measured_args), so a
rerun on the same corpus gives the same file. Report: reports/calibrate.md.
"""
import argparse
import collections
import json
import os
import re
import subprocess
import sys
import tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from common import paths  # noqa: E402
from common.hashes import FAMILIES  # noqa: E402
from common.report import toml_value as enc  # noqa: E402

DATA = os.path.join(paths.get("gsc_data"), "builtins")
REPORT = os.path.join(HERE, "reports", "calibrate.md")
MARK = "; corpus second measure: "

# game: (corpus folders under corpus/ (absolute paths allowed), `gruff check --game`, auto-correct)
GAMES = {
    "iw1": (["iw1/source"], "iw1", True), "iw3": (["iw3/source"], "iw3", True),
    "iw4": (["iw4/source"], "iw4", True), "iw5": (["iw5/decompiled"], "iw5", True),
    "iw6": (["iw6/decompiled"], "iw6", True), "s1": (["s1/decompiled"], "s1", True),
    "h1": (["h1/decompiled"], "h1", True), "iw7": (["iw7/decompiled"], "iw7", True),
    "iw8": (["iw8/decompiled"], "iw8", True), "jup": (["jup/decompiled"], "jup", True),
    "jup-sp": (["jup_sp/decompiled"], "jup-sp", True),
    "t4": (["t4/source"], "t4", True), "t5": (["t5/source", "t5/source-modtools"], "t5", True),
    "t6": (["t6/decompiled"], "t6", True), "t7": (["t7/source-modtools", "t7/decompiled"], "t7", False),
    "t8": (["t8/decompiled"], "t8", False), "t9": (["t9/decompiled"], "t9", False),
    "t10": ([], "t10", False), "t11": (["@bo7_corpus"], "t11", False),
}
# executables of the game the catalogue does not cover
COVERAGE = {
    "iw1": "only the campaign game module (gamex86.dll) was extracted: the multiplayer module's "
           "builtins (setclientcvar, getguid...) are missing",
    "iw3": "only iw3mp.exe was extracted (iw3sp.exe is SteamStub-encrypted): the campaign's "
           "builtins (getnode, getaiarray...) are missing",
    "iw4": "only iw4mp.exe was extracted, and most entries have no name (only IW5 handler matches)",
    "iw6": "only the campaign dump was extracted (iw6mp64_ship.exe is encrypted)",
    "t4": "only the campaign executable was extracted: the multiplayer builtins are missing",
}
# T8+: statements the compiler emits as opcodes and the decompiler writes as calls (acts opcode
# names as the cross-check); each is checked absent from every table before it is added
LANGUAGE_INTRINSICS = {
    "t8": ["endon", "notify", "waittill", "waittillmatch", "waittillframeend", "gettime"],
    "t9": ["endon", "notify", "waittill", "waittillmatch", "waittillframeend", "gettime",
           "endoncallbackparam"],
    "t10": ["endon", "notify", "waittill", "waittillmatch", "waittillframeend", "flat_args"],
    "t11": ["endon", "notify", "waittill", "waittillmatch", "waittillframeend", "flat_args"],
    "jup": ["isdefined", "istrue", "endon", "notify", "waittill", "waittillmatch",
            "waittillframeend", "waitframe"],
}
LANGUAGE_INTRINSICS["jup-sp"] = LANGUAGE_INTRINSICS["jup"]
ANALOGUE = {"t10": "t11"}  # a game without a corpus, and the game whose corpus stands in
ACTS_OPCODE = {"endon": "EndOn", "notify": "Notify", "waittill": "WaitTill",
               "waittillmatch": "WaitTillMatch", "waittillframeend": "WaitTillFrameEnd",
               "gettime": "GetTime", "endoncallbackparam": "T9_EndOnCallbackParam",
               "flat_args": "T10_FlatArgs", "isdefined": "IsDefined", "istrue": "IW_IsTrue",
               "waitframe": "WaitFrame / IW_WaitFrame"}
IW_OPCODE = {"endon": "IW_SingleEndon", "notify": "IW_Notify", "waittill": "IW_SingleWaitTill",
             "waittillmatch": "IW_WaitTillMatch"}  # JUP's spellings in acts
B002 = re.compile(r"builtin (function|method) `(\w+)` takes .*, but (\d+) (?:were|was) given")
B001 = re.compile(r"unknown builtin (?:function|method) `(\w+)`|`(\w+)` is a (GSC|CSC) builtin")


# ---------------------------------------------------------------------------------------------
# the TOML as blocks of lines (a header, then one block per [[builtin]] / [[intrinsic]])
def split_blocks(text):
    lines = text.split("\n")
    blocks, cur = [], []
    for ln in lines:
        if ln in ("[[builtin]]", "[[intrinsic]]") and cur:
            blocks.append(cur)
            cur = []
        cur.append(ln)
    blocks.append(cur)
    return blocks


def get(block, key):
    for ln in block:
        if ln.startswith(key + " = "):
            return tomllib.loads(ln)[key]
    return None


def put(block, key, value, after=None):
    """Set `key` (TOML-encoded value), in place, or after the line of `after` (else at the end)."""
    line = "%s = %s" % (key, enc(value))
    for i, ln in enumerate(block):
        if ln.startswith(key + " = "):
            block[i] = line
            return
    if after:
        for i, ln in enumerate(block):
            if ln.startswith(after + " = "):
                block.insert(i + 1, line)
                return
    end = len(block)
    while end and block[end - 1] == "":
        end -= 1
    block.insert(end, line)


def drop(block, key):
    block[:] = [ln for ln in block if not ln.startswith(key + " = ")]


def restore(blocks):
    """Undo a previous pass: measured values back, our lines and tables out."""
    head = blocks[0]
    out = []
    skip = False
    for ln in head:  # the [coverage] table (written by this pass only)
        if ln == "[coverage]":
            skip = True
            continue
        if skip and (ln.startswith("[") or ln == ""):
            skip = False
            if ln == "":
                continue
        if not skip:
            out.append(ln)
    head[:] = out
    drop(head, "corpus_second_measure")
    keep = [head]
    for b in blocks[1:]:
        if b[0] == "[[intrinsic]]":
            if get(b, "arity_source") == "corpus call sites (corpus_args)":
                for k in ("min_args", "max_args", "variadic", "arity_source"):
                    drop(b, k)
            if (get(b, "evidence") or "").startswith("added by calibrate.py"):
                continue
        m = get(b, "measured_args")
        if m is not None:
            lo, hi = m
            put(b, "min_args", lo)
            put(b, "max_args", hi)
            if get(b, "corpus_arity") == "unknown":
                drop(b, "arity_unknown")
            drop(b, "measured_args")
            drop(b, "corpus_arity")
            ev = get(b, "evidence") or ""
            put(b, "evidence", ev.split(MARK)[0])
        keep.append(b)
    blocks[:] = keep


# ---------------------------------------------------------------------------------------------
def corpus_dirs(game):
    out = []
    for c in GAMES[game][0]:
        out.append(paths.need(c[1:]) if c.startswith("@") else os.path.join(paths.get("corpus"), c))
    return [d for d in out if os.path.isdir(d)]


def run_check(gsc, game, dirs, select):
    if not dirs:
        return []
    cmd = [gsc, "check", "--isolated", "--exit-zero", "--output-format", "json", "--select", select,
           "--game", GAMES[game][1], "--data-dir", paths.get("gsc_data")] + dirs
    r = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace")
    if r.returncode not in (0, 1):
        sys.exit("calibrate: %s failed: %s" % (" ".join(cmd[:4]), r.stderr[:500]))
    return json.loads(r.stdout or "[]")


def short(path):
    p = path.replace("\\", "/")
    m = re.search(r"/corpus/(.*)$", p)
    return m.group(1) if m else p.rsplit("/", 2)[-2] + "/" + p.rsplit("/", 1)[-1]


def coverage(game, blocks, header):
    unnamed = sum(1 for b in blocks[1:] if b[0] == "[[builtin]]"
                  and get(b, "name") is None and get(b, "hash") is None)
    notes = []
    if game in COVERAGE:
        notes.append(COVERAGE[game])
    if unnamed:
        notes.append("%d %s neither a name nor a hash" % (unnamed, "entry has" if unnamed == 1 else "entries have"))
    if not notes:
        return None
    return ["[coverage]", "complete = false", "note = %s" % enc("; ".join(notes)), ""]


def intrinsics(game, cat, blocks, gsc):
    """Language intrinsics (LANGUAGE_INTRINSICS) and every intrinsic's arity from corpus_args."""
    log = []
    fam = FAMILIES.get(cat.get("hash_family"))
    hashes = {int(b["hash"], 16) for b in cat["builtin"] if "hash" in b}
    names = {b.get("name") for b in cat["builtin"]}
    have = {get(b, "name") for b in blocks[1:] if b[0] == "[[intrinsic]]"}
    want = [n for n in LANGUAGE_INTRINSICS.get(game, []) if n not in have]
    if want:
        diags = run_check(gsc, game, corpus_dirs(game), "B001,B002")
        sites = collections.Counter()
        files = collections.defaultdict(set)
        argcs = collections.defaultdict(set)
        for d in diags:
            m = B001.search(d["message"])
            if m:
                n = (m.group(1) or m.group(2)).lower()
                sites[n] += 1
                files[n].add(d["filename"])
        for n in want if not corpus_dirs(game) and game in ANALOGUE else []:
            if (fam is not None and fam(n) in hashes) or n in names:
                log.append("intrinsic %s: a table holds it, not added" % n)
                continue
            ev = ("added by calibrate.py: no lookup of the executable resolves it (its hash is in "
                  "no table); no %s corpus here, the %s corpus calls it (same compiler generation; "
                  "acts opcode %s)" % (game, ANALOGUE[game], ACTS_OPCODE.get(n, "?")))
            blocks.append(["[[intrinsic]]", "name = %s" % enc(n), "evidence = %s" % enc(ev), ""])
            log.append("intrinsic %s added (absent from the tables; by analogy with %s)" % (
                n, ANALOGUE[game]))
        for n in want if corpus_dirs(game) else []:
            if not sites[n]:
                log.append("intrinsic %s: no unresolved call site, not added" % n)
                continue
            in_table = (fam is not None and fam(n) in hashes) or n in names
            if in_table and n != "gettime":
                log.append("intrinsic %s: a table holds it, not added" % n)
                continue
            argcs[n] = arg_counts(files[n], n)
            vm_note = "the GSC table holds it, the CSC lookups do not: " if in_table else ""
            ev = ("added by calibrate.py: %sno lookup of the executable resolves it, %d call sites "
                  "in %d files of the decompiled corpus (acts opcode %s)" % (
                      vm_note, sites[n], len(files[n]),
                      (IW_OPCODE if game.startswith("jup") else {}).get(n) or ACTS_OPCODE.get(n, "?")))
            b = ["[[intrinsic]]", "name = %s" % enc(n)]
            if in_table:
                b.append('vm = "csc"')
                b.append('also_builtin = ["gsc_function"]')
            b.append("corpus_args = %s" % enc(sorted(argcs[n])))
            b.append("evidence = %s" % enc(ev))
            b.append("")
            blocks.append(b)
            log.append("intrinsic %s added (%d sites, %d files, argc %s)" % (
                n, sites[n], len(files[n]), sorted(argcs[n])))
    vmax = cat.get("extraction", {}).get("variadic_max", 255)
    for b in blocks[1:]:
        if b[0] != "[[intrinsic]]" or get(b, "min_args") is not None:
            continue
        ca = get(b, "corpus_args") or []
        if not ca:
            continue
        lo, hi = min(ca), max(ca)
        if len(set(ca)) >= 4:
            hi = vmax
            put(b, "variadic", True, after="corpus_args")
        put(b, "max_args", hi, after="corpus_args")
        put(b, "min_args", lo, after="corpus_args")
        put(b, "arity_source", "corpus call sites (corpus_args)", after="max_args")
    return log


CALL = re.compile(r"\b(\w+)\s*\(")


def arg_counts(files, name):
    """Argument counts of `name(...)` calls in `files` (top-level commas)."""
    out = set()
    for f in files:
        try:
            src = open(f, encoding="utf-8", errors="replace").read()
        except OSError:
            continue
        for m in re.finditer(r"\b%s\s*\(" % re.escape(name), src, re.I):
            i, depth, n, empty = m.end(), 1, 0, True
            while i < len(src) and depth:
                c = src[i]
                if c in "([{":
                    depth += 1
                elif c in ")]}":
                    depth -= 1
                elif c == "," and depth == 1:
                    n += 1
                elif not c.isspace() and depth >= 1:
                    empty = False
                i += 1
            out.add(0 if empty else n + 1)
    return out


def correct(game, cat, blocks, gsc, rep):
    auto = GAMES[game][2]
    dirs = corpus_dirs(game)
    before = run_check(gsc, game, dirs, "B002")
    groups = collections.defaultdict(lambda: {"argc": collections.Counter(), "files": set()})
    for d in before:
        m = B002.search(d["message"])
        if not m:
            continue
        kind, name, argc = m.group(1), m.group(2).lower(), int(m.group(3))
        vm = "csc" if d["filename"].lower().endswith(".csc") else "gsc"
        g = groups[(name, kind, vm)]
        g["argc"][argc] += 1
        g["files"].add(short(d["filename"]))
    by_key = collections.defaultdict(list)
    for b in blocks[1:]:
        if b[0] == "[[builtin]]" and get(b, "name"):
            by_key[(get(b, "name"), get(b, "kind"), get(b, "vm"))].append(b)
    changed, review, rare = [], [], []
    for key, g in sorted(groups.items(), key=lambda kv: -sum(kv[1]["argc"].values())):
        name, kind, vm = key
        sites, nfiles = sum(g["argc"].values()), len(g["files"])
        ents = [b for b in by_key.get(key, []) if get(b, "min_args") is not None
                and get(b, "min_args") >= 0 and not get(b, "arity_unknown")]
        table = [b for b in ents if get(b, "arity_mode") == "table"]
        argcs = sorted(g["argc"])
        where = "%d call sites in %d files (%s), argc %s" % (
            sites, nfiles, ", ".join(sorted(g["files"])[:3]) + (", ..." if nfiles > 3 else ""),
            "/".join(str(a) for a in argcs))
        if not ents:
            continue
        lo, hi = get(ents[0], "min_args"), get(ents[0], "max_args")
        if not auto or (table and nfiles < 2 and sites < 3):
            review.append("%s %s `%s` (catalogue %d..%d): %s" % (vm, kind, name, lo, hi, where))
            continue
        if nfiles < 2 and sites < 3 and (lo, hi) != (0, 0):  # 0..0 is the value-check artifact
            rare.append("%s %s `%s` (catalogue %d..%d): %s" % (vm, kind, name, lo, hi, where))
            continue
        corp = "corpus/" + ", corpus/".join(GAMES[game][0]) if not GAMES[game][0][0].startswith("@") \
            else "the BO7 archive"
        for b in ents:
            blo, bhi = get(b, "min_args"), get(b, "max_args")
            put(b, "measured_args", [blo, bhi])
            if blo == 0 and bhi == 0:
                put(b, "arity_unknown", True)
                put(b, "corpus_arity", "unknown")
                what = "the measured 0..0 is discarded (arity_unknown: a value-checked parameter " \
                       "rejects the neutral values)"
            else:
                nlo, nhi = min(blo, argcs[0]), max(bhi, argcs[-1])
                put(b, "min_args", nlo)
                put(b, "max_args", nhi)
                put(b, "corpus_arity", "widened")
                what = "widened from %d..%d to %d..%d" % (blo, bhi, nlo, nhi)
            ev = get(b, "evidence") or ""
            put(b, "evidence", ev + MARK + "%s of %s pass argc %s outside the measured %d..%d: %s" % (
                "%d call sites in %d files" % (sites, nfiles), corp,
                "/".join(str(a) for a in argcs if not blo <= a <= bhi), blo, bhi, what))
            # keep the fields in a stable place
            for k in ("measured_args", "corpus_arity", "arity_unknown"):
                v = get(b, k)
                if v is not None:
                    drop(b, k)
                    put(b, k, v, after="max_args")
            ev2 = get(b, "evidence")
            drop(b, "evidence")
            put(b, "evidence", ev2)
        changed.append("%s %s `%s` (%d..%d): %s; %s" % (vm, kind, name, lo, hi, where, what))
    rep["changed"], rep["review"], rep["rare"] = changed, review, rare
    rep["before"] = len(before)
    return changed


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("games", nargs="+")
    ap.add_argument("--gsc", default=os.path.join(HERE, "..", "..", "target", "release",
                                                   "gruff.exe" if os.name == "nt" else "gruff"))
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()
    reports = []
    for game in a.games:
        path = os.path.join(DATA, game + ".toml")
        text = open(path, encoding="utf-8").read()
        blocks = split_blocks(text)
        restore(blocks)
        cov = coverage(game, blocks, blocks[0])
        if cov:
            i = blocks[0].index("[extraction]")
            blocks[0][i:i] = cov
        rep = {"game": game}
        base = "\n".join("\n".join(b) for b in blocks)
        open(path, "w", encoding="utf-8", newline="\n").write(base)  # gruff check reads the file
        cat = tomllib.loads(base)
        rep["intrinsics"] = intrinsics(game, cat, blocks, a.gsc)
        open(path, "w", encoding="utf-8", newline="\n").write("\n".join("\n".join(b) for b in blocks))
        changed = correct(game, cat, blocks, a.gsc, rep)
        if changed:
            i = next(i for i, ln in enumerate(blocks[0]) if ln.startswith("counts = "))
            blocks[0].insert(i + 1, "corpus_second_measure = %s" % enc(
                "tools/re/calibrate.py: %d entries corrected from the game's corpus (B002 before: %d)"
                % (sum(1 for b in blocks[1:] if get(b, "corpus_arity")), rep["before"])))
        out = "\n".join("\n".join(b) for b in blocks)
        if a.dry_run:
            open(path, "w", encoding="utf-8", newline="\n").write(text)
        else:
            open(path, "w", encoding="utf-8", newline="\n").write(out)
        rep["after"] = len(run_check(a.gsc, game, corpus_dirs(game), "B002"))
        if a.dry_run:
            open(path, "w", encoding="utf-8", newline="\n").write(text)
        reports.append(rep)
        print("%s: B002 %d -> %d; %d corrected, %d to review, %d rare; %s" % (
            game, rep["before"], rep["after"], len(rep["changed"]), len(rep["review"]),
            len(rep["rare"]), "; ".join(rep["intrinsics"])))
    if not a.dry_run:
        write_report(reports)


def write_report(reports):
    old = {}
    if os.path.isfile(REPORT):
        cur = None
        for ln in open(REPORT, encoding="utf-8").read().split("\n"):
            m = re.match(r"^## (\S+)$", ln)
            if m:
                cur = m.group(1)
                old[cur] = []
            elif cur:
                old[cur].append(ln)
    for r in reports:
        L = ["", "B002 on the game's corpus: %d before the pass, %d after." % (r["before"], r["after"]), ""]
        for title, key in (("Intrinsics", "intrinsics"), ("Corrected from the corpus", "changed"),
                           ("Table arities contradicted (review, not changed)", "review"),
                           ("Rare contradictions (one file, at most two sites): possible script bugs",
                            "rare")):
            if r[key]:
                L += ["### " + title, ""] + ["- " + x for x in r[key]] + [""]
        old[r["game"]] = L
    out = ["# Corpus second measure of the builtin catalogues", "",
           "Generated by `calibrate.py` (see its docstring). Per game: what the pass changed in "
           "gruff-data/builtins/<game>.toml and what it left for review.", ""]
    for g in sorted(old):
        out += ["## " + g] + old[g]
    open(REPORT, "w", encoding="utf-8", newline="\n").write("\n".join(out).rstrip("\n") + "\n")


if __name__ == "__main__":
    main()
