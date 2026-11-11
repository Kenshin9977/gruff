"""Write gruff-data/semantics/t7.toml from the conformance suite's two runs (retail and developer).

    py -3.11 -I tools/re/vm/t7_suite.py && py -3.11 -I tools/re/vm/t7_suite.py --developer
    py -3.11 -I tools/re/vm/t7_facts.py [--date YYYY-MM-DD]

One [[behaviour]] per test: the question, the answer (the test's `// fact:`), the golden trace of
the retail run, the developer run when it differs, the claims of documents and tools with
whether the measure agrees, and the static evidence. Facts only (CC0): our own scripts' traces
and the engine's short error messages, no game code.
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
import t7_suite  # noqa: E402

CLAIM = re.compile(r"^([a-z0-9-]+(?:#\S+)?) \((agrees|disagrees)\): (.*)$")
HEAD = """# Black Ops III script VM semantics, measured by running this project's own conformance
# scripts (gruff/conformance/t7) in the game's own VM, emulated offline from the executable
# by gruff/tools/re/vm (Unicorn over the dump; nothing launched). Facts only.
# Schema: ../schema/semantics.schema.json. Report: gruff/tools/re/reports/vm_t7.md.
"""


def load(path):
    with open(path) as f:
        return {r["name"]: r for r in json.load(f)}


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--date", default=datetime.date.today().isoformat())
    ap.add_argument("--out", default=os.path.join(paths.get("gsc_data") or "", "semantics",
                                                  "t7.toml"))
    a = ap.parse_args()
    out_dir = os.path.join(RE, "out")
    retail = load(os.path.join(out_dir, "vm_t7_results.json"))
    dev = load(os.path.join(out_dir, "vm_t7_results_dev.json"))
    exe = paths.need("exe.t7")
    lines = [HEAD, "[meta]"]
    meta = {"game": "t7", "extractor": "tools/re/vm/t7_suite.py",
            "dump": os.path.basename(exe),
            "date": a.date, "licence": "CC0-1.0", "suite": "conformance/t7",
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
        claims = []
        for c in r.get("claims", []):
            m = CLAIM.match(c)
            if not m:
                raise SystemExit("%s: claim without (agrees|disagrees): %s" % (name, c))
            claims.append({"source": m.group(1), "agrees": m.group(2) == "agrees",
                           "claim": m.group(3)})
        if r.get("evidence"):
            b["evidence"] = r["evidence"]
        lines.append("")
        lines.append("[[behaviour]]")
        lines += ["%s = %s" % (k, toml_value(v)) for k, v in b.items()]
        if claims:
            lines.append("claims = [")
            lines += ["  %s," % toml_value(c) for c in claims]
            lines.append("]")
    os.makedirs(os.path.dirname(a.out), exist_ok=True)
    with open(a.out, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines) + "\n")
    print("%s: %d behaviours %s" % (a.out, len(retail), counts))


if __name__ == "__main__":
    main()
