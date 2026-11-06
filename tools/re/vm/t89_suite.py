"""Run the T8 or T9 conformance suite (every .gsc of conformance/<game>) in the emulated game VM.

    py -3.11 -I tools/re/vm/t89_suite.py --game t8 [name ...] [--jobs 2]

Header format and result classes: conformance/README.md (the same as T7's; the compiler is acts,
so `expect compile error` is acts' message). Results: tools/re/out/vm_<game>_results.json.
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
RUNNER = os.path.join(HERE, "t89_run.py")
sys.path.insert(0, HERE)
from t7_suite import header  # noqa: E402  (same header lines as T7)


def suite_dir(game):
    return os.path.join(ROOT, "conformance", game)


def files_of(game, path, h):
    return [path] + [os.path.join(suite_dir(game), w) for w in h["with"]]


def judge(h, out):
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
        return "UNSUPPORTED", "acts rejects it: %s" % out["error"]
    if h["expect compile error"]:
        return "FAIL", "expected a compile error, acts accepted it"
    if trace != h["expect"]:
        return "FAIL", "trace differs"
    if h["expect error"]:
        if not out.get("error") or h["expect error"] not in out["error"]:
            return "FAIL", "error differs"
    elif out.get("error"):
        return "FAIL", "unexpected error"
    return "PASS", None


def run_one(game, path, script_errors="stop"):
    h = header(path)
    name = os.path.splitext(os.path.basename(path))[0]
    res = {"name": name, "question": h["question"], "claims": h["claim"], "facts": h["fact"],
           "evidence": h["evidence"], "expect": h["expect"], "expect_error": h["expect error"],
           "expect_compile_error": h["expect compile error"], "script_errors": script_errors}
    missing = [w for w in h["with"] if not os.path.exists(os.path.join(suite_dir(game), w))]
    if missing:
        res.update(result="UNSUPPORTED", reason="helper missing: %s" % ", ".join(missing))
        return res
    cmd = [sys.executable, "-I", RUNNER, "--game", game] + files_of(game, path, h) + ["--json"]
    if h["entry"]:
        cmd += ["--entry", h["entry"]]
    cmd += ["--script-errors", script_errors]
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
    if h["unsupported"]:
        res["result"], why = "UNSUPPORTED", h["unsupported"]
    if why:
        res["reason"] = why
    return res


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--game", choices=("t8", "t9"), required=True)
    ap.add_argument("names", nargs="*")
    ap.add_argument("--jobs", type=int, default=2, help="parallel runs (each maps a 0.3-0.5 GB dump)")
    ap.add_argument("--script-errors", choices=("stop", "continue"), default="stop",
                    help="Com_Error on a script error: stop the run (the dump's session state) or "
                         "return (the VM's own recovery then runs)")
    ap.add_argument("--out")
    a = ap.parse_args()
    paths = [p for p in sorted(glob.glob(os.path.join(suite_dir(a.game), "*.gsc")))
             if not header(p)["helper"]]
    if a.names:
        paths = [p for p in paths if os.path.splitext(os.path.basename(p))[0] in a.names]
    # compile everything first, one acts at a time
    import t89_run
    for p in paths:
        h = header(p)
        fs = files_of(a.game, p, h)
        if all(os.path.exists(f) for f in fs):
            t89_run.compile_scripts(a.game, fs)
    with cf.ThreadPoolExecutor(max_workers=max(1, min(a.jobs, 2))) as ex:
        results = list(ex.map(lambda p: run_one(a.game, p, a.script_errors), paths))
    counts = {}
    for r in results:
        counts[r["result"]] = counts.get(r["result"], 0) + 1
        why = ("  (%s)" % r["reason"]) if r.get("reason") else ""
        print("%-12s %s%s" % (r["result"], r["name"], why))
        if r["result"] == "FAIL":
            for ln in r.get("trace", []):
                print("      got: " + ln)
            for ln in r["expect"]:
                print("   expect: " + ln)
            if r.get("error"):
                print("    error: " + r["error"])
    print("STATUS %s%s: " % (a.game, " (script errors continue)" if a.script_errors == "continue"
                             else "") + ", ".join("%s %d" % kv for kv in sorted(counts.items())))
    out = a.out or os.path.join(os.path.dirname(HERE), "out", "vm_%s_results%s%s.json"
                                % (a.game, "_continue" if a.script_errors == "continue" else "",
                                   "_subset" if a.names else ""))
    with open(out, "w") as f:
        json.dump(results, f, indent=1)
    return 0 if counts.get("FAIL", 0) == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
