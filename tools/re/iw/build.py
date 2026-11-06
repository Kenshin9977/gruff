"""Names, validation and output for one IW game: reads out/<game>.json (extract.py), writes
gruff-data/builtins/<game>.toml and reports/iw/<game>.md.

    py -3.11 -I build.py <game> [...]

Names: the executable's own (IW1, IW3: the table points at the string), else gsc-tool's id
tables (GPL-3.0, a cross-check: such an entry is labelled GPL-3.0-only, gsc-tool its name
source), else none (the id or index stays). Validation: the game's corpus call sites
(corpus/<game>), counts only.
"""
import collections
import json
import ntpath
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(1, os.path.dirname(HERE))
from common.hashes import iw9_scr as iw9_hash  # noqa: E402  (the JUP table hash)
from common import paths  # noqa: E402
import gsctool  # noqa: E402
import iwcorpus  # noqa: E402

CORPUS = paths.get("corpus")
DATA = os.path.join(paths.get("gsc_data"), "builtins")
REPORTS = os.path.join(HERE, "..", "reports", "iw")
GSCTOOL = {"iw5": "iw5_pc", "iw6": "iw6_pc", "s1": "s1_pc", "h1": "h1", "iw7": "iw7",
           "s2": "s2", "iw8": "iw8", "jup": "iw9", "jup_sp": "iw9"}


TITLES = {"iw1": "Call of Duty (IW1)", "iw2": "Call of Duty 2 (IW2)", "iw3": "Call of Duty 4 (IW3)",
          "iw4": "Modern Warfare 2 (IW4)", "iw5": "Modern Warfare 3 (IW5)", "iw6": "Ghosts (IW6)",
          "s1": "Advanced Warfare (S1)", "h1": "Modern Warfare Remastered (H1)",
          "iw7": "Infinite Warfare (IW7)", "s2": "WWII (S2)", "iw8": "Modern Warfare 2019 (IW8)",
          "jup": "Modern Warfare III multiplayer (JUP)", "jup_sp": "Modern Warfare III campaign (JUP)"}
NMAX = 16
TYPE_LABELS = {"localized string": "istring", "undefined": "undefined", "codepos": "codepos"}


def tlabel(s):
    s = TYPE_LABELS.get(s, s)
    return "".join(c for c in s.replace(" ", "") if c.isalnum() or c == "|").lower() or "?"


def param_types(types, acc, n):
    """The first `n` parameter types of a handler's runs: {index: [accessor va]} -> the
    accessors' type labels per index, "?" where none is known."""
    pts = []
    for i in range(n):
        labs = sorted({tlabel(acc[a]["label"]) for a in types.get(str(i), []) if a in acc})
        labs = [x for x in labs if x != "?"]
        pts.append("|".join(labs) if labs else "?")
    return pts


_IW4 = {}


def iw4_names():
    """match.py's names for the IW4 entries (index:kind -> name), inferred from IW5."""
    if not _IW4:
        p = os.path.join(HERE, "out", "iw4_names.json")
        _IW4.update(json.load(open(p)) if os.path.exists(p) else {"": ""})
    return _IW4


def load(game):
    with open(os.path.join(HERE, "out", game + ".json")) as f:
        return json.load(f)


def entries(doc, game):
    fn_names, me_names = gsctool.names(GSCTOOL[game]) if game in GSCTOOL else ({}, {})
    acc = doc["vm"]["accessors"]
    out = []
    for e in doc["entries"]:
        kind = e["kind"] or "function"
        r = doc["runs"].get("%x:%s" % (int(e["handler"], 16), kind), {})
        b = {"kind": kind, "handler": int(e["handler"], 16), "where": e["where"],
             "type": e.get("type")}
        if "name" in e:
            b["name"], b["name_source"], b["license"] = e["name"], "executable", "CC0-1.0"
        elif e.get("hash") is not None:
            b["hash"] = e["hash"]
            hv = int(e["hash"], 16)
            nm = (me_names if kind == "method" else fn_names).get(hv)
            if nm:
                b["name"], b["name_source"], b["license"] = nm.lower(), "gsc-tool", "GPL-3.0-only"
            else:
                b["license"] = "CC0-1.0"
            b["stored"] = (e["stored_min"], e["stored_max"])
        elif e.get("id") is not None:
            b["id"] = e["id"]
            nm = (me_names if kind == "method" else fn_names).get(e["id"])
            if nm:
                b["name"], b["name_source"], b["license"] = nm.lower(), "gsc-tool", "GPL-3.0-only"
            else:
                b["license"] = "CC0-1.0"
        else:
            b["index"] = e.get("index")
            b["license"] = "CC0-1.0"
            inferred = iw4_names().get("%d:%s" % (e.get("index"), kind))
            if inferred:
                b["name"], b["name_source"], b["license"] = inferred, "iw5-handler-match", "GPL-3.0-only"
        if r.get("arity"):
            b["emu_arity"] = tuple(r["arity"])
        if "stored" in b and b["stored"] != (0, 0):
            # (0, 0) stored: no arity recorded (the handler checks it), as T7's -1/-1
            # JUP stores min/max in the entry: those are the facts; the handler runs are the
            # second measure
            mn, mx = b["stored"]
            b["min"], b["max"] = mn, mx
            b["variadic"] = mx >= 255
            b["max_checked"] = True
            b["mode"] = "table"
            b["g_reads"] = r.get("g_reads", 0)
            b["holes"] = []
            b["param_types"] = param_types(r.get("types", {}), acc, min(mx, NMAX))
        elif r.get("arity") and "stored" in b:
            # JUP entry storing 0/0: the handler runs in this obfuscated dump agree with only
            # 57% (MP) / 75% (SP) of the corpus call sites on such entries, against 97% for the
            # stored arities: not published as an arity, kept in the evidence
            b["stop"] = "the entry stores no arity; handler runs say %d..%d (%s), not trusted" % (
                r["arity"][0], r["arity"][1], r.get("mode"))
        elif r.get("arity"):
            mn, mx = r["arity"]
            b["min"], b["max"] = mn, mx
            b["variadic"] = bool(r.get("variadic"))
            b["max_checked"] = bool(r.get("max_checked"))
            b["mode"] = r.get("mode")
            b["g_reads"] = r.get("g_reads", 0)
            b["param_types"] = param_types(r.get("types", {}), acc, mx if not b["variadic"] else min(mx, NMAX))
            b["holes"] = r.get("holes", [])
        else:
            b["stop"] = r.get("runs", {}).get(str(NMAX), [None, None])[1] if r else "not run"
        out.append(b)
    return out, fn_names, me_names


def validate(game, ents, fn_names, me_names):
    root = os.path.join(CORPUS, game)
    if not os.path.isdir(root):
        return None
    calls, defined, files = iwcorpus.scan(root)
    by_name = {}
    by_id = {}
    for e in ents:
        if "name" in e:
            by_name.setdefault((e["name"], e["kind"]), e)
        if "id" in e:
            by_id[(e["kind"], e["id"])] = e
    inv = {("function", n.lower()): i for i, n in fn_names.items()}
    inv.update({("method", n.lower()): i for i, n in me_names.items()})
    by_hash = {(e["kind"], int(e["hash"], 16)): e for e in ents if "hash" in e}

    def resolve(name, kind):
        if by_hash:
            if name.startswith("function_") and len(name) > 9:
                try:
                    hv = int(name[9:], 16)
                except ValueError:
                    hv = None
            else:
                hv = iw9_hash(name)
            return by_hash.get((kind, hv)), "hash"
        ph = gsctool.placeholder(name)
        if ph:
            return by_id.get(ph), "placeholder"
        if (name, kind) in by_name:
            return by_name[(name, kind)], "name"
        i = inv.get((kind, name))
        if i is not None and (kind, i) in by_id:
            return by_id[(kind, i)], "gsc-tool id"
        return None, None
    st = collections.Counter()
    out_rows = collections.Counter()
    names_called = set()
    names_found = set()
    other_kind = collections.Counter()
    for (name, kind, n), cnt in calls.items():
        if name in defined:
            continue
        e, how = resolve(name, kind)
        names_called.add((name, kind))
        if e is None:
            other = resolve(name, "method" if kind == "function" else "function")[0]
            if other is not None:
                other_kind[(name, kind)] += cnt
                st["kind_mismatch_sites"] += cnt
            else:
                st["unresolved_sites"] += cnt
            continue
        names_found.add((name, kind))
        st["sites"] += cnt
        if "min" not in e:
            st["no_arity_sites"] += cnt
            continue
        st["checked_sites"] += cnt
        hi = 10 ** 9 if e.get("variadic") else e["max"]
        if e["min"] <= n <= hi:
            st["within"] += cnt
        else:
            out_rows[(e.get("name") or name, kind, n, e["min"], e["max"])] += cnt
    st["files"] = files
    return {"stats": dict(st), "outside": out_rows, "names_called": len(names_called),
            "names_found": len(names_found), "kind_mismatch": other_kind,
            "unresolved": sorted(nk for nk in names_called - names_found
                                 if nk not in other_kind)}


def toml_str(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def write_toml(game, doc, ents, val):
    path = os.path.join(DATA, game.replace("_", "-") + ".toml")
    L = []
    L.append("# %s engine builtins, extracted from the game's executable by" % TITLES[game])
    L.append("# gruff/tools/re/iw (tables and roles from the engine's own lookups or registrar,")
    L.append("# arity and parameter types from every handler run under Unicorn for N = 0..16).")
    L.append("# Schema: ../schema/builtins.schema.json (plus id/index/license/max_checked/arity_mode,")
    L.append("# proposed). Facts only. Per-entry licence: an entry named from gsc-tool's GPL-3.0 table")
    L.append("# is GPL-3.0-only (name_source = \"gsc-tool\"); everything else is CC0-1.0.")
    L.append("")
    L.append("schema_version = 1")
    L.append('game = %s' % toml_str(game.replace("_", "-")))
    if doc["method"] == "hash":
        L.append('hash_family = "iw9.scr"')
    L.append('license = "CC0-1.0"')
    L.append('source = "extracted from game executable"')
    L.append("")
    L.append("[extraction]")
    L.append('tool = "gruff/tools/re/iw/extract.py + build.py"')
    L.append("executable = %s" % toml_str("%s (%s)" % (os.path.basename(doc["exe"]), doc["note"])))
    L.append("pe_timestamp = %d" % doc["pe_timestamp"])
    if doc.get("sp"):
        sp = doc["sp"]
        L.append("executable_sp = %s" % toml_str("%s (%s)" % (os.path.basename(sp["exe"]), sp["note"])))
        L.append("pe_timestamp_sp = %d" % sp["pe_timestamp"])
        L.append("merge = { sp_only = %d, arity_agree = %d, arity_differ = %d }" % (
            sp["added"], sp["agree"], sp["differ"]))
    if doc["method"] == "named":
        L.append('entry_layout = "{const char *name, handler, int developer} (12 bytes, x86)"')
    elif doc["method"] == "hash":
        L.append('entry_layout = "{u64 name hash, u64 handler, u8 developer, u8 min_args, u8 max_args, '
                 '5 bytes 0} (24 bytes), appended at init to the function or the method list"')
    elif doc["method"] == "desc":
        L.append('entry_layout = "{u64 id, u64 handler, u64 developer} (24 bytes), tables described by '
                 '{entries, count}, stored at init into the function or method array of the VM context"')
    elif doc["method"] == "id":
        L.append('entry_layout = "{u64 id, u64 handler, u32 developer, u32 pad} (24 bytes), registered '
                 'at init through one registrar"')
    else:
        L.append('entry_layout = "{u64 handler, u32 developer, u32 pad} (16 bytes), appended in '
                 'order to one array at init (no name, no id)"')
    if doc["method"] == "hash":
        L.append('arity_source = "the entry (u8 min, u8 max); handler emulation as the second measure"')
    else:
        L.append('arity_source = "handler emulation: N = 0..16 parameters, accessors hooked, ok(N) = '
                 'the run raises no script error and reads no index >= N"')
    L.append("variadic_max = %d" % NMAX)
    kinds = collections.Counter(e["kind"] for e in ents)
    L.append("counts = { %s }" % ", ".join("%s = %d" % (k, v) for k, v in sorted(kinds.items())))
    g, t, base = int(doc["vm"]["G"], 16), int(doc["vm"]["T"], 16), int(doc["image_base"], 16)
    if doc["vm"].get("ctx"):  # IW8, JUP: fields of the VM context passed in rcx
        ctx = int(doc["vm"]["ctx"], 16)
        L.append('vm_globals = { base = "context", outparamcount = "0x%x", top = "0x%x" }' % (g - ctx, t - ctx))
    elif g:  # globals of the image: RVAs
        L.append('vm_globals = { base = "image", outparamcount = "0x%x", top = "0x%x" }' % (g - base, t - base))
    # IW1: the VM lives in CoDMP.exe, reached through engine slots (iw1vm.py): no address here
    L.append("")

    def key(e):
        return (e["kind"], e.get("name") or "~", e.get("id", 0) or 0, e.get("index", 0) or 0)
    base = int(doc["image_base"], 16)
    for e in sorted(ents, key=key):
        L.append("[[builtin]]")
        if "name" in e:
            L.append("name = %s" % toml_str(e["name"]))
            L.append("name_source = %s" % toml_str(e["name_source"]))
        if "hash" in e:
            L.append("hash = %s" % toml_str(e["hash"]))
        if "id" in e:
            L.append("id = %d" % e["id"])
        if "index" in e and e["index"] is not None:
            L.append("index = %d" % e["index"])
        L.append('vm = "gsc"')
        L.append("kind = %s" % toml_str(e["kind"]))
        if "min" in e:
            L.append("min_args = %d" % e["min"])
            L.append("max_args = %d" % (NMAX if e["variadic"] else e["max"]))
            if e["variadic"]:
                L.append("variadic = true")
            L.append("max_checked = %s" % ("true" if e["max_checked"] else "false"))
            L.append("arity_mode = %s" % toml_str(e["mode"]))
        else:
            L.append("min_args = -1")
            L.append("max_args = -1")
            L.append("arity_unknown = true")
        L.append("dev_only = %s" % ("true" if e.get("type") == 1 else "false"))
        if e.get("type") is not None:
            L.append("type_field = %d" % e["type"])
        if e.get("param_types") and any(p != "?" for p in e["param_types"]):
            L.append("param_types = [%s]" % ", ".join(toml_str(p) for p in e["param_types"]))
        b0 = int(doc["sp"]["image_base"], 16) if e.get("sp_only") else base
        L.append('handler_rva = "0x%x"' % (e["handler"] - b0))
        L.append("executables = [%s]" % ", ".join(toml_str(x) for x in e.get("exes", ["mp"])))
        L.append("license = %s" % toml_str(e["license"]))
        ev = [e["where"]]
        if e.get("mode") == "table":
            ea = e.get("emu_arity")
            ev.append("min/max stored in the entry (u8 at +17, +18); handler runs N=0..16: %s" % (
                "ok for N in %d..%d" % ea if ea else "no N passes"))
        elif "min" in e:
            ev.append("handler runs N=0..16 (%s): ok for N in %d..%d%s%s; numparam reads %d" % (
                e["mode"], e["min"], e["max"] if not e["variadic"] else NMAX,
                ", holes %s" % e["holes"] if e["holes"] else "",
                ", larger N rejected" if e["max_checked"] else ", larger N accepted (extra ignored)",
                e["g_reads"]))
        else:
            ev.append(e.get("stop") if (e.get("stop") or "").startswith("the entry stores") else
                      "handler runs: no N passes (%s)" % e.get("stop"))
        if e.get("stored") == (0, 0):
            ev.append("the entry stores min 0, max 0 (no arity recorded): arity from the handler runs")
        if e.get("sp_arity"):
            ev.append("the SP executable's handler measures %d..%d" % e["sp_arity"])
        if e.get("arity_from") == "sp":
            ev.append("arity from the SP executable's handler")
        if e.get("name_source") == "gsc-tool":
            ev.append("name from gsc-tool's table by id (GPL-3.0)")
        if e.get("name_source") == "iw5-handler-match":
            ev.append("name inferred: the one IW5 handler of the same code shape and arity (name from gsc-tool, GPL-3.0)")
        L.append("evidence = %s" % toml_str("; ".join(ev)))
        L.append("")
    os.makedirs(DATA, exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(L))
    return path


def exe_basename(line):
    """A log line naming the executable keeps its file name only (older extracts logged the path)."""
    m = re.match(r"exe (.+?\.(?:exe|dll)):", line, re.I)
    return "exe %s:%s" % (ntpath.basename(m.group(1)), line[m.end():]) if m else line


def write_report(game, doc, ents, val):
    os.makedirs(REPORTS, exist_ok=True)
    path = os.path.join(REPORTS, game + ".md")
    R = ["# %s builtin catalogue: extraction and validation" % TITLES[game], ""]
    R.append("Generated by `build.py` from `extract.py %s`. Executable: `%s` (%s), PE timestamp %d." % (
        game, os.path.basename(doc["exe"]), doc["note"], doc["pe_timestamp"]))
    R.append("")
    R.append("## Extraction log")
    R.append("")
    R.append("```")
    R += [exe_basename(x) for x in doc["log"] if not x.startswith("  handlers run")]
    R.append("```")
    R.append("")
    s = summary(game, ents, val)
    R.append("## Summary")
    R.append("")
    for k, v in s.items():
        R.append("- %s: %s" % (k, v))
    R.append("")
    if val:
        R.append("## Corpus call sites outside the extracted min..max (most frequent 40)")
        R.append("")
        R.append("| name | kind | args | sites | extracted min..max |")
        R.append("|---|---|---|---|---|")
        for (nm, k, n, mn, mx), c in val["outside"].most_common(40):
            R.append("| %s | %s | %d | %d | %d..%d |" % (nm, k, n, c, mn, mx))
        R.append("")
        R.append("## Called names with no entry of that kind (most frequent 40)")
        R.append("")
        R.append("Kind mismatches (the name exists with the other kind): %s" % ", ".join(
            "%s as %s (%d)" % (n, k, c) for (n, k), c in val["kind_mismatch"].most_common(20)))
        R.append("")
        R.append("Unresolved: %s" % ", ".join("%s (%s)" % nk for nk in val["unresolved"][:60]))
        R.append("")
    noar = collections.Counter(e.get("stop") for e in ents if "min" not in e)
    R.append("## Entries with no arity, by the reason the N=16 run stopped")
    R.append("")
    for k, v in noar.most_common(15):
        R.append("- %s: %d" % (k, v))
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(R) + "\n")
    return path


def summary(game, ents, val):
    n = len(ents)
    ar = sum(1 for e in ents if "min" in e)
    named = sum(1 for e in ents if "name" in e)
    gpl = sum(1 for e in ents if e.get("license") == "GPL-3.0-only")
    withp = [e for e in ents if "min" in e and e["max"] > 0]
    typed1 = sum(1 for e in withp if any(p != "?" for p in e["param_types"]))
    typed_all = sum(1 for e in withp if all(p != "?" for p in e["param_types"]))
    s = collections.OrderedDict()
    s["entries"] = "%d (%s)" % (n, dict(collections.Counter(e["kind"] for e in ents)))
    s["named"] = "%d (%d from the executable, %d from gsc-tool, GPL-3.0)" % (named, named - gpl, gpl)
    s["dev_only"] = sum(1 for e in ents if e.get("type") == 1)
    s["arity measured"] = "%d/%d (%.1f%%; strict %d, lenient %d)" % (
        ar, n, 100.0 * ar / max(n, 1), sum(1 for e in ents if e.get("mode") == "strict"),
        sum(1 for e in ents if e.get("mode") == "lenient"))
    s["parameter types"] = "%d/%d entries with parameters have at least one typed (%.1f%%), %d all typed" % (
        typed1, len(withp), 100.0 * typed1 / max(len(withp), 1), typed_all)
    if val:
        st = val["stats"]
        s["corpus"] = "%d files; %d call sites to a builtin of the catalogue, %d with an arity" % (
            st.get("files", 0), st.get("sites", 0), st.get("checked_sites", 0))
        s["call sites within min..max"] = "%d/%d (%.1f%%)" % (
            st.get("within", 0), st.get("checked_sites", 0),
            100.0 * st.get("within", 0) / max(st.get("checked_sites", 0), 1))
        s["called builtin names found"] = "%d/%d (%.1f%%); kind mismatches %d sites, unresolved %d sites" % (
            val["names_found"], val["names_called"], 100.0 * val["names_found"] / max(val["names_called"], 1),
            st.get("kind_mismatch_sites", 0), st.get("unresolved_sites", 0))
    return s


def key(e):
    for k in ("id", "hash", "name"):
        if k in e:
            return (e["kind"], k, e[k])
    return None


def merge_sp(game, doc, ents):
    """The single-player executable's catalogue merged in: by (kind, id) (or hash, or name).
    An entry both executables register keeps the multiplayer measure, the other is checked
    against it; an SP-only entry is added. Each entry says which executables have it."""
    path = os.path.join(HERE, "out", game + "_sp.json")
    if game == "jup":
        path = ""  # MWIII campaign is its own catalogue (jup-sp), not merged
    if not os.path.exists(path):
        for e in ents:
            e["exes"] = ["mp"] if game not in ("iw6",) else ["sp"]
        return ents, None
    doc2 = load(game + "_sp")
    ents2, _, _ = entries(doc2, game)
    by = {key(e): e for e in ents}
    for e in ents:
        e["exes"] = ["mp"]
    added = agree = differ = 0
    for e in ents2:
        k = key(e)
        if k in by:
            m = by[k]
            m["exes"].append("sp")
            if "min" in m and "min" in e:
                if (m["min"], m["max"]) == (e["min"], e["max"]):
                    agree += 1
                else:
                    differ += 1
                    m["sp_arity"] = (e["min"], e["max"])
            elif "min" not in m and "min" in e:
                for f in ("min", "max", "variadic", "max_checked", "mode", "g_reads", "param_types",
                          "holes"):
                    if f in e:
                        m[f] = e[f]
                m["arity_from"] = "sp"
                m.pop("stop", None)
        else:
            e["exes"] = ["sp"]
            e["sp_only"] = True
            ents.append(e)
            added += 1
    info = {"exe": doc2["exe"], "note": doc2["note"], "pe_timestamp": doc2["pe_timestamp"],
            "added": added, "agree": agree, "differ": differ, "image_base": doc2["image_base"]}
    return ents, info


def main():
    for game in sys.argv[1:]:
        doc = load(game)
        ents, fn, me = entries(doc, game)
        ents, sp = merge_sp(game, doc, ents)
        doc["sp"] = sp
        val = validate(game, ents, fn, me)
        p1 = write_toml(game, doc, ents, val)
        p2 = write_report(game, doc, ents, val)
        print(game)
        for k, v in summary(game, ents, val).items():
            print("  %s: %s" % (k, v))
        print("  wrote", p1, p2)


if __name__ == "__main__":
    main()
