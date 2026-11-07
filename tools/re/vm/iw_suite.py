"""Run an Infinity Ward / Sledgehammer x64 game's conformance suite (iw5...) in its emulated VM.

    py -3.11 -I tools/re/vm/iw_suite.py --game iw5 [name ...] [--jobs 1]

Header format and result classes: conformance/README.md. One emulation process at a time by
default (each holds the whole executable mapped). An UNSUPPORTED test is still run and judged
against its own expectations: when that judgement is FAIL, the reason says so (a silent drift is
worse than a refusal).
"""
import argparse
import concurrent.futures as cf
import glob
import json
import os
import re
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
RUNNER = os.path.join(HERE, "iw_run.py")
GAMES = ("iw5", "iw6", "s1", "h1", "iw7")
KEYS = ("question", "claim", "expect compile error", "expect error", "expect", "with", "helper",
        "entry", "fact", "unsupported", "evidence", "vs iw3", "vs iw5", "vs iw6", "vs s1",
        "vs h1")
HEADER = re.compile(r"^//\s*(" + "|".join(KEYS) + r"):\s?(.*)$")


def header(path):
    h = {"question": "", "claim": [], "expect": [], "expect error": None, "with": [],
         "helper": False, "entry": None, "expect compile error": None, "fact": [],
         "unsupported": None, "evidence": []}
    with open(path, encoding="latin-1") as f:
        for line in f:
            m = HEADER.match(line.rstrip("\n"))
            if not m:
                continue
            k, v = m.groups()
            if k.startswith("vs "):
                h["vs"] = (k[3:], v.strip())
            elif isinstance(h[k], list):
                h[k].append(v.rstrip() if k == "expect" else v.strip())
            elif k == "helper":
                h[k] = True
            else:
                h[k] = v.strip()
    return h


def judge(h, out):
    """(result, reason) for one run's output."""
    trace = [ln for ln in out["lines"] if not ln.startswith("error: ")]
    if h["expect compile error"] and h["expect"]:
        # a refused script runs nothing: a behaviour whose form the compiler refuses is
        # `// unsupported:` with the message (conformance/README.md), never a golden trace
        return "FAIL", "header: expect compile error with expect lines"
    if out["status"] == "compile_error":
        want = h["expect compile error"]
        if want and want in out["error"]:
            return "PASS", None
        if want:
            return "FAIL", "compile error differs"
        return "FAIL", "the game's compiler rejects it: %s" % out["error"]
    if h["expect compile error"]:
        return "FAIL", "expected a compile error, the game's compiler accepted it"
    if trace != h["expect"]:
        return "FAIL", "trace differs"
    if h["expect error"]:
        if not out.get("error") or h["expect error"] not in out["error"]:
            return "FAIL", "error differs"
    elif out.get("error"):
        return "FAIL", "unexpected error"
    return "PASS", None


def run_one(game, suite, path):
    h = header(path)
    name = os.path.splitext(os.path.basename(path))[0]
    res = {"name": name, "question": h["question"], "claims": h["claim"], "facts": h["fact"],
           "evidence": h["evidence"], "expect": h["expect"], "expect_error": h["expect error"],
           "expect_compile_error": h["expect compile error"],
           "vs": h.get("vs")}
    missing = [w for w in h["with"] if not os.path.exists(os.path.join(suite, w))]
    if missing:
        res.update(result="UNSUPPORTED", reason="helper missing: %s" % ", ".join(missing))
        return res
    cmd = [sys.executable, "-I", RUNNER, "--game", game, path]
    cmd += [os.path.join(suite, w) for w in h["with"]] + ["--json"]
    t0 = time.time()
    cp = subprocess.run(cmd, capture_output=True, stdin=subprocess.DEVNULL)
    res["elapsed_s"] = round(time.time() - t0, 2)
    try:
        out = json.loads(cp.stdout.decode("utf-8", "replace"))
    except ValueError:
        res.update(result="FAIL", reason="runner crashed: %s"
                   % cp.stderr.decode("utf-8", "replace")[-800:])
        return res
    res.update(trace=[ln for ln in out["lines"] if not ln.startswith("error: ")],
               error=out.get("error"), status=out.get("status"),
               stubs=sorted(out.get("stubs", {})), frames=out.get("frames"))
    res["result"], why = judge(h, out)
    res["judged"] = res["result"]
    if h["unsupported"]:  # run and recorded, not judged: the behaviour cannot exist here
        why = h["unsupported"] + ("" if res["judged"] == "PASS" else
                                  " [run differs from its own expectation: %s]" % why)
        res["result"] = "UNSUPPORTED"
    if why:
        res["reason"] = why
    return res


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--game", choices=GAMES, required=True)
    ap.add_argument("names", nargs="*")
    ap.add_argument("--jobs", type=int, default=1)
    ap.add_argument("--out")
    a = ap.parse_args()
    suite = os.path.join(ROOT, "conformance", a.game)
    paths = [p for p in sorted(glob.glob(os.path.join(suite, "*.gsc"))) if not header(p)["helper"]]
    if a.names:
        paths = [p for p in paths if os.path.splitext(os.path.basename(p))[0] in a.names]
    with cf.ThreadPoolExecutor(max_workers=max(1, min(a.jobs, 2))) as ex:
        results = list(ex.map(lambda p: run_one(a.game, suite, p), paths))
    counts = {}
    for r in results:
        counts[r["result"]] = counts.get(r["result"], 0) + 1
        why = ("  (%s)" % r["reason"]) if r.get("reason") else ""
        print("%-12s %s%s" % (r["result"], r["name"], why))
        if r.get("judged") == "FAIL" or r["result"] == "FAIL":
            for ln in r.get("trace", []):
                print("      got: " + ln)
            for ln in r["expect"]:
                print("   expect: " + ln)
            if r.get("error"):
                print("    error: " + r["error"])
    print("STATUS %s: " % a.game
          + ", ".join("%s %d" % kv for kv in sorted(counts.items())))
    out = a.out or os.path.join(os.path.dirname(HERE), "out", "vm_%s_results%s.json"
                                % (a.game, "_subset" if a.names else ""))
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "w") as f:
        json.dump(results, f, indent=1)
    return 0 if counts.get("FAIL", 0) == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
