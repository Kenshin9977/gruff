"""Write gruff-data/semantics/<game>.toml (t8 or t9) from the suite's two runs.

    py -3.11 -I tools/re/vm/t89_suite.py --game t8
    py -3.11 -I tools/re/vm/t89_suite.py --game t8 --script-errors continue
    py -3.11 -I tools/re/vm/t89_facts.py --game t8 [--date YYYY-MM-DD]

One [[behaviour]] per test, as t7_facts.py, plus `dialect` (vs t7: the test's `// vs t7:` line,
`same` or the T7 answer) and the continue-mode trace as `alt_trace` when it differs. Facts only
(CC0): our own scripts' traces and the engine's error codes, no game code.
"""
import argparse
import datetime
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
RE = os.path.dirname(HERE)
sys.path.insert(0, RE)
sys.path.insert(0, HERE)
from common import paths  # noqa: E402
from common.report import toml_value  # noqa: E402
import t89_suite  # noqa: E402

CLAIM = re.compile(r"^([a-z0-9-]+(?:#\S+)?) \((agrees|disagrees)\): (.*)$")
VS = re.compile(r"^//\s*vs (t\d+):\s*(.*)$", re.M)
TITLE = {"t8": "Black Ops 4", "t9": "Black Ops Cold War"}


def load(path):
    with open(path) as f:
        return {r["name"]: r for r in json.load(f)}


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--game", choices=("t8", "t9"), required=True)
    ap.add_argument("--date", default=datetime.date.today().isoformat())
    ap.add_argument("--out")
    a = ap.parse_args()
    g = a.game
    out = a.out or os.path.join(paths.get("gsc_data") or "", "semantics", "%s.toml" % g)
    out_dir = os.path.join(RE, "out")
    stop = load(os.path.join(out_dir, "vm_%s_results.json" % g))
    cont = load(os.path.join(out_dir, "vm_%s_results_continue.json" % g))
    exe = paths.need("exe." + g)
    head = ("# %s script VM semantics, measured by running this project's own conformance scripts\n"
            "# (gruff/conformance/%s, compiled by acts: the game has no official mod tools linker) in\n"
            "# the game's own VM, emulated offline from the executable by gruff/tools/re/vm (Unicorn\n"
            "# over the dump; nothing launched, the game cannot be run). Facts only.\n"
            "# Schema: ../schema/semantics.schema.json. Report: gruff/tools/re/reports/vm_%s.md.\n"
            % (TITLE[g], g, g))
    lines = [head, "[meta]"]
    meta = {"game": g, "extractor": "tools/re/vm/t89_suite.py --game %s" % g,
            "dump": os.path.basename(exe),
            "date": a.date, "licence": "CC0-1.0", "suite": "conformance/%s" % g,
            "time_unit": "scrVmPub.frame, one server frame (levelTime += 50 ms per frame)",
            "modes": ["script errors stop (Com_Error 0x20 aborts: the dump's session state)",
                      "script errors continue (Com_Error returns: the VM's own recovery runs)"]}
    lines += ["%s = %s" % (k, toml_value(v)) for k, v in meta.items()]
    counts = {}
    for name in sorted(stop):
        r = stop[name]
        c = cont.get(name, {})
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
        same = (c.get("trace"), c.get("error")) == (r.get("trace"), r.get("error"))
        if c and not same:
            b["alt_trace"] = c.get("trace", [])[:8]
            if c.get("error"):
                b["alt_error"] = c["error"]
        b["modes_agree"] = bool(c) and same
        with open(os.path.join(t89_suite.suite_dir(g), name + ".gsc"), encoding="latin-1") as f:
            m = VS.search(f.read())
        if m:
            note = m.group(2).strip()
            d = {"vs": m.group(1), "same": note == "same"}
            if note != "same":
                d["note"] = note
            b["dialect"] = d
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
            if k == "dialect":
                continue
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
    print("%s: %d behaviours %s" % (out, len(stop), counts))


if __name__ == "__main__":
    main()
