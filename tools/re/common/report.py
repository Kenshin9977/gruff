"""Writers and checks shared by the builders: the builtins TOML (gruff-data/schema/
builtins.schema.json), the call-site validation against a corpus, and Markdown helpers.

A builder hands normalised entries (dicts) to these functions:

    name, name_source            optional; the name only when its hash is the entry's (or the
                                 table stores the string itself: name_source "executable")
    hash                         int or None (engines whose tables hold names)
    vm ("gsc"|"csc"), kind ("function"|"method"), min, max (ints; -1/-1 unchecked)
    variadic_max                 the engine's "no limit" max, or None
    type_flags, type_field       ints or None; dev_only bool or None
    param_types                  list or None; shadowed bool; handler_rva int; evidence str
"""
import collections
import json
import os
import re

from .corpus import hashed_name

VM_ORDER = ["gsc_function", "gsc_method", "csc_function", "csc_method"]


def toml_str(s):
    bs = chr(92)
    return '"' + s.replace(bs, bs + bs).replace('"', bs + '"') + '"'


def toml_key(k):
    """A TOML key: bare when it may be, else quoted."""
    return k if re.fullmatch(r"[A-Za-z0-9_-]+", k) else json.dumps(k)


def toml_value(v):
    """A TOML value of a bool, int, float, string, list or dict (an inline table)."""
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, int):
        return str(v)
    if isinstance(v, float):
        return repr(v)
    if isinstance(v, list):
        return "[" + ", ".join(toml_value(x) for x in v) + "]"
    if isinstance(v, dict):
        return "{ " + ", ".join("%s = %s" % (toml_key(k), toml_value(x)) for k, x in v.items()) + " }"
    return json.dumps(v, ensure_ascii=False)


def pct(a, b):
    return "%.1f%%" % (100.0 * a / b) if b else "n/a"


def fmt_max(m, variadic_max):
    return "var" if variadic_max is not None and m >= variadic_max else str(m)


def role_of(e):
    return "%s_%s" % (e["vm"], e["kind"])


# ---------------------------------------------------------------------------------------------
# TOML
def write_builtins_toml(path, header_comment, top, extraction, lookups, entries, hash_digits=8,
                        intrinsics=None):
    """`top`: ordered (key, value) of the top-level keys after schema_version (game, license,
    source, hash_family). `extraction`: ordered (key, raw TOML value) pairs. `lookups`: {role:
    (rva, [(table rva, count)])} or None. `intrinsics`: list of dicts written as [[intrinsic]]."""
    os.makedirs(os.path.dirname(path), exist_ok=True)
    L = ["# " + line for line in header_comment]
    L += ["", "schema_version = 1"]
    for k, v in top:
        L.append("%s = %s" % (k, toml_str(v)))
    L += ["", "[extraction]"]
    for k, v in extraction:
        L.append("%s = %s" % (k, v))
    if lookups:
        L += ["", "[extraction.lookups]"]
        for role in [r for r in VM_ORDER if r in lookups] + sorted(set(lookups) - set(VM_ORDER)):
            rva, tabs = lookups[role]
            L.append("%s = { rva = %s, tables = [%s] }" % (
                role, toml_str("0x%x" % rva),
                ", ".join("{ rva = %s, count = %d }" % (toml_str("0x%x" % t), n) for t, n in tabs)))
    L.append("")
    order = sorted(entries, key=lambda e: (
        VM_ORDER.index(role_of(e)), e.get("name") or "~", e.get("hash") or 0,
        e.get("table", 0), e.get("index", 0)))
    for e in order:
        L.append("[[builtin]]")
        if e.get("name"):
            L.append("name = %s" % toml_str(e["name"]))
            L.append("name_source = %s" % toml_str(e["name_source"]))
        if e.get("hash") is not None:
            L.append('hash = "0x%0*x"' % (hash_digits, e["hash"]))
        L.append('vm = "%s"' % e["vm"])
        L.append('kind = "%s"' % e["kind"])
        L.append("min_args = %d" % e["min"])
        L.append("max_args = %d" % e["max"])
        vmax = e.get("variadic_max")
        if vmax is not None and e["max"] >= vmax:
            L.append("variadic = true")
        if e["min"] < 0:
            L.append("arity_unchecked = true")
        if e.get("type_flags") is not None:
            L.append("type_flags = %d" % e["type_flags"])
        if e.get("type_field") is not None and e.get("type_field") != e.get("type_flags"):
            L.append("type_field = %d" % e["type_field"])
        if e.get("dev_only") is not None:
            L.append("dev_only = %s" % ("true" if e["dev_only"] else "false"))
        if e.get("param_types"):
            L.append("param_types = [%s]" % ", ".join(toml_str(t) for t in e["param_types"]))
        if e.get("shadowed"):
            L.append("shadowed = true")
        if e.get("handler_rva") is not None:
            L.append('handler_rva = "0x%x"' % e["handler_rva"])
        L.append("evidence = %s" % toml_str(e["evidence"]))
        L.append("")
    for it in intrinsics or ():
        L.append("[[intrinsic]]")
        for k in ("name", "hash", "vm", "opcode", "also_builtin", "corpus_args", "evidence"):
            if it.get(k) is None:
                continue
            v = it[k]
            if isinstance(v, bool):
                L.append("%s = %s" % (k, "true" if v else "false"))
            elif isinstance(v, int):
                L.append("%s = %d" % (k, v))
            elif isinstance(v, list):
                L.append("%s = [%s]" % (k, ", ".join(
                    toml_str(x) if isinstance(x, str) else str(x) for x in v)))
            else:
                L.append("%s = %s" % (k, toml_str(v)))
        L.append("")
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(L))


# ---------------------------------------------------------------------------------------------
# call sites
def effective_entries(E, names):
    """T7-T9 extraction entries as the engine's lookup returns them: shadowed (a duplicate the
    lookup never returns: kept, flagged), kind, VM, effective type (the lookup's, else the
    table's), name, key = hash = canon, signed min/max (0xFFFFFFFF/0xFFFFFFFF is -1/-1: no
    arity recorded, `unchecked`)."""
    for e in E:
        e["shadowed"] = e["lookup_handler_rva"] != e["handler_rva"]
        e["kind"] = "method" if e["role"].endswith("method") else "function"
        e["vm"] = e["role"][:3]
        e["eff_type"] = e["lookup_type"] if e["lookup_type"] is not None else e["type_field"]
        e["name"] = names.get(e["canon"])
        e["key"] = e["hash"] = e["canon"]
        for k in ("min", "max"):
            if e[k] >= 0x80000000:
                e[k] -= 1 << 32
        e["unchecked"] = e["min"] < 0


def vm_kind_counts(E):
    """The `counts` value of [extraction]: entries per VM and kind, shadowed ones left out."""
    cnt = collections.Counter((e["vm"], e["kind"]) for e in E if not e["shadowed"])
    return "{ %s }" % ", ".join("%s_%s = %d" % (vm, k, cnt[(vm, k)])
                                for vm in ("gsc", "csc") for k in ("function", "method"))


def lookups_of(ex, roles=VM_ORDER):
    """{role: (lookup rva, [(table rva, count)])} of a T7-T9 extraction, for the TOML."""
    return {role: (ex["lookups"][role]["rva"], [(t["rva"], t["count"])
                                                for t in ex["lookups"][role]["tables"]])
            for role in roles}


def lookup_entry(e, nsrc, origin, variadic_max, param_types, extra_evidence=""):
    """A T7-T9 entry for write_builtins_toml: `origin` says how its table was read."""
    hp = e.get("handler") or {}
    ev = ["table %s[%d] entry %d (%s)" % (e["role"], e["table"], e["index"], origin)]
    ev.append("lookup round trip %s" % ("shadowed by an earlier table" if e["shadowed"] else "ok"))
    if hp:
        ev.append("handler run N=%d read %s (%s), N=%d read %s%s" % (
            hp["n_max"], hp["read_at_max"], hp["stop_at_max"].split(" at ")[0],
            hp["n_min"], hp["read_at_min"], extra_evidence))
    return {
        "name": e["name"], "name_source": nsrc.get(e["canon"]), "hash": e["canon"],
        "vm": e["vm"], "kind": e["kind"], "min": e["min"], "max": e["max"],
        "variadic_max": variadic_max, "type_flags": e["eff_type"], "type_field": e["type_field"],
        "dev_only": e["eff_type"] == 1, "param_types": param_types, "shadowed": e["shadowed"],
        "handler_rva": e["handler_rva"], "evidence": "; ".join(ev),
        "table": e["table"], "index": e["index"],
    }


def by_key(entries):
    """{(hash or name, vm, kind): entry} over the effective (non-shadowed) entries."""
    out = {}
    for e in entries:
        if e.get("shadowed"):
            continue
        out.setdefault((e["key"], e["vm"], e["kind"]), e)
    return out


def validate_calls(entries, calls, defined, key_of, intrinsics=()):
    """Every call site to a builtin of the matching syntax kind and VM (names the scripts define
    excluded) must fall within min..max. `key_of(name)` -> the table key of a corpus name (its
    hash, or the name), or None. Entries carry "key". Returns a stats dict."""
    bk = by_key(entries)
    intr = {i.lower() for i in intrinsics}
    st = dict(total=0, builtin=0, ok=0, bad=collections.Counter(), kind=collections.Counter(),
              unchecked=collections.Counter(), intrinsic=collections.Counter(),
              unknown=collections.Counter(), called=set(), called_names=set(),
              script_defined=0)
    for (name, kind, vm, nargs), n in calls.items():
        st["total"] += n
        if name in defined:
            st["script_defined"] += n
            continue
        k = key_of(name)
        vms = ["gsc", "csc"] if vm == "gsh" else [vm]
        cands = [bk[(k, v, kind)] for v in vms if (k, v, kind) in bk] if k is not None else []
        if not cands:
            alt = [bk[(k, v, kk)] for v in vms for kk in ("function", "method")
                   if (k, v, kk) in bk] if k is not None else []
            if name in intr:
                st["intrinsic"][name] += n
            elif alt:
                st["kind"][(name, kind, vm)] += n
            else:
                st["unknown"][(name, kind, vm)] += n
            continue
        if all(c["min"] < 0 for c in cands):
            st["unchecked"][name] += n
            continue
        st["builtin"] += n
        for c in cands:
            st["called"].add(id(c))
        st["called_names"].add((k, kind, tuple(vms)))
        if any(c["min"] <= nargs <= c["max"] for c in cands if c["min"] >= 0):
            st["ok"] += n
        else:
            c = cands[0]
            st["bad"][(name, kind, vm, nargs, c["min"], c["max"])] += n
    return st


def hashed_key(hash_fn):
    """key_of for hashed engines: function_<hex> is its hash, any other name is hashed."""
    def key_of(name):
        h = hashed_name(name)
        return h if h is not None else hash_fn(name)
    return key_of


def calls_section(st, corpus_desc, variadic_max, limit=60):
    """Markdown lines for a validate_calls result."""
    L = ["## Call sites (%s)" % corpus_desc, ""]
    L.append("- Call sites seen: %d; to script-defined names: %d; to a builtin of the matching "
             "kind and VM: %d." % (st["total"], st["script_defined"], st["builtin"]))
    L.append("- Within the extracted min..max: %d/%d (%s)." % (st["ok"], st["builtin"],
                                                              pct(st["ok"], st["builtin"])))
    L.append("- Calls to entries with no arity (-1/-1), not counted above: %s." % (
        ", ".join("%s %d" % kv for kv in st["unchecked"].most_common()) or "none"))
    L.append("- Calls to compiler intrinsics (opcodes, not table builtins): %d sites over %d "
             "names." % (sum(st["intrinsic"].values()), len(st["intrinsic"])))
    L.append("- Call sites whose syntax kind has no builtin of that kind in that VM (but the "
             "other kind exists): %d sites over %d (name, kind, file type)."
             % (sum(st["kind"].values()), len(st["kind"])))
    L.append("- Call sites to a name that is neither script-defined nor a builtin: %d sites over "
             "%d (name, kind, file type) (calls to functions of scripts outside the corpus, "
             "or builtins of a table not extracted)."
             % (sum(st["unknown"].values()), len(st["unknown"])))
    L.append("")
    L.append("### Call sites outside min..max")
    L.append("")
    L.append("| name | kind | file type | args | sites | extracted min..max |")
    L.append("|---|---|---|---|---|---|")
    for (name, kind, vm, nargs, mn, mx), n in sorted(st["bad"].items())[:limit]:
        L.append("| %s | %s | %s | %d | %d | %d..%s |" % (name, kind, vm, nargs, n, mn,
                                                        fmt_max(mx, variadic_max)))
    L.append("")
    L.append("### Kind mismatches at call sites (most frequent 40)")
    L.append("")
    L.append("| name | syntax kind | file type | sites |")
    L.append("|---|---|---|---|")
    for (name, kind, vm), n in st["kind"].most_common(40):
        L.append("| %s | %s | %s | %d |" % (name, kind, vm, n))
    L.append("")
    return L


def coverage(entries, st):
    """(called effective entries, effective entries)."""
    live = [e for e in entries if not e.get("shadowed")]
    return sum(1 for e in live if id(e) in st["called"]), len(live)


def handler_section(entries, variadic_max):
    """Markdown lines summarising the handler runs (entries carry "handler")."""
    hp = [e for e in entries if e.get("handler")]
    with_types = [e for e in hp if e["handler"]["types"]]
    full = [e for e in with_types if (variadic_max is None or e["max"] < variadic_max)
            and len(e["handler"]["types"]) == min(e["max"], e["handler"]["n_max"])]
    over = [e for e in hp if (variadic_max is None or e["max"] < variadic_max)
            and e["handler"]["types"] and max(int(k) for k in e["handler"]["types"]) >= e["max"]]
    stops = collections.Counter(e["handler"]["stop_at_max"].split(" at ")[0] for e in hp)
    L = ["## Handler emulation (parameter types)", ""]
    L.append("- Handlers run: %d entries. With at least one typed parameter read: %d (%s). Every "
             "parameter up to min(max, 16) typed: %d." % (len(hp), len(with_types),
                                                          pct(len(with_types), len(hp)), len(full)))
    L.append("- Third measure of max: entries whose handler reads an index >= max: %d." % len(over))
    L.append("- Methods whose handler resolves its entref: %d/%d; functions that do: %d." % (
        sum(1 for e in hp if e["kind"] == "method" and e["handler"]["resolves_self"]),
        sum(1 for e in hp if e["kind"] == "method"),
        sum(1 for e in hp if e["kind"] == "function" and e["handler"]["resolves_self"])))
    L.append("- How the N=max runs ended: %s." % ", ".join("%s %d" % kv for kv in stops.most_common()))
    L.append("")
    if over:
        L.append("| name | VM | kind | min..max | indices read |")
        L.append("|---|---|---|---|---|")
        for e in over[:60]:
            L.append("| %s | %s | %s | %d..%s | %s |" % (
                e.get("name") or "0x%x" % (e.get("hash") or 0), e["vm"], e["kind"], e["min"],
                fmt_max(e["max"], variadic_max), sorted(int(k) for k in e["handler"]["types"])))
        L.append("")
    return L, dict(with_types=len(with_types), runs=len(hp), full=len(full), over=len(over))


def write_md(path, lines):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines) + "\n")
