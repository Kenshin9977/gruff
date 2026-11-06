"""Run a GSC test script in World at War's (t4) or Call of Duty 4's (iw3) own script VM, offline,
and print its trace.

    py -3.11 -I tools/re/vm/old_run.py --game t4 test.gsc [helper.gsc ...] [--frames N] [--json]

Both games compile plain-text GSC at load with their own compiler: the script is handed to the
game's Scr_LoadScript as `zz/<file>` (the file system read is the only stub on that path;
`#include zz\\other;` loads a helper the same way), parsed and compiled by the
game's code, and its entry function (`main`, or `// entry: name`) started with Scr_ExecThread,
self = level. Frames are then stepped as G_RunFrame does (the runner, then the frame's tail with
time += 1) until nothing has happened for --idle frames or --frames is reached.

Trace lines: `f<time> T<n> <text>` for every call of the trace builtin (t4: iprintln, iw3:
logprint; its arguments joined), `f` being scrVarPub.time (one 50 ms server frame per unit) and
T0 the entry thread, T1, T2... the other threads in creation order. Runtime script errors appear as
`f<time> T<n> script error[ (terminal)]: <message>`; a compile error, a load failure or a
Com_Error ends the trace with `error: <message>`.
"""
import argparse
import json
import os
import re
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
RE = os.path.dirname(HERE)
sys.path.insert(0, RE)
sys.path.insert(0, HERE)
from common import paths  # noqa: E402
from vmcore32 import Fault  # noqa: E402

# the compiler formats "<name>.gsc" into a 64-byte buffer (Scr_LoadScriptInternal 0x689980): a
# short folder keeps the longest test name inside it (maps/zz_harness/ did not: "Scr_LoadScript
# returned 0" for notify_wakes_waiter_before_notifier_continues)
DIR = "zz/"


def profile(game):
    if game == "t4":
        import t4vm
        return t4vm.OldVM, t4vm.T4
    if game == "iw3":
        import iw3vm
        return iw3vm.IW3VM, iw3vm.IW3
    raise SystemExit("unknown game %s" % game)


def script_name(path):
    return DIR + os.path.splitext(os.path.basename(path))[0]


def run(game, files, entry=None, frames=200, idle=20, verbose=False, exe=None, developer=False):
    main_file = files[0]
    with open(main_file, encoding="latin-1") as fh:
        src = fh.read()
    m = re.search(r"^//\s*entry:\s*(\w+)", src, re.M)
    entry = entry or (m.group(1) if m else "main")
    cls, prof = profile(game)
    out = {"game": game, "status": None, "lines": [], "error": None, "developer": developer}
    vm = cls(exe or paths.need("exe." + game), prof, verbose=verbose)
    vm.boot(developer=developer)
    vm.track_threads()
    label, handler = prof["log_builtin"]
    vm.trace_builtin(handler, label)
    for name, va in prof.get("extra_trace", {}).items():
        vm.trace_builtin(va, name)
    vm.trace_errors()
    for f in files:
        with open(f, "rb") as fh:
            vm.add_script(script_name(f) + ".gsc", fh.read())
    t0 = time.time()
    status = "ok"
    phase = "load"
    n_load_errors = 0
    try:
        ok = vm.load(script_name(main_file))
        n_load_errors = len(vm.errors)
        if not ok:
            raise Fault("Scr_LoadScript returned 0 for %s" % script_name(main_file))
        handle = vm.function_handle(script_name(main_file), entry)
        if not handle:
            raise Fault("no function handle for %s::%s" % (script_name(main_file), entry))
        vm.end_load()
        phase = "run"
        vm.exec_thread(handle)
        last = 0
        n0 = len(vm.trace) + len(vm.errors)
        while vm.frame < frames:
            vm.run_frame()
            n = len(vm.trace) + len(vm.errors)
            if n != n0:
                last, n0 = vm.frame, n
            elif vm.frame - last >= idle:
                break
    except Fault as f:
        status = "error"
        msg = str(f)
        if msg.startswith(("Com_Error", "Sys_Error")):
            msg = " | ".join(x.strip() for x in msg.replace("\x15", "").split("\n") if x.strip())
            if "compile error" in msg and phase == "load":
                status = "compile_error"
        else:
            msg = msg.split("\n")[0]
        out["error"] = msg
    if phase == "load":
        n_load_errors = len(vm.errors)
    out["elapsed_s"] = round(time.time() - t0, 3)
    # errors the compiler raised while folding constants are part of the compile error, not
    # of the run
    errors = vm.errors[n_load_errors:]
    out["compile_messages"] = [e[2] for e in vm.errors[:n_load_errors]]
    events = [(k, "print", f, tid, args if lb == label else [lb + ": "] + args)
              for k, (f, tid, lb, args) in enumerate(vm.trace)]
    events += [(k - 0.5, "error", f, tid, (msg, term)) for f, tid, msg, term, k in errors]
    for _, kind, f, tid, args in sorted(events, key=lambda e: e[0]):
        if kind == "print":
            out["lines"].append("f%d %s %s" % (f, tid, "".join(args)))
        else:
            msg, term = args
            out["lines"].append("f%d %s script error%s: %s" % (
                f, tid, " (terminal)" if term else "", msg))
    out["vm_messages"] = ["f%d %s" % (f, t.strip()) for f, ch, t in vm.prints if t.strip()]
    if out["error"]:
        out["lines"].append("error: " + out["error"])
    out.update(status=status, frames=vm.frame, stubs=vm.stubs,
               unmapped=["%#x->%#x" % (a, b) for a, b in vm.unmapped],
               null_writes=["%#x->%#x x%d" % (ip, a, n) for ip, (a, n) in vm.null_writes.items()],
               threads=vm.thread_count, files_read=vm.file_requests, boot=vm.boot_log)
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--game", choices=("t4", "iw3"), required=True)
    ap.add_argument("files", nargs="+", help="the test script, then any helper scripts")
    ap.add_argument("--entry")
    ap.add_argument("--frames", type=int, default=200)
    ap.add_argument("--idle", type=int, default=20, help="stop after this many quiet frames")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--verbose", action="store_true")
    ap.add_argument("--developer", action="store_true",
                    help="developer and developer_script on (Scr_Settings); retail is off")
    a = ap.parse_args()
    res = run(a.game, a.files, a.entry, a.frames, a.idle, a.verbose, developer=a.developer)
    if a.json:
        json.dump(res, sys.stdout, indent=1)
        print()
        return
    for line in res["lines"]:
        print(line)
    if a.verbose:
        for m in res.get("vm_messages", []):
            print("# vm: " + m)
        print("# null-page writes: " + ", ".join(res.get("null_writes", [])))
        print("# stubs: " + ", ".join("%s x%d" % (k, v["hits"]) for k, v in
                                      sorted(res.get("stubs", {}).items())))


if __name__ == "__main__":
    main()
