# The local GSC corpus

The test corpus of the GSC toolchain: every script of every Call of Duty game installed on the
machine, exported from the user's own installs, plus the mod tools' plain sources.

**The corpus is Activision's code. It must never be committed, pushed, uploaded, attached to an
issue or shared in any form.** It lives outside every git repository
(`gsc-toolchain/corpus/`, beside `gruff/`, not inside it) and each machine rebuilds its own from
its own installs. Only these scripts are versioned; a bug report carries a reduced fixture written
by hand, never a corpus file.

## Layout

```
corpus/
  <game>/source/            plain source the game ships (CoD1 to BO1, MW2, CoD Online)
  <game>/decompiled/        bytecode decompiled by acts (Treyarch, MWIII) or gsc-tool (IW, SH)
  <game>/source-modtools/   the mod tools' raw scripts (t7: BO3, t5: BO1)
  <game>/_export/           GrayWolf's report, export.log and run.json for that export
  _runs.tsv                 one row per export run: start, end, wall time, counts
  MANIFEST.tsv              one row per file (below)
  SUMMARY.md                counts, sizes, decompiler failures, missing games
```

`<game>` is the engine's code name: iw1 (CoD1), iw2 (CoD2), iw3 (CoD4), t4 (WaW), iw4 (MW2),
iw5 (MW3), t5 (BO1), t6 (BO2), t7 (BO3), t8 (BO4), t9 (Cold War), iw6 (Ghosts), s1 (AW),
h1 (MWR), iw7 (IW), s2 (WWII), iw8 (MW2019), jup / jup_sp (MWIII multiplayer / campaign),
codol (CoD Online).

A compiled script the decompiler failed on is kept as bytecode beside the others
(`.gscc`/`.cscc` for Treyarch, `.gscbin`/`.cscbin` for IW): status `bytecode` in the manifest.
GrayWolf writes one file per script path: a script several zones hold is kept once, from the last
zone that wrote it.

## Rebuilding it on another machine

What it needs, all local:

- **GrayWolf** (github.com/Kenshin9977/GrayWolf, built per its BUILDING.md), with
  `oo2core_6_win64.dll` / `oo2core_8_win64.dll` beside the exe (from the user's own game installs),
  its `zonekit/profiles` and `package_index` (the build puts them there). Run it from a copy of its
  output folder so that a rebuild of the checkout never meets a running export (LNK1104).
- **acts** (github.com/ate47/atian-cod-tools, 3.3.0 here) with its `deps/` dumps
  (`BlackOpsColdWar_dump.exe`...), and **gsc-tool** (github.com/xensik/gsc-tool, 1.5.1 here).
- For MWIII, the dumped game exes (`--game-exe`, the string literals' decryption); without them
  the scripts decompile with their strings left encrypted.
- Python 3.11+ (stdlib only).

Steps:

1. Copy `games.example.json` (placeholder paths) to `games.local.json` beside it and set the paths
   (`games.local.json` is gitignored and is the scripts' default `--config`; never commit it): GrayWolf, acts, gsc-tool,
   the corpus and a staging folder, and each game's install. Remove the games you do not have.
   A game's `input` is what `GrayWolf disk-export` takes: an install or zone folder, a `.ff`,
   `casc:<glob>` with `install` (Battle.net games), or a `.txt` list of zones. `zones_glob` (with
   `zones_exclude_suffix`) makes that list from a folder, without the transient `_tr` zones (they
   hold no script). MW2019's list globs GrayWolf's `%TEMP%\greyhound_iw8ff` cache of extracted
   zones; where that cache is empty, use `"input": "casc:*.ff"` (slow: 25 000 transient zones) or
   fill the cache first.
2. Export, one game at a time (GrayWolf runs one export and one memory pool at a time; never run
   two exports side by side). Long: about an hour for everything here, mostly BO4 and Cold War.
   ```
   python -I export_corpus.py                  # every game not exported yet
   python -I export_corpus.py --only t7 --force
   ```
   Games already exported (a kept report with `result=ok`) are skipped, so a cut run resumes.
3. Copy the mod tools' sources: `<BO3>/share/raw/scripts` into `corpus/t7/source-modtools/scripts`,
   and the `.gsc`/`.csc` of `<BO1 mod tools>/raw` (relative paths kept) into
   `corpus/t5/source-modtools/`.
4. Measure it:
   ```
   python -I build_manifest.py [--config <file>] [--corpus <dir>]
   ```

Nothing here launches a game, attaches to a process or writes to an install: GrayWolf reads the
files only, and the game data it reads is treated as data.

## MANIFEST.tsv

Tab-separated, one row per file: `game`, `kind` (`source`, `decompiled`, `source-modtools`,
`external`), `path` (relative to the corpus; absolute for an external corpus), `sha256`, `size`,
`origin_zone`, `decompiler` (`none`, `acts`, `gsc-tool`; `... (failed)` for a file left as
bytecode), `decompiler_version`, `status` (`source`, `decompiled`, `bytecode`, `empty`, `other`).

`origin_zone` is read from GrayWolf's `export.log`: each zone's window, from "zone X opened" to its
"X script: ... written" line, and the file's write time inside one of them. It is the zone that
wrote the kept version. External corpora (`external` in the config: the renamed BO3 tree
`t7-src`, the frozen BO7 archive) are listed by absolute path and never copied.
