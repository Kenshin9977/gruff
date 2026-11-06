# Official BO3 compiler harness

`gsc_link.py` compiles `.gsc`/`.csc` files with the official Black Ops III linker from the mod
tools (`bin/linker_modtools.exe`) and prints the verdict as JSON. It is the differential oracle for
the T7 parser: the official compiler accepts or rejects, and says where.

Stdlib Python 3.11+, Windows, the BO3 mod tools installed. It never starts the game.

```sh
python -I gsc_link.py my.gsc                                # accept / reject + error lines
python -I gsc_link.py a.gsc lib.gsc=scripts/zz/lib.gsc h.gsh=scripts/zz/h.gsh
python -I gsc_link.py my.gsc --disasm                       # + acts disassembly and import table
python -I gsc_link.py my.gsc --gscc-out <dir>               # + the compiled .gscc and its .gdb line table (your own scripts only)
python -I run_probes.py --out <dir>                         # every probe in probes/, one JSON each
python -I gscc_imports.py file.gscc --names word1 word2     # export/import tables of a .gscc
```

Exit status: 0 accepted, 1 rejected, 2 harness error (timeout, linker missing).

## How it works

The mod tools have no standalone script compiler. Scripts are compiled when the linker builds a
zone that lists them. For each run the harness:

1. creates `<bo3>/mods/zz_gsc_harness/` (wiped first if left over), copies each input to its
   virtual path (default `scripts/zz_harness/<name>`; give `file=scripts/x/y.gsc` to choose, which
   matters for `#using` and `#insert`), and writes `zone_source/zm_mod.zone` with one
   `scriptparsetree,<path>` line per `.gsc`/`.csc` (`.gsh` files are only copied);
2. runs, from `<bo3>/bin`, with `TA_GAME_PATH`, `TA_TOOLS_PATH` and `TA_LOCAL_ASSET_CACHE` set:

   ```
   linker_modtools.exe -language english -fs_game zz_gsc_harness -modsource zm_mod
   ```

   (`-modsource` takes the zone name; the mod is named by `-fs_game`. Passing the mod name to
   `-modsource` fails with "Linker no longer supports linking more than one file");
3. parses the output. Errors look like
   `ERR(0) scripts/zz_harness/x.gsc (6,4) in "main()" : syntax error, unexpected TOKEN_EQUALS, expecting TOKEN_LEFT_PAREN : <source line>`;
   the harness returns them as `{file, line, col, function, message}` and keeps the other
   error/warning lines (`UNRECOVERABLE ERROR`, `SCRIPT ERROR: No generated data ...`) in
   `other_lines`. A file is accepted when the linker exits 0 and writes `zone/zm_mod.ff`;
4. with `--disasm`, dumps the `scriptparsetree` assets of the fastfile with
   `acts fastfile -r bo3`, disassembles each `.gscc` with `acts gscd -a`, and reads its export and
   import tables (`gscc_imports.py`; names resolved by hashing every identifier of the inputs);
5. deletes the mod folder (`--keep` keeps it) and the temporary folder.

A lock file `<bo3>/mods/zz_gsc_harness.lock` serialises runs. A link takes 1.5 to 5 s.

## Behaviour of the official compiler worth knowing

- It stops at the **first** syntax error of a file: one error line per run.
- It does **not** resolve calls: a call to a function that exists nowhere compiles and links
  (`probes/bare_call_builtin_only.gsc`). Name errors appear only when the game loads the zone, so
  this oracle covers syntax and compile-time semantics, not "undefined function".
- Side effects outside the mod folder: the linker opens the mod tools' GDT database, so
  `<bo3>/gdtdb/gdt.db-shm` (SQLite shared memory) gets a new timestamp. No shipped file changes.

## Probes

`probes/` holds small scripts written for this project (never game scripts). The first line
`// Probe <question>: ...` says what each one asks; `// probe-with: f.gsc` links a helper beside
it. Their verdicts, with disassembly and import tables, are in
`gruff-data/evidence/linker-t7/<probe>.json`.

## Settings

| Option | Env | Default |
|---|---|---|
| `--bo3` | `GSC_BO3_ROOT` | `C:\Program Files (x86)\Steam\steamapps\common\Call of Duty Black Ops III` |
| `--acts` | `GSC_ACTS` | `acts` on `PATH` (atian-cod-tools 3.3.0 was used) |
| `--mod` | | `zz_gsc_harness` (must start with `zz_`) |
| `--zone` | | `zm_mod` |
| `--timeout` | | 300 s |
| `--raw` | | add the full linker output to the JSON |
