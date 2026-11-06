#!/usr/bin/env python3
"""Compile GSC/CSC files with the official Black Ops III linker and report the verdict as JSON.

The BO3 mod tools ship no standalone script compiler: scripts are compiled by linker_modtools.exe
while it links a zone. This harness builds a throwaway mod around the input files
(<bo3>/mods/<mod>/scripts/..., zone_source/<zone>.zone listing them as scriptparsetree assets),
runs the linker offline, parses its error lines and, with --disasm, extracts the compiled
scriptparsetree from the produced fastfile and disassembles it with acts.

It never launches the game and never writes outside <bo3>/mods/<mod>/ and a temporary folder.
The mod folder is deleted after the run unless --keep is given. Stdlib only.

    python -I gsc_link.py file.gsc [more.gsc=scripts/x/more.gsc header.gsh ...] [--disasm]

Exit status: 0 accepted, 1 rejected, 2 harness error (linker missing, timeout...).
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

DEFAULT_BO3 = r"C:\Program Files (x86)\Steam\steamapps\common\Call of Duty Black Ops III"
DEFAULT_ACTS = shutil.which("acts") or "acts.exe"  # or set GSC_ACTS / --acts
DEFAULT_MOD = "zz_gsc_harness"
DEFAULT_ZONE = "zm_mod"
SCRIPT_EXTS = {".gsc", ".csc"}
COPY_EXTS = {".gsc", ".csc", ".gsh"}

# ERR(0) scripts/zz_harness/test.gsc (6,4) in "main()" : syntax error, unexpected ... : 	y =
ERR_RE = re.compile(
    r'^(?:\^\d)?(?P<kind>ERR|WARN(?:ING)?)\((?P<code>\d+)\)\s+(?P<file>\S+)\s+'
    r'\((?P<line>\d+),(?P<col>\d+)\)\s+(?:in\s+"(?P<func>[^"]*)"\s+)?:\s*(?P<msg>.*)$'
)
COLOR_RE = re.compile(r"\^\d")


def parse_inputs(specs: list[str]) -> list[tuple[Path, str]]:
    """'file' or 'file=scripts/virtual/path.gsc' -> (source, virtual path)."""
    out = []
    for spec in specs:
        src, sep, virt = spec.partition("=")
        p = Path(src)
        if not p.is_file():
            raise SystemExit(f"input not found: {src}")
        ext = p.suffix.lower()
        if ext not in COPY_EXTS:
            raise SystemExit(f"unsupported extension {ext!r}: {src} (want .gsc, .csc or .gsh)")
        if not sep:
            virt = f"scripts/zz_harness/{p.name}"
        virt = virt.replace("\\", "/").lstrip("/")
        if ".." in virt.split("/"):
            raise SystemExit(f"virtual path may not contain '..': {virt}")
        out.append((p, virt))
    return out


def acquire_lock(lock: Path, wait_s: float) -> int:
    deadline = time.monotonic() + wait_s
    while True:
        try:
            return os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
        except FileExistsError:
            if time.monotonic() > deadline:
                raise SystemExit(f"harness lock held: {lock} (another run, or a stale lock to delete)")
            time.sleep(0.5)


def parse_linker_output(text: str) -> dict:
    errors, seen, other = [], set(), []
    for raw in text.splitlines():
        line = raw.rstrip()
        m = ERR_RE.match(line.strip())
        if m:
            msg = m.group("msg").rstrip()
            key = (m.group("file"), m.group("line"), m.group("col"), COLOR_RE.sub("", msg).strip())
            if key in seen:
                continue
            seen.add(key)
            errors.append({
                "severity": "error" if m.group("kind") == "ERR" else "warning",
                "code": int(m.group("code")),
                "file": m.group("file"),
                "line": int(m.group("line")),
                "col": int(m.group("col")),
                "function": m.group("func"),
                "message": COLOR_RE.sub("", msg).strip(),
            })
        elif re.search(r"(?i)\b(error|warning)\b", line) and "There were no errors" not in line:
            other.append(COLOR_RE.sub("", line).strip())
    return {"errors": errors, "other_lines": other}


def run_linker(bo3: Path, mod: str, zone: str, timeout: float) -> tuple[int | None, str, float]:
    exe = bo3 / "bin" / "linker_modtools.exe"
    if not exe.is_file():
        raise SystemExit(f"linker not found: {exe}")
    env = dict(os.environ)
    root = str(bo3) + "\\"
    env.update(TA_GAME_PATH=root, TA_TOOLS_PATH=root,
               TA_LOCAL_ASSET_CACHE=str(bo3 / "share" / "assetconvert"))
    cmd = [str(exe), "-language", "english", "-fs_game", mod, "-modsource", zone]
    t0 = time.monotonic()
    try:
        cp = subprocess.run(cmd, cwd=bo3 / "bin", env=env, capture_output=True,
                            stdin=subprocess.DEVNULL, timeout=timeout)
    except subprocess.TimeoutExpired as e:
        out = (e.stdout or b"") + (e.stderr or b"")
        return None, out.decode("utf-8", "replace"), time.monotonic() - t0
    out = cp.stdout + b"\n" + cp.stderr
    return cp.returncode, out.decode("utf-8", "replace"), time.monotonic() - t0


def load_tables_reader():
    """gscc_imports.read_tables, loaded by path (python -I keeps the script dir off sys.path)."""
    spec = importlib.util.spec_from_file_location("gscc_imports", Path(__file__).with_name("gscc_imports.py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod.read_tables


def dump_gscc(acts: Path, ff: Path, work: Path, timeout: float) -> Path:
    """Dump the scriptparsetree assets of ff with acts into work/dump; returns that folder."""
    dump = work / "dump"
    subprocess.run([str(acts), "-t", "--noUpdater", "fastfile", "-o", str(dump), "-r", "bo3",
                    "-a", "scriptparsetree", "--disableScriptsDecomp", str(ff)],
                   cwd=work, capture_output=True, stdin=subprocess.DEVNULL, timeout=timeout)
    return dump


def disassemble(acts: Path, ff: Path, work: Path, timeout: float, words: list[str]) -> list[dict]:
    """Dump the scriptparsetree assets of ff with acts, disassemble each one, read its tables."""
    if not acts.is_file():
        return [{"error": f"acts not found: {acts}"}]
    read_tables = load_tables_reader()
    asm = work / "asm"
    dump = dump_gscc(acts, ff, work, timeout)
    results = []
    for gscc in sorted(dump.rglob("*.?scc")):
        rel = gscc.relative_to(dump).as_posix()
        out_dir = asm / gscc.stem
        subprocess.run([str(acts), "-t", "--noUpdater", "gscd", "-a", "-o", str(out_dir), str(gscc)],
                       cwd=work, capture_output=True, stdin=subprocess.DEVNULL, timeout=timeout)
        texts = [p.read_text("utf-8", "replace") for p in sorted(out_dir.rglob("*")) if p.is_file()]
        data = gscc.read_bytes()
        try:
            tables = read_tables(data, words)
        except Exception as e:  # a layout this reader does not know: keep the asm anyway
            tables = {"error": f"{type(e).__name__}: {e}"}
        results.append({"gscc": rel, "size": len(data), "tables": tables, "asm": "\n".join(texts)})
    return results


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("inputs", nargs="+", metavar="FILE[=VIRTUAL_PATH]",
                    help="a .gsc/.csc to compile, or a .gsh to make available; default virtual "
                         "path scripts/zz_harness/<name>")
    ap.add_argument("--bo3", default=os.environ.get("GSC_BO3_ROOT", DEFAULT_BO3),
                    help="BO3 install with the mod tools (env GSC_BO3_ROOT)")
    ap.add_argument("--acts", default=os.environ.get("GSC_ACTS", DEFAULT_ACTS),
                    help="acts.exe, for --disasm (env GSC_ACTS)")
    ap.add_argument("--mod", default=DEFAULT_MOD, help="scratch mod folder name (must start zz_)")
    ap.add_argument("--zone", default=DEFAULT_ZONE, help="zone name to link")
    ap.add_argument("--disasm", action="store_true", help="add acts disassembly of the output")
    ap.add_argument("--gscc-out", metavar="DIR",
                    help="copy each compiled .gscc (the linked scriptparsetree, via acts) and the "
                         "linker's line table (scriptgdb/<path>.gdb) into DIR; your own scripts' "
                         "output only, never commit it")
    ap.add_argument("--keep", action="store_true", help="keep the scratch mod folder afterwards")
    ap.add_argument("--raw", action="store_true", help="include the full linker output")
    ap.add_argument("--timeout", type=float, default=300.0)
    args = ap.parse_args()

    if not args.mod.startswith("zz_") or "/" in args.mod or "\\" in args.mod:
        raise SystemExit("--mod must be a plain folder name starting with zz_")
    bo3 = Path(args.bo3)
    inputs = parse_inputs(args.inputs)
    if not any(Path(v).suffix.lower() in SCRIPT_EXTS for _, v in inputs):
        raise SystemExit("need at least one .gsc or .csc")

    mods = bo3 / "mods"
    mod_dir = mods / args.mod
    lock = mods / f"{args.mod}.lock"
    fd = acquire_lock(lock, args.timeout)
    work = Path(tempfile.mkdtemp(prefix="gsc_link_"))
    try:
        if mod_dir.exists():
            shutil.rmtree(mod_dir)
        (mod_dir / "zone_source").mkdir(parents=True)
        zone_lines = []
        for src, virt in inputs:
            dst = mod_dir / Path(virt)
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(src, dst)
            if dst.suffix.lower() in SCRIPT_EXTS:
                zone_lines.append(f"scriptparsetree,{virt}")
        (mod_dir / "zone_source" / f"{args.zone}.zone").write_text("\n".join(zone_lines) + "\n")

        code, out, elapsed = run_linker(bo3, args.mod, args.zone, args.timeout)
        ff = mod_dir / "zone" / f"{args.zone}.ff"
        parsed = parse_linker_output(out)
        if code is None:
            status = "timeout"
        elif code == 0 and ff.is_file() and "UNRECOVERABLE ERROR" not in out and not any(
                e["severity"] == "error" for e in parsed["errors"]):
            status = "accept"
        else:
            status = "reject"
        result = {
            "status": status,
            "accepted": status == "accept",
            "exit_code": code,
            "elapsed_s": round(elapsed, 2),
            "inputs": [{"source": str(s), "path": v} for s, v in inputs],
            **parsed,
        }
        if args.raw:
            result["linker_output"] = out
        if args.disasm and status == "accept":
            words = set()
            for src, _ in inputs:
                words.update(re.findall(r"[A-Za-z_][A-Za-z0-9_]*", src.read_text("latin-1")))
            result["scripts"] = disassemble(Path(args.acts), ff, work, args.timeout, sorted(words))
        if args.gscc_out and status == "accept":
            dump = work / "dump"
            if not dump.is_dir():
                dump = dump_gscc(Path(args.acts), ff, work, args.timeout)
            out_dir = Path(args.gscc_out)
            out_dir.mkdir(parents=True, exist_ok=True)
            copied = []
            for gscc in sorted(dump.rglob("*.?scc")):
                dst = out_dir / gscc.relative_to(dump)
                dst.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(gscc, dst)
                copied.append(str(dst))
            # the linker also writes a line table per script (zone_source/all/scriptgdb/
            # <path>.gdb: the code offset of each source line): the VM tracer maps code back to
            # source lines with it
            gdb_root = mod_dir / "zone_source" / "all" / "scriptgdb"
            for gdb in sorted(gdb_root.rglob("*.gdb")) if gdb_root.is_dir() else []:
                dst = out_dir / "scriptgdb" / gdb.relative_to(gdb_root)
                dst.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(gdb, dst)
                copied.append(str(dst))
            result["gscc_out"] = copied
    finally:
        if not args.keep and mod_dir.exists():
            shutil.rmtree(mod_dir, ignore_errors=True)
        shutil.rmtree(work, ignore_errors=True)
        os.close(fd)
        lock.unlink(missing_ok=True)

    json.dump(result, sys.stdout, indent=1)
    sys.stdout.write("\n")
    return {"accept": 0, "reject": 1}.get(result["status"], 2)


if __name__ == "__main__":
    sys.exit(main())
