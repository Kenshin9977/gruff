"""The second measure of an event catalogue: the script corpus. Counts only, no content kept.

For every script of a game: the event names given as a literal to `waittill` / `waittillmatch`
(first argument; CSC's optional leading `localclientnum` skipped) and to `notify`. Decompiled
Treyarch corpora write hashed literals as `#"name"` (name known) or `#"hash_<hex>"` (unknown):
both are kept, the caller unifies them by hash. A waited event that no script notifies has to be
sent by the engine; the share of those the engine catalogue holds is the coverage.
"""
import collections
import os
import re

COMMENT = re.compile(r"//[^\n]*|/\*.*?\*/|/@.*?@/", re.S)
LIT = r'(#?"(?:[^"\\\n]|\\.)*"|&"(?:[^"\\\n]|\\.)*")'
WAIT = re.compile(r"\bwaittill(match)?\s*\(\s*(?:[A-Za-z_]\w*\s*,\s*)?" + LIT)
ENDON = re.compile(r"\bendon\s*\(\s*(?:[A-Za-z_]\w*\s*,\s*)?" + LIT)
ANYLIT = re.compile(LIT)
WAITALL = re.compile(r"\bwaittill\s*\(([^()]*)\)")
NOTIFY = re.compile(r"\bnotify\s*\(\s*(?:[A-Za-z_]\w*\s*,\s*)?" + LIT)
HASHLIT = re.compile(r'^#"hash_([0-9a-fA-F]{1,16})"$')


def literal_key(tok):
    """('name', text) or ('hash', int) for a literal token."""
    m = HASHLIT.match(tok)
    if m:
        return ("hash", int(m.group(1), 16))
    s = tok.lstrip("#&")[1:-1]
    return ("name", s)


def scan(roots, exts=("gsc", "csc", "gsh"), wait_any=False):
    """{'wait': Counter(key), 'notify': Counter(key), 'files': n, 'strings': set(all string
    literals, for naming hashes)} over every script under `roots`."""
    wait, notify = collections.Counter(), collections.Counter()
    wait_vm = collections.defaultdict(set)
    strings = set()
    lits = collections.Counter()        # literal uses outside waittill / endon
    match_only = collections.Counter()  # waits that are waittillmatch (the anim-flag form)
    files = 0
    for root in roots:
        for dp, dn, fn in os.walk(root):
            for f in fn:
                ext = f.rsplit(".", 1)[-1].lower()
                if ext not in exts:
                    continue
                files += 1
                with open(os.path.join(dp, f), "rb") as fh:
                    src = COMMENT.sub(" ", fh.read().decode("latin1"))
                for m in WAIT.finditer(src):
                    k = literal_key(m.group(2))
                    wait[k] += 1
                    wait_vm[k].add(ext)
                    if m.group(1):
                        match_only[k] += 1
                    lits[k] -= 1
                if wait_any:  # T8+: every literal of waittill( a, b, ... ) is an event
                    for m in WAITALL.finditer(src):
                        for x in list(ANYLIT.finditer(m.group(1)))[1:]:
                            k = literal_key(x.group(1))
                            wait[k] += 1
                            wait_vm[k].add(ext)
                            lits[k] -= 1
                for m in ENDON.finditer(src):
                    lits[literal_key(m.group(1))] -= 1
                for m in NOTIFY.finditer(src):
                    notify[literal_key(m.group(1))] += 1
                for m in ANYLIT.finditer(src):
                    lits[literal_key(m.group(1))] += 1
                for m in re.finditer(r'"([A-Za-z0-9_ .\-]{1,80})"', src):
                    strings.add(m.group(1))
    return {"wait": wait, "notify": notify, "wait_vm": wait_vm, "files": files,
            "strings": strings, "other_literals": lits, "match": match_only}


def unify(keys, hash_fn, lower=True):
    """Map ('name', s) / ('hash', h) keys to one identity: the hash for hashed engines
    (hash_fn given), the name (lower-cased when the engine compares case-insensitively)
    otherwise. Returns {identity: set(original keys)}."""
    out = collections.defaultdict(set)
    for k in keys:
        if hash_fn is not None:
            ident = k[1] if k[0] == "hash" else hash_fn(k[1])
        else:
            if k[0] == "hash":
                ident = ("hash", k[1])
            else:
                ident = k[1].lower() if lower else k[1]
        out[ident].add(k)
    return out


def coverage(engine_idents, scan_res, hash_fn, lower=True):
    """The corpus measure: waited events split into script-notified, engine-only and
    unexplained. Returns a dict with the counts and the lists."""
    W = unify(scan_res["wait"], hash_fn, lower)
    N = unify(scan_res["notify"], hash_fn, lower)
    eng = set(engine_idents)
    by_scripts, by_engine, unexplained, both = [], [], [], []
    for ident in W:
        if ident in N:
            by_scripts.append(ident)
            if ident in eng:
                both.append(ident)
        elif ident in eng:
            by_engine.append(ident)
        else:
            unexplained.append(ident)
    sites = {ident: sum(scan_res["wait"][k] for k in ks) for ident, ks in W.items()}
    return {"waited": len(W), "by_scripts": by_scripts, "by_engine": by_engine,
            "unexplained": unexplained, "both": both, "sites": sites,
            "names": {ident: sorted(k[1] for k in ks if k[0] == "name") for ident, ks in W.items()},
            "engine_not_waited": sorted((e for e in eng if e not in W), key=str)}
