"""From t7_fields.py's extraction (out/t7_fields_extract.json): resolve names and type labels, check
against the official field list and the mod-tools scripts, simulate the "likely typo of an engine
field" lint, write gruff-data/fields/t7.toml and reports/t7_fields.md.

    py -3.11 -I treyarch/build_t7_fields.py [--extract out/t7_fields_extract.json] [--data ../../../gruff-data]

Names: a canon hash is named only by a string whose t7.canon hash equals it, taken from (in
order) the official field list (ScriptObjectFields.xlsx, read locally), the identifiers of the
mod-tools scripts, the BO3 hash list. Only names and counts are used from the field list; its
type column is used only to label the executable's numeric type codes by majority, and the
agreement is reported. No other text of it is written anywhere.
"""
import argparse
import collections
import csv
import json
import os
import random
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from common.hashes import t7_canon  # noqa: E402
from common import corpus  # noqa: E402
from common.report import toml_str, pct  # noqa: E402
import officialfields  # noqa: E402
from common import paths  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
RE = os.path.dirname(HERE)
HASH_CSV = paths.get("bo3_hash_csv")  # tools/re/paths.local.json or GSC_RE_BO3_HASH_CSV
CORPUS = os.path.join(paths.get("corpus"), "t7", "source-modtools")

# owner kind -> sheet of the official list
SHEET = {"entity": "Ent", "sentient": "Sentient", "actor": "Actor", "player": "Client",
         "vehicle": "Vehicle", "hudelem": "HudElem", "pathnode": "PathNode", "weapon": "Weapons",
         "centity": "CG Cent", "dynent": "CG DynEnt", "vehiclenode": None}
KIND_ORDER = ["entity", "sentient", "actor", "player", "vehicle", "hudelem", "pathnode",
              "vehiclenode", "weapon", "centity", "dynent"]
OWNER_NOTE = {
    "entity": "every game entity (class 0, offsets 0x0000+)",
    "sentient": "entities with a sentient (class 0, offsets 0x2000+)",
    "actor": "AI actors (class 0, offsets 0x4000+ and 0xa000+)",
    "player": "player entities, gclient fields (class 0, offsets 0x6000+)",
    "vehicle": "vehicle entities (class 0, offsets 0x8000+)",
    "hudelem": "hud elements (class 1)",
    "pathnode": "path nodes (class 2)",
    "vehiclenode": "vehicle path nodes (class 3)",
    "weapon": "weapon objects (class 5, same table in both VMs)",
    "centity": "client entities (CSC class 0; four groups at offsets 0x0000, 0x2000, 0x4000, 0x8000)",
    "dynent": "client dynamic entities (CSC class 4)",
}


# ---------------------------------------------------------------------------------------------
# corpus: field accesses and assignments
FIELD = re.compile(r"(?<=[A-Za-z0-9_\])])\s*\.\s*([A-Za-z_][A-Za-z0-9_]*)")
ASSIGN_TAIL = re.compile(r"\s*(\[[^\]\n]*\]\s*)*(=(?!=)|\+=|-=|\*=|/=|%=|\|=|&=|\^=|\+\+|--)")
VAR = re.compile(r"(?m)^\s*var\s+([A-Za-z_][A-Za-z0-9_]*)\s*;")
OWNER = re.compile(r"([A-Za-z_][A-Za-z0-9_]*)\s*$")


def scan_fields(root=CORPUS):
    """Per lower-cased field name: access sites, assignment sites, `self.` sites; one sample
    location per name (file, line) for the hand classification. Stores counts only."""
    acc, asg, selfacc = collections.Counter(), collections.Counter(), collections.Counter()
    where = {}
    files = 0
    for dp, dn, fn in os.walk(root):
        for f in fn:
            if f.rsplit(".", 1)[-1].lower() not in ("gsc", "csc", "gsh"):
                continue
            files += 1
            p = os.path.join(dp, f)
            with open(p, "rb") as fh:
                src = corpus.blank(fh.read().decode("latin1"))
            for m in VAR.finditer(src):  # a class member declaration defines the name too
                asg[m.group(1).lower()] += 1
            for m in FIELD.finditer(src):
                n = m.group(1).lower()
                before = src[max(0, m.start() - 40):m.start()]
                # a number like 1.5 is not a field access
                ow = OWNER.search(before)
                if ow and ow.group(1)[0].isdigit():
                    continue
                acc[n] += 1
                if ow and ow.group(1).lower() == "self":
                    selfacc[n] += 1
                if ASSIGN_TAIL.match(src, m.end()):
                    asg[n] += 1
                if n not in where:
                    where[n] = (os.path.relpath(p, root), src.count("\n", 0, m.start()) + 1)
    return acc, asg, selfacc, where, files


def lev(a, b, cap=3):
    if abs(len(a) - len(b)) >= cap:
        return cap
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        cur = [i] + [0] * len(b)
        best = i
        for j, cb in enumerate(b, 1):
            cur[j] = min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (ca != cb))
            best = min(best, cur[j])
        if best >= cap:
            return cap
        prev = cur
    return min(prev[-1], cap)


# ---------------------------------------------------------------------------------------------
def load_names(hashes, sheets, corpus_names):
    names, src = {}, {}
    for sheet, fields in sheets.items():
        for n in fields:
            h = t7_canon(n)
            if h in hashes and h not in names:
                names[h], src[h] = n, "official-doc"
    for n in sorted(corpus_names):
        h = t7_canon(n)
        if h in hashes and h not in names:
            names[h], src[h] = n, "mod-tools-scripts"
    if os.path.exists(HASH_CSV):
        with open(HASH_CSV, encoding="utf-8", errors="replace") as f:
            for row in csv.reader(f):
                if len(row) < 2 or row[0] == "hash":
                    continue
                try:
                    h = int(row[0], 16)
                except ValueError:
                    continue
                if h in hashes and h not in names and re.match(r"^[a-z_][a-z0-9_]*$", row[1].lower()) \
                        and t7_canon(row[1]) == h:
                    names[h], src[h] = row[1].lower(), "bo3-hash-list"
    return names, src


def build(extract_path, data_dir, report_path):
    with open(extract_path, encoding="utf-8") as f:
        ex = json.load(f)
    sheets = officialfields.field_names()
    acc, asg, selfacc, where, nfiles = scan_fields()

    # kinds: from the read-only messages; a group with none takes the sheet its names match best
    hashes = {e["canon"] for t in ex["tables"] for e in t["entries"]}
    names, nsrc = load_names(hashes, sheets, set(acc) | set(asg))
    for t in ex["tables"]:
        t["kind"] = t["kind_from_messages"]
        t["kind_evidence"] = "read-only message: %s" % "; ".join(t["readonly_messages"]) \
            if t["kind"] else None
        if not t["kind"]:
            best = max(SHEET.items(), key=lambda kv: sum(
                1 for e in t["entries"] if kv[1] and names.get(e["canon"]) in sheets[kv[1]]))
            n = sum(1 for e in t["entries"] if names.get(e["canon"]) in sheets[best[1]])
            t["kind"] = best[0]
            t["kind_evidence"] = "no read-only message; %d/%d names in the official %s sheet" % (
                n, t["count"], best[1])
    # fields: weapon is one table shared by both VMs
    F = {}
    for t in ex["tables"]:
        vm = "gsc" if t["inst"] == 0 else "csc"
        for e in t["entries"]:
            key = (t["kind"], e["canon"], e["offset"], e["entry_rva"])
            if key in F:
                F[key]["vm"].append(vm)
                continue
            F[key] = dict(e, kind=t["kind"], vm=[vm], name=names.get(e["canon"]),
                          table_rva=t["rva"], stride=t["stride"])
    fields = sorted(F.values(), key=lambda e: (KIND_ORDER.index(e["kind"]), e["offset"]))

    # type labels: majority of the official type over the matched names, per code
    votes = collections.defaultdict(collections.Counter)
    for e in fields:
        sh = SHEET.get(e["kind"])
        if sh and e["name"] in sheets.get(sh, {}):
            ty = sheets[sh][e["name"]]
            if ty and ty.startswith("F_"):
                votes[e["type"]][ty] += 1
    type_names = {}
    for code, c in sorted(votes.items()):
        lab, n = c.most_common(1)[0]
        type_names[code] = {"name": lab, "agree": n, "of": sum(c.values())}

    # official list agreement
    off = {}
    for kind, sh in SHEET.items():
        if not sh:
            continue
        ours = {e["name"] for e in fields if e["kind"] == kind and e["name"]}
        anyk = {e["name"] for e in fields if e["name"]}
        doc = set(sheets[sh])
        off[kind] = {"sheet": sh, "doc": len(doc), "found_same_kind": len(doc & ours),
                     "found_any_kind": len(doc & anyk), "missing": sorted(doc - anyk),
                     "elsewhere": sorted((doc & anyk) - ours),
                     "extracted": sum(1 for e in fields if e["kind"] == kind),
                     "extra": sum(1 for e in fields if e["kind"] == kind and e["name"] not in doc)}
    # types vs doc per field
    t_agree = t_tot = 0
    for e in fields:
        sh = SHEET.get(e["kind"])
        if sh and e["name"] in sheets.get(sh, {}) and (sheets[sh][e["name"]] or "").startswith("F_"):
            t_tot += 1
            t_agree += type_names.get(e["type"], {}).get("name") == sheets[sh][e["name"]]

    engine = {e["name"] for e in fields if e["name"]}
    stats = corpus_stats(acc, asg, selfacc, engine)
    typo = typo_rule(acc, asg, engine, where)
    write_toml(ex, fields, nsrc, type_names, os.path.join(data_dir, "fields", "t7.toml"))
    write_report(ex, fields, nsrc, type_names, off, (t_agree, t_tot), stats, typo, nfiles,
                 report_path)


def corpus_stats(acc, asg, selfacc, engine):
    def split(counter):
        names = set(counter)
        eng = names & engine
        scr = (names - engine) & set(asg)
        nei = names - engine - set(asg)
        return {"names": len(names), "engine": len(eng), "script": len(scr), "neither": len(nei),
                "sites": sum(counter.values()), "engine_sites": sum(counter[n] for n in eng),
                "script_sites": sum(counter[n] for n in scr),
                "neither_sites": sum(counter[n] for n in nei)}
    return {"all": split(acc), "self": split(selfacc),
            "engine_assigned": len(engine & set(asg)), "engine_total": len(engine)}


def typo_rule(acc, asg, engine, where):
    """Flag an accessed name that is not an engine field and never assigned anywhere in the
    workspace but within edit distance 1-2 of an engine field."""
    eng = sorted(engine)
    flagged = []
    for n in sorted(acc):
        if n in engine or n in asg:
            continue
        best = min(((lev(n, e), e) for e in eng), default=(3, None))
        if 1 <= best[0] <= 2:
            near = sorted(e for e in eng if lev(n, e) == best[0])
            flagged.append({"name": n, "sites": acc[n], "distance": best[0], "near": near,
                            "where": where.get(n)})
    rnd = random.Random(7)
    sample = rnd.sample(flagged, min(20, len(flagged)))
    sample.sort(key=lambda x: x["name"])
    cand = [n for n in acc if n not in engine and n not in asg]
    return {"flagged": flagged, "sample": sample, "candidates": len(cand),
            "flagged_sites": sum(x["sites"] for x in flagged)}


# ---------------------------------------------------------------------------------------------
def write_toml(ex, fields, nsrc, type_names, path):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    cnt = collections.Counter(e["kind"] for e in fields)
    L = [
        "# Black Ops III (T7) script-visible engine fields of every object kind, extracted from the",
        "# decrypted game executable by gruff/tools/re/t7_fields.py: the static initialisers",
        "# and every Scr_AddClassField registration run under Unicorn, read-only measured by",
        "# running each field's setter and the VM's object-field setter dispatch.",
        "# Schema: ../schema/fields.schema.json. Facts only; no documentation text.",
        "# GSC objects also take arbitrary script-defined fields: this list is the engine's only.",
        "",
        "schema_version = 1",
        'game = "t7"',
        'license = "CC0-1.0"',
        'source = "extracted from game executable"',
        'sources = ["t7-exe-extraction", "bo3-script-object-fields", "mod-tools-scripts", "bo3-hash-list"]',
        'hash_family = "t7.canon"',
        "",
        "[extraction]",
        'tool = "gruff/tools/re/t7_fields.py + build_t7_fields.py"',
        'executable = "BlackOps3.exe, Cordycep dump flattened (bo3_flat.exe)"',
        "pe_timestamp = %d" % ex["pe_timestamp"],
        'add_class_field_rva = "%s"' % ex["anchors"]["add_class_field"],
        'entry_layout = "u32 canon, u32 member offset, u32 size, u32 type, then setter and getter '
        'pointers (0x20 and 0x28 strides) or a writable byte (0x14 stride)"',
        "counts = { %s }" % ", ".join("%s = %d" % (k, cnt[k]) for k in KIND_ORDER if cnt[k]),
        "",
    ]
    seen = set()
    for t in ex["tables"]:
        if t["rva"] in seen:
            continue
        seen.add(t["rva"])
        vms = sorted({"gsc" if u["inst"] == 0 else "csc" for u in ex["tables"] if u["rva"] == t["rva"]},
                     key=["gsc", "csc"].index)
        L.append("[[extraction.table]]")
        L.append('kind = "%s"' % t["kind"])
        L.append("vm = [%s]" % ", ".join('"%s"' % v for v in vms))
        L.append("class = %d" % t["cls"])
        L.append('offset_base = "0x%x"' % (t["group"] << 13))
        L.append('rva = "0x%x"' % t["rva"])
        L.append('stride = "0x%x"' % t["stride"])
        L.append("count = %d" % t["count"])
        # the data file paraphrases: no game text in CC0 data
        ke = ("the read-only error its setters raise names the %s kind" % t["kind"]
              if t["kind_from_messages"] else t["kind_evidence"])
        L.append("kind_evidence = %s" % toml_str(ke))
        L.append("")
    L.append("[types]")
    L.append('# numeric type code of the entry -> label; label = majority of the official list\'s type')
    L.append("# over the fields whose name it lists (agreement given)")
    for code, v in sorted(type_names.items()):
        L.append('%d = { label = "%s", agree = %d, of = %d }' % (code, v["name"], v["agree"], v["of"]))
    L.append("")
    for e in fields:
        L.append("[[field]]")
        if e["name"]:
            L.append("name = %s" % toml_str(e["name"]))
            L.append('name_source = "%s"' % nsrc[e["canon"]])
        L.append('hash = "0x%08x"' % e["canon"])
        L.append('kind = "%s"' % e["kind"])
        L.append("vm = [%s]" % ", ".join('"%s"' % v for v in e["vm"]))
        L.append('offset = "0x%x"' % e["offset"])
        L.append("type = %d" % e["type"])
        if e["read_only"] is not None:
            L.append("read_only = %s" % ("true" if e["read_only"] else "false"))
        L.append("evidence = %s" % toml_str("registered (inst %s, class %d) by 0x%s; table 0x%x entry %d; read-only: %s" % (
            "/".join("0" if v == "gsc" else "1" for v in e["vm"]), e["cls"],
            "/0x".join("%x" % r for r in e["registered_by"][:1]), e["table_rva"], e["index"],
            e["read_only_evidence"])))
        L.append("")
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(L))


def write_report(ex, fields, nsrc, type_names, off, tagr, st, typo, nfiles, path):
    L = []
    w = L.append
    cnt = collections.Counter(e["kind"] for e in fields)
    w("# T7 (Black Ops III) engine fields: extraction and validation")
    w("")
    w("Generated by `build_t7_fields.py` from `t7_fields.py extract`. Executable: Cordycep dump of "
      "`BlackOps3.exe`, flattened (`bo3_flat.exe`), PE timestamp %d, image base 0x%x. "
      "Data: `gruff-data/fields/t7.toml`." % (ex["pe_timestamp"], ex["image_base"]))
    w("")
    w("## Extraction log")
    w("")
    w("```")
    L.extend(ex["log"])
    w("```")
    w("")
    w("## Counts per owner kind")
    w("")
    w("| kind | owner | VM | fields | named | read-only | writable | read-only unknown |")
    w("|---|---|---|---|---|---|---|---|")
    for k in KIND_ORDER:
        es = [e for e in fields if e["kind"] == k]
        if not es:
            continue
        vms = sorted({v for e in es for v in e["vm"]}, key=["gsc", "csc"].index)
        w("| %s | %s | %s | %d | %d | %d | %d | %d |" % (
            k, OWNER_NOTE[k], "+".join(vms), len(es), sum(1 for e in es if e["name"]),
            sum(1 for e in es if e["read_only"] is True), sum(1 for e in es if e["read_only"] is False),
            sum(1 for e in es if e["read_only"] is None)))
    tot = len(fields)
    w("")
    src = collections.Counter(nsrc[e["canon"]] for e in fields if e["name"])
    w("%d fields (the weapon table counted once though both VMs register it). Named: %d (%s); "
      "unnamed (hash only): %d." % (tot, sum(1 for e in fields if e["name"]),
                                    ", ".join("%s %d" % kv for kv in src.most_common()),
                                    sum(1 for e in fields if not e["name"])))
    w("")
    w("No class is registered for `level`, `game` or `anim`: they are plain script objects, "
      "with no engine fields. The VM has six field classes (the CSC registration initialises "
      "classes 0 to 5): 0 entity, 1 hud element (GSC only), 2 path node, 3 vehicle node, "
      "4 dynamic entity (CSC only), 5 weapon.")
    w("")
    w("### How each kind was named")
    w("")
    w("| VM | class | offsets | table | kind | evidence |")
    w("|---|---|---|---|---|---|")
    for t in ex["tables"]:
        w("| %s | %d | 0x%x+ | 0x%x | %s | %s |" % ("gsc" if t["inst"] == 0 else "csc", t["cls"],
                                                 t["group"] << 13, t["rva"], t["kind"],
                                                 t["kind_evidence"]))
    w("")
    w("## Independent measures")
    w("")
    w("- Registration: each field is one Scr_AddClassField call made by the engine's own "
      "registration code under emulation (class, canon, offset as the VM stores them).")
    w("- Tables: per (VM, class, offset group), entries at one constant stride, offset low bits "
      "= entry index, and the entry after the last has canon 0 (every table above: see the log).")
    rs = collections.Counter(e["read_only_evidence"].split(";")[0] for e in fields)
    w("- Read-only: by the field's own setter run alone, else by the VM's object-field setter "
      "dispatch (Scr_SetObjectField, entity 0): %s." % ", ".join("%s %d" % kv for kv in rs.most_common()))
    agree = sum(1 for e in fields if "agrees" in e["read_only_evidence"])
    dis = [e for e in fields if "DISAGREES" in e["read_only_evidence"]]
    w("- Both measures decided and agree on %d fields; disagree on %d. The dispatch reaches the "
      "sub-kind setters (sentient, actor, player, vehicle) only on a live entity, so on entity 0 it "
      "stops first; the CSC dispatch reads the value before it looks the field up." % (agree, len(dis)))
    w("- Vehicle nodes carry a writable byte in the entry: it agrees with the dispatch on %d/%d." % (
        sum(1 for e in fields if "writable flag agrees" in e["read_only_evidence"]),
        sum(1 for e in fields if "writable_flag" in e)))
    w("")
    w("## Official field list (ScriptObjectFields.xlsx, read locally, names only)")
    w("")
    w("| kind | sheet | sheet names | found, same kind | found, any kind | extracted | extracted, not in the sheet |")
    w("|---|---|---|---|---|---|---|")
    D = F = A = 0
    for k in KIND_ORDER:
        if k not in off:
            continue
        o = off[k]
        D += o["doc"]
        F += o["found_same_kind"]
        A += o["found_any_kind"]
        w("| %s | %s | %d | %d (%s) | %d | %d | %d |" % (k, o["sheet"], o["doc"], o["found_same_kind"],
                                                     pct(o["found_same_kind"], o["doc"]),
                                                     o["found_any_kind"], o["extracted"], o["extra"]))
    w("| **all** | | %d | %d (%s) | %d (%s) | | |" % (D, F, pct(F, D), A, pct(A, D)))
    w("")
    for k in KIND_ORDER:
        if k in off and (off[k]["missing"] or off[k]["elsewhere"]):
            if off[k]["missing"]:
                w("- %s: sheet names absent from the executable: %s" % (k, ", ".join(off[k]["missing"])))
            if off[k]["elsewhere"]:
                w("- %s: sheet names the executable registers under another kind: %s" % (
                    k, ", ".join(off[k]["elsewhere"])))
    w("")
    w("Type codes: the executable's numeric type, labelled by the majority type of the sheet over "
      "the matched names. Per-field agreement with the sheet: %d/%d (%s)." % (tagr[0], tagr[1], pct(*tagr)))
    w("")
    w("| code | label | agree/of |")
    w("|---|---|---|")
    for code, v in sorted(type_names.items()):
        w("| %d | %s | %d/%d |" % (code, v["name"], v["agree"], v["of"]))
    w("")
    a, s = st["all"], st["self"]
    w("## Field accesses in the mod-tools scripts (corpus/t7/source-modtools, %d files)" % nfiles)
    w("")
    w("A field access is `<expr>.<name>`; a name is script-defined when some site assigns it "
      "(`x.name =`, `x.name[i] =`, `+=`, `++` ...) or a class declares it (`var name;`) anywhere in the scripts; commented-out code does not count. GSC objects take "
      "arbitrary fields, so script-defined is the normal case.")
    w("")
    w("| accesses | distinct names | engine fields | script-defined (assigned, not engine) | neither |")
    w("|---|---|---|---|---|")
    for lab, x in (("all `x.name`", a), ("`self.name` only", s)):
        w("| %s | %d | %d (%s) | %d (%s) | %d (%s) |" % (lab, x["names"], x["engine"], pct(x["engine"], x["names"]),
                                                    x["script"], pct(x["script"], x["names"]),
                                                    x["neither"], pct(x["neither"], x["names"])))
        w("| %s, sites | %d | %d (%s) | %d (%s) | %d (%s) |" % (lab, x["sites"], x["engine_sites"],
                                                            pct(x["engine_sites"], x["sites"]),
                                                            x["script_sites"], pct(x["script_sites"], x["sites"]),
                                                            x["neither_sites"], pct(x["neither_sites"], x["sites"])))
    w("")
    w("Engine fields the scripts also assign: %d of %d named engine fields." % (
        st["engine_assigned"], st["engine_total"]))
    w("")
    w("## Simulated lint: likely typo of an engine field")
    w("")
    w("Rule: flag `x.name` when `name` is not an engine field (any kind), is assigned nowhere in "
      "the workspace, and is within edit distance 1-2 of an engine field.")
    w("")
    w("- Names neither engine nor assigned (the rule's candidates): %d." % typo["candidates"])
    w("- Flagged: %d names, %d sites." % (len(typo["flagged"]), typo["flagged_sites"]))
    d1 = [x for x in typo["flagged"] if x["distance"] == 1]
    w("- Of these at distance 1: %d names (%s); at distance 2: %d." % (
        len(d1), ", ".join(x["name"] for x in d1), len(typo["flagged"]) - len(d1)))
    w("")
    w("20 flagged names (seeded sample), classified by hand:")
    w("")
    w("| name | sites | nearest engine field(s) | distance | class |")
    w("|---|---|---|---|---|")
    for x in typo["sample"]:
        w("| %s | %d | %s | %d | %s |" % (x["name"], x["sites"], ", ".join(x["near"][:4]),
                                          x["distance"], CLASSIFY.get(x["name"], "TODO")))
    w("")
    L.extend(REPORT_TAIL)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(L) + "\n")


A_ = "false positive: asset or map data (script bundle, settings struct, Radiant key)"
B_ = "false positive: assigned through a macro (DEFAULT)"
C_ = "false positive: set outside the workspace (compiled-only or other-mode scripts)"
D_ = "false positive: only assigned in commented-out code (a dead read, not a typo)"
E_ = "true positive: misspelling of the engine field"
CLASSIFY = {
    "_on": C_, "a_owner": C_, "balls": C_, "carried": C_, "deathfx": C_, "end": A_, "fov": A_,
    "heroes": C_, "killer": D_, "ksexplosioninnerdamage": B_, "ksexplosioninnerradius": B_,
    "kshealth": A_, "lifetime": A_, "nodamagefeedback": D_, "nohitmarkers": "uncertain: one letter from a weapon-only engine field, but read on a player", "org": D_,
    "scenetype": A_, "script_noteworty": E_ + " (consistent with its Radiant key and assert text: "
    "it works as written)", "tacos": C_, "vmtype": A_,
}
REPORT_TAIL = [
    "Of the 20: 1 true positive (`script_noteworty`), 1 uncertain (`nohitmarkers`: the engine's "
    "`nohitmarker` is a weapon field, the read is on a player), 18 false positives: 6 asset or map data, 2 macro assignments, 7 set "
    "outside the workspace, 3 dead reads.",
    "",
    "What this says about the rule: it is quiet (a few dozen names over the whole mod-tools tree) "
    "but most hits are not typos. To make it usable: (1) expand macros before collecting "
    "assignments (`DEFAULT(x, v)` assigns `x`); (2) do not flag fields read off objects that come "
    "from asset getters (script bundles, `settings`), which the workspace never assigns; (3) keep "
    "distance 1 for short names (the distance-2 hits on 2-4 letter names, `_on`/`font`, "
    "`org`/`dmg`, are noise); (4) report it as a hint, not a warning, when the workspace is "
    "partial (the mod tools ship only part of the game's scripts).",
    "",
    "## Open issues",
    "",
    "- Type codes with one or two matched names (1, 11, 15, 17) have weak labels; code 1 is "
    "labelled F_VECTOR from a single field while code 6 is the usual vector type.",
    "- The read-only verdict of 108 fields with no setter rests on the generic field write path "
    "(the dispatch does not reach sub-kind fields on entity 0). Making the dispatch reach them "
    "needs a live entity with its sentient/actor/client/vehicle pointers set.",
    "- The CSC registration (0xe3c6e0) faults in the tail call that follows its last "
    "registration (the weapon table); every table it registers is complete by the terminator check.",
    "- The official Vehicle sheet also lists 11 vehicle-node fields; the executable registers them "
    "on class 3, not on vehicle entities.",
    "- The dump is the current Steam build (PE timestamp above); the field list is from 2016.",
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--extract", default=os.path.join(RE, "out", "t7_fields_extract.json"))
    ap.add_argument("--data", default=paths.get("gsc_data"))
    ap.add_argument("--report", default=os.path.join(RE, "reports", "t7_fields.md"))
    ap.add_argument("--dump-typo", action="store_true", help="print every flagged name with its location")
    a = ap.parse_args()
    if a.dump_typo:
        acc, asg, selfacc, where, n = scan_fields()
        with open(a.extract, encoding="utf-8") as f:
            ex = json.load(f)
        hashes = {e["canon"] for t in ex["tables"] for e in t["entries"]}
        names, _ = load_names(hashes, officialfields.field_names(), set(acc) | set(asg))
        t = typo_rule(acc, asg, set(names.values()), where)
        for x in t["sample"]:
            print(x)
        return
    build(a.extract, a.data, a.report)


if __name__ == "__main__":
    main()
