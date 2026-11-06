# Export every installed game's scripts into the local GSC corpus through GrayWolf, one game at a
# time. Reads the installs only. Usage and config format: README.md beside this file.
import argparse
import datetime
import glob
import json
import os
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_CONFIG = os.path.join(HERE, "games.local.json")  # gitignored; games.example.json is the template

def log(msg):
    print(f"[{datetime.datetime.now():%Y-%m-%d %H:%M:%S}] {msg}", flush=True)


def read_report(path):
    out = {}
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            for line in f:
                k, sep, v = line.rstrip("\r\n").partition("=")
                if sep and k not in out:
                    out[k] = v
    except OSError:
        pass
    return out


def zone_list(game, work):
    # a .txt list of zones for disk-export, without the excluded suffix (transient _tr zones)
    excl = game.get("zones_exclude_suffix", "").lower()
    names = []
    for pattern in game["zones_glob"] if isinstance(game["zones_glob"], list) else [game["zones_glob"]]:
        names += [p for p in glob.glob(os.path.expandvars(pattern)) if not (excl and p.lower().endswith(excl))]
    names = sorted(set(names))
    path = os.path.join(work, f"{game['key']}_zones.txt")
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(n.replace(os.sep, "/") for n in names) + "\n")
    return path, len(names)


def append_run(corpus, row):
    runs = os.path.join(corpus, "_runs.tsv")
    cols = ["key", "id", "kind", "start", "end", "wall_s", "rc", "result", "zones", "scripts",
            "decompiled", "decompile_failed", "bytecode"]
    new = not os.path.exists(runs)
    with open(runs, "a", encoding="utf-8", newline="\n") as f:
        if new:
            f.write("\t".join(cols) + "\n")
        f.write("\t".join(str(row.get(c, "")) for c in cols) + "\n")


def export_game(cfg, game, force):
    key, gid, kind = game["key"], game["id"], game["kind"]
    corpus = cfg["corpus"]
    dest = os.path.join(corpus, key, kind)
    meta = os.path.join(corpus, key, "_export")
    report_kept = os.path.join(meta, "report.txt")
    if not force and read_report(report_kept).get("result") == "ok" and os.path.isdir(dest):
        log(f"{key}: already exported (report ok), skipped; --force to redo")
        return
    staging = os.path.join(cfg["staging"], key)
    shutil.rmtree(staging, ignore_errors=True)
    os.makedirs(staging, exist_ok=True)
    work = os.path.join(cfg["staging"], "_work")
    os.makedirs(work, exist_ok=True)

    inp = os.path.expandvars(game.get("input", ""))
    if game.get("zones_glob"):
        inp, n = zone_list(game, work)
        log(f"{key}: {n} zones listed in {inp}")
    report = os.path.join(work, f"{key}_report.txt")
    if os.path.exists(report):
        os.remove(report)
    cmd = [cfg["graywolf"], "disk-export", "--game", gid, "--type", game.get("type", "script"), "--decompile",
           "--out", staging, "--report", report]
    if cfg.get("acts"):
        cmd += ["--acts-exe", cfg["acts"]]
    if cfg.get("gsc_tool"):
        cmd += ["--gsc-tool-exe", cfg["gsc_tool"]]
    if game.get("install"):
        cmd += ["--install", game["install"]]
    cmd += game.get("args", [])
    cmd.append(inp)
    log(f"{key}: start {cmd}")
    start = datetime.datetime.now()
    t0 = time.monotonic()
    rc = subprocess.run(cmd, stdin=subprocess.DEVNULL).returncode
    wall = time.monotonic() - t0
    end = datetime.datetime.now()
    rep = read_report(report)
    log(f"{key}: end rc={rc} result={rep.get('result')} reason={rep.get('reason')} wall={wall:.0f}s "
        f"zones={rep.get('zones')} scripts={rep.get('scripts')} decompiled={rep.get('scripts_decompiled')} "
        f"failed={rep.get('scripts_decompile_failed')}")

    # GrayWolf writes <staging>/<id>/<kind folder>/... and <staging>/<id>/export.log; with
    # keep_ext (a rawfile export), only the files with those extensions are kept
    src = os.path.join(staging, gid, game.get("folder", "scripts"))
    if os.path.isdir(dest):
        shutil.rmtree(dest)
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    keep = tuple(e.lower() for e in game.get("keep_ext", []))
    if os.path.isdir(src) and not keep:
        shutil.move(src, dest)
    else:
        os.makedirs(dest, exist_ok=True)
        for root, _, files in os.walk(src):
            for name in files:
                if name.lower().endswith(keep):
                    rel = os.path.relpath(os.path.join(root, name), src)
                    os.makedirs(os.path.dirname(os.path.join(dest, rel)), exist_ok=True)
                    shutil.move(os.path.join(root, name), os.path.join(dest, rel))
    os.makedirs(meta, exist_ok=True)
    if os.path.exists(report):
        shutil.copy2(report, report_kept)
    elog = os.path.join(staging, gid, "export.log")
    if os.path.exists(elog):
        shutil.copy2(elog, os.path.join(meta, "export.log"))
    run = {"key": key, "id": gid, "kind": kind, "cmd": cmd, "start": start.isoformat(timespec="seconds"),
           "end": end.isoformat(timespec="seconds"), "wall_s": round(wall, 1), "rc": rc,
           "result": rep.get("result", ""), "reason": rep.get("reason", ""), "zones": rep.get("zones", ""),
           "scripts": rep.get("scripts", ""), "decompiled": rep.get("scripts_decompiled", ""),
           "decompile_failed": rep.get("scripts_decompile_failed", ""), "bytecode": rep.get("scripts_bytecode", "")}
    with open(os.path.join(meta, "run.json"), "w", encoding="utf-8") as f:
        json.dump(run, f, indent=1)
    append_run(corpus, run)
    shutil.rmtree(staging, ignore_errors=True)


def main():
    ap = argparse.ArgumentParser(description="Export installed games' scripts into the GSC corpus (GrayWolf)")
    ap.add_argument("--config", default=DEFAULT_CONFIG,
                    help="games JSON with this machine's paths (default: games.local.json here; see README.md)")
    ap.add_argument("--only", default="", help="comma-separated game keys")
    ap.add_argument("--force", action="store_true", help="redo games already exported")
    a = ap.parse_args()
    if not os.path.isfile(a.config):
        sys.exit(f"config not found: {a.config}\n"
                 f"Copy {os.path.join(HERE, 'games.example.json')} to games.local.json beside it "
                 "(gitignored), set this machine's paths, or pass --config <file>.")
    with open(a.config, encoding="utf-8") as f:
        cfg = json.load(f)
    only = {k for k in a.only.split(",") if k}
    log(f"export_corpus start, {len(cfg['games'])} games in config")
    for game in cfg["games"]:
        if only and game["key"] not in only:
            continue
        if game.get("skip"):
            log(f"{game['key']}: skipped ({game['skip']})")
            continue
        try:
            export_game(cfg, game, a.force)
        except Exception as e:  # one game's failure does not stop the others
            log(f"{game['key']}: ERROR {e!r}")
    log("export_corpus end")


if __name__ == "__main__":
    sys.exit(main())
