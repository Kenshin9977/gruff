"""Run a GSC test script in Black Ops II's (t6) or Black Ops' (t5) own script VM, offline, and
print its trace.

    py -3.11 -I tools/re/vm/t56_run.py --game t6 test.gsc [helper.gsc ...] [--developer] [--json]

T6 loads compiled bytecode: the scripts are compiled by gsc-tool (xensik, `gsc-tool -m comp -g t6
-s pc32`; GSC_TOOL, `gsctool` in paths.local.json, or gsc-tool.exe beside `gsctool_src`), output
cached by content under tools/re/out/vm_gscc_t6/ (never committed); BO2 has no official mod tools
compiler. T5 compiles plain text at load time with the engine's own compiler: the source is given
to the game as is. Each file is registered as `maps/mp/zz_harness/<file>` for the hooked asset
lookup, loaded by the game's own Scr_LoadScript (includes through the same lookup), and its entry
function (`main`, or `// entry: name`) started with Scr_ExecThread, self = level. Frames are then
stepped as G_RunFrame does until nothing has happened for --idle frames or --frames is reached.

Trace lines: `f<time> T<n> <text>` for every logprint() call (its arguments joined), `f` being
gScrVarPub.time (one 50 ms server frame per unit), T0 the entry thread, T1, T2... the other threads
in creation order. Runtime script errors appear as `f<time> T<n> script error[ (terminal)]:
<message>`; a load failure or a fatal error ends the trace with `error: <message>`.
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
from t56vm import T56VM, Fault, PROFILES  # noqa: E402

FOLDER = "maps/mp/zz_harness"
# T5: Scr_LoadScriptInternal formats "<name>.gsc" into a 64-byte buffer (MAX_QPATH): with
# maps/mp/ in front, notify_wakes_waiter_before_notifier_continues.gsc is cut and not found
FOLDER_OF = {"t5": "zz_harness"}


def gsctool_exe():
    g = os.environ.get("GSC_TOOL") or paths.get("gsctool")
    if not g and paths.get("gsctool_src"):
        # gsctool_src points into the source tree (<gsc-tool>/src/src/gsc/engine): the
        # release binary sits at the tree's root
        d = paths.get("gsctool_src")
        for _ in range(5):
            if os.path.isfile(os.path.join(d, "gsc-tool.exe")):
                g = os.path.join(d, "gsc-tool.exe")
                break
            d = os.path.dirname(d)
    g = g or shutil.which("gsc-tool")
    if not g or not os.path.isfile(g):
        raise SystemExit("gsc-tool not found: set GSC_TOOL or `gsctool` in tools/re/paths.local.json")
    return g


def virtual_path(path, game="t6"):
    return FOLDER_OF.get(game, FOLDER) + "/" + os.path.basename(path)


def gsctool_flags(path):
    with open(path, encoding="latin-1") as fh:
        m = re.search(r"^//\s*gsctool-flags:\s*(.*)$", fh.read(), re.M)
    return m.group(1).split() if m else []


def compile_scripts(game, files):
    """({"accepted", "errors", "compiler"}, {virtual path: bytes}). T6: each file compiled by
    gsc-tool (with the main file's `// gsctool-flags:`); T5: the sources as they are."""
    if game.startswith("t5"):
        scripts = {}
        for f in files:
            with open(f, "rb") as fh:
                scripts[virtual_path(f, game)] = fh.read()
        return {"accepted": True, "errors": [], "compiler": "the game's own (load time)"}, scripts
    flags = gsctool_flags(files[0])
    h = hashlib.sha1(game.encode() + " ".join(flags).encode())
    for f in files:
        h.update(os.path.basename(f).encode() + b"\0")
        with open(f, "rb") as fh:
            h.update(fh.read())
    out = os.path.join(RE, "out", "vm_gscc_%s" % game, h.hexdigest()[:16])
    result_file = os.path.join(out, "compile.json")
    if not os.path.exists(result_file):
        tool = gsctool_exe()
        os.makedirs(out, exist_ok=True)
        res = {"accepted": True, "errors": [],
               "compiler": " ".join(["gsc-tool -m comp -g t6 -s pc32"] + flags)}
        for f in files:
            with tempfile.TemporaryDirectory(dir=out) as tmp:
                src = os.path.join(tmp, "src")
                os.makedirs(os.path.join(src, *FOLDER.split("/")))
                shutil.copy(f, os.path.join(src, *FOLDER.split("/")))
                cmd = [tool, "-m", "comp", "-g", "t6", "-s", "pc32"] + flags + [src]
                cp = subprocess.run(cmd, capture_output=True, stdin=subprocess.DEVNULL,
                                    timeout=120, cwd=tmp)
                text = (cp.stdout + cp.stderr).decode("utf-8", "replace")
                obj = os.path.join(tmp, "compiled", "t6", *FOLDER.split("/"), os.path.basename(f))
                if os.path.exists(obj):
                    shutil.copy(obj, os.path.join(out, os.path.basename(f) + "c"))
                else:
                    res["accepted"] = False
                    res["errors"] += [re.sub(r"\S*zz_harness[\\/]", "", ln.strip())
                                      for ln in text.splitlines()
                                      if ln.strip() and "created by xensik" not in ln]
        with open(result_file, "w") as fh:
            json.dump(res, fh, indent=1)
    with open(result_file) as fh:
        res = json.load(fh)
    if not res["accepted"]:
        return res, {}
    scripts = {}
    for f in files:
        with open(os.path.join(out, os.path.basename(f) + "c"), "rb") as fh:
            scripts[virtual_path(f)] = fh.read()
    return res, scripts


IMPORT_KIND = {1: "ref", 2: "call", 3: "thread", 4: "method", 5: "method thread"}


def t6_imports(data):
    """The import table of a T6 object as the game's linker (0x7e08b0) reads it: header +0x20
    offset, +0x36 count; entries {u16 name, u16 namespace (string offsets), u16 refs, u8
    params, u8 flags (low 4 bits the kind, 0x10 developer)} + refs x u32. An empty namespace is
    a builtin lookup, any other a script file ("<namespace>.gsc")."""
    import struct
    if data[:4] != b"\x80GSC":
        return []
    off = struct.unpack_from("<I", data, 0x20)[0]
    n = struct.unpack_from("<H", data, 0x36)[0]

    def s(o):
        return data[o:data.index(b"\0", o)].decode("latin-1")
    out = []
    for _ in range(n):
        name, ns, refs, params, flags = struct.unpack_from("<HHHBB", data, off)
        out.append("%s::%s %s %d%s" % (s(ns), s(name), IMPORT_KIND.get(flags & 0xF, flags & 0xF),
                                       params, " dev" if flags & 0x10 else ""))
        off += 8 + 4 * refs
    return out


def run(game, files, entry=None, frames=200, idle=20, verbose=False, exe=None, developer=False):
    main_file = files[0]
    with open(main_file, encoding="latin-1") as fh:
        src = fh.read()
    m = re.search(r"^//\s*entry:\s*(\w+)", src, re.M)
    entry = entry or (m.group(1) if m else "main")
    comp, scripts = compile_scripts(game, files)
    out = {"game": game, "status": None, "lines": [], "error": None, "compile": comp,
           "developer": developer,
           "imports": {os.path.basename(k): t6_imports(v) for k, v in scripts.items()}}
    if not scripts:
        out.update(status="compile_error", error="compile: %s" % " | ".join(comp["errors"]))
        out["lines"].append("error: " + out["error"])
        return out
    vm = T56VM(game, exe or paths.need(PROFILES[game]["exe_key"]), verbose=verbose,
               developer=developer)
    vm.boot()
    vm.track_threads()
    vm.trace_builtin(vm.P["logprint"], "logprint")
    vm.trace_errors()
    for name, data in scripts.items():
        vm.add_script(name, data)
    vpath = virtual_path(main_file, game)[:-4]
    t0 = time.time()
    status = "ok"
    try:
        vm.begin_load()
        if not vm.load_script(vpath):
            raise Fault("Scr_LoadScript returned 0 for %s%s" % (
                vpath, "".join(": " + t for _, t in vm.messages[-1:])))
        handle = vm.function_handle(vpath, entry)
        if not handle:
            raise Fault("no function handle for %s::%s" % (vpath, entry))
        vm.end_load()
        vm.exec_thread(handle)
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
        msg = str(f)
        if msg.startswith("Com_Error"):  # the engine's own message, its lines joined
            msg = " | ".join(x.strip() for x in msg.split("\n") if x.strip())
        else:
            msg = msg.split("\n")[0]
        out["error"] = msg
        m = re.match(r"Com_Error \d+: [\x00-\x20]*Server script compile error \| ([^|]+)", msg)
        if m and not vm.trace:
            # T5: the game's own compiler refused the script at load (CompileError ends in
            # Com_Error): a compile error, as gsc-tool's refusal is for T6
            status = "compile_error"
            out["error"] = "compile: " + m.group(1).strip()
    out["elapsed_s"] = round(time.time() - t0, 3)
    events = [(k, "print", f, tid, args) for k, (f, tid, _, args) in enumerate(vm.trace)]
    events += [(k - 0.5, "error", f, tid, (msg, term)) for f, tid, msg, term, k in vm.errors]
    for _, kind, f, tid, args in sorted(events, key=lambda e: e[0]):
        if kind == "print":
            out["lines"].append("f%d %s %s" % (f, tid, "".join(args)))
        else:
            msg, term = args
            out["lines"].append("f%d %s script error%s: %s" % (
                f, tid, " (terminal)" if term else "", msg))
    out["vm_messages"] = ["f%d %s" % m for m in vm.messages]
    if out["error"]:
        out["lines"].append("error: " + out["error"])
    out.update(status=status, frames=vm.frames_run, stubs=vm.stubs,
               unmapped=["%#x->%#x" % (a, b) for a, b in vm.unmapped],
               threads=vm.thread_count, assets=["%#x %s" % r for r in vm.asset_requests])
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
    ap.add_argument("--developer", action="store_true",
                    help="Scr_Settings developer = developer_script = 1 (retail is 0)")
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
        print("# stubs: " + ", ".join(sorted(res.get("stubs", {}))))
        print("# assets: " + ", ".join(res.get("assets", [])))
        for k, v in res.get("imports", {}).items():
            print("# imports %s: %s" % (k, ", ".join(v)))


if __name__ == "__main__":
    main()
