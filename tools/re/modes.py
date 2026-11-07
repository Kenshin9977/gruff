"""Which game modes (sp, mp, zm) have each builtin, from one extraction per executable; run after
any build step, like calibrate.py:

    py -3.11 -I modes.py <game>... [--iw-out DIR] [--dry-run]     (from tools/re)

A game's catalogue is built from its primary executable, with the entries only a sibling
executable has merged in (T5: BlackOpsMP.exe; T6: t6zm, t6sp; IW5/S1/H1: the SP executable). This
pass reads each executable's own extraction again and writes, per catalogue:

- `modes` in [extraction]: every executable read, by the modes that run it (T4/T5: the campaign
  executable also runs zombies; S1: the multiplayer executable runs Exo Zombies too). A mode
  absent from it was not read (an encrypted executable): tools then cannot narrow its files.
- `modes` on an entry: the modes whose executables hold it (its (name, VM, kind) in T4-T6, its
  (kind, id) in the IW registration builds), only when that is not all of them;
- `mode_args` on an entry whose executables store different arities (T6: the table's min/max,
  which the linker checks): each mode's [min, max]. min_args/max_args stay the catalogue's (the
  primary executable's, widened by calibrate.py), for scripts of no known mode;
- `[coverage]` again (calibrate.set_coverage): `complete_modes` names the modes whose scripts can
  call no builtin outside the catalogue.

Sources: out/<id>_extract.json (treyarch/old_builtins.py extract; `--no-handlers` is enough) and
iw/out/<id>.json (iw/extract.py; `--limit 1` is enough: only the tables are read). The pass is
idempotent and never changes another key. Report: reports/modes.md.
"""
import argparse
import collections
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from calibrate import DATA, split_blocks, get, drop, set_coverage  # noqa: E402
from common.report import toml_value as enc  # noqa: E402

REPORT = os.path.join(HERE, "reports", "modes.md")
ORDER = ("sp", "mp", "zm")
# game: [(extraction id, executable, the modes it runs)], primary first; `unread`: why a mode
# has no executable here
GAMES = {
    "t4": {"family": "treyarch", "exes": [("t4", "CoDWaW LanFixed.exe", ["sp", "zm"])],
           "unread": "CoDWaWmp.exe is SteamStub-encrypted (.text entropy 8.0)"},
    "t5": {"family": "treyarch", "exes": [("t5", "BlackOps.exe", ["sp", "zm"]),
                                          ("t5mp", "BlackOpsMP.exe", ["mp"])]},
    "t6": {"family": "treyarch", "exes": [("t6", "t6mp.exe", ["mp"]), ("t6zm", "t6zm.exe", ["zm"]),
                                          ("t6sp", "t6sp.exe", ["sp"])]},
    "iw1": {"family": "iw", "exes": [("iw1", "gamex86.dll", ["sp"])],
            "unread": "the multiplayer game module was not extracted"},
    "iw3": {"family": "iw", "exes": [("iw3", "iw3mp.exe", ["mp"])],
            "unread": "iw3sp.exe is SteamStub-encrypted"},
    "iw4": {"family": "iw", "exes": [("iw4", "iw4mp.exe", ["mp"])],
            "unread": "iw4sp.exe is readable, but IW4 entries have neither a name nor an id: "
                      "its entries cannot be keyed to the multiplayer ones"},
    "iw5": {"family": "iw", "exes": [("iw5", "iw5mp.exe", ["mp"]), ("iw5_sp", "iw5sp.exe", ["sp"])]},
    "iw6": {"family": "iw", "exes": [("iw6", "iw6sp64_ship.exe", ["sp"])],
            "unread": "iw6mp64_ship.exe (multiplayer and Extinction) is encrypted"},
    "s1": {"family": "iw", "exes": [("s1", "s1_mp64_ship.exe", ["mp", "zm"]),
                                    ("s1_sp", "s1_sp64_ship.exe", ["sp"])]},
    "h1": {"family": "iw", "exes": [("h1", "h1_mp64_ship.exe", ["mp"]),
                                    ("h1_sp", "h1_sp64_ship.exe", ["sp"])]},
}


def treyarch_keys(xid):
    """(name, vm, kind) of every entry of the four lookups, shadowed ones included (the name is
    still found, through the earlier table), with the table's (min, max) where the layout stores
    one (T6; the first table holding the name, as the lookup returns it)."""
    with open(os.path.join(HERE, "out", "%s_extract.json" % xid), encoding="utf-8") as f:
        ex = json.load(f)
    out = {}
    for role, ro in ex["roles"].items():
        vm, kind = role.split("_")
        for t in ro["tables"]:
            for e in t["entries"]:
                k = (e["name"].lower(), vm, kind)
                if k in out:
                    continue
                mn, mx = e.get("min"), e.get("max")
                out[k] = (mn, mx) if mn is not None and mn != 0xFFFFFFFF else None
    return out, ex["pe_timestamp"]


def iw_keys(xid, iw_out):
    """(kind, id) of every registered entry (IW5+ registration builds), or (kind, name) for the
    builds that keep names (IW1, IW3)."""
    with open(os.path.join(iw_out, "%s.json" % xid), encoding="utf-8") as f:
        doc = json.load(f)
    out = {}
    for e in doc["entries"]:
        kind = e.get("kind") or "function"
        if e.get("id") is not None:
            out[(kind, "id", e["id"])] = None
        elif e.get("name"):
            out[(kind, "name", e["name"].lower())] = None
    return out, doc["pe_timestamp"]


def entry_key(family, b):
    if family == "treyarch":
        return (get(b, "name"), get(b, "vm"), get(b, "kind"))
    if get(b, "id") is not None:
        return (get(b, "kind"), "id", get(b, "id"))
    return (get(b, "kind"), "name", get(b, "name"))


def annotate(game, iw_out, rep):
    cfg = GAMES[game]
    path = os.path.join(DATA, game + ".toml")
    text = open(path, encoding="utf-8").read()
    blocks = split_blocks(text)
    head = blocks[0]
    read = {}  # mode -> executable
    for _, exe, modes in cfg["exes"]:
        for m in modes:
            read[m] = exe
    covered = [m for m in ORDER if m in read]
    # header: modes after the last executable line of [extraction]
    drop(head, "modes")
    i = head.index("[extraction]") + 1
    last = i
    for j in range(i, len(head)):
        if head[j].startswith("[") and j > i:
            break
        if head[j].split(" = ")[0] in ("executable", "pe_timestamp", "executable_sp",
                                       "pe_timestamp_sp", "merge"):
            last = j
    head.insert(last + 1, "modes = %s" % enc({m: read[m] for m in covered}))
    sets = {}
    if len(cfg["exes"]) > 1:
        for xid, exe, modes in cfg["exes"]:
            keys, ts = (treyarch_keys(xid) if cfg["family"] == "treyarch" else iw_keys(xid, iw_out))
            sets[xid] = (keys, modes)
            rep.setdefault("exes", []).append((xid, exe, modes, len(keys), ts))
    per = collections.Counter()
    missing = []
    for b in blocks[1:]:
        if b[0] != "[[builtin]]":
            continue
        drop(b, "modes")
        drop(b, "mode_args")
        if not sets:
            continue
        k = entry_key(cfg["family"], b)
        have = sorted({m for keys, modes in sets.values() if k in keys for m in modes},
                      key=ORDER.index)
        if not have:
            missing.append(k)
            continue
        # the IW merge's own record (`executables`) must agree
        exes = get(b, "executables")
        if exes is not None:
            want = sorted({m for xid, _, modes in cfg["exes"] if
                           ("sp" if xid.endswith("_sp") or game == "iw6" else "mp") in exes
                           for m in modes}, key=ORDER.index)
            if want != have:
                rep.setdefault("disagree", []).append((k, exes, have))
        per[tuple(have)] += 1
        at = next(j for j, ln in enumerate(b) if ln.startswith("evidence = "))
        if have != covered:
            b.insert(at, "modes = %s" % enc(have))
            at += 1
        # each mode's stored arity, when the executables disagree
        ar = {m: keys[k] for keys, modes in sets.values() if k in keys for m in modes}
        if None not in ar.values() and len(set(ar.values())) > 1:
            b.insert(at, "mode_args = %s" % enc({m: list(ar[m]) for m in ORDER if m in ar}))
            rep["mode_args"] = rep.get("mode_args", 0) + 1
    set_coverage(game, blocks)
    rep["covered"] = covered
    rep["unread"] = cfg.get("unread")
    rep["per"] = per
    rep["missing"] = missing
    return path, text, "\n".join("\n".join(b) for b in blocks)


def write_report(reps):
    L = ["# Builtins per game mode (tools/re/modes.py)", "",
         "Each executable's extraction read again; an entry's `modes` are the modes whose "
         "executables hold it (absent: all the modes read). Generated file.", "",
         "| game | modes read | not read | executables (entries) | entries per modes | mode_args |",
         "|---|---|---|---|---|---|"]
    for r in reps:
        exes = "; ".join("%s %s: %d" % (e[1], "/".join(e[2]), e[3]) for e in r.get("exes", [])) \
            or "one executable"
        per = ", ".join("%s %d" % ("+".join(k), v) for k, v in
                        sorted(r["per"].items(), key=lambda kv: (-len(kv[0]), kv[0])))
        L.append("| %s | %s | %s | %s | %s | %s |" % (
            r["game"], ", ".join(r["covered"]), r["unread"] or "", exes, per or "n/a",
            r.get("mode_args", "")))
    for r in reps:
        if r["missing"] or r.get("disagree"):
            L += ["", "## %s" % r["game"], ""]
            if r["missing"]:
                L.append("Entries no executable's extraction holds (left without `modes`): %d, "
                         "e.g. %s" % (len(r["missing"]), r["missing"][:5]))
            if r.get("disagree"):
                L.append("Entries whose `executables` disagree with the extractions: %d, e.g. %s"
                         % (len(r["disagree"]), r["disagree"][:5]))
    os.makedirs(os.path.dirname(REPORT), exist_ok=True)
    with open(REPORT, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(L) + "\n")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("games", nargs="+", choices=sorted(GAMES))
    ap.add_argument("--iw-out", default=os.path.join(HERE, "iw", "out"))
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()
    reps = []
    for game in a.games:
        rep = {"game": game}
        path, old, new = annotate(game, a.iw_out, rep)
        if not a.dry_run and new != old:
            open(path, "w", encoding="utf-8", newline="\n").write(new)
        print(game, "read:", ",".join(rep["covered"]), "per modes:", dict(rep["per"]),
              "mode_args:", rep.get("mode_args", 0), "missing:", len(rep["missing"]),
              "disagree:", len(rep.get("disagree", [])))
        reps.append(rep)
    if not a.dry_run:
        write_report(reps)


if __name__ == "__main__":
    main()
