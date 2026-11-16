# Decompile again the scripts an export left as bytecode, with another gsc-tool build (a patched
# one, see gsc-tool-patches/). Writes <name>.gsc beside each <name>.gscbin of corpus/<game>/decompiled
# that gsc-tool now decompiles, never replacing an existing file and never removing the bytecode;
# the new file keeps the bytecode's write time (so build_manifest.py still finds its origin zone) and
# is recorded in corpus/<game>/_export/redecompiled.tsv. Stdlib only; run with `python -I`:
#
#   python -I redecompile.py --game s2 --gsc-tool <patched gsc-tool.exe> [--gsc-game s2] [--only 1357.gscbin]
import argparse
import datetime
import hashlib
import os
import re
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_CORPUS = os.path.normpath(os.path.join(HERE, "..", "..", "..", "corpus"))
BYTECODE_EXT = (".gscbin", ".cscbin")
LOG_HEADER = "path\tbytecode_sha256\tgsc_tool\tgsc_tool_version\tgsc_tool_sha256\tresult\tdate\n"


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def main():
    ap = argparse.ArgumentParser(description="Decompile again a game's bytecode files with another gsc-tool")
    ap.add_argument("--corpus", default=DEFAULT_CORPUS)
    ap.add_argument("--game", required=True, help="corpus folder, e.g. s2")
    ap.add_argument("--gsc-game", default=None, help="gsc-tool's -g value (default: --game)")
    ap.add_argument("--gsc-tool", required=True, help="the gsc-tool.exe to use")
    ap.add_argument("--only", action="append", default=[], help="a path relative to <game>/decompiled (repeatable)")
    a = ap.parse_args()

    ddir = os.path.join(os.path.abspath(a.corpus), a.game, "decompiled")
    if not os.path.isdir(ddir):
        sys.exit(f"not a corpus folder: {ddir}")
    tool = os.path.abspath(a.gsc_tool)
    p = subprocess.run([tool, "--version"], capture_output=True, text=True, stdin=subprocess.DEVNULL)
    m = re.search(r"GSC Tool (\S+)", p.stdout + p.stderr)
    tool_v, tool_h = (m.group(1) if m else "?"), sha256(tool)

    files = []
    for root, _, names in os.walk(ddir):
        for n in sorted(names):
            if n.lower().endswith(BYTECODE_EXT):
                files.append(os.path.relpath(os.path.join(root, n), ddir).replace(os.sep, "/"))
    if a.only:
        files = [f for f in files if f in set(a.only)]

    log = os.path.join(os.path.abspath(a.corpus), a.game, "_export", "redecompiled.tsv")
    new_log = not os.path.exists(log)
    done = 0
    with open(log, "a", encoding="utf-8", newline="\n") as out:
        if new_log:
            out.write(LOG_HEADER)
        for rel in files:
            src = os.path.join(ddir, rel)
            dst = src[: -len("bin")]  # x.gscbin -> x.gsc
            if os.path.exists(dst):
                print(f"skip {rel}: {os.path.basename(dst)} exists")
                continue
            with tempfile.TemporaryDirectory() as work:
                shutil.copy2(src, os.path.join(work, os.path.basename(src)))
                r = subprocess.run([tool, "-m", "decomp", "-g", a.gsc_game or a.game, "-s", "pc", os.path.basename(src)],
                                   cwd=work, capture_output=True, text=True, stdin=subprocess.DEVNULL)
                made = [os.path.join(rt, n) for rt, _, ns in os.walk(os.path.join(work, "decompiled")) for n in ns]
                if r.returncode != 0 or len(made) != 1:
                    msg = (r.stdout + r.stderr).strip().splitlines()
                    result = "failed: " + (msg[-1] if msg else f"exit {r.returncode}")
                    print(f"{rel}: {result}")
                else:
                    shutil.copyfile(made[0], dst)
                    st = os.stat(src)
                    os.utime(dst, (st.st_atime, st.st_mtime))
                    result = "decompiled"
                    done += 1
                    print(f"{rel}: decompiled -> {os.path.basename(dst)}")
            out.write(f"{a.game}/decompiled/{rel}\t{sha256(src)}\t{os.path.basename(tool)}\t{tool_v}\t{tool_h}\t"
                      f"{result}\t{datetime.date.today().isoformat()}\n")
    print(f"{done}/{len(files)} decompiled; log: {log}")


if __name__ == "__main__":
    main()
