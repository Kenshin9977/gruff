"""Write gruff-data/semantics/<game>.toml (t6 or t5) from the suite's two runs (retail and developer).

    py -3.11 -I tools/re/vm/t56_suite.py --game t6
    py -3.11 -I tools/re/vm/t56_suite.py --game t6 --developer
    py -3.11 -I tools/re/vm/t56_facts.py --game t6 [--date YYYY-MM-DD]

One [[behaviour]] per test, as t7_facts.py, plus `dialect` against the next game (the test's
`// vs t7:` line for t6, `// vs t6:` for t5). Facts only (CC0): our own scripts' traces and the
engine's short error messages, no game code.
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
from common.events_build import sha256  # noqa: E402
from common.report import toml_value  # noqa: E402
import t56_suite  # noqa: E402
from t56vm import PROFILES  # noqa: E402

CLAIM = re.compile(r"^([a-z0-9-]+(?:#\S+)?) \((agrees|disagrees)\): (.*)$")
VS = re.compile(r"^//\s*vs (t\d+):\s*(.*)$", re.M)
COMPILER = {"t6": "compiled by gsc-tool: the game has no official mod tools compiler",
            "t5": "compiled at load time by the game's own compiler"}


def load(path):
    with open(path) as f:
        return {r["name"]: r for r in json.load(f)}


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--game", choices=sorted(PROFILES), required=True)
    ap.add_argument("--date", default=datetime.date.today().isoformat())
    ap.add_argument("--out")
    a = ap.parse_args()
    g = a.game
    P = PROFILES[g]
    out = a.out or os.path.join(paths.get("gsc_data") or "", "semantics", "%s.toml" % g)
    out_dir = os.path.join(RE, "out")
    retail = load(os.path.join(out_dir, "vm_%s_results.json" % g))
    dev = load(os.path.join(out_dir, "vm_%s_results_dev.json" % g))
    exe = paths.need(P["exe_key"])
    head = ("# %s script VM semantics, measured by running this project's own conformance scripts\n"
            "# (gruff/conformance/%s, %s) in the game's own VM,\n"
            "# emulated offline from the executable by gruff/tools/re/vm (Unicorn; nothing launched).\n"
            "# Facts only. Schema: ../schema/semantics.schema.json. Report: gruff/tools/re/reports/vm_%s.md.\n"
            % (P["title"], g, COMPILER[g], g))
    lines = [head, "[meta]"]
    meta = {"game": g, "extractor": "tools/re/vm/t56_suite.py --game %s" % g,
            "dump": "%s sha256:%s" % (os.path.basename(exe), sha256(exe)),
            "date": a.date, "licence": "CC0-1.0", "suite": "conformance/%s" % g,
            "time_unit": "gScrVarPub.time, one 50 ms server frame",
            "modes": ["retail (developer 0)", "developer 1"]}
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
        refused = res == "unsupported" and (r.get("expect_compile_error") or r.get("expect_error"))
        if r.get("judged") == "FAIL":
            raise SystemExit("%s: its refusal differs from the header: review it" % name)
        if r.get("facts") and not refused:
            b["answer"] = " ".join(r["facts"])
        b["result"] = res
        if r.get("reason"):
            b["reason"] = r["reason"]
        if r.get("expect_compile_error"):
            b["compile_error"] = r["expect_compile_error"]
        elif refused:
            # refused at load by the linker: the message only, no trace (nothing ran)
            b["error"] = r["expect_error"]
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
        with open(os.path.join(t56_suite.suite_dir(g), name + ".gsc"), encoding="latin-1") as f:
            m = VS.search(f.read())
        dialect = None
        if m:
            note = m.group(2).strip()
            # "same", "same: <in other words>", "same answer; <a different mechanism>"
            same = note == "same" or note.startswith(("same:", "same answer"))
            dialect = {"vs": m.group(1), "same": same}
            if note != "same":
                dialect["note"] = note
        if r.get("evidence"):
            b["evidence"] = r["evidence"]
        claims = []
        for c in r.get("claims", []):
            mm = CLAIM.match(c)
            if not mm:
                raise SystemExit("%s: claim without (agrees|disagrees): %s" % (name, c))
            claims.append({"source": mm.group(1), "agrees": mm.group(2) == "agrees",
                           "claim": mm.group(3)})
        lines.append("")
        lines.append("[[behaviour]]")
        lines += ["%s = %s" % (k, toml_value(v)) for k, v in b.items()]
        if dialect:
            lines.append("dialect = %s" % toml_value(dialect))
        if claims:
            lines.append("claims = [")
            lines += ["  %s," % toml_value(c) for c in claims]
            lines.append("]")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines) + "\n")
    print("%s: %d behaviours %s" % (out, len(retail), counts))


if __name__ == "__main__":
    main()
