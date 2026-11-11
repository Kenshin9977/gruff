"""Re-measure what the rules report on the local corpora, and compare with the records of
docs/rules/precision.toml.

    py -3.11 -I tools/precision.py [--gruff PATH] [--rule CODE ...]

It counts findings; it cannot judge them. A record whose `reports` no longer matches today's count
is stale: the sample it was adjudicated on is not what the rule reports any more, so the verdicts
behind its `true` and `false` do not describe the current rule. Judge the new findings by hand and
write the record again (the rule's own page under docs/rules/ holds the reasoning).

Exit status is 1 when a record is stale, so a run can be wired into a check.
"""
import argparse
import os
import subprocess
import sys
import tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
TOOLCHAIN = os.path.dirname(ROOT)
RECORDS = os.path.join(ROOT, "docs", "rules", "precision.toml")
CORPUS = os.path.join(TOOLCHAIN, "corpus")


def corpora():
    """(game id, folder) for every corpus folder, the game id as the catalogues spell it."""
    out = []
    for game in sorted(os.listdir(CORPUS)):
        for sub in ("source", "decompiled", "source-modtools", "_export"):
            d = os.path.join(CORPUS, game, sub)
            if os.path.isdir(d):
                out.append((game.replace("_sp", "-sp"), d))
    return out


def count(gruff, game, folder, code):
    """How many `code` findings on this folder."""
    p = subprocess.run(
        [gruff, "check", folder, "--game", game, "--preview", "--select", code,
         "--output-format", "concise", "--quiet"],
        capture_output=True, text=True, encoding="utf-8", errors="replace")
    return sum(1 for ln in p.stdout.splitlines() if (" %s " % code) in ln)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--gruff", default=os.path.join(
        ROOT, "target", "x86_64-pc-windows-msvc", "release",
        "gruff.exe" if os.name == "nt" else "gruff"))
    ap.add_argument("--rule", action="append", default=[])
    a = ap.parse_args()
    if not os.path.isfile(a.gruff):
        sys.exit("no gruff at %s (pass --gruff)" % a.gruff)
    with open(RECORDS, "rb") as fh:
        records = tomllib.load(fh)
    records.pop("gate", None)
    codes = a.rule or sorted(records)
    stale = []
    for code in codes:
        rec = records.get(code)
        if rec is None:
            print("  %-6s no record" % code)
            continue
        now = sum(count(a.gruff, game, folder, code) for game, folder in corpora())
        was = rec["reports"]
        mark = "ok" if now == was else "STALE"
        print("  %-6s recorded %4d  now %4d  %s" % (code, was, now, mark))
        if now != was:
            stale.append((code, was, now))
    if stale:
        print("\n%d record%s no longer describes what the rule reports:"
              % (len(stale), "" if len(stale) == 1 else "s"))
        for code, was, now in stale:
            print("  %s: adjudicated on %d findings, the rules now give %d" % (code, was, now))
        print("Judge the new findings and write the record again.")
        return 1
    print("\nevery record matches today's counts")
    return 0


if __name__ == "__main__":
    sys.exit(main())
