"""From an events extraction (out/events_<game>.json) to gruff-data/events/<game>.toml and
reports/events_<game>.md: each site's event named, the sites merged per event, the corpus
measure. Engine-neutral: the profile gives the hash, the corpus folders and the VM of each inst.

Site -> event:
  - a C-string notifier: the string read at the notifier's entry;
  - a value that is a scr_const token: that constant's string;
  - otherwise (hashed engines) the value is the event hash, named by the image's own strings,
    the scr_const strings, then the corpus (each name re-hashed: a list is never trusted);
  - several immediates written to the event argument on the paths to the call (a branch picks
    the event): each is an event of the site;
  - a sentinel (the caller forwards one of its arguments), a register no path set, an SL id
    that is no constant: unresolved, listed in the report.
Arguments: the pushes seen on the path after the previous notify, in waittill order (the
reverse of push order: the last value pushed is the first waittill variable, checked on
`grenade_fire` and `death` in T7); kept only when their number equals the count argument
(the two measures), else the count is kept with unknown types.
"""
import collections
import datetime
import hashlib
import json
import os

from . import paths
from . import events_corpus as C
from .events import TOKEN_BASE, TOKEN16, sentinel_index, sentinel_ptr_index
from .names import files_under, words_of
from .report import toml_str, pct

RE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPORTS = os.path.join(RE, "reports")
GARBAGE = (0x4F000000, 0x50000000)  # register values the probe starts with


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for b in iter(lambda: f.read(1 << 22), b""):
            h.update(b)
    return h.hexdigest()


def rebase(site, base):
    """VAs to RVAs in a site record (the JSON keeps addresses relative to the image)."""
    out = {}
    for k, v in site.items():
        if k in ("site", "notifier", "fn") and isinstance(v, int):
            out[k] = v - base
        elif k == "pushes":
            out[k] = [dict(p, at=p["at"] - base, fn=p["fn"] - base,
                           source=(p["source"] - base) if p.get("source") else None) for p in v]
        else:
            out[k] = v
    return out


# ---------------------------------------------------------------------------------------------
def site_events(s, n, d, prof):
    """[(kind, value)] for one resolved site: ("name", s, source) / ("hash", h) / ("fail", why)."""
    width = prof.get("const_width", 4)
    base = TOKEN_BASE if width == 4 else TOKEN16
    cmask = 0xFFFFFFFF if width == 4 else 0xFFFF
    strs = d["const_strings"]
    mask = prof.get("hash_mask", 0xFFFFFFFF)
    v = s.get("event")
    if v is None:
        return [("fail", "no event value")]
    if n["event_kind"] == "cstr":
        if s.get("event_str"):
            return [("name", s["event_str"], "string literal")]
        si = sentinel_index(v)
        return [("fail", "forwarded argument %d" % si if si is not None else "C string not in "
                 "the image")]
    k = (v & cmask) - base
    # a 64-bit hash (T11) is never a token; an id read from a 64-bit register may carry junk
    # above its 32 bits
    if 0 <= k < len(strs) and (v < (1 << 32) or mask <= 0xFFFFFFFF or n["event_kind"] == "sl"):
        return [("name", strs[k], "scr_const")]
    small = v < (1 << 32)
    if small and sentinel_ptr_index(v) is not None:
        return [("fail", "forwarded argument %d (by pointer)" % sentinel_ptr_index(v))]
    if small and sentinel_index(v) is not None:
        return [("fail", "forwarded argument %d (a caller too large to probe as a wrapper)"
                 % sentinel_index(v))]
    if small and GARBAGE[0] <= v < GARBAGE[1]:
        return [("fail", "event register not set on the emulated path")]
    if n["event_kind"] == "sl" or prof.get("hash") is None:
        return [("fail", "string id %#x is no scr_const: a run-time string (an event name read from data the dump does not hold)" % (v & cmask))]
    vals = {v & mask}
    alts = [a & mask for a in s.get("alternatives", [])]
    if len(alts) > 1 or (alts and alts[0] not in vals):
        vals |= set(alts)
    return [("hash", h) for h in sorted(vals)]


def site_count_args(s, n):
    """(count, [types] in waittill order, how)."""
    pushes = s.get("pushes", [])
    c = s.get("count")
    implicit = n["count"] is not None and n["count"][0] == "global"
    if implicit or n["count"] is None:
        return len(pushes), pushes[::-1], "pushes seen (the count is the VM's own)"
    if c is not None and 0 <= c < 32:
        if c == len(pushes):
            return c, pushes[::-1], "count argument = pushes seen"
        return c, None, "count argument %d, %d pushes seen" % (c, len(pushes))
    sc = s.get("static_count") or []
    if len(sc) == 1:
        if sc[0] == len(pushes):
            return sc[0], pushes[::-1], "static count = pushes seen"
        return sc[0], None, "static count %d, %d pushes seen" % (sc[0], len(pushes))
    return len(pushes), pushes[::-1], "pushes seen (count argument not resolved)"


def arg_entry(p, prof):
    if p is None:
        return {"type": "unknown"}
    t = p["type"]
    out = {"type": t}
    raw = p["vm_type"]
    src = p.get("source")
    label = prof.get("object_sources", {}).get(src) if src is not None else None
    if label:
        raw = label
    if t == "unknown" or (t == "entity" and raw not in ("pointer", "object")):
        out["vm_type"] = raw
    return out


def name_hashes(d, prof, corpus_strings):
    """{hash: (name, source)} from the image strings, the scr_const strings and the corpus."""
    hf = prof.get("hash")
    mask = prof.get("hash_mask", 0xFFFFFFFF)
    out = {}
    for h, names in d.get("image_names", {}).items():
        out[int(h)] = (sorted(names, key=lambda x: (x != x.lower(), x))[0].lower(), "image string")
    if hf is None:
        return out
    for s in d["const_strings"]:
        h = hf(s) & mask
        out.setdefault(h, (s.lower(), "scr_const"))
    for s in corpus_strings:
        h = hf(s) & mask
        out.setdefault(h, (s.lower(), "corpus"))
    if prof.get("name_lists"):
        wanted = {e for e in wanted_hashes(d, prof) if e not in out}
        for label, words in external_words():
            n = 0
            for w in words:
                h = hf(w) & mask
                if h in wanted and h not in out:
                    out[h] = (w, label)
                    n += 1
            print("names: %s: %d candidates, names %d" % (label, len(words), n))
    return out


def wanted_hashes(d, prof):
    mask = prof.get("hash_mask", 0xFFFFFFFF)
    out = set()
    for s in d["sites"]:
        for v in [s.get("event")] + s.get("alternatives", []):
            if v is not None:
                out.add(v & mask)
    return out


def external_words():
    """This machine's name lists (paths.local.json): BO4 glossary, GrayWolf name indexes,
    cod-hash-recovery, the local hash lists. Each word is hashed again: no claim is trusted."""
    import glob
    gw = paths.get("graywolf_package_index") or ""
    chr_ = paths.get("cod_hash_recovery") or ""
    glossary = [g for p in paths.get_list("bo4_glossary")
                for g in (glob.glob(p) if "*" in p else [p])]
    yield "bo4-glossary", words_of(glossary)
    if gw:
        yield "graywolf-name-index", words_of(glob.glob(os.path.join(gw, "*.wni")))
    if chr_:
        yield "cod-hash-recovery", words_of(files_under(chr_))
    yield "hash-list", words_of(p for root in paths.get_list("hash_lists")
                                for p in files_under(root))
    # the local hash packages (gsc-toolchain/hash-packages: never committed): their names only
    hp = paths.get("hash_packages")
    if hp and os.path.isdir(hp):
        yield "hash-package", words_of(files_under(hp, ("tsv",)))


def corpus_roots(names):
    """Each a folder of the corpus, or "@key": a corpus kept elsewhere (paths.local.json key);
    the folders that exist."""
    corpus_root = paths.get("corpus")
    roots = [(paths.get(r[1:]) or "") if r.startswith("@") else os.path.join(corpus_root, r)
             for r in names]
    return [r for r in roots if os.path.isdir(r)]


def cross_measure(prof, events):
    """A game without its own corpus (BO6): the catalogue's named events measured against a
    sibling's corpus (BO7), the names hashed as that corpus spells them. None if not set."""
    x = prof.get("cross_corpus")
    if not x:
        return None
    roots = corpus_roots(x["roots"])
    if not roots:
        return {"label": x["label"], "missing": True}
    cs = C.scan(roots, wait_any=prof.get("waittill_any", False))
    idents = [x["hash"](e["name"]) for e in events if e["name"]]
    cov = C.coverage(idents, cs, x["hash"], lower=True)
    return {"label": x["label"], "files": cs["files"], "cov": cov}


def site_event_records(s, n, d, p, prof, tag, events, fails, fail_sites, named):
    """Adds one site's events to `events` (identity -> record), or its failure. `d`, `p`: the
    extraction and profile the site belongs to; `prof`: the catalogue's (hash, payload)."""
    hf = prof.get("hash")
    mask = prof.get("hash_mask", 0xFFFFFFFF)
    if "fail" in s:
        fails[s["fail"]] += 1
        fail_sites.append((s["site"], tag + s["fail"]))
        return
    evs = site_events(s, n, d, p)
    count, pushes, how = site_count_args(s, n)
    if prof.get("payload") == "struct":  # T8+: waittill returns one struct of fields
        count, pushes, how = 0, [], "payload is the waitresult struct"
    vm = p["vm_of_inst"].get(s.get("inst"), None)
    if vm is None and isinstance(s.get("inst"), int) and p.get("vm_of_ctx"):
        # a VM context pointer (T10, T11): its rva names the VM
        vm = p["vm_of_ctx"].get(s["inst"] - d["image_base"])
    for ev in evs:
        if ev[0] == "fail":
            fails[ev[1]] += 1
            fail_sites.append((s["site"], tag + ev[1]))
            continue
        if ev[0] == "name":
            name, src = ev[1], ev[2]
            h = (hf(name) & mask) if hf else None
            ident = h if hf else name
        else:
            h = ev[1]
            ident = h
            name, src = named.get(h, (None, None))
        r = events.setdefault(ident, {"name": name, "name_source": src, "hash": h,
                                      "sites": [], "vms": set(), "recv": set()})
        if r["name"] is None and name:
            r["name"], r["name_source"] = name, src
        r["sites"].append({"rva": s["site"], "count": count, "pushes": pushes, "how": how,
                           "multi": len(evs) > 1})
        if vm:
            r["vms"].add(vm)
        r["recv"].add(n["recv"] or "any")


# ---------------------------------------------------------------------------------------------
def build(game, prof, json_path, events_dir=None, today=None):
    with open(json_path, encoding="utf-8") as f:
        d = json.load(f)
    hf = prof.get("hash")
    mask = prof.get("hash_mask", 0xFFFFFFFF)
    roots = corpus_roots(prof["corpus"])
    cs = C.scan(roots, wait_any=prof.get("waittill_any", False))
    corpus_strings = set(cs["strings"])
    for k in list(cs["wait"]) + list(cs["notify"]):
        if k[0] == "name":
            corpus_strings.add(k[1])
    named = name_hashes(d, prof, corpus_strings)

    events = {}  # identity -> record
    fails = collections.Counter()
    fail_sites = []
    # another executable of the same game (BO6: the campaign beside multiplayer), extracted
    # with its own profile: its sites join this catalogue, each resolved against its own dump
    parts = [(None, d, prof)]
    for g, p in prof.get("merge", ()):
        with open(os.path.join(os.path.dirname(json_path), "events_%s.json" % g),
                  encoding="utf-8") as f:
            parts.append((g, json.load(f), p))
        named.update({h: v for h, v in name_hashes(parts[-1][1], p, set()).items()
                      if h not in named})
    n_sites = sum(len(x[1]["sites"]) for x in parts)
    for part, d_i, p_i in parts:
        nt_i = {n["rva"]: n for n in d_i["notifiers"]}
        tag = ("[%s] " % part) if part else ""
        for s in d_i["sites"]:
            site_event_records(s, nt_i[s["notifier"]], d_i, p_i, prof, tag, events, fails,
                               fail_sites, named)
    d["merged"] = [(g, x["exe"], x["sha256"], len(x["sites"])) for g, x, p in parts[1:]]

    # merge
    out_events = []
    for ident, r in events.items():
        counts = collections.Counter(x["count"] for x in r["sites"])
        mx = max(counts)
        typed = [x for x in r["sites"] if x["count"] == mx and x["pushes"] is not None]
        args = []
        for i in range(mx):
            ts = collections.Counter(json.dumps(arg_entry(x["pushes"][i], prof), sort_keys=True)
                                     for x in typed)
            if len(ts) == 1:
                args.append(json.loads(next(iter(ts))))
            else:
                args.append({"type": "unknown"})
        recv = r["recv"].pop() if len(r["recv"]) == 1 else "any"
        ev = {"name": r["name"], "name_source": r["name_source"], "hash": r["hash"],
              "receiver": recv, "args": args, "args_variable": len(counts) > 1,
              "sites": len(r["sites"]), "vms": sorted(r["vms"]),
              "counts": dict(counts), "typed_sites": len(typed),
              "how": collections.Counter(x["how"].split(" (")[0] for x in r["sites"])}
        out_events.append(ev)
    out_events.sort(key=lambda e: (e["name"] is None, e["name"] or "", e["hash"] or 0))

    # corpus measure
    idents = [(e["hash"] if hf else e["name"].lower()) for e in out_events
              if (e["hash"] if hf else e["name"]) is not None]
    cov = C.coverage(idents, cs, hf and (lambda x: hf(x) & mask), lower=True)
    resolved_sites = sum(len(r["sites"]) for r in events.values())
    unnamed = [e for e in out_events if not e["name"]]
    complete = (not fail_sites) and not unnamed
    cat = classify_unexplained(cov, cs, hf and (lambda x: hf(x) & mask))

    meta = {
        "game": game, "extractor": prof["extractor"],
        "dump": "%s sha256:%s" % (d["exe"], d["sha256"]),
        "date": today or datetime.date.today().isoformat(), "licence": "CC0-1.0",
    }
    coverage = {
        "engine_sites": n_sites, "named": sum(1 for e in out_events if e["name"]),
        "corpus_waittill_events": cov["waited"], "explained_by_engine": len(cov["by_engine"]),
        "explained_by_scripts": len(cov["by_scripts"]), "unexplained": len(cov["unexplained"]),
        "complete": complete,
    }
    edir = events_dir or os.path.join(paths.get("gsc_data"), "events")
    os.makedirs(edir, exist_ok=True)
    write_toml(os.path.join(edir, "%s.toml" % game), prof, d, meta, coverage, out_events,
               resolved_sites, fails)
    write_report(game, prof, d, meta, coverage, out_events, fails, fail_sites, cov, cat, cs,
                 cross_measure(prof, out_events))
    print("%s: %d sites, %d events (%d named), corpus %d waited: scripts %d, engine %d, "
          "unexplained %d; complete=%s" % (game, n_sites, len(out_events), coverage["named"],
                                            cov["waited"], len(cov["by_scripts"]),
                                            len(cov["by_engine"]), len(cov["unexplained"]),
                                            complete))


def classify_unexplained(cov, cs, hf):
    """Waited events nobody notifies, by the kind of data that most likely sends them."""
    W = C.unify(cs["wait"], hf, True)
    Lit = C.unify([k for k, v in cs["other_literals"].items() if v > 0], hf, True)
    M = C.unify(cs["match"], hf, True)
    out = collections.defaultdict(list)
    for ident in cov["unexplained"]:
        names = cov["names"].get(ident) or []
        n_match = sum(cs["match"][k] for k in M.get(ident, ()))
        n_wait = sum(cs["wait"][k] for k in W.get(ident, ()))
        if ident in Lit:
            out[OTHER_LIT].append(ident)
        elif n_match and n_match == n_wait:
            out[MATCH_ONLY].append(ident)
        elif not names:
            out[UNNAMED].append(ident)
        else:
            out[DATA].append(ident)
    return out


OTHER_LIT = ("also written as a string literal elsewhere in the scripts: most likely sent by a "
             "script `notify( var )` or named by a builtin's notify parameter (a sound's or a "
             "move's done-notify)")
MATCH_ONLY = "waited only with `waittillmatch` (animation notetrack flags)"
UNNAMED = "unnamed hash (the decompiler could not name it)"
DATA = ("written only in waittill/endon: sent by data (animation notetracks, map key/value "
        "notifies, menus) or by engine code this extraction did not reach")


def fmt_hash(h, prof):
    return "0x%0*x" % (prof.get("hash_digits", 8), h)


def write_toml(path, prof, d, meta, coverage, events, resolved_sites, fails):
    L = ["# %s engine-emitted events: every call site of the VM's notify found in the" % prof["title"],
         "# executable, its event, receiver and pushed arguments, measured under Unicorn by",
         "# gruff/%s. Facts only. Schema: ../schema/events.schema.json." % prof["extractor"],
         ""]
    L.append("[meta]")
    for k in ("game", "extractor", "dump", "date", "licence"):
        L.append("%s = %s" % (k, toml_str(meta[k])))
    if prof.get("hash_family"):
        L.append("hash_family = %s" % toml_str(prof["hash_family"]))
    L.append("")
    L.append("[coverage]")
    for k in ("engine_sites", "named", "corpus_waittill_events", "explained_by_engine",
              "explained_by_scripts", "unexplained"):
        L.append("%s = %d" % (k, coverage[k]))
    L.append("complete = %s" % ("true" if coverage["complete"] else "false"))
    L.append("sites_resolved = %d" % resolved_sites)
    L.append("sites_unresolved = %d" % sum(fails.values()))
    L.append("")
    for e in events:
        L.append("[[event]]")
        if e["name"]:
            L.append("name = %s" % toml_str(e["name"]))
            if e["name_source"]:
                L.append("name_source = %s" % toml_str(e["name_source"]))
        if e["hash"] is not None:
            L.append("hash = %s" % toml_str(fmt_hash(e["hash"], prof)))
            if not e["name"] and prof.get("hash_family"):
                L.append("hash_family = %s" % toml_str(prof["hash_family"]))
        L.append("receiver = %s" % toml_str(e["receiver"]))
        L.append("args = [%s]" % ", ".join(
            "{ %s }" % ", ".join("%s = %s" % (k, toml_str(v)) for k, v in a.items())
            for a in e["args"]))
        L.append("args_variable = %s" % ("true" if e["args_variable"] else "false"))
        L.append("sites = %d" % e["sites"])
        if e["vms"]:
            L.append("vm = [%s]" % ", ".join(toml_str(v) for v in e["vms"]))
        L.append('confidence = "measured"')
        ev = "%d site(s); argument counts %s; types from %d site(s) where count = pushes; %s" % (
            e["sites"], ", ".join("%d x%d" % kv for kv in sorted(e["counts"].items())),
            e["typed_sites"], ", ".join("%s %d" % kv for kv in e["how"].most_common()))
        L.append("evidence = %s" % toml_str(ev))
        L.append("")
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(L))


def write_report(game, prof, d, meta, coverage, events, fails, fail_sites, cov, cat, cs,
                 cross=None):
    os.makedirs(REPORTS, exist_ok=True)
    L = ["# Engine events: %s (%s)" % (prof["title"], game), ""]
    L.append("Generated by `%s build --game %s` from `out/events_%s.json`. Dump: `%s`." % (
        prof["extractor"], game, game, meta["dump"]))
    for g, exe, sha, n in d.get("merged", ()):
        L.append("")
        L.append("Merged: `out/events_%s.json` (%d sites), dump `%s sha256:%s`; its unresolved "
                 "sites are marked [%s] below." % (g, n, exe, sha, g))
    L.append("")
    if prof.get("method_note"):
        L.append("## How the notify core was found")
        L.append("")
        L.append(prof["method_note"])
        L.append("")
    L.append("## Notifiers")
    L.append("")
    L.append("| rva | role | event | count | inst | receiver |")
    L.append("|---|---|---|---|---|---|")
    for n in d["notifiers"]:
        L.append("| 0x%x | %s | %s %s | %s | %s | %s |" % (
            n["rva"], n["label"], n["event_kind"], n["event"], n["count"], n["inst"],
            n["recv"] or "argument %s" % (n["obj"],)))
    L.append("")
    L.append("## Sites")
    L.append("")
    n_sites = coverage["engine_sites"]
    nf = sum(fails.values())
    L.append("- Call sites of the notifiers (outside the wrappers): %d." % n_sites)
    L.append("- Events: %d, named %d (%s); unnamed hashes: %d." % (
        len(events), coverage["named"], pct(coverage["named"], len(events)),
        len(events) - coverage["named"]))
    src = collections.Counter(e["name_source"] for e in events if e["name"])
    L.append("- Name sources: %s." % (", ".join("%s %d" % kv for kv in src.most_common()) or "none"))
    L.append("- Unresolved site events: %d%s." % (nf, (": " + "; ".join(
        "%s x%d" % kv for kv in fails.most_common())) if nf else ""))
    L.append("- Argument types: kept where the count argument equals the pushes seen on the "
             "path; per event, a type disagreeing between sites is `unknown`.")
    L.append("- `complete` = %s (true only when every site resolved and every event is named)." %
             ("true" if coverage["complete"] else "false"))
    L.append("")
    L.append("## Corpus measure")
    L.append("")
    L.append("Scripts scanned: %d (%s). Literal events of `waittill`/`waittillmatch`: %d; "
             "literal events of `notify`: %d." % (cs["files"], ", ".join(prof["corpus"]),
                                                   cov["waited"], len(C.unify(cs["notify"], prof.get("hash") and (lambda x: prof["hash"](x) & prof.get("hash_mask", 0xFFFFFFFF)), True))))
    L.append("")
    nb = len(cov["by_engine"]) + len(cov["unexplained"])
    L.append("- Waited events some script notifies: %d (of which also engine-sent: %d)." % (
        len(cov["by_scripts"]), len(cov["both"])))
    L.append("- Waited events no script notifies: %d; explained by the engine catalogue: %d "
             "(**%s**); unexplained: %d." % (nb, len(cov["by_engine"]),
                                            pct(len(cov["by_engine"]), nb), len(cov["unexplained"])))
    data = len(cat.get(DATA, ()))
    L.append("- Leaving out the waits whose name the scripts also write elsewhere (a script "
             "`notify( var )` or a builtin's notify parameter can send them) and the "
             "`waittillmatch`-only flags: the engine explains %d of %d (**%s**); the other %d "
             "are written only in waittill/endon." % (len(cov["by_engine"]),
                                                     len(cov["by_engine"]) + data,
                                                     pct(len(cov["by_engine"]),
                                                         len(cov["by_engine"]) + data), data))
    L.append("- Engine events no script waits for (literally): %d." % len(cov["engine_not_waited"]))
    L.append("")
    L.append("### Unexplained waited events, by likely origin")
    L.append("")
    L.append("Waited for, notified by no script literal and by no engine site found. Many are "
             "sent by data rather than by a notify call in the code: animation notetracks, map "
             "key/value notifies, menu responses, or a script `notify( var )`.")
    L.append("")
    for k, idents in sorted(cat.items(), key=lambda kv: -len(kv[1])):
        L.append("- %s (%d): %s" % (k, len(idents), ", ".join(
            "`%s`" % (cov["names"].get(i) or [fmt_hash(i, prof) if isinstance(i, int) else str(i)])[0]
            for i in sorted(idents, key=lambda i: -cov["sites"].get(i, 0)))))
    L.append("")
    if cross:
        L.append("## Cross-check against another game's corpus")
        L.append("")
        if cross.get("missing"):
            L.append("%s: not on this machine." % cross["label"])
        else:
            c = cross["cov"]
            nb = len(c["by_engine"]) + len(c["unexplained"])
            L.append("This game has no script corpus: the `[coverage]` corpus counts are 0. "
                     "Measured instead against the %s (%d scripts): of its %d waited events, "
                     "%d are notified by its scripts; of the %d others, this catalogue holds %d "
                     "(**%s**). A sibling's scripts, not this game's: an indication only." % (
                         cross["label"], cross["files"], c["waited"], len(c["by_scripts"]), nb,
                         len(c["by_engine"]), pct(len(c["by_engine"]), nb)))
        L.append("")
    if fail_sites:
        L.append("## Unresolved sites")
        L.append("")
        for rva, why in fail_sites:
            L.append("- 0x%x: %s" % (rva, why))
        L.append("")
    with open(os.path.join(REPORTS, "events_%s.md" % game), "w", encoding="utf-8",
              newline="\n") as f:
        f.write("\n".join(L))
