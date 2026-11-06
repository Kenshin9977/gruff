"""Run a GSC test script in Black Ops 4's (t8) or Cold War's (t9) own script VM, offline, and print
its trace.

    py -3.11 -I tools/re/vm/t89_run.py --game t8 test.gsc [helper.gsc ...] [--frames N] [--json]

Neither game has an official mod tools linker: the scripts are compiled by acts (`acts gscc -g
bo4|cw`, located by GSC_ACTS, `acts` in paths.local.json or PATH; output cached by content under
tools/re/out/vm_gscc_<game>/, never committed). Each is registered as the ScriptParseTree
`scripts/zz_harness/<file>` for the hooked asset lookup, linked by the game's own Scr_GscLink
(includes found through the same lookup), and its entry function (`main`, or `// entry: name`)
started with Scr_ExecThread, self = level. Frames are then stepped as G_RunFrame does (levelTime
+= 50 ms) until nothing has happened for --idle frames or --frames is reached.

Trace lines: `f<frame> T<n> <text>` for every iprintln() call (its arguments joined; logprint
does not exist in T8/T9), `f` being scrVmPub.frame and T0 the entry thread, T1, T2... the other
threads in creation order. Runtime script errors appear as `f<frame> T<n> script error[
(terminal)]: <message>`; a load failure or a fatal error ends the trace with `error: <message>`.
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
RE = os.path.dirname(HERE)
sys.path.insert(0, RE)
sys.path.insert(0, HERE)
from common import paths  # noqa: E402
from t89vm import T89VM, Fault, PROFILES  # noqa: E402

ACTS_GAME = {"t8": "bo4", "t9": "cw"}
LOG_BUILTIN = "iprintln"
# engine side of the trace, per game: Com_Printf (stripped in these builds: a no-op that the
# loader and linker still call with their messages) and Sys_Error(code, details) (fatal). Runtime
# error messages are 32-bit codes whose texts live in a string table asset (0x289aa20 decodes
# them through the asset database), not in the executable: the trace shows the code
# Com_Error(file, line, code, fmt, ...): the runtime error reporter (0x27758b0) always ends in it,
# code 0x20 (script error) for a non-terminal error; what Com_Error then does depends on a
# session byte (0x8a57023, not identified): set, it reports and returns, and ScrVm_Error longjmps
# back into VM_Execute's recovery (the thread goes on); clear (its value in the dump), it aborts
# to the main loop. Both are run: --script-errors stop (the default) or continue.
ENGINE = {
    "t8": {"printf": 0x288E770, "sys_error": 0x3D34E70, "com_error": 0x2889710},
    "t9": {"printf": 0x56B5E40, "sys_error": 0xCB27920, "com_error": 0x56463A0},
}
ERR_SCRIPT = 0x20


def acts_exe():
    a = os.environ.get("GSC_ACTS") or paths.get("acts") or shutil.which("acts")
    if not a or not os.path.isfile(a):
        raise SystemExit("acts not found: set GSC_ACTS or `acts` in tools/re/paths.local.json")
    return a


def namespace_of(src, path):
    m = re.search(r"^\s*#namespace\s+(\w+)\s*;", src, re.M)
    return m.group(1) if m else os.path.splitext(os.path.basename(path))[0]


def virtual_path(path):
    return "scripts/zz_harness/" + os.path.basename(path)


def acts_flags(path):
    """Extra acts gscc options of a test: `// acts-flags: --no-devcall-inline`."""
    with open(path, encoding="latin-1") as fh:
        m = re.search(r"^//\s*acts-flags:\s*(.*)$", fh.read(), re.M)
    return m.group(1).split() if m else []


def compile_scripts(game, files):
    """({"accepted", "errors"}, {virtual path: gscc bytes}), each file compiled by acts alone
    (with the main file's `// acts-flags:`)."""
    flags = acts_flags(files[0])
    h = hashlib.sha1(game.encode() + " ".join(flags).encode())
    for f in files:
        h.update(os.path.basename(f).encode() + b"\0")
        with open(f, "rb") as fh:
            h.update(fh.read())
    out = os.path.join(RE, "out", "vm_gscc_%s" % game, h.hexdigest()[:16])
    result_file = os.path.join(out, "compile.json")
    if not os.path.exists(result_file):
        acts = acts_exe()
        os.makedirs(out, exist_ok=True)
        res = {"accepted": True, "errors": [],
               "compiler": " ".join(["acts gscc -g", ACTS_GAME[game]] + flags)}
        for f in files:
            stem = os.path.splitext(os.path.basename(f))[0]
            cmd = [acts, "-t", "--noUpdater", "gscc", "-g", ACTS_GAME[game]] + flags + [
                "-o", os.path.join(out, stem), "--name", virtual_path(f), os.path.abspath(f)]
            cp = subprocess.run(cmd, capture_output=True, stdin=subprocess.DEVNULL, timeout=120,
                                cwd=out)
            text = (cp.stdout + cp.stderr).decode("utf-8", "replace")
            if not os.path.exists(os.path.join(out, stem + ".gscc")):
                res["accepted"] = False
                res["errors"] += ["%s: %s" % (os.path.basename(f), ln.strip())
                                  for ln in text.splitlines() if ln.strip() and "Done into" not in ln]
        with open(result_file, "w") as fh:
            json.dump(res, fh, indent=1)
    with open(result_file) as fh:
        res = json.load(fh)
    if not res["accepted"]:
        return res, {}
    scripts = {}
    for f in files:
        stem = os.path.splitext(os.path.basename(f))[0]
        with open(os.path.join(out, stem + ".gscc"), "rb") as fh:
            scripts[virtual_path(f)] = fh.read()
    return res, scripts


class Run:
    def __init__(self, game, exe, verbose=False, script_errors="stop"):
        self.game = game
        self.script_errors = script_errors
        self.vm = vm = T89VM(game, exe, verbose=verbose)
        E = ENGINE[game]
        self.messages = []
        vm.hook(E["printf"], Run._printf.__get__(self), "Com_Printf",
                "stripped (a no-op) in this build: the message is recorded")
        vm.hook(E["sys_error"], Run._sys_error.__get__(self), "Sys_Error",
                "fatal: the run stops with its code and details")
        vm.hook(E["com_error"], Run._com_error.__get__(self), "Com_Error",
                "script errors (code 0x20): recorded, then the run stops, or with --script-errors "
                "continue Com_Error returns at once; any other code stops the run")
        vm.boot()
        vm.track_threads()
        h = vm.builtin_handler(LOG_BUILTIN)
        if h is None:
            raise SystemExit("%s: no gsc builtin %s" % (game, LOG_BUILTIN))
        vm.trace_builtin(h, LOG_BUILTIN)
        vm.trace_errors()

    def _printf(self, vm):
        fmt = vm.cstr(vm.emu.arg(0), 4096) or ""
        text = vm.format(fmt, lambda k: vm.emu.arg(1 + k)).strip()
        if text:
            self.messages.append((vm.now(), text))
        return 0

    def _com_error(self, vm):
        code = vm.emu.arg(2) & 0xFFFFFFFF
        fmt = vm.cstr(vm.emu.arg(3), 4096) or ""
        text = vm.format(fmt, lambda k: vm.emu.arg(4 + k)).strip()
        self.messages.append((vm.now(), "Com_Error %#x: %s" % (code, text)))
        if code == ERR_SCRIPT and self.script_errors == "continue":
            return 0
        vm.stop_reason = "Com_Error %#x%s" % (code, ": script error" if code == ERR_SCRIPT else
                                             (": " + text if text else ""))
        vm.uc.emu_stop()
        return 0

    def _sys_error(self, vm):
        code = vm.emu.arg(0) & 0xFFFFFFFF
        details = vm.cstr(vm.emu.arg(1), 4096) or ""
        vm.stop_reason = "Sys_Error %#010x%s" % (code, (" " + details) if details else "")
        vm.uc.emu_stop()
        return 0


def run(game, files, entry=None, frames=200, idle=20, verbose=False, exe=None,
        script_errors="stop"):
    main_file = files[0]
    with open(main_file, encoding="latin-1") as fh:
        src = fh.read()
    m = re.search(r"^//\s*entry:\s*(\w+)", src, re.M)
    entry = entry or (m.group(1) if m else "main")
    comp, scripts = compile_scripts(game, files)
    out = {"game": game, "status": None, "lines": [], "error": None, "compile": comp}
    if not scripts:
        out.update(status="compile_error", error="compile: %s" % " | ".join(comp["errors"]))
        out["lines"].append("error: " + out["error"])
        return out
    r = Run(game, exe or paths.need("exe." + game), verbose=verbose, script_errors=script_errors)
    out["script_errors"] = script_errors
    vm = r.vm
    for name, data in scripts.items():
        vm.add_script(name, data)
    t0 = time.time()
    status = "ok"
    try:
        vm.begin_load()
        vm.link(virtual_path(main_file))
        vm.end_load()
        if not vm.loaded_objects():
            raise Fault("nothing loaded: %s" % "; ".join(t for _, t in r.messages))
        handle = vm.function_handle(virtual_path(main_file), namespace_of(src, main_file), entry)
        if not handle:
            raise Fault("no export %s::%s in the loaded object" % (namespace_of(src, main_file),
                                                                     entry))
        level = vm.global_id("level")
        vm.exec_thread(handle, level)
        last = 0
        n0 = len(vm.trace) + len(vm.errors)
        while vm.frames_run < frames:
            vm.run_frame()
            n = len(vm.trace) + len(vm.errors)
            if n != n0:
                last, n0 = vm.frames_run, n
            elif vm.frames_run - last >= idle:
                break
    except Fault as f:
        status = "error"
        out["error"] = str(f).split("\n")[0]
    out["elapsed_s"] = round(time.time() - t0, 3)
    events = [(k, "print", f, tid, args) for k, (f, tid, _, args) in enumerate(vm.trace)]
    events += [(k - 0.5, "error", f, tid, (code, term)) for f, tid, code, term, k in vm.errors]
    for _, kind, f, tid, args in sorted(events, key=lambda e: e[0]):
        if kind == "print":
            out["lines"].append("f%d %s %s" % (f, tid, "".join(args)))
        else:
            code, term = args
            out["lines"].append("f%d %s script error%s: %s" % (
                f, tid, " (terminal)" if term else "", "#%08x" % code))
    out["vm_messages"] = ["f%d %s" % m for m in r.messages]
    if out["error"]:
        out["lines"].append("error: " + out["error"])
    out.update(status=status, frames=vm.frames_run, stubs=vm.stubs,
               unmapped=["%#x->%#x" % (a, b) for a, b in vm.unmapped],
               threads=vm.thread_count, level_time_ms=vm.level_time)
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--game", choices=sorted(PROFILES), required=True)
    ap.add_argument("files", nargs="+", help="the test script, then any helper scripts")
    ap.add_argument("--entry")
    ap.add_argument("--frames", type=int, default=200)
    ap.add_argument("--idle", type=int, default=20, help="stop after this many quiet frames")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--verbose", action="store_true")
    ap.add_argument("--script-errors", choices=("stop", "continue"), default="stop",
                    help="what Com_Error does with a script error (see ENGINE)")
    a = ap.parse_args()
    res = run(a.game, a.files, a.entry, a.frames, a.idle, a.verbose,
              script_errors=a.script_errors)
    if a.json:
        json.dump(res, sys.stdout, indent=1)
        print()
        return
    for line in res["lines"]:
        print(line)
    if a.verbose:
        for m in res.get("vm_messages", []):
            print("# vm: " + m)
        print("# stubs: " + ", ".join(sorted(res.get("stubs", {}))))


if __name__ == "__main__":
    main()
