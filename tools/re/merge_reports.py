"""Community corpus reports (`gruff report`) into draft gruff-data entries:

    py -3.11 -I merge_reports.py <report.json>... [--game G] [--out DIR] [--standalone]
                                 [--min-sites 3] [--min-files 2] [--compare] [--dry-run]

Writes `gruff-data/community/<game>/` (or --out):

- `builtins.toml`: a draft catalogue, `confidence = "corpus"` on every entry. Names are the
  calls no script of any report defines; min/max are the observed argument counts
  (`max_checked = false`: a script never passing more says nothing about the engine's bound);
  `arity_unknown` below --min-sites call sites or --min-files files. Entries the measured
  catalogue (`gruff-data/builtins/<game>.toml`) already holds are left out: a draft never
  overwrites or shadows a measured entry (--standalone ignores the measured catalogue, for the
  validation below).
- `events.toml`: every literal event name with its notify / waittill / endon counts and receiver
  kinds; `script_notified = false` marks the engine candidates (waited, never notified by any
  script of any report).
- `profile.toml`: one proposal per profile feature from the construct counts, next to the
  profile's current value (`agrees`); the profile itself is never written.

Every entry carries its provenance: the ids of the reports that hold it and the number of
independent reports (distinct corpus fingerprints: two players with the same install send equal
fingerprints, which confirms a report but adds no data). Counts of equal-fingerprint reports are
taken once; across fingerprints the maximum is kept (installs overlap: summing would count a
shared script twice). The files are regenerated from the reports each run.

--compare (games with a measured catalogue): precision and recall of the draft names, arity
agreement, profile agreement, to reports/community_<game>.md. That is the quality estimate of a
catalogue built from reports alone.
"""
import argparse
import collections
import json
import os
import re
import sys
import tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from common import paths  # noqa: E402
from common.report import toml_value as enc  # noqa: E402

FORMAT_VERSION = 1
DATA = paths.get("gsc_data")

ENUM_CONSTRUCTS = {
    ("import_directive", "`#using`"): "using",
    ("import_directive", "`#include`"): "include",
    ("header_directive", "`#insert`"): "insert",
    ("header_directive", "`#inline`"): "inline",
    ("function_pointer_style", "a `&` function pointer"): "ampersand",
    ("function_pointer_style", "a `::` function pointer"): "colon",
    ("doc_comment_style", "a `/@ @/` doc comment"): "at-block",
}
# constructs that prove the opposite of the feature (a use means value false)
NEGATIVE = {
    ("function_keyword", "declaration without function"),
    ("imports_before_declarations", "import after a declaration"),
}
# decompiler output, not source: a use proves nothing about the compiler
DECOMPILER_ONLY = {"goto_labels"}
TYPE_LABELS = {"int", "float", "number", "bool", "string", "istring", "hash", "vector", "array",
               "struct", "entity", "function", "anim", "animtree", "weapon", "object",
               "undefined"}


# ---------------------------------------------------------------------------------------------
def load_reports(files, game):
    reps = []
    for f in files:
        with open(f, encoding="utf-8") as fh:
            r = json.load(fh)
        if r.get("format") != "gsc-corpus-report":
            sys.exit("merge_reports: %s is not a gsc corpus report" % f)
        if r.get("format_version") != FORMAT_VERSION:
            sys.exit("merge_reports: %s: format_version %s, this tool reads %d" % (
                f, r.get("format_version"), FORMAT_VERSION))
        reps.append(r)
    games = {r["profile"]["id"] for r in reps}
    if game is None:
        if len(games) != 1:
            sys.exit("merge_reports: reports of several games (%s): pass --game" % ", ".join(sorted(games)))
        game = games.pop()
    reps = [r for r in reps if r["profile"]["id"] == game]
    if not reps:
        sys.exit("merge_reports: no report for %s" % game)
    # one report per report id, then one per corpus fingerprint (the first by id)
    by_id = {}
    for r in reps:
        by_id.setdefault(r["report_id"], r)
    by_fp = collections.defaultdict(list)
    for rid in sorted(by_id):
        by_fp[by_id[rid]["corpus"]["fingerprint"]].append(by_id[rid])
    return game, by_fp


class Agg:
    """One entry over the reports: max of each count across fingerprints, provenance."""

    def __init__(self):
        self.reports = set()
        self.fps = set()
        self.n = collections.Counter()
        self.maps = collections.defaultdict(collections.Counter)
        self.lists = []
        self.flags = set()

    def add(self, fp, rids, counts, maps=None, lists=None, flags=()):
        self.reports.update(rids)
        self.fps.add(fp)
        for k, v in counts.items():
            self.n[k] = max(self.n[k], v)
        for name, m in (maps or {}).items():
            for k, v in m.items():
                self.maps[name][k] = max(self.maps[name][k], v)
        for j, m in enumerate(lists or []):
            while len(self.lists) <= j:
                self.lists.append(collections.Counter())
            for k, v in m.items():
                self.lists[j][k] = max(self.lists[j][k], v)
        self.flags.update(flags)

    def provenance(self):
        return {"reports": sorted(self.reports), "independent_reports": len(self.fps)}


def aggregate(by_fp):
    builtins = collections.defaultdict(Agg)
    events = collections.defaultdict(Agg)
    features = collections.defaultdict(Agg)
    for fp, reps in sorted(by_fp.items()):
        r = reps[0]
        rids = [x["report_id"] for x in reps]
        for b in r["builtins"]:
            builtins[(b["name"], b["vm"], b["kind"])].add(
                fp, rids, {"sites": b["sites"], "files": b["files"], "threaded": b["threaded"]},
                {"argc": b["argc"], "receivers": b["receivers"]}, b["arg_literals"],
                ["keyword"] if b["keyword"] else [])
        for e in r["events"]["names"]:
            events[e["name"]].add(
                fp, rids, {k: e[k] for k in ("emit", "wait", "endon", "files")},
                {"ops": e["ops"], "receivers": e["receivers"], "payload": e["payload"]},
                flags=["pattern"] if e["pattern"] else [])
        for f in r["features"]:
            features[(f["feature"], f["construct"])].add(
                fp, rids, {"uses": f["uses"], "files": f["files"]})
    return builtins, events, features


def header_reports(by_fp):
    out = []
    for fp, reps in sorted(by_fp.items()):
        for r in reps:
            c = r["corpus"]
            out += ["[[report]]",
                    "id = %s" % enc(r["report_id"]),
                    "fingerprint = %s" % enc(fp),
                    "toolchain = %s" % enc(r["toolchain"]["version"]),
                    "profile_xxh3 = %s" % enc(r["profile"]["xxh3"]),
                    "mode = %s" % enc(r["profile"]["mode"]),
                    "files = %d" % c["files"],
                    "decompiled = %d" % c["decompiled"],
                    ""]
    return out


def measured_catalogue(game):
    p = os.path.join(DATA, "builtins", game + ".toml")
    if not os.path.isfile(p):
        return None
    with open(p, "rb") as fh:
        return tomllib.load(fh)


def catalogue_keys(cat):
    keys, names, intr = set(), set(), set()
    if cat:
        for b in cat.get("builtin", []):
            if b.get("shadowed") or "name" not in b:
                continue
            keys.add((b["name"], b["vm"], b["kind"]))
            names.add(b["name"])
        intr = {i["name"] for i in cat.get("intrinsic", [])}
    return keys, names, intr


# ---------------------------------------------------------------------------------------------
def draft_builtins(game, by_fp, builtins, cat, args):
    """The draft catalogue's lines and its entries (for --compare)."""
    keys, _, intr = catalogue_keys(cat)
    # the profile's opcode_builtins: names the compiler turns into an opcode (documentation in
    # the profile, but the best a draft has)
    opcodes = current_profile(game).get("features", {}).get("opcode_builtins", {}).get("value")
    opcodes = set(opcodes) if isinstance(opcodes, list) else set()
    entries, intrinsics, skipped = [], [], 0
    for k in sorted(builtins):
        a = builtins[k]
        name, vm, kind = k
        intrinsic = "keyword" in a.flags or (kind == "function" and name in opcodes)
        if cat is not None and (k in keys or (intrinsic and name in intr)):
            skipped += 1
            continue
        argc = sorted(int(x) for x in a.maps["argc"])
        sites, files = a.n["sites"], a.n["files"]
        known = sites >= args.min_sites and files >= args.min_files
        e = {"name": name, "vm": vm, "kind": kind, "keyword": intrinsic,
             "observed": argc, "sites": sites, "files": files, "known": known,
             "lo": argc[0] if known else -1, "hi": argc[-1] if known else -1,
             "prov": a.provenance(), "lits": a.lists}
        (intrinsics if e["keyword"] else entries).append(e)
    lines = [
        "# DRAFT engine builtins of %s, from community corpus reports (gruff report), merged by" % game,
        "# gruff/tools/re/merge_reports.py. confidence = \"corpus\": names and argument counts seen",
        "# at call sites of the reported scripts, never measured in the executable. Regenerated from",
        "# the reports on every run. Facts only, no script text. See gruff/docs/community-reports.md.",
        "",
        "schema_version = 1",
        "game = %s" % enc(game),
        "license = \"CC0-1.0\"",
        "source = \"community corpus reports\"",
        "draft = true",
        "",
        "[coverage]",
        "complete = false",
        "note = %s" % enc("draft from %d report(s), %d independent: a builtin no reported script calls "
                         "is missing, and a name may be a script function of a file no report holds"
                         % (sum(len(v) for v in by_fp.values()), len(by_fp))),
        "",
        "[merge]",
        "min_sites = %d" % args.min_sites,
        "min_files = %d" % args.min_files,
        "measured_catalogue = %s" % enc("ignored (--standalone)" if cat is None and args.standalone
                                        else "none" if cat is None else
                                        "builtins/%s.toml: %d entries it holds left out" % (game, skipped)),
        "",
    ] + header_reports(by_fp)
    for e in entries:
        lines += ["[[builtin]]",
                  "name = %s" % enc(e["name"]),
                  "name_source = \"corpus-scripts\"",
                  "vm = %s" % enc(e["vm"]),
                  "kind = %s" % enc(e["kind"]),
                  "confidence = \"corpus\"",
                  "min_args = %d" % e["lo"],
                  "max_args = %d" % e["hi"],
                  "max_checked = false"]
        if not e["known"]:
            lines.append("arity_unknown = true")
        lines.append("observed_args = %s" % enc(e["observed"]))
        types = []
        for m in e["lits"][:16]:
            t = collections.Counter()
            for k2, v in m.items():
                t[k2 if k2 in TYPE_LABELS else "unknown"] += v
            types.append(dict(sorted(t.items(), key=lambda kv: (-kv[1], kv[0]))))
        if types:
            lines.append("corpus_arg_types = %s" % enc(types))
        lines += ["sites = %d" % e["sites"],
                  "files = %d" % e["files"],
                  "reports = %s" % enc(e["prov"]["reports"]),
                  "independent_reports = %d" % e["prov"]["independent_reports"],
                  "evidence = %s" % enc("called at %d sites in %d files, defined by no reported script; "
                                        "arity %s" % (e["sites"], e["files"],
                                                      "= the observed range" if e["known"] else
                                                      "unknown (under %d sites or %d files)" % (
                                                          args.min_sites, args.min_files))),
                  ""]
    for e in intrinsics:
        lines += ["[[intrinsic]]",
                  "name = %s" % enc(e["name"]),
                  "vm = %s" % enc(e["vm"]),
                  "confidence = \"corpus\"",
                  "corpus_args = %s" % enc(e["observed"]),
                  "reports = %s" % enc(e["prov"]["reports"]),
                  "independent_reports = %d" % e["prov"]["independent_reports"],
                  "evidence = %s" % enc("%s, called at %d sites in %d files" % (
                      "in the profile's opcode_builtins" if e["name"] in opcodes
                      else "a keyword of the profile", e["sites"], e["files"])),
                  ""]
    return lines, entries, intrinsics


def draft_events(game, by_fp, events):
    lines = [
        "# DRAFT event names of %s, from community corpus reports (gruff report), merged by" % game,
        "# gruff/tools/re/merge_reports.py. Literal names of notify / waittill / endon sites of the",
        "# reported scripts; script_notified = false: waited or endon'd but notified by no reported",
        "# script (an engine event candidate). Facts only. See gruff/docs/community-reports.md.",
        "",
        "schema_version = 1",
        "game = %s" % enc(game),
        "license = \"CC0-1.0\"",
        "source = \"community corpus reports\"",
        "draft = true",
        "",
    ] + header_reports(by_fp)
    for name in sorted(events):
        a = events[name]
        lines += ["[[event]]",
                  "name = %s" % enc(name),
                  "pattern = %s" % enc("pattern" in a.flags),
                  "notify = %d" % a.n["emit"],
                  "waittill = %d" % a.n["wait"],
                  "endon = %d" % a.n["endon"],
                  "files = %d" % a.n["files"],
                  "script_notified = %s" % enc(a.n["emit"] > 0),
                  "ops = %s" % enc(dict(sorted(a.maps["ops"].items()))),
                  "receivers = %s" % enc(dict(sorted(a.maps["receivers"].items())))]
        if a.maps["payload"]:
            lines.append("notify_payload = %s" % enc(dict(sorted(a.maps["payload"].items()))))
        prov = a.provenance()
        lines += ["reports = %s" % enc(prov["reports"]),
                  "independent_reports = %d" % prov["independent_reports"], ""]
    return lines


def feature_registry():
    with open(os.path.join(DATA, "schema", "profile.schema.json"), encoding="utf-8") as fh:
        s = json.load(fh)
    out = {}
    for k, v in s["properties"]["features"]["properties"].items():
        val = json.dumps(v["properties"]["value"])
        out[k] = "list" if '"array"' in val else "bool" if '"boolean"' in val else "enum"
    return out


def proposals(game, features):
    """Per feature: (proposed value or None, uses, files, note, provenance)."""
    reg = feature_registry()
    by_feat = collections.defaultdict(list)
    for (f, c), a in features.items():
        by_feat[f].append((c, a))
    cur_import = current_profile(game).get("features", {}).get("import_directive", {}).get("value")
    include_game = cur_import == "include" or any(
        f == "import_directive" and c == "`#include`" for f, c in features)
    out = {}
    for f, kind in reg.items():
        rows = by_feat.get(f, [])
        uses = sum(a.n["uses"] for _, a in rows)
        files = max([a.n["files"] for _, a in rows] or [0])
        reps, fps = set(), set()
        for _, a in rows:
            reps |= a.reports
            fps |= a.fps
        prov = {"reports": sorted(reps), "independent_reports": len(fps)}
        value, note = None, ""
        if f in DECOMPILER_ONLY:
            note = "decompiler output, not evidence of the compiler"
        elif f == "namespace_qualified_calls" and include_game:
            note = ("%d one-word qualifiers: in an #include game the probe cannot tell a "
                    "namespace from a one-component path" % uses)
        elif not rows:
            note = "0 uses (absence of use, not proof of absence)"
        elif kind == "bool":
            neg = [a for c, a in rows if (f, c) in NEGATIVE]
            value = False if neg else True
        elif kind == "enum":
            seen = collections.Counter()
            for c, a in rows:
                v = ENUM_CONSTRUCTS.get((f, c))
                if v:
                    seen[v] += a.n["uses"]
            if len(seen) == 1:
                value = next(iter(seen))
            elif seen:
                note = "several values used: " + ", ".join("%s %d" % kv for kv in sorted(seen.items()))
        else:
            items = []
            for c, _ in rows:
                m = re.fullmatch(r"a `(.)\"\.\.\.\"` string|`([a-z_]+)`", c)
                if m:
                    items.append(m.group(1) or m.group(2))
            if items:
                value = sorted(set(items))
                note = "the values used; others may exist"
        out[f] = (value, uses, files, note, prov, [(c, a.n["uses"], a.n["files"]) for c, a in sorted(rows, key=lambda r: r[0])])
    return out


def current_profile(game):
    p = os.path.join(DATA, "profiles", game + ".toml")
    with open(p, "rb") as fh:
        return tomllib.load(fh)


def agree(value, cur):
    """A proposed list agrees when the profile holds every value used."""
    if isinstance(value, list) and isinstance(cur, list):
        return set(value) <= set(cur)
    return cur == value


def draft_profile(game, by_fp, props):
    prof = current_profile(game)
    feats = prof.get("features", {})
    lines = [
        "# DRAFT profile feature proposals for %s, from community corpus reports (gruff report)," % game,
        "# merged by gruff/tools/re/merge_reports.py. A proposal is what the construct counts",
        "# support (a use proves the feature; 0 uses proves nothing); `current` is the profile's",
        "# value. The profile is never written: a reviewer copies what holds.",
        "",
        "schema_version = 1",
        "game = %s" % enc(game),
        "license = \"CC0-1.0\"",
        "draft = true",
        "",
    ] + header_reports(by_fp)
    stats = collections.Counter()
    for f in sorted(props):
        value, uses, files, note, prov, rows = props[f]
        cur = feats.get(f, {}).get("value", "unknown")
        lines.append("[feature.%s]" % f)
        lines.append("proposed = %s" % enc(value if value is not None else "unknown"))
        lines.append("current = %s" % enc(cur))
        lines.append("current_confidence = %s" % enc(feats.get(f, {}).get("confidence", "none")))
        if value is not None:
            agrees = agree(value, cur)
            lines.append("agrees = %s" % enc(agrees))
            stats["agree" if agrees else ("new" if cur == "unknown" else "disagree")] += 1
        else:
            stats["no proposal"] += 1
        lines.append("uses = %d" % uses)
        lines.append("files = %d" % files)
        if rows:
            lines.append("constructs = %s" % enc({c: u for c, u, _ in rows}))
        if note:
            lines.append("note = %s" % enc(note))
        if prov["reports"]:
            lines.append("reports = %s" % enc(prov["reports"]))
            lines.append("independent_reports = %d" % prov["independent_reports"])
        lines.append("")
    return lines, stats


# ---------------------------------------------------------------------------------------------
def arity_agreement(entries, measured, extraction):
    """Draft arities against the catalogue's (calibrated: min/max) or the extraction's own
    (measured_args where calibrate.py changed them; the calibrated arity read this corpus)."""
    ar = collections.Counter()
    contra = []
    for e in entries:
        b = measured.get((e["name"], e["vm"], e["kind"]))
        if b is None:
            continue
        lo, hi = b["min_args"], b["max_args"]
        unknown = b.get("arity_unknown") or b.get("arity_unchecked")
        if extraction and "measured_args" in b:
            lo, hi = b["measured_args"]
            unknown = b.get("arity_unchecked")
        if unknown or lo < 0:
            ar["measured unknown"] += 1
            continue
        if not e["known"]:
            ar["draft unknown"] += 1
            continue
        ar["compared"] += 1
        if e["lo"] == lo and e["hi"] == hi:
            ar["exact"] += 1
        if e["lo"] == lo:
            ar["min equal"] += 1
        if lo <= e["lo"] and (e["hi"] <= hi or b.get("max_checked") is False):
            ar["consistent"] += 1
        else:
            contra.append((e["name"], e["vm"], e["kind"], e["observed"], lo, hi))
        if e["lo"] > lo:
            ar["min too high (optional args never omitted)"] += 1
        if e["hi"] < hi and hi < 1000:
            ar["max below measured"] += 1
    return ar, contra


def compare(game, cat, entries, intrinsics, prof_stats, props, args):
    """Precision / recall of the draft names and arity agreement against the measured catalogue."""
    keys, names, intr = catalogue_keys(cat)
    called = set()
    for b in cat.get("builtin", []):
        if "name" in b and not b.get("shadowed") and b.get("corpus_arg_types") is not None:
            called.add((b["name"], b["vm"], b["kind"]))
    measured = {}
    for b in cat.get("builtin", []):
        if "name" in b and not b.get("shadowed"):
            measured.setdefault((b["name"], b["vm"], b["kind"]), b)
    known_all = keys | {(i, vm, "function") for i in intr for vm in ("gsc", "csc")}

    def pr(sel):
        d = {(e["name"], e["vm"], e["kind"]) for e in sel}
        tp = d & keys
        tp_any = {k for k in d if k in keys or k[0] in names or k[0] in intr}
        return {"draft": len(d), "tp": len(tp), "tp_name": len(tp_any),
                "precision": len(tp) / len(d) if d else 0.0,
                "precision_name": len(tp_any) / len(d) if d else 0.0,
                "recall": len(tp) / len(keys) if keys else 0.0,
                "recall_called": len(tp & called) / len(called) if called else 0.0}

    rows = []
    for mf in (1, 2, 3, 5):
        sel = [e for e in entries if e["files"] >= mf]
        rows.append((mf, pr(sel)))
    ar, contra = arity_agreement(entries, measured, extraction=False)
    ar_x, contra_x = arity_agreement(entries, measured, extraction=True)
    fp = sorted((e for e in entries if (e["name"], e["vm"], e["kind"]) not in keys and e["files"] >= args.min_files),
                key=lambda e: (-e["files"], -e["sites"], e["name"]))
    fn = sorted(called - {(e["name"], e["vm"], e["kind"]) for e in entries})
    out = ["# Community-report draft vs the measured catalogue: %s" % game, "",
           "Generated by `tools/re/merge_reports.py --compare --standalone`: a draft catalogue built from",
           "the game's own corpus report alone (`gruff report`, no catalogue read), compared with",
           "`gruff-data/builtins/%s.toml` (RE-extracted, corpus-calibrated). It estimates the quality of a" % game,
           "draft for a game whose catalogue can only come from community reports.", "",
           "Keys are (name, vm, kind). `precision (name)` also counts a draft key whose name the catalogue",
           "holds under another vm or kind, or as an intrinsic. `recall (called)` is against the measured",
           "entries the corpus calls (those with corpus_arg_types): a builtin no script calls cannot be",
           "found from scripts.", "",
           "| min files | draft keys | correct | precision | precision (name) | recall (all %d) | recall (called %d) |" % (len(keys), len(called)),
           "|---:|---:|---:|---:|---:|---:|---:|"]
    for mf, r in rows:
        out.append("| %d | %d | %d | %.1f%% | %.1f%% | %.1f%% | %.1f%% |" % (
            mf, r["draft"], r["tp"], 100 * r["precision"], 100 * r["precision_name"],
            100 * r["recall"], 100 * r["recall_called"]))
    out += ["", "## Arity (draft entries with a known arity: >= %d sites in >= %d files)" % (
        args.min_sites, args.min_files), "",
        "Against the catalogue (`min_args`/`max_args`, calibrated on this same corpus) and against the",
        "extraction alone (`measured_args` where calibrate.py changed an entry): the second is the",
        "fair estimate for a game whose catalogue has no executable measure behind it.", "",
        "| measure | vs catalogue | share | vs extraction | share |", "|---|---:|---:|---:|---:|"]
    n, nx = ar["compared"] or 1, ar_x["compared"] or 1
    for k in ("compared", "exact", "min equal", "consistent", "min too high (optional args never omitted)",
              "max below measured"):
        out.append("| %s | %d | %.1f%% | %d | %.1f%% |" % (k, ar[k], 100 * ar[k] / n, ar_x[k], 100 * ar_x[k] / nx))
    out += ["| draft unknown (too few sites) | %d | | %d | |" % (ar["draft unknown"], ar_x["draft unknown"]),
            "| measured unknown | %d | | %d | |" % (ar["measured unknown"], ar_x["measured unknown"]), "",
            "`consistent`: the observed counts fit the measured range (or exceed a max the handler does not",
            "check). The draft's max is the largest count seen, a lower bound of the engine's.", ""]
    for title, cs in (("catalogue", contra), ("extraction", contra_x)):
        if cs:
            out += ["Contradictions with the %s (observed counts outside the measured range): %d" % (title, len(cs)), ""]
            for name, vm, kind, obs, lo, hi in cs[:25]:
                out.append("- `%s` (%s %s): observed %s, measured %d..%d" % (name, vm, kind, obs, lo, hi))
            out.append("")
    ints = {e["name"] for e in intrinsics}
    out += ["## Intrinsics", "",
            "Draft intrinsics (keyword calls, and the profile's opcode_builtins called as functions): %d;"
            " measured [[intrinsic]]: %d; in both: %d. Draft only: %s." % (
                len(ints), len(intr), len(ints & intr), ", ".join(sorted(ints - intr)) or "none"), "",
            "## Draft names the catalogue lacks (files >= %d, top 30 by files)" % args.min_files, "",
            "Most are script functions of files the corpus lacks, other-VM calls, or decompiler names.", ""]
    for e in fp[:30]:
        other = " (catalogue: same name, other vm/kind)" if e["name"] in names else ""
        out.append("- `%s` %s %s: %d sites, %d files%s" % (e["name"], e["vm"], e["kind"], e["sites"], e["files"], other))
    out += ["", "## Called catalogue entries the draft misses: %d" % len(fn), ""]
    for k in fn[:30]:
        out.append("- `%s` %s %s" % k)
    out += ["", "## Profile proposals vs the profile", "",
            "| result | features |", "|---|---:|"]
    for k in ("agree", "disagree", "new", "no proposal"):
        out.append("| %s | %d |" % (k, prof_stats[k]))
    dis = []
    prof = current_profile(game).get("features", {})
    for f, (value, uses, files, note, prov, rows_) in sorted(props.items()):
        cur = prof.get(f, {}).get("value", "unknown")
        if value is None:
            continue
        if not agree(value, cur):
            dis.append("- `%s`: proposed %s, profile %s (%d uses in %d files)" % (f, enc(value), enc(cur), uses, files))
    if dis:
        out += [""] + dis
    out.append("")
    return out, rows, ar, ar_x


def write(path, lines, dry):
    text = "\n".join(lines).rstrip("\n") + "\n"
    if path.endswith(".toml"):
        tomllib.loads(text)  # the writer's own check: a draft must parse
    if dry:
        print("would write %s (%d lines)" % (path, len(lines)))
        return
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(text)
    print("wrote %s" % path)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("reports", nargs="+")
    ap.add_argument("--game")
    ap.add_argument("--out", help="output folder (default gruff-data/community/<game>)")
    ap.add_argument("--standalone", action="store_true",
                    help="ignore the measured catalogue (validation: the draft from reports alone)")
    ap.add_argument("--min-sites", type=int, default=3)
    ap.add_argument("--min-files", type=int, default=2)
    ap.add_argument("--compare", action="store_true",
                    help="compare with gruff-data/builtins/<game>.toml, to reports/community_<game>.md")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()
    game, by_fp = load_reports(args.reports, args.game)
    builtins, events, features = aggregate(by_fp)
    measured = measured_catalogue(game)
    cat = None if args.standalone else measured
    out = args.out or os.path.join(DATA, "community", game)
    blines, entries, intrinsics = draft_builtins(game, by_fp, builtins, cat, args)
    write(os.path.join(out, "builtins.toml"), blines, args.dry_run)
    write(os.path.join(out, "events.toml"), draft_events(game, by_fp, events), args.dry_run)
    props = proposals(game, features)
    plines, pstats = draft_profile(game, by_fp, props)
    write(os.path.join(out, "profile.toml"), plines, args.dry_run)
    print("%s: %d report(s), %d independent; %d builtin entries (%d with an arity), %d intrinsics, "
          "%d events, profile proposals %s" % (
              game, sum(len(v) for v in by_fp.values()), len(by_fp), len(entries),
              sum(1 for e in entries if e["known"]), len(intrinsics), len(events), dict(pstats)))
    if args.compare:
        if measured is None:
            sys.exit("merge_reports: --compare needs gruff-data/builtins/%s.toml" % game)
        if cat is not None:
            sys.exit("merge_reports: --compare needs --standalone (the draft from the reports alone)")
        lines, rows, ar, ar_x = compare(game, measured, entries, intrinsics, pstats, props, args)
        write(os.path.join(HERE, "reports", "community_%s.md" % game), lines, args.dry_run)
        for mf, r in rows:
            print("  files>=%d: %d keys, precision %.1f%% (name %.1f%%), recall %.1f%% (called %.1f%%)" % (
                mf, r["draft"], 100 * r["precision"], 100 * r["precision_name"], 100 * r["recall"],
                100 * r["recall_called"]))
        for label, a in (("catalogue", ar), ("extraction", ar_x)):
            n = a["compared"] or 1
            print("  arity vs %s: %d compared, exact %.1f%%, min equal %.1f%%, consistent %.1f%%" % (
                label, a["compared"], 100 * a["exact"] / n, 100 * a["min equal"] / n,
                100 * a["consistent"] / n))


if __name__ == "__main__":
    main()
