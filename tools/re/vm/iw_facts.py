"""Write gruff-data/semantics/<game>.toml (iw5, iw6, s1, h1, iw7) from the suite's run.

    py -3.11 -I tools/re/vm/iw_suite.py --game iw5
    py -3.11 -I tools/re/vm/iw_facts.py --game iw5 [--date YYYY-MM-DD]

One [[behaviour]] per test, as old_facts.py, plus `dialect` (the test's `// vs <game>:` line:
`same` or the other game's answer). One mode only: these builds have no developer switch that
the measured behaviours depend on (the loop guard reads the script debugger's flag), so
`modes_agree` is true by construction and meta.modes says so. An UNSUPPORTED test keeps its
reason and, when the compiler (gsc-tool) refused it, the compile error. Facts only (CC0): our own
scripts' traces, no game code.
"""
import argparse
import datetime
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
RE = os.path.dirname(HERE)
ROOT = os.path.dirname(os.path.dirname(RE))
sys.path.insert(0, RE)
sys.path.insert(0, HERE)
from common import paths  # noqa: E402
from common.events_build import sha256  # noqa: E402
from common.report import toml_value  # noqa: E402

CLAIM = re.compile(r"^([a-z0-9-]+(?:#\S+)?) \((agrees|disagrees)\): (.*)$")
VS = re.compile(r"^//\s*vs (t\d+|iw\d+|s\d+|h\d+):\s*(.*)$", re.M)
TITLE = {"iw5": "Modern Warfare 3", "iw6": "Ghosts", "s1": "Advanced Warfare",
         "h1": "Modern Warfare Remastered", "iw7": "Infinite Warfare"}
EXE_NOTE = {"iw5": "the multiplayer executable (the x64 re-release)",
            "iw6": "the single-player executable dump", "s1": "the multiplayer executable dump",
            "h1": "the multiplayer executable dump", "iw7": "the executable dump"}


def load(path):
    with open(path) as f:
        return {r["name"]: r for r in json.load(f)}


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--game", choices=sorted(TITLE), required=True)
    ap.add_argument("--date", default=datetime.date.today().isoformat())
    ap.add_argument("--out")
    a = ap.parse_args()
    g = a.game
    out = a.out or os.path.join(paths.get("gsc_data") or "", "semantics", "%s.toml" % g)
    retail = load(os.path.join(RE, "out", "vm_%s_results.json" % g))
    exe = paths.need("exe." + g)
    head = ("# %s script VM semantics, measured by running this project's own conformance scripts\n"
            "# (gruff/conformance/%s) in the game's own VM, emulated offline from %s\n"
            "# by gruff/tools/re/vm (Unicorn over the executable; nothing launched). The game\n"
            "# ships no compiler (its scripts are bytecode compiled offline): gsc-tool compiles\n"
            "# each script and the game's own loader links it. Facts only.\n"
            "# Schema: ../schema/semantics.schema.json. Report: gruff/tools/re/reports/vm_iw.md.\n"
            % (TITLE[g], g, EXE_NOTE[g]))
    lines = [head, "[meta]"]
    meta = {"game": g, "extractor": "tools/re/vm/iw_suite.py --game %s" % g,
            "dump": "%s sha256:%s" % (os.path.basename(exe), sha256(exe)),
            "date": a.date, "licence": "CC0-1.0", "suite": "conformance/%s" % g,
            "time_unit": "scrVarPub.time counted from the entry thread's start, one 50 ms "
                         "server frame",
            "modes": ["retail (script debugger off); one mode only: no developer switch "
                      "changes these behaviours, so modes_agree is true by construction"]}
    lines += ["%s = %s" % (k, toml_value(v)) for k, v in meta.items()]
    counts = {}
    for name in sorted(retail):
        r = retail[name]
        res = r["result"].lower()
        if res == "fail":
            raise SystemExit("%s FAILs against its golden: review it before writing facts" % name)
        counts[res] = counts.get(res, 0) + 1
        b = {"id": name, "question": r["question"]}
        if r.get("facts"):
            b["answer"] = " ".join(r["facts"])
        b["result"] = res
        if r.get("reason"):
            b["reason"] = r["reason"]
        if r.get("expect_compile_error"):
            b["compile_error"] = r["expect_compile_error"]
        else:
            b["trace"] = r.get("trace", [])
            if r.get("error"):
                b["error"] = r["error"]
        b["modes_agree"] = True
        with open(os.path.join(ROOT, "conformance", g, name + ".gsc"), encoding="latin-1") as f:
            m = VS.search(f.read())
        if m:
            note = m.group(2).strip()
            same_ans = note == "same" or note.startswith(("same (", "same:", "same order"))
            dd = {"vs": m.group(1), "same": same_ans}
            if note != "same":
                dd["note"] = note
            b["dialect"] = dd
        if r.get("evidence"):
            b["evidence"] = r["evidence"]
        claims = []
        for cl in r.get("claims", []):
            mm = CLAIM.match(cl)
            if not mm:
                raise SystemExit("%s: claim without (agrees|disagrees): %s" % (name, cl))
            claims.append({"source": mm.group(1), "agrees": mm.group(2) == "agrees",
                           "claim": mm.group(3)})
        lines.append("")
        lines.append("[[behaviour]]")
        for k, v in b.items():
            if k != "dialect":
                lines.append("%s = %s" % (k, toml_value(v)))
        if "dialect" in b:
            lines.append("dialect = %s" % toml_value(b["dialect"]))
        if claims:
            lines.append("claims = [")
            lines += ["  %s," % toml_value(cl) for cl in claims]
            lines.append("]")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines) + "\n")
    print("%s: %d behaviours %s" % (out, len(retail), counts))


if __name__ == "__main__":
    main()
