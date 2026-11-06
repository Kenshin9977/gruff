"""Run the T6 or T5 conformance suite (every .gsc of conformance/<game>) in the emulated game VM.

    py -3.11 -I tools/re/vm/t56_suite.py --game t6 [name ...] [--developer] [--jobs 1]

Header format and result classes: conformance/README.md (T6: the compiler is gsc-tool, so
`expect compile error` is gsc-tool's message; T5: the game's own compiler, whose errors are load
errors). Results: tools/re/out/vm_<game>_results[_dev].json.
"""
import argparse
import concurrent.futures as cf
import glob
import json
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
RUNNER = os.path.join(HERE, "t56_run.py")
sys.path.insert(0, HERE)
from t7_suite import header  # noqa: E402  (same header lines as T7)
from t89_suite import judge  # noqa: E402  (same verdicts)
from t56vm import PROFILES  # noqa: E402


def suite_dir(game):
    return os.path.join(ROOT, "conformance", game)


def files_of(game, path, h):
    return [path] + [os.path.join(suite_dir(game), w) for w in h["with"]]


def run_one(game, path, developer=False):
    h = header(path)
    name = os.path.splitext(os.path.basename(path))[0]
    res = {"name": name, "question": h["question"], "claims": h["claim"], "facts": h["fact"],
           "evidence": h["evidence"], "expect": h["expect"], "expect_error": h["expect error"],
           "expect_compile_error": h["expect compile error"], "developer": developer}
    missing = [w for w in h["with"] if not os.path.exists(os.path.join(suite_dir(game), w))]
    if missing:
        res.update(result="UNSUPPORTED", reason="helper missing: %s" % ", ".join(missing))
        return res
    cmd = [sys.executable, "-I", RUNNER, "--game", game] + files_of(game, path, h) + ["--json"]
    if h["entry"]:
        cmd += ["--entry", h["entry"]]
    if developer:
        cmd.append("--developer")
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
               stubs=sorted(out.get("stubs", {})), frames=out.get("frames"),
               vm_messages=out.get("vm_messages", [])[:8])
    res["result"], why = judge(h, out)
    if why and why.startswith("acts rejects"):
        why = why.replace("acts rejects", "the compiler rejects")
    res["judged"] = res["result"]
    # run and recorded, not judged: the behaviour cannot exist here (the compiler or the linker
    # refuses its form: the header still names the refusal), or the harness cannot vouch for it
    if h["unsupported"]:
        res["result"], why = "UNSUPPORTED", h["unsupported"]
    if why:
        res["reason"] = why
    return res


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--game", choices=sorted(PROFILES), required=True)
    ap.add_argument("names", nargs="*")
    ap.add_argument("--jobs", type=int, default=1,
                    help="parallel runs (default 1: memory is shared with other work)")
    ap.add_argument("--developer", action="store_true",
                    help="Scr_Settings developer = 1 (retail is 0)")
    ap.add_argument("--out")
    a = ap.parse_args()
    paths = [p for p in sorted(glob.glob(os.path.join(suite_dir(a.game), "*.gsc")))
             if not header(p)["helper"]]
    if a.names:
        paths = [p for p in paths if os.path.splitext(os.path.basename(p))[0] in a.names]
    import t56_run
    for p in paths:  # compile everything first, one compiler at a time
        h = header(p)
        fs = files_of(a.game, p, h)
        if all(os.path.exists(f) for f in fs):
            t56_run.compile_scripts(a.game, fs)
    with cf.ThreadPoolExecutor(max_workers=max(1, min(a.jobs, 2))) as ex:
        results = list(ex.map(lambda p: run_one(a.game, p, a.developer), paths))
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
    print("STATUS %s%s: " % (a.game, " (developer 1)" if a.developer else "")
          + ", ".join("%s %d" % kv for kv in sorted(counts.items())))
    out = a.out or os.path.join(os.path.dirname(HERE), "out", "vm_%s_results%s%s.json"
                                % (a.game, "_dev" if a.developer else "",
                                   "_subset" if a.names else ""))
    with open(out, "w") as f:
        json.dump(results, f, indent=1)
    return 0 if counts.get("FAIL", 0) == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
