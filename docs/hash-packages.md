# Hash packages

Black Ops III, Black Ops 4 to Black Ops 7 and Modern Warfare III compile most script identifiers
to hashes. A decompiler prints what it cannot name by hash: `function_1a2b3c4d`,
`namespace_5e6f...`, `var_...`, `#"hash_..."`, `script_<hex>` file names. Gruff handles these
games in two steps:

1. **Hashes unify with names.** Every function, namespace and script file is indexed by the hash
   of its name as well as by the name, with the game's families. A definition the decompiler named
   by hash and a call it printed by name are the same symbol: `foo()` reaches
   `function_<hash of foo>`, `#using script_<hex>` reaches the file kept under its clear path, a
   builtin spelled `function_<hex>` is the builtin. No name list is needed for that.
2. **Hash packages give names back.** A package is a list of names; the toolchain hashes each one
   itself with the game's families, so a plain name list suffices. The language server shows a
   hash a package (or a clear name elsewhere in the workspace) names as `name (#hash)` in hover,
   completion, document symbols and diagnostics, and an unknown one as `#hash`. Completion offers
   the recovered name: the compiler hashes it back to the same value.

## The families per game

`gruff hash --list --game <id>` prints them.

| game | functions, classes | fields, locals | namespaces | events | `#"..."` | script files |
|---|---|---|---|---|---|---|
| BO3 (`t7`) | `t7.canon` | `t7.canon` | `t7.canon` | `t7.canon` | `t7.canon` | - |
| BO4, Cold War (`t8`, `t9`) | `t89.scr` | `t89.scr` | `t89.scr` | `t89.scr` | `t8.fnv64` | `t8.fnv64` of the path with its extension |
| MWII, MWIII (`iw9`, `jup`) | `iw9.scr` | `iw9.scr` | `iw9.scr` | `iw9.xhash` | `iw9.xhash` | `iw.asset` of the path with `.gsc` |
| BO6, BO7 (`t10`, `t11`) | `t10.function` | `t10.var` | `t10.namespace` | `t10.event` | `t10.xhash` | `t10.script` |

Values are compared at the family's width; dictionaries key the 63- and 64-bit families on their
low 60 bits, the width some tools print them at.

## Format

UTF-8 text, one entry per line, after a header of `# key: value` lines. The canonical
specification is `gruff-data/schema/hash-package.md` (CC0); in short:

```text
# gsc-hash-package: 1
# game: t8, t9
# source: the names of my mod
# licence: CC0-1.0
my_mod_init
my_mod_spawn_wave
# family: t89.scr
1a2b3c4d	my_mod_on_player_spawned
```

- **Header** (before the first entry): `gsc-hash-package` (format version, 1), `game` (profile
  ids, comma-separated, `*` or absent for any game), `family` (a family id; absent: every family
  of the game), `source`, `licence`. Other `# key: value` lines are kept as metadata; `#` lines
  without a colon are comments.
- **Entries**: a line is a **name**, hashed on load with the section's family, or with every
  family of the game when the section has none; or a **pair** `hex<TAB>name` (`0x`, `hash_`,
  `function_`... prefixes accepted). A pair under a `family` is kept as given (and counted
  unverified when the name does not re-hash); a pair without one is kept for each family of the
  game that re-hashes the name, and dropped otherwise.
- **Sections**: a later `# family: <id>` line starts a new section (`# family: *` returns to
  "every family").
- Blank lines are ignored; a byte-order mark and CRLF line ends are accepted. Names cannot hold a
  tab or start with `#`.

## Using packages

Packages are files or directories (every `.txt`, `.tsv` and `.names` file under it, recursively).
They add up, in this order:

- `--hash-package <path>` on `gruff check`, `gruff hash --reverse`, `gruff hash --coverage`,
  `gruff mcp`, `gruff index` and `gruff types` (repeatable; the last two resolve through the names and
  keep the scripts' spellings in their output);
- `hash_packages = [...]` in `gruff.toml`, relative to the file;
- the `gruff.hashPackages` setting of the editor (relative to the first workspace folder);
- `$GSC_HASH_PACKAGES`, a path list (`;`-separated on Windows, `:` elsewhere).

```sh
gruff hash --reverse --game t11 --hash-package ../names function_3e93ae4f7823fe81 '#"hash_75dff854da9c4d73"'
gruff hash --coverage --game jup --hash-package ../names path/to/scripts
```

`--reverse` prints one line per family that names a value; `--coverage` counts the identifiers of
a tree by where their name comes from (written in clear, a package, the tree's own clear names,
unknown). Packages for another game are skipped; the load summary goes to stderr.

## Building your own

Two legal sources need no one's name list:

- **Your own code.** Every identifier of your mod, one per line, is a package (`game: *`). It names
  your functions in a dump of the game you shipped them to.
- **Names you have the right to use.** Lists published under a licence that allows it, with the
  licence in the header.

Name lists recovered from a game's files (decompiled scripts, zone strings) are derived from
Activision's material, and lists taken from GPL projects carry the GPL: **neither goes into
gruff-data**, whose data is CC0. Keep them as local packages, outside every repository (the
repositories' `.gitignore` refuses `/hash-packages/`, `*.hashes.tsv` and `*.wni`).

`tools/hash_packages/convert.py` (Python 3.11+, stdlib only) converts the dictionaries a machine
already has, reading them and writing packages, never changing an input:

```sh
python -I tools/hash_packages/convert.py bo7-archive <bo7-gsc-archive>/names/all_names.tsv --game "t10, t11"
python -I tools/hash_packages/convert.py wni <GrayWolf>/package_index/bo7_names.wni --game t11
python -I tools/hash_packages/convert.py wni <...>/bo3_script_hashes.wni --names-only
python -I tools/hash_packages/convert.py csv <list.csv> --game t8,t9 [--family t8.fnv64]
python -I tools/hash_packages/convert.py scripts <a script tree with names>   # its identifiers
python -I tools/hash_packages/convert.py all     # every source of tools/hash_packages/sources.local.json
```

`sources.example.json` lists the kinds of source with placeholder paths and the licence line each
package gets; copy it to `sources.local.json` (gitignored) and set the paths. The output goes to
`hash-packages/` beside the repositories. `--names-only` drops the hashes: the names then serve
every game, rehashed with its own families (BO3 and BO7 names name a large part of MWIII, BO4 and
Cold War).

A 32-bit family (BO3, BO4, Cold War) holds four billion values: with hundreds of thousands of
names, a few values get a wrong name by chance. A name recovered for a 32-bit hash is probable,
not proven; 60- and 64-bit names do not collide in practice.
