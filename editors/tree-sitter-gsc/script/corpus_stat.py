#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Error-free share of a local GSC corpus under tree-sitter-gsc, per game.

    python script/corpus_stat.py <corpus dir> [game ...] [--failures N] [--json out.json]

<corpus dir> holds one folder per game (the layout of tools/corpus). A file counts as error-free
when its tree holds no ERROR and no MISSING node. Compiled scripts (`\\x80GSC` or a NUL byte in the
first KiB) are skipped, as in crates/gsc_syntax/tests/corpus.rs. Corpus files never enter the
repository; this script only prints counts and paths.
"""

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXTS = (".gsc", ".csc", ".gsh")
FAIL_LINE = re.compile(r"^(.*?)\s+Parse:.*\((ERROR|MISSING)")


def is_binary(path):
    with open(path, "rb") as f:
        head = f.read(1024)
    return head.startswith(b"\x80GSC") or b"\x00" in head


def game_files(root):
    out = []
    for dirpath, _, names in os.walk(root):
        for n in names:
            if n.lower().endswith(EXTS):
                p = os.path.join(dirpath, n)
                if not is_binary(p):
                    out.append(p)
    out.sort()
    return out


def run_game(files):
    with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False, encoding="utf-8") as f:
        f.write("\n".join(files))
        list_path = f.name
    try:
        cli = os.path.join(HERE, "node_modules", ".bin", "tree-sitter")
        if os.name == "nt":
            cli += ".cmd"
        proc = subprocess.run(
            [cli, "parse", "-q", "--paths", list_path],
            cwd=HERE,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            shell=False,
        )
    finally:
        os.unlink(list_path)
    failures = []
    for line in proc.stdout.splitlines():
        m = FAIL_LINE.match(line)
        if m:
            failures.append(m.group(1).strip())
    return failures


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("corpus")
    ap.add_argument("games", nargs="*")
    ap.add_argument("--failures", type=int, default=0, help="print up to N failing paths per game")
    ap.add_argument("--json", help="write the counts as JSON")
    args = ap.parse_args()
    games = args.games or sorted(
        d for d in os.listdir(args.corpus)
        if os.path.isdir(os.path.join(args.corpus, d)) and not d.startswith("_")
    )
    results = {}
    total_files = total_ok = 0
    for g in games:
        files = game_files(os.path.join(args.corpus, g))
        if not files:
            continue
        failures = run_game(files)
        ok = len(files) - len(failures)
        results[g] = {"files": len(files), "error_free": ok, "failures": failures}
        total_files += len(files)
        total_ok += ok
        print(f"{g:8} {len(files):6} files  {ok:6} error-free  {100.0 * ok / len(files):6.2f}%")
        for p in failures[: args.failures]:
            print("    " + p)
        sys.stdout.flush()
    if total_files:
        print(f"{'all':8} {total_files:6} files  {total_ok:6} error-free  "
              f"{100.0 * total_ok / total_files:6.2f}%")
    if args.json:
        with open(args.json, "w", encoding="utf-8") as f:
            json.dump(results, f, indent=1)


if __name__ == "__main__":
    main()
