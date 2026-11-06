# Measure the local GSC corpus: writes <corpus>/MANIFEST.tsv (one row per file) and
# <corpus>/SUMMARY.md. Stdlib only; run with `python -I`. Details: README.md beside this file.
import argparse
import bisect
import datetime
import hashlib
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_CORPUS = os.path.normpath(os.path.join(HERE, "..", "..", "..", "corpus"))
DEFAULT_CONFIG = os.path.join(HERE, "games.local.json")  # gitignored; games.example.json is the template


def require_config(path):
    """Exit with a how-to when the games config is missing."""
    if not os.path.isfile(path):
        sys.exit(f"config not found: {path}\n"
                 f"Copy {os.path.join(HERE, 'games.example.json')} to games.local.json beside it "
                 "(gitignored), set this machine's paths, or pass --config <file>.")
KINDS = ("source", "decompiled", "source-modtools")
BYTECODE_EXT = (".gscc", ".cscc", ".gscbin", ".cscbin")
SCRIPT_EXT = (".gsc", ".csc", ".gsh") + BYTECODE_EXT
# GrayWolf game id -> the decompiler its script export drives (ScriptExport.h's Vm per game)
ACTS_GAMES = {"bo2", "bo3", "bo4", "coldwar", "mwiii", "mwiii_sp", "bo6", "bo6sp", "bo7"}
GSCTOOL_GAMES = {"mw3", "ghosts", "aw", "mwr", "iw", "ww2", "mw2019"}

TS = r"\[(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d\.\d{3})\]"
ZONE_OPEN = re.compile(TS + r".*?\s+zone (\S+) \((\d+)/(\d+)\) opened")
ZONE_DONE = re.compile(TS + r".*?\s{3,}(\S+) (?:script|rawfile): (\d+) seen, (\d+) written")


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def tool_version(cmd, pattern):
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=30, stdin=subprocess.DEVNULL)
        m = re.search(pattern, p.stdout + p.stderr)
        return m.group(1) if m else ""
    except (OSError, subprocess.SubprocessError):
        return ""


def parse_ts(text):
    return datetime.datetime.strptime(text, "%Y-%m-%d %H:%M:%S.%f").timestamp()


class ZoneWindows:
    # Each zone's script window in GrayWolf's export.log: from "zone X opened" to "X script: ...
    # written". A file's origin zone is the window holding its write time: the zone that wrote its
    # final version (a script several zones hold is written by each, the last one kept).
    def __init__(self, log_path):
        self.starts, self.ends, self.zones, self.written = [], [], [], {}
        if not os.path.exists(log_path):
            return
        opened = {}
        with open(log_path, encoding="utf-8", errors="replace") as f:
            for line in f:
                m = ZONE_OPEN.match(line)
                if m:
                    opened[m.group(2)] = parse_ts(m.group(1))
                    continue
                m = ZONE_DONE.match(line)
                if m and m.group(2) in opened:
                    zone = m.group(2)
                    self.starts.append(opened.pop(zone) - 0.002)
                    self.ends.append(parse_ts(m.group(1)) + 0.002)
                    self.zones.append(zone)
                    self.written[zone] = self.written.get(zone, 0) + int(m.group(4))
        order = sorted(range(len(self.starts)), key=lambda i: self.starts[i])
        self.starts = [self.starts[i] for i in order]
        self.ends = [self.ends[i] for i in order]
        self.zones = [self.zones[i] for i in order]

    def zone_of(self, mtime):
        i = bisect.bisect_right(self.starts, mtime) - 1
        if i >= 0 and mtime <= self.ends[i]:
            return self.zones[i]
        return ""


def status_of(kind, name, size):
    low = name.lower()
    if low.endswith(BYTECODE_EXT):
        return "bytecode"
    if size == 0:
        return "empty"
    return "source" if kind != "decompiled" else "decompiled"


def main():
    ap = argparse.ArgumentParser(description="Write MANIFEST.tsv and SUMMARY.md for the local GSC corpus")
    ap.add_argument("--corpus", default=DEFAULT_CORPUS)
    ap.add_argument("--config", default=DEFAULT_CONFIG,
                    help="games JSON with this machine's paths (default: games.local.json here)")
    ap.add_argument("--acts", default=None, help="acts.exe, for its version (default: from the config)")
    ap.add_argument("--gsc-tool", default=None, help="gsc-tool.exe, for its version (default: from the config)")
    a = ap.parse_args()
    corpus = os.path.abspath(a.corpus)
    require_config(a.config)
    with open(a.config, encoding="utf-8") as f:
        cfg = json.load(f)
    acts = a.acts or cfg.get("acts", "")
    gsct = a.gsc_tool or cfg.get("gsc_tool", "")
    acts_v = tool_version([acts, "-h"], r"Atian tools (\S+)") if acts else ""
    gsct_v = tool_version([gsct, "--version"], r"GSC Tool (\S+)") if gsct else ""
    games = {g["key"]: g for g in cfg.get("games", [])}

    def decompiler_of(key, kind, run):
        if kind == "source-modtools":
            return "none", "mod tools source"
        if kind == "source":
            return "none", "plain source in the game files"
        gid = (run or {}).get("id") or games.get(key, {}).get("id", "")
        if gid in ACTS_GAMES:
            return "acts", acts_v
        if gid in GSCTOOL_GAMES:
            return "gsc-tool", gsct_v
        return "unknown", ""

    rows, stats = [], {}
    for key in sorted(os.listdir(corpus)):
        gdir = os.path.join(corpus, key)
        if not os.path.isdir(gdir) or key.startswith("_"):
            continue
        meta = os.path.join(gdir, "_export")
        run = None
        if os.path.exists(os.path.join(meta, "run.json")):
            with open(os.path.join(meta, "run.json"), encoding="utf-8") as f:
                run = json.load(f)
        windows = ZoneWindows(os.path.join(meta, "export.log"))
        for kind in KINDS:
            kdir = os.path.join(gdir, kind)
            if not os.path.isdir(kdir):
                continue
            dec, ver = decompiler_of(key, kind, run)
            st = stats.setdefault((key, kind), {"files": 0, "bytes": 0, "source": 0, "decompiled": 0,
                                                "bytecode": 0, "empty": 0, "other": 0, "zoned": 0,
                                                "per_zone": {}, "bytecode_files": [], "run": run,
                                                "windows": windows})
            for root, _, files in os.walk(kdir):
                for name in sorted(files):
                    p = os.path.join(root, name)
                    s = os.stat(p)
                    rel = os.path.relpath(p, corpus).replace(os.sep, "/")
                    status = status_of(kind, name, s.st_size) if name.lower().endswith(SCRIPT_EXT) else "other"
                    zone = windows.zone_of(s.st_mtime) if kind != "source-modtools" else ""
                    if kind == "source-modtools":
                        zone = "(mod tools raw)"
                    st["files"] += 1
                    st["bytes"] += s.st_size
                    st[status] += 1
                    if zone and kind != "source-modtools":
                        st["zoned"] += 1
                        st["per_zone"][zone] = st["per_zone"].get(zone, 0) + 1
                    if status == "bytecode":
                        st["bytecode_files"].append(rel)
                    rows.append((key, kind, rel, sha256(p), s.st_size, zone, dec if status != "bytecode" else dec + " (failed)",
                                 ver, status))

    ext_stats = []
    for ext in cfg.get("external", []):
        root_dir = ext["path"]
        n = size = 0
        if os.path.isdir(root_dir):
            for root, _, files in os.walk(root_dir):
                for name in sorted(files):
                    if not name.lower().endswith(SCRIPT_EXT):
                        continue
                    p = os.path.join(root, name)
                    s = os.stat(p)
                    n += 1
                    size += s.st_size
                    status = "bytecode" if name.lower().endswith(BYTECODE_EXT) else ("empty" if s.st_size == 0 else "decompiled")
                    rows.append((ext["key"], "external", os.path.abspath(p).replace(os.sep, "/"), sha256(p), s.st_size, "",
                                 ext.get("decompiler", ""), ext.get("version", ""), status))
        ext_stats.append((ext, n, size))

    with open(os.path.join(corpus, "MANIFEST.tsv"), "w", encoding="utf-8", newline="\n") as f:
        f.write("game\tkind\tpath\tsha256\tsize\torigin_zone\tdecompiler\tdecompiler_version\tstatus\n")
        for r in rows:
            f.write("\t".join(str(x) for x in r) + "\n")

    write_summary(corpus, cfg, games, stats, ext_stats, acts_v, gsct_v, len(rows))
    print(f"MANIFEST.tsv: {len(rows)} rows; SUMMARY.md written in {corpus}")


def fmt_size(n):
    for unit in ("B", "KB", "MB", "GB"):
        if n < 1024 or unit == "GB":
            return f"{n:.1f} {unit}" if unit != "B" else f"{n} B"
        n /= 1024


def read_report(path):
    out, failures = {}, []
    if os.path.exists(path):
        with open(path, encoding="utf-8", errors="replace") as f:
            for line in f:
                k, sep, v = line.rstrip("\r\n").partition("=")
                if not sep:
                    continue
                if k == "failure":
                    failures.append(v)
                elif k not in out:
                    out[k] = v
    return out, failures


def write_summary(corpus, cfg, games, stats, ext_stats, acts_v, gsct_v, nrows):
    total_files = sum(s["files"] for s in stats.values())
    total_bytes = sum(s["bytes"] for s in stats.values())
    L = ["# GSC corpus summary", "",
         f"Generated {datetime.datetime.now():%Y-%m-%d %H:%M} by gruff/tools/corpus/build_manifest.py. "
         "Local only: never commit, upload or share any file of this folder.", "",
         f"Decompilers: acts {acts_v or '?'}, gsc-tool {gsct_v or '?'}. "
         f"MANIFEST.tsv: {nrows} rows. In this folder: {total_files} files, {fmt_size(total_bytes)}.", "",
         "## Per game", "",
         "| game | title | kind | files | source | decompiled | bytecode (failed) | empty | size | zones | export wall time | origin zone known |",
         "|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|"]
    for (key, kind), s in sorted(stats.items()):
        run = s["run"] or {}
        title = games.get(key, {}).get("title", "")
        wall = f"{run['wall_s']:.0f} s" if run.get("wall_s") is not None and kind != "source-modtools" else ""
        zones = run.get("zones", "") if kind != "source-modtools" else ""
        known = f"{s['zoned']}/{s['files']}" if kind != "source-modtools" else ""
        L.append(f"| {key} | {title} | {kind} | {s['files']} | {s['source']} | {s['decompiled']} | {s['bytecode']} | "
                 f"{s['empty']} | {fmt_size(s['bytes'])} | {zones} | {wall} | {known} |")
    L += ["", "`empty`: a script without code (a header-only GSCOBJ), written empty by GrayWolf. "
          "`origin zone known`: files whose write time falls in one zone's window of export.log "
          "(the zone that wrote the kept version; a script several zones hold is kept once). "
          "In a `decompiled` folder, `decompiled` counts every readable file: the IW games also ship some "
          "scripts as plain source beside the bytecode (the report's `scripts seen` minus `decompiled` minus "
          "`decompile failed`). `scripts seen` counts every zone's copy, `files` the kept paths. "
          "codol: a rawfile export, its .gsc/.csc kept.", "",
          "## Export reports (GrayWolf)", "",
          "| game | result | zones | zones failed | scripts seen | written | decompiled | decompile failed | runs cut short | strings left encrypted | failures |",
          "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|"]
    fail_lines = []
    for key in sorted({k for k, _ in stats}):
        rep, failures = read_report(os.path.join(corpus, key, "_export", "report.txt"))
        if not rep:
            continue
        L.append(f"| {key} | {rep.get('result', '')} | {rep.get('zones', '')} | {rep.get('zones_failed', '')} | "
                 f"{rep.get('scripts', '')} | {rep.get('scripts_written', '')} | {rep.get('scripts_decompiled', '')} | "
                 f"{rep.get('scripts_decompile_failed', '')} | {rep.get('scripts_decompiler_runs_cut_short', '')} | "
                 f"{rep.get('scripts_strings_encrypted', rep.get('scripts_literals_encrypted', ''))} | {len(failures)} |")
        for fl in failures[:20]:
            fail_lines.append(f"- {key}: `{fl}`")
        if len(failures) > 20:
            fail_lines.append(f"- {key}: ... {len(failures) - 20} more in {key}/_export/report.txt")
    L += ["", "## Decompiler failures (files kept as bytecode)", ""]
    any_bc = False
    for (key, kind), s in sorted(stats.items()):
        if s["bytecode_files"]:
            any_bc = True
            L.append(f"- **{key}** ({len(s['bytecode_files'])}): " + ", ".join(f"`{p}`" for p in s["bytecode_files"][:40])
                     + (" ..." if len(s["bytecode_files"]) > 40 else ""))
    if not any_bc:
        L.append("None.")
    if fail_lines:
        L += ["", "Export failures reported by GrayWolf:", ""] + fail_lines
    L += ["", "## External corpora (referenced, not copied)", "",
          "| key | path | files | size | note |", "|---|---|---:|---:|---|"]
    for ext, n, size in ext_stats:
        L.append(f"| {ext['key']} | `{ext['path']}` | {n} | {fmt_size(size)} | {ext.get('note', '')} |")
    L += ["", "## Missing or skipped games", ""]
    for g in cfg.get("games", []):
        if g.get("skip"):
            L.append(f"- {g['key']} ({g.get('title', g['id'])}): skipped, {g['skip']}")
    for m in cfg.get("missing", []):
        L.append(f"- {m['key']} ({m['title']}): {m['why']}")
    with open(os.path.join(corpus, "SUMMARY.md"), "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(L) + "\n")


if __name__ == "__main__":
    sys.exit(main())
