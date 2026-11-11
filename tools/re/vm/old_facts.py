"""Write gruff-data/semantics/<game>.toml (t4 or iw3) from the suite's two runs (retail, developer).

    py -3.11 -I tools/re/vm/old_suite.py --game t4
    py -3.11 -I tools/re/vm/old_suite.py --game t4 --developer
    py -3.11 -I tools/re/vm/old_facts.py --game t4 [--date YYYY-MM-DD]

One [[behaviour]] per test, as t7_facts.py, plus `dialect` (the test's `// vs t7:` or `// vs t4:`
line: `same` or the other game's answer). An UNSUPPORTED test keeps its reason and, when the game's
compiler refused it, the compile error. Facts only (CC0): our own scripts' traces and the engine's
short messages, no game code.
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
from common.report import toml_value  # noqa: E402

CLAIM = re.compile(r"^([a-z0-9-]+(?:#\S+)?) \((agrees|disagrees)\): (.*)$")
VS = re.compile(r"^//\s*vs (t\d+|iw\d+):\s*(.*)$", re.M)
TITLE = {"t4": "World at War", "iw3": "Call of Duty 4"}
EXE_NOTE = {"t4": "the unwrapped single-player executable", "iw3": "the multiplayer executable"}


def load(path):
    with open(path) as f:
        return {r["name"]: r for r in json.load(f)}


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--game", choices=("t4", "iw3"), required=True)
    ap.add_argument("--date", default=datetime.date.today().isoformat())
    ap.add_argument("--out")
    a = ap.parse_args()
    g = a.game
    out = a.out or os.path.join(paths.get("gsc_data") or "", "semantics", "%s.toml" % g)
    out_dir = os.path.join(RE, "out")
    retail = load(os.path.join(out_dir, "vm_%s_results.json" % g))
    dev = load(os.path.join(out_dir, "vm_%s_results_dev.json" % g))
    exe = paths.need("exe." + g)
    head = ("# %s script VM semantics, measured by running this project's own conformance scripts\n"
            "# (gruff/conformance/%s) in the game's own VM, emulated offline from %s\n"
            "# by gruff/tools/re/vm (Unicorn over the executable; nothing launched). The game's own\n"
            "# compiler compiles each script at load. Facts only.\n"
            "# Schema: ../schema/semantics.schema.json. Report: gruff/tools/re/reports/vm_%s.md.\n"
            % (TITLE[g], g, EXE_NOTE[g], g))
    lines = [head, "[meta]"]
    meta = {"game": g, "extractor": "tools/re/vm/old_suite.py --game %s" % g,
            "dump": os.path.basename(exe),
            "date": a.date, "licence": "CC0-1.0", "suite": "conformance/%s" % g,
            "time_unit": "scrVarPub.time, one 50 ms server frame",
            "modes": ["retail (developer 0, developer_script 0)",
                      "developer 1, developer_script 1 (abort_on_error follows developer)"]}
    lines += ["%s = %s" % (k, toml_value(v)) for k, v in meta.items()]
    counts = {}
    for name in sorted(retail):
        r = retail[name]
        d = dev.get(name, {})
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
        same = (d.get("trace"), d.get("error")) == (r.get("trace"), r.get("error"))
        if d and not same:
            b["developer_trace"] = d.get("trace", [])[:8]
            if d.get("error"):
                b["developer_error"] = d["error"]
        b["modes_agree"] = bool(d) and same
        with open(os.path.join(ROOT, "conformance", g, name + ".gsc"), encoding="latin-1") as f:
            m = VS.search(f.read())
        if m:
            note = m.group(2).strip()
            # "same" alone, or "same (...)" / "same: ..." with a note on the mechanism
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
