"""Run a GSC test script in Black Ops III's own script VM, offline, and print its trace.

    py -3.11 -I tools/re/vm/t7_run.py test.gsc [helper.gsc ...] [--frames N] [--notify] [--json]
    py -3.11 -I tools/re/vm/t7_run.py test.gsc --trace out.jsonl [--calls] [--ops [N]]

The script is compiled by the official mod tools linker (tools/linker/gsc_link.py, cached by
content under tools/re/out/vm_gscc/, never committed), loaded through the game's own
Scr_LoadScript and linker (t7vm.py), and its entry function (`main`, or `// entry: name`) is
started with Scr_ExecThread. Frames are then stepped (Scr_RunCurrentThreads, time += 1) until
nothing has happened for --idle frames or --frames is reached.

Trace lines: `f<frame> T<n> <text>` for every logprint() call (its arguments joined), T0 being
the entry thread and T1, T2... the other threads in order of first appearance; with --notify,
`f<frame> T<n> notify <object> <event>` for every notify. A load failure or a Com_Error ends
the trace with `error: <message>`. Runtime script errors printed by the VM are kept as
`f<frame> script-error <message>`.

--trace FILE writes a structured trace (gsc-trace JSONL, docs/trace-format.md; t7_trace.py):
threads, waits, notifies, endons, builtin calls, script errors; with --calls every script
function call and return; with --ops every opcode (capped, default 20000). Code positions are
source lines when the official linker's .gdb line table is there (fetched with the .gscc).
View it with tools/re/vm/trace_view.py.
"""
import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
RE = os.path.dirname(HERE)
sys.path.insert(0, RE)
sys.path.insert(0, HERE)
from common import paths  # noqa: E402
from common.hashes import t7_canon  # noqa: E402
import t7vm  # noqa: E402
from t7vm import T7VM, Fault, ADDR  # noqa: E402

LINKER = os.path.join(os.path.dirname(RE), "linker", "gsc_link.py")
GSCC_CACHE = os.path.join(RE, "out", "vm_gscc")
LOG_BUILTIN = 0x1AA3630  # logprint's handler (gsc function table 1 entry 125, phase 2)
VM_NOTIFY = 0x12D36C0    # VM_Notify(inst, objectId, event canon, top): the Notify handler's call


def namespace_of(src, path):
    m = re.search(r"^\s*#namespace\s+(\w+)\s*;", src, re.M)
    return m.group(1) if m else os.path.splitext(os.path.basename(path))[0]


def virtual_path(path):
    return "scripts/zz_harness/" + os.path.basename(path)


def compile_scripts(files, acts=None, gdb=None):
    """{virtual path: gscc bytes} for every input, from the official linker (cached by content).
    With a dict as `gdb`, it also gets {virtual path: .gdb bytes} (the linker's line tables;
    an older cache entry without them is linked again)."""
    h = hashlib.sha1()
    for f in files:
        h.update(os.path.basename(f).encode() + b"\0")
        with open(f, "rb") as fh:
            h.update(fh.read())
    out = os.path.join(GSCC_CACHE, h.hexdigest()[:16])
    result_file = os.path.join(out, "link.json")
    gdb_dir = os.path.join(out, "scriptgdb")
    stale = gdb is not None and os.path.exists(result_file) and not os.path.isdir(gdb_dir)
    if stale:
        with open(result_file) as fh:
            stale = bool(json.load(fh).get("accepted"))
    if not os.path.exists(result_file) or stale:
        env = dict(os.environ)
        if acts:
            env["GSC_ACTS"] = acts
        cmd = [sys.executable, "-I", LINKER] + ["%s=%s" % (f, virtual_path(f)) for f in files]
        cmd += ["--gscc-out", out]
        cp = subprocess.run(cmd, capture_output=True, env=env, stdin=subprocess.DEVNULL)
        try:
            res = json.loads(cp.stdout.decode("utf-8", "replace"))
        except ValueError:
            raise SystemExit("linker harness failed:\n%s" % cp.stderr.decode("utf-8", "replace"))
        os.makedirs(out, exist_ok=True)
        with open(result_file, "w") as fh:
            json.dump({k: v for k, v in res.items() if k != "gscc_out"}, fh, indent=1)
    with open(result_file) as fh:
        res = json.load(fh)
    if not res.get("accepted"):
        return res, {}
    scripts = {}
    for root, _, names in os.walk(out):
        for n in names:
            if n.endswith(".gscc"):
                p = os.path.join(root, n)
                rel = p.replace("\\", "/").split("/scripts/", 1)[1]
                with open(p, "rb") as fh:
                    scripts["scripts/" + rel[:-5] + ".gsc"] = fh.read()
            elif n.endswith(".gdb") and gdb is not None:
                p = os.path.join(root, n)
                rel = os.path.relpath(p, gdb_dir).replace("\\", "/")
                with open(p, "rb") as fh:
                    gdb[rel[:-4]] = fh.read()
    return res, scripts


class Run:
    """One script run: the VM, the trace with thread labels, the notifies, the errors."""

    def __init__(self, exe, verbose=False, developer=False):
        self.vm = T7VM(exe, log=print, verbose=verbose)
        self.vm.boot(developer=developer)
        self.vm.track_threads()
        self.vm.trace_builtin(LOG_BUILTIN, "logprint")
        self.vm.trace_errors()
        self.notifies = []
        self.vm.watch(VM_NOTIFY, Run._on_notify.__get__(self))

    def _on_notify(self, vm):
        obj, ev = vm.emu.arg(1) & 0xFFFFFFFF, vm.emu.arg(2) & 0xFFFFFFFF
        self.notifies.append((vm.now(), vm.thread_name(vm.current_thread()), obj, ev,
                              len(vm.trace)))

    def obj_name(self, oid):
        for k in ("levelId", "gameId", "animId", "worldId"):
            if oid == self.vm.pub(self.vm.inst, k):
                return k[:-2]
        return "obj%d" % oid


def run(files, entry=None, frames=200, idle=20, notify=False, verbose=False, exe=None,
        developer=False, trace=None, calls=False, ops=0):
    """Compile, load, run; returns {"status", "lines", "error", "stubs", ...}. With `trace` (a
    path), also writes the structured trace there ("-": kept in out["trace"])."""
    main = files[0]
    with open(main, encoding="latin-1") as fh:
        src = fh.read()
    m = re.search(r"^//\s*entry:\s*(\w+)", src, re.M)
    entry = entry or (m.group(1) if m else "main")
    gdbs = {} if trace else None
    link, scripts = compile_scripts(files, gdb=gdbs)
    out = {"status": None, "lines": [], "error": None, "link": link, "developer": developer}
    if not scripts:
        errs = link.get("errors") or link.get("other_lines")
        out.update(status="compile_error", error="compile: %s" % json.dumps(errs))
        out["lines"].append("error: " + out["error"])
        return out
    r = Run(exe or paths.need("exe.t7"), verbose=verbose, developer=developer)
    vm = r.vm
    for name, data in scripts.items():
        vm.add_script(name, data)
    words = set(re.findall(r"[A-Za-z_]\w*", src))
    for f in files[1:]:
        with open(f, encoding="latin-1") as fh:
            words |= set(re.findall(r"[A-Za-z_]\w*", fh.read()))
    words |= set(re.findall(r'"([^"\\]*)"', src))
    names = {t7_canon(w): w for w in words}
    tracer = None
    if trace:
        tracer = start_trace(vm, files, scripts, gdbs, names, entry, calls, ops, developer)
    vpath = virtual_path(main)[:-4]
    t0 = time.time()
    status = "ok"
    try:
        ok = vm.load(vpath)
        if not ok:
            raise Fault("Scr_LoadScript returned 0 for %s" % vpath)
        handle = vm.function_handle(vpath, namespace_of(src, main), entry)
        if not handle:
            raise Fault("no function handle for %s::%s" % (namespace_of(src, main), entry))
        vm.end_load()
        vm.exec_thread(handle)
        last = 0
        n0 = len(vm.trace) + len(r.notifies)
        while vm.frame < frames:
            if tracer:
                tracer.tr.emit("frame", vm.now())
            vm.run_frame()
            n = len(vm.trace) + len(r.notifies)
            if n != n0:
                last, n0 = vm.frame, n
            elif vm.frame - last >= idle:
                break
    except Fault as f:
        status = "error"
        msg = str(f)
        if msg.startswith("Com_Error"):  # the engine's own message, banners dropped
            msg = " | ".join(x.strip() for x in msg.split("\n") if x.strip() and "=====" not in x)
        else:
            msg = msg.split("\n")[0]
        out["error"] = msg
    out["elapsed_s"] = round(time.time() - t0, 3)
    events = [(f, k, tid, "logprint", args) for k, (f, tid, _, args) in enumerate(vm.trace)]
    events += [(f, k - 0.25, tid, "error", (msg, term)) for f, tid, msg, term, k in vm.errors]
    if notify:
        events += [(f, k - 0.5, tid, "notify", (r.obj_name(obj), names.get(ev, "#%x" % ev)))
                   for f, tid, obj, ev, k in r.notifies]
    for f, _, tid, kind, args in sorted(events, key=lambda e: (e[1])):
        if kind == "logprint":
            out["lines"].append("f%d %s %s" % (f, tid, "".join(args)))
        elif kind == "error":
            out["lines"].append("f%d %s script error%s: %s" % (f, tid,
                                                                " (terminal)" if args[1] else "",
                                                                args[0]))
        else:
            out["lines"].append("f%d %s notify %s %s" % (f, tid, args[0], args[1]))
    out["vm_messages"] = ["f%d %s" % (f, t.strip()) for f, ch, t in vm.prints if t.strip()]
    if tracer:
        tracer.tr.finish(vm.now(), status, error=out["error"], frames=vm.frame)
        if trace == "-":
            out["trace"] = [json.loads(ln) for ln in tracer.tr.lines()]
        else:
            tracer.tr.write(trace)
    if out["error"]:
        out["lines"].append("error: " + out["error"])
    out.update(status=status, frames=vm.frame, stubs=vm.stubs,
               unmapped=["%#x->%#x" % (a, b) for a, b in vm.unmapped],
               threads=vm.thread_count)
    return out


def start_trace(vm, files, scripts, gdbs, names, entry, calls, ops, developer):
    """Hook a Tracer (t7_trace.py) on the booted VM and register the loaded scripts."""
    import t7_trace
    from trace import Trace
    tr = Trace("t7", "gruff/tools/re/vm/t7_run.py (emulated BO3 VM, %s)"
               % os.path.basename(paths.need("exe.t7")), time_unit="frame", frame_ms=50,
               options={"calls": bool(calls), "ops": ops, "entry": entry,
                        "developer": bool(developer)},
               caps={"op": ops} if ops else None)
    gdata = paths.get("gsc_data")
    toml = os.path.join(gdata, "builtins", "t7.toml") if gdata else None
    builtins = t7_trace.builtin_names(toml) if toml and os.path.exists(toml) else {}
    tracer = t7_trace.Tracer(vm, tr, names, builtins=builtins, calls=calls, ops=ops)
    root = os.path.dirname(os.path.dirname(RE))  # gruff
    by_name = {os.path.basename(f): f for f in files}
    for vpath, data in scripts.items():
        f = by_name.get(os.path.basename(vpath))
        rel = None
        if f:
            rel = os.path.relpath(os.path.abspath(f), root).replace("\\", "/")
            if rel.startswith(".."):
                rel = os.path.basename(f)
        tracer.add_script(vpath, data, gdb=gdbs.get(vpath), file=rel)
    return tracer


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("files", nargs="+", help="the test script, then any helper scripts")
    ap.add_argument("--entry", help="entry function (default: // entry: or main)")
    ap.add_argument("--frames", type=int, default=200)
    ap.add_argument("--idle", type=int, default=20, help="stop after this many quiet frames")
    ap.add_argument("--notify", action="store_true", help="trace notifies too")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--trace", metavar="FILE", help="write a gsc-trace (JSONL) of the run")
    ap.add_argument("--calls", action="store_true", help="trace: every script call and return")
    ap.add_argument("--ops", type=int, nargs="?", const=20000, default=0, metavar="N",
                    help="trace: every opcode, at most N records (default 20000)")
    ap.add_argument("--verbose", action="store_true", help="echo every engine print")
    ap.add_argument("--developer", action="store_true",
                    help="gScrVarPub.developer = 1 (mod tools developer mode; retail is 0)")
    a = ap.parse_args()
    if (a.calls or a.ops) and not a.trace:
        ap.error("--calls and --ops need --trace FILE")
    res = run(a.files, a.entry, a.frames, a.idle, a.notify, a.verbose, developer=a.developer,
              trace=a.trace, calls=a.calls, ops=a.ops)
    if a.json:
        json.dump(res, sys.stdout, indent=1)
        print()
        return
    for line in res["lines"]:
        print(line)
    if a.verbose:
        for m in res.get("vm_messages", []):
            print("# vm: " + m)


if __name__ == "__main__":
    main()
