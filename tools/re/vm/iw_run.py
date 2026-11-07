"""Run a GSC test script in an Infinity Ward / Sledgehammer x64 game's own script VM (iw5...),
offline, and print its trace.

    py -3.11 -I tools/re/vm/iw_run.py --game iw5 test.gsc [helper.gsc ...] [--frames N] [--json]

These games load compiled ScriptFile assets (bytecode compiled offline; no compiler ships with
them), so the scripts are compiled by gsc-tool (`gsc-tool -m comp -g <game> -s <system>`, helpers
first: gsc-tool reads an #include'd file's compiled output), cached in
`tools/re/out/vm_gscc_<game>/`, and handed to the game's loader as ScriptFile assets named
`zz/<file>`; the loader links them (far calls, includes) and the entry `main` is started with
Scr_ExecThread, self = level. Frames are then stepped as G_RunFrame does until nothing has
happened for --idle frames or --frames is reached.

Trace lines as old_run.py: `f<time> T<n> <text>` for each logprint (the arguments joined),
`f<time> T<n> script error: opcode 0x..` for a script error (these builds keep no error text: the
opcode that raised it), and `error: <message>` for a load failure or a Com_Error.
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
RE = os.path.dirname(HERE)
sys.path.insert(0, RE)
sys.path.insert(0, HERE)
from common import paths  # noqa: E402
from vmcore import Fault  # noqa: E402
from t56_run import gsctool_exe  # noqa: E402

FOLDER = "zz"
SYSTEM = {"iw5": "pc64", "iw6": "pc", "s1": "pc", "h1": "pc", "iw7": "pc"}
# the token id of `main` in each game's canonical string table (gsc-tool's token tables;
# GScr_LoadScripts passes the same id to Scr_GetFunctionHandle)
MAIN_ID = {"iw5": 0x672, "iw6": 0x4FDD, "s1": 0x26B, "h1": 0x267, "iw7": 0x1CB}


def script_name(path):
    return FOLDER + "/" + os.path.splitext(os.path.basename(path))[0]


def compile_scripts(game, files):
    """({"accepted", "errors", "compiler"}, {script name: gscbin bytes}): helpers first, each
    compiled in a work folder that holds the helpers' outputs (gsc-tool reads an include's
    compiled file)."""
    system = SYSTEM[game]
    order = list(files[1:]) + [files[0]]
    h = hashlib.sha1((game + system).encode())
    for f in order:
        h.update(os.path.basename(f).encode() + b"\0")
        with open(f, "rb") as fh:
            h.update(fh.read())
    out = os.path.join(RE, "out", "vm_gscc_%s" % game, h.hexdigest()[:16])
    result_file = os.path.join(out, "compile.json")
    if not os.path.exists(result_file):
        tool = gsctool_exe()
        os.makedirs(out, exist_ok=True)
        res = {"accepted": True, "errors": [],
               "compiler": "gsc-tool -m comp -g %s -s %s" % (game, system)}
        with tempfile.TemporaryDirectory(dir=out) as tmp:
            os.makedirs(os.path.join(tmp, FOLDER))
            for f in order:
                src = os.path.join(tmp, "src")
                shutil.rmtree(src, ignore_errors=True)
                os.makedirs(os.path.join(src, FOLDER))
                shutil.copy(f, os.path.join(src, FOLDER))
                cmd = [tool, "-m", "comp", "-g", game, "-s", system, src]
                cp = subprocess.run(cmd, capture_output=True, stdin=subprocess.DEVNULL,
                                    timeout=120, cwd=tmp)
                text = (cp.stdout + cp.stderr).decode("utf-8", "replace")
                base = os.path.splitext(os.path.basename(f))[0] + ".gscbin"
                obj = os.path.join(tmp, "compiled", game, FOLDER, base)
                if os.path.exists(obj) and "[ERROR]" not in text:
                    shutil.copy(obj, os.path.join(out, base))
                    shutil.copy(obj, os.path.join(tmp, FOLDER, base))
                else:
                    res["accepted"] = False
                    res["errors"] += [re.sub(r"\S*[\\/]" + FOLDER + r"[\\/]", "", ln.strip())
                                      for ln in text.splitlines()
                                      if ln.strip() and "created by xensik" not in ln
                                      and not ln.startswith("compiled ")]
                    break
        with open(result_file, "w") as fh:
            json.dump(res, fh, indent=1)
    with open(result_file) as fh:
        res = json.load(fh)
    if not res["accepted"]:
        return res, {}
    scripts = {}
    for f in files:
        base = os.path.splitext(os.path.basename(f))[0] + ".gscbin"
        with open(os.path.join(out, base), "rb") as fh:
            scripts[script_name(f)] = fh.read()
    return res, scripts


def run(game, files, frames=200, idle=20, verbose=False, exe=None):
    import iwvm
    out = {"game": game, "status": None, "lines": [], "error": None}
    comp, scripts = compile_scripts(game, files)
    out["compiler"] = comp["compiler"]
    if not comp["accepted"]:
        out.update(status="compile_error", error="; ".join(comp["errors"]),
                   compile_messages=comp["errors"], frames=0)
        out["lines"].append("error: " + out["error"])
        return out
    vm = iwvm.IWVM(game, exe or paths.need("exe." + game), verbose=verbose)
    vm.boot()
    vm.track_threads()
    label, idx = vm.P["log_builtin"]
    vm.trace_builtin(label, idx)
    vm.trace_errors()
    for name, data in scripts.items():
        vm.add_script(name, data)
    t0 = time.time()
    status = "ok"
    phase = "load"
    main = script_name(files[0])
    try:
        ok = vm.load(main)
        if not ok:
            raise Fault("Scr_LoadScript returned 0 for %s" % main)
        handle = vm.function_handle(main, MAIN_ID[game])
        if not handle:
            raise Fault("no function handle for %s::main" % main)
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
        if msg.startswith("Com_Error"):
            msg = " | ".join(x.strip() for x in msg.split("\n") if x.strip())
            if phase == "load":
                status = "link_error"
        else:
            msg = msg.split("\n")[0] if not verbose else msg
        out["error"] = msg
    out["elapsed_s"] = round(time.time() - t0, 3)
    events = [(k, "print", f, tid, args if lb == label else [lb + ": "] + args)
              for k, (f, tid, lb, args) in enumerate(vm.trace)]
    events += [(k - 0.5, "error", f, tid, (msg, term)) for f, tid, msg, term, k in vm.errors]
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
               threads=vm.thread_count, assets=vm.asset_requests, boot=vm.boot_log)
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--game", choices=sorted(SYSTEM), required=True)
    ap.add_argument("files", nargs="+", help="the test script, then any helper scripts")
    ap.add_argument("--frames", type=int, default=200)
    ap.add_argument("--idle", type=int, default=20, help="stop after this many quiet frames")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--verbose", action="store_true")
    a = ap.parse_args()
    res = run(a.game, a.files, a.frames, a.idle, a.verbose)
    if a.json:
        json.dump(res, sys.stdout, indent=1)
        print()
        return
    for line in res["lines"]:
        print(line)
    if a.verbose:
        for m in res.get("vm_messages", []):
            print("# vm: " + m)
        print("# assets: %s" % res.get("assets"))
        print("# unmapped: %s" % res.get("unmapped"))
        print("# stubs: " + ", ".join("%s x%d" % (k, v["hits"]) for k, v in
                                      sorted(res.get("stubs", {}).items())))


if __name__ == "__main__":
    main()
