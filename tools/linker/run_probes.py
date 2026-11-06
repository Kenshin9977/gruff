#!/usr/bin/env python3
"""Run every probe in probes/ through gsc_link.py --disasm; write <out>/<probe>.json (see README)."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--out", required=True, help="folder for <probe>.json")
    ap.add_argument("names", nargs="*", help="probe names (default: all)")
    args = ap.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    ok = True
    probes = sorted(p for p in (HERE / "probes").iterdir() if p.suffix in (".gsc", ".csc"))
    for p in probes:
        lines = p.read_text("latin-1").splitlines()
        if not lines or not lines[0].startswith("// Probe"):
            continue  # helpers start with "// Helper"
        if args.names and p.stem not in args.names:
            continue
        extra = [str(p.parent / ln.split(":", 1)[1].strip()) for ln in lines
                 if ln.startswith("// probe-with:")]
        cmd = [sys.executable, "-I", str(HERE / "gsc_link.py"), str(p), *extra, "--disasm"]
        cp = subprocess.run(cmd, capture_output=True, text=True)
        try:
            res = json.loads(cp.stdout)
        except json.JSONDecodeError:
            res = {"status": "harness-error", "stderr": cp.stderr[-2000:]}
        res["inputs"] = [{"path": i["path"]} for i in res.get("inputs", [])]  # drop local paths
        (out / f"{p.stem}.json").write_text(json.dumps(res, indent=1) + "\n", "utf-8")
        ok &= res["status"] in ("accept", "reject")
        print(f"{p.stem}\t{res['status']}\t{len(res.get('errors', []))} error line(s)")
    return 0 if ok else 2


if __name__ == "__main__":
    sys.exit(main())
