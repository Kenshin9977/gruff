"""Parameter types of the builtins, measured on the corpus (the second measure of param_types):

    py -3.11 -I types_corpus.py <game>... [--gsc PATH] [--min-sites N] [--dry-run]    (from tools/re)

For every call site of a builtin in the game's own scripts (the corpus folders of calibrate.py,
each folder its own workspace), `gruff types` infers each argument's type where the scripts alone
determine it (gsc_sema::types):

- literals: int, float, string, istring (`&"..."`), hash (`#"..."`), vector `(x, y, z)`, `[]`,
  `{}`, function pointers, `%anim`, `#animtree`, `true`/`false` (bool);
- the result of a builtin every engine types the same way (randomint: int, distance: float,
  anglestoforward: vector, getentarray: array, spawn: entity; weapon getters: weapon objects in
  T7+ and IW8+), and `.size`, `.health` (int), `.origin`, `.angles` (vector);
- local flow: a variable has the join of everything assigned to it in its function;
- script functions: a parameter has the join of the arguments of every resolved call site and of
  its default value, a function's result the join of its returns, iterated over the workspace to
  a fixpoint. A function the engine or a pointer may call (no call site, autoexec, codecall,
  callbacks, address taken) has unknown parameters.

Anything else (fields, array elements, self, waittill and foreach bindings) is unknown. Per
builtin parameter the counts go to `corpus_arg_types` (one table per argument index, at most 16),
and a derived type to `corpus_param_types` when the evidence is consistent: at least
--min-sites typed sites (unknown and undefined left out; bool counts as int), and one type
holding at least 95% of them; int and float mixed make `number`, string and hash mixed
`hash|string`; '?' otherwise. The handler-measured `param_types` are never changed: where both
give a type and they disagree, the report lists it (reports/types_<game>.md: counts only, no
script text). T7 also gets the BO3 documentation's examples as an oracle (the literal argument
types of each EXAMPLE call; read locally, only counts are written; a CSC example often leaves
out the leading localclientnum, which shifts its arguments).

The pass is idempotent: it drops its own fields first.
"""
import argparse
import collections
import html
import json
import os
import re
import subprocess
import sys
import tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import calibrate  # noqa: E402
from calibrate import split_blocks, get, put, drop, enc  # noqa: E402
from common import paths  # noqa: E402
from common.corpus import hashed_name  # noqa: E402

DATA = calibrate.DATA
REPORTS = os.path.join(HERE, "reports")
FIELDS = ("corpus_arg_types", "corpus_param_types")
MAX_INDEX = 16
SHARE = 0.95
NUMERIC = {"int", "float", "number"}
# handler/documented types that say nothing about the value
VAGUE = {"?", "any", "unknown", "undefined", "pointer"}
# corpus types a handler accessor type accepts
ACCEPTS = {
    "int": {"int"}, "float": {"float", "int", "number"}, "string": {"string", "istring"},
    "istring": {"istring"}, "hash": {"hash", "string", "hash|string"},
    "vector": {"vector"}, "entity": {"entity"}, "object": {"entity", "struct", "array", "object"},
    "team": {"string", "hash"}, "weapon": {"weapon"}, "anim": {"anim"},
    "animtree": {"animtree"}, "function": {"function"}, "uint64": {"int", "hash"},
}


def concrete(t):
    """The alternatives of a handler type that name a type (`any|string` -> {string})."""
    return {x for x in (t or "?").split("|") if x not in VAGUE and not re.match(r"^tag\d|^t\d", x)}


def derive(counts, min_sites):
    c = collections.Counter()
    for t, n in counts.items():
        if t in ("unknown", "undefined"):
            continue
        c["int" if t == "bool" else t] += n
    total = sum(c.values())
    if total < min_sites:
        return "?"
    t, n = c.most_common(1)[0]
    if n >= SHARE * total:
        return t
    if sum(c[x] for x in NUMERIC) >= SHARE * total:
        return "number"
    if c["string"] + c["hash"] >= SHARE * total:
        return "hash|string"
    return "?"


def agree(handler, corpus):
    """exact / compatible / conflict / None (nothing to compare)."""
    h = concrete(handler)
    if not h or corpus == "?":
        return None
    accepted = set().union(*(ACCEPTS.get(x, {x}) for x in h))
    if corpus in h or corpus == "|".join(sorted(h)):
        return "exact"
    if corpus in accepted or (corpus == "number" and "int" in h):
        return "compatible"
    return "conflict"


# ---------------------------------------------------------------------------------------------
def run_types(gsc, game, d):
    cmd = [gsc, "types", d, "--game", calibrate.GAMES[game][1], "--data-dir", paths.get("gsc_data")]
    r = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace")
    if r.returncode != 0:
        sys.exit("types: %s failed: %s" % (" ".join(cmd[:4]), r.stderr[:500]))
    return json.loads(r.stdout)


def measure(gsc, game):
    """(per (name, vm, kind): {sites, files, args: [Counter]}, totals)."""
    out = {}
    tot = collections.Counter()
    for d in calibrate.corpus_dirs(game):
        j = run_types(gsc, game, d)
        for k in ("files", "functions", "params", "params_typed"):
            tot[k] += j[k]
        for b in j["builtins"]:
            e = out.setdefault((b["name"], b["vm"], b["kind"]),
                               {"sites": 0, "files": 0, "args": []})
            e["sites"] += b["sites"]
            e["files"] += b["files"]
            for i, a in enumerate(b["args"][:MAX_INDEX]):
                while len(e["args"]) <= i:
                    e["args"].append(collections.Counter())
                e["args"][i].update(a)
    return out, tot


def enc_counts(c):
    return "{ " + ", ".join("%s = %d" % (k if re.match(r"^\w+$", k) else json.dumps(k), v)
                            for k, v in sorted(c.items(), key=lambda kv: (-kv[1], kv[0]))) + " }" \
        if c else "{}"


def known(types):
    return any(concrete(t) for t in types or [])


def coverage(cat_blocks):
    """(entries taking parameters, with >= 1 known handler type, with >= 1 known after)."""
    n = before = after = 0
    for b in cat_blocks[1:]:
        if b[0] != "[[builtin]]" or get(b, "shadowed"):
            continue
        if get(b, "max_args") == 0 and not get(b, "arity_unknown"):
            continue
        n += 1
        pt = get(b, "param_types") or []
        ct = get(b, "corpus_param_types") or []
        before += known(pt)
        after += known(pt) or known(ct)
    return n, before, after


# ---------------------------------------------------------------------------------------------
# the BO3 documentation's EXAMPLE calls, as a second oracle for T7 (literal types only)
def doc_examples():
    sys.path.insert(0, os.path.join(HERE, "treyarch"))
    import officialdoc  # noqa: E402
    if not os.path.isfile(officialdoc.DEFAULT):
        return {}
    text = open(officialdoc.DEFAULT, "rb").read().decode("latin1")
    out = {}
    for m in officialdoc.H1.finditer(text):
        head = html.unescape(re.sub(r"<[^>]+>", "", m.group(1))).strip()
        mm = re.match(r"^\S+\s+(<[^>]*>\s+)?([A-Za-z_]\w*)\s*\(", head)
        ex = re.search(r"EXAMPLE:\s*</B>(.*?)<BR>", m.group(2), re.S)
        if not mm or not ex:
            continue
        name = mm.group(2).lower()
        src = html.unescape(re.sub(r"<[^>]+>", "", ex.group(1)))
        c = re.search(r"\b%s\s*\(" % re.escape(name), src, re.I)
        if not c:
            continue
        args = split_args(src[c.end():])
        out[(name, "method" if mm.group(1) else "function")] = [lit_type(a) for a in args]
    return out


def split_args(s):
    args, depth, cur = [], 1, ""
    for ch in s:
        if ch in "([":
            depth += 1
        elif ch in ")]":
            depth -= 1
            if depth == 0:
                break
        if ch == "," and depth == 1:
            args.append(cur.strip())
            cur = ""
        else:
            cur += ch
    if cur.strip():
        args.append(cur.strip())
    return args


def lit_type(a):
    if re.match(r'^&"', a):
        return "istring"
    if re.match(r'^"[^"]*"$', a):
        return "string"
    if re.match(r"^-?\d+$", a):
        return "int"
    if re.match(r"^-?\d*\.\d+$|^-?\d+\.\d*$", a):
        return "float"
    if re.match(r"^\(.*,.*,.*\)$", a):
        return "vector"
    if a in ("true", "false"):
        return "int"
    return "?"


# ---------------------------------------------------------------------------------------------
def apply(game, blocks, meas, min_sites, rep):
    by_key = collections.defaultdict(list)
    by_hash = collections.defaultdict(list)
    for b in blocks[1:]:
        if b[0] != "[[builtin]]" or get(b, "shadowed"):
            continue
        k = (get(b, "vm"), get(b, "kind"))
        if get(b, "name"):
            by_key[(get(b, "name"),) + k].append(b)
        if get(b, "hash"):
            by_hash[(int(get(b, "hash"), 16),) + k].append(b)
    disagree, comp = [], collections.Counter()
    written = 0
    for (name, vm, kind), m in sorted(meas.items()):
        ents = by_key.get((name, vm, kind), [])
        h = hashed_name(name)
        if not ents and h is not None:
            ents = by_hash.get((h, vm, kind), [])
        if not ents:
            rep["unmatched"] += 1
            continue
        derived = [derive(c, min_sites) for c in m["args"]]
        for b in ents:
            mx = get(b, "max_args")
            n = len(m["args"]) if mx is None or mx < 0 else min(len(m["args"]), max(mx, 0))
            if not n:
                continue
            line = "corpus_arg_types = [%s]" % ", ".join(enc_counts(c) for c in m["args"][:n])
            at = next((i + 1 for i, ln in enumerate(b) if ln.startswith("param_types = ")), None)
            if at is None:
                at = next(i for i, ln in enumerate(b) if ln.startswith("evidence = "))
            b.insert(at, line)
            d = derived[:n]
            if any(x != "?" for x in d):
                put(b, "corpus_param_types", d, after="corpus_arg_types")
                written += 1
            pt = get(b, "param_types") or []
            for i, x in enumerate(d):
                if i >= len(pt):
                    break
                a = agree(pt[i], x)
                if a:
                    comp[a] += 1
                if a == "conflict":
                    disagree.append("%s %s `%s` arg %d: handler `%s`, corpus `%s` (%s)" % (
                        vm, kind, name, i + 1, pt[i], x, ", ".join(
                            "%s %d" % kv for kv in m["args"][i].most_common())))
    rep["written"], rep["disagree"], rep["agree"] = written, disagree, comp


def check_doc(blocks, rep):
    ex = doc_examples()
    comp = collections.Counter()
    rows = []
    for b in blocks[1:]:
        if b[0] != "[[builtin]]" or not get(b, "name"):
            continue
        ct = get(b, "corpus_param_types") or []
        doc = ex.get((get(b, "name"), get(b, "kind")))
        if not doc:
            continue
        for i, x in enumerate(ct):
            if i >= len(doc) or doc[i] == "?" or x == "?":
                continue
            a = agree(doc[i], x)
            if doc[i] == "int" and x in ("float", "number"):
                a = "compatible"  # an example's `1` for a float parameter
            comp[a] += 1
            if a == "conflict":
                rows.append("%s %s `%s` arg %d: documented example `%s`, corpus `%s`" % (
                    get(b, "vm"), get(b, "kind"), get(b, "name"), i + 1, doc[i], x))
    rep["doc"], rep["doc_rows"], rep["doc_entries"] = comp, sorted(set(rows)), len(ex)


def strip(blocks):
    for b in blocks[1:]:
        for k in FIELDS:
            drop(b, k)
    drop(blocks[0], "corpus_type_measure")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("games", nargs="+")
    ap.add_argument("--gsc", default=os.path.join(HERE, "..", "..", "target", "release",
                                                   "gruff.exe" if os.name == "nt" else "gruff"))
    ap.add_argument("--min-sites", type=int, default=5)
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()
    for game in a.games:
        if not calibrate.corpus_dirs(game):
            print("%s: no corpus here, skipped" % game)
            continue
        path = os.path.join(DATA, game + ".toml")
        text = open(path, encoding="utf-8").read()
        blocks = split_blocks(text)
        strip(blocks)
        n, before, _ = coverage(blocks)
        meas, tot = measure(a.gsc, game)
        rep = {"unmatched": 0}
        apply(game, blocks, meas, a.min_sites, rep)
        n, before, after = coverage(blocks)
        if rep["written"]:
            i = next(i for i, ln in enumerate(blocks[0]) if ln.startswith("counts = "))
            blocks[0].insert(i + 1, "corpus_type_measure = %s" % enc(
                "tools/re/types_corpus.py: argument types of %d call sites in %d files; "
                "corpus_param_types derived for %d entries (>= %d typed sites, >= 95%% one type)"
                % (sum(m["sites"] for m in meas.values()), tot["files"], rep["written"],
                   a.min_sites)))
        if game == "t7":
            check_doc(blocks, rep)
        out = "\n".join("\n".join(b) for b in blocks)
        tomllib.loads(out)
        if not a.dry_run:
            open(path, "w", encoding="utf-8", newline="\n").write(out)
        ag = rep["agree"]
        cmp_n = sum(ag.values())
        print("%s: %d sites, %d builtins called (%d unmatched); known param type %d/%d (%.1f%%) -> "
              "%d/%d (%.1f%%); vs handler types: %d compared, %d exact, %d compatible, %d conflict"
              % (game, sum(m["sites"] for m in meas.values()), len(meas), rep["unmatched"],
                 before, n, 100.0 * before / max(n, 1), after, n, 100.0 * after / max(n, 1),
                 cmp_n, ag["exact"], ag["compatible"], ag["conflict"]))
        if "doc" in rep:
            d = rep["doc"]
            print("   vs BO3 doc examples: %d compared, %d exact, %d compatible, %d conflict" % (
                sum(d.values()), d["exact"], d["compatible"], d["conflict"]))
        if not a.dry_run:
            write_report(game, rep, meas, tot, (n, before, after), a.min_sites)


def write_report(game, rep, meas, tot, cov, min_sites):
    n, before, after = cov
    ag = rep["agree"]
    cmp_n = sum(ag.values())
    pct = lambda x, y: "%.1f%%" % (100.0 * x / y) if y else "-"  # noqa: E731
    L = ["# Corpus parameter types: %s" % game, "",
         "Generated by `types_corpus.py` (see its docstring for the method). Counts only, no script text.",
         "",
         "- corpus: %d files, %d builtin call sites, %d builtins called (%d names the catalogue "
         "does not hold)" % (tot["files"], sum(m["sites"] for m in meas.values()), len(meas),
                             rep["unmatched"]),
         "- script functions: %d, parameters %d, typed by the fixpoint %d (%s)" % (
             tot["functions"], tot["params"], tot["params_typed"],
             pct(tot["params_typed"], tot["params"])),
         "- entries with a derived `corpus_param_types` (>= %d typed sites, >= 95%% one type): %d"
         % (min_sites, rep["written"]),
         "- builtins taking parameters with at least one known type: %d/%d (%s) from the "
         "handler measure, %d/%d (%s) with the corpus measure" % (
             before, n, pct(before, n), after, n, pct(after, n)),
         "- against the handler-measured `param_types` (where both give a type): %d compared, "
         "%d exact (%s), %d compatible (%s: the accessor accepts the corpus type, e.g. float "
         "accessor, int arguments), %d conflict (%s)" % (
             cmp_n, ag["exact"], pct(ag["exact"], cmp_n), ag["compatible"],
             pct(ag["compatible"], cmp_n), ag["conflict"], pct(ag["conflict"], cmp_n)), ""]
    if "doc" in rep:
        d = rep["doc"]
        k = sum(d.values())
        L += ["- against the BO3 documentation's EXAMPLE calls (literal argument types of %d "
              "documented examples): %d compared, %d exact (%s), %d compatible, %d conflict (%s)"
              % (rep["doc_entries"], k, d["exact"], pct(d["exact"], k), d["compatible"],
                 d["conflict"], pct(d["conflict"], k)), ""]
    if rep["disagree"]:
        L += ["## Disagreements with the handler-measured types", "",
              "The handler type is the accessor the handler used under emulation (best effort); "
              "the corpus type is what the game's own scripts pass.", ""]
        L += ["- " + x for x in rep["disagree"]] + [""]
    if rep.get("doc_rows"):
        L += ["## Disagreements with the documented examples", ""]
        L += ["- " + x for x in rep["doc_rows"]] + [""]
    os.makedirs(REPORTS, exist_ok=True)
    with open(os.path.join(REPORTS, "types_%s.md" % game), "w", encoding="utf-8",
              newline="\n") as f:
        f.write("\n".join(L).rstrip("\n") + "\n")


if __name__ == "__main__":
    main()
