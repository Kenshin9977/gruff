# Gruff

A fast, editor-independent toolchain for **GSC**, the scripting language of the Call of Duty
series, from Call of Duty (2003) to Black Ops 7 (2025). One binary, `gruff`, gives you:

| Command | What it does |
|---|---|
| `gruff check` | A Ruff-style linter: syntax errors, syntax the selected game lacks, unknown or misspelled engine builtins, wrong argument counts, infinite loops without a wait, unknown entity fields. Fixes, `gruff.toml` configuration, suppression comments, text / JSON / GitHub / SARIF output. |
| `gruff format` | A formatter on a Prettier-style layout engine: it breaks long calls, conditions and chains to fit 100 columns, keeps every comment, and never changes the token stream (`--style minimal` only normalises whitespace). Files with syntax errors are left alone. `--check` and `--diff` for CI. |
| `gruff index` | A grep-able TSV index of a workspace (`SYMBOLS`, `CALLS`, `FIELDS`, `INCLUDES`, `EVENTS`), for scripts, search tools and local LLM agents. |
| `gruff parse` | The resilient parser on its own: diagnostics and corpus statistics, or the syntax tree. |
| `gruff hash` | Every identifier hash family of the series (`t7.canon`, `t8.fnv64`, `t10.function`...): hash names, list the families of a game, name hashes from hash packages (`--reverse`), measure a tree's name coverage (`--coverage`). |
| `gruff lsp` | A language server: diagnostics, quick fixes, go to definition, references, hover, completion, signature help, rename, call hierarchy, semantic highlighting, formatting. Works with VS Code (extension included), Neovim, Helix and any LSP client. |
| `gruff types` | The argument types the scripts pass to each engine builtin, as JSON (literals, builtin results, local flow, script parameters). |
| `gruff report` | A facts-only JSON report of a local script tree (feature counts, unknown calls and their argument counts, event and field names; no code, no strings), to share so that games without a catalogue get one. See [docs/community-reports.md](docs/community-reports.md). |
| `gruff mcp` | An MCP server for coding agents (opencode, Claude Code...): read-only tools for symbols, definitions, references, callers and callees, diagnostics, events, builtins and hashes, with compact paginated answers for small models. See [docs/mcp.md](docs/mcp.md). |

The engine is written in Rust and needs no runtime, and it collects no telemetry. Nothing reaches
the network except `gruff hash-package fetch`, which only runs when you ask for it, and only
towards the git repositories you name yourself in `gruff.toml`: checking, formatting, indexing and
the language server never open a connection.

> **Formerly `gsc-tools`.** The binary was `gsc`; `gsc` still works as a deprecated alias of
> `gruff` for one version (it prints a notice). `gsc.toml`, `$GSC_DATA` and a `gsc-data` data
> folder keep working (see [Configuration](#configuration-grufftoml)). Suppression comments
> stay `// gsc: ignore[...]` (`// gruff: ignore[...]` is accepted too).

> **Not affiliated with Activision.** Gruff is an independent community project. It is not
> affiliated with, endorsed or sponsored by Activision Publishing, Infinity Ward, Treyarch,
> Sledgehammer Games, Raven Software or any of their affiliates. **No game files are included**:
> no scripts (shipped, decompiled or dumped), no documentation text, no assets, no executables.
> You point the tools at your own game install, mod tools or dumps, and whatever they derive
> stays on your machine. "Call of Duty", "Black Ops", "Modern Warfare" and the other game names
> are trademarks of their respective owners, used here only to say which games the tools work
> with.

## Supported games

The language differs from game to game. Each game has a **dialect profile** in the separate
[gruff-data](../gruff-data) repository (CC0), and every fact in a profile carries a confidence
level: *verified-on-corpus* (measured by this project on the game's shipped scripts),
*documented* (a cited source says so) or *assumed*. Where a value is unknown, the tools stay
silent rather than guess. The full table of games and features, with the grade of every value,
is [gruff-data/docs/dialect-matrix.md](../gruff-data/docs/dialect-matrix.md).

| id | Game | Profile grade | Builtin catalogue |
|---|---|---|---|
| `iw1` | Call of Duty (2003) | verified-on-corpus | corpus-corrected, partial (SP module) |
| `iw1-uo` | United Offensive (2004) | core (inferred from `iw1`) | |
| `iw2` | Call of Duty 2 (2005) | verified-on-corpus | |
| `iw3` | Call of Duty 4 (2007) | verified-on-corpus | corpus-corrected, partial (MP only) |
| `t4` | World at War (2008) | verified-on-corpus | corpus-corrected, partial (SP only) |
| `iw4` | Modern Warfare 2 (2009) | verified-on-corpus | corpus-corrected, partial (names) |
| `t5` | Black Ops (2010) | verified-on-corpus | corpus-corrected |
| `iw5` | Modern Warfare 3 (2011) | documented | corpus-corrected |
| `codol` | Call of Duty Online | verified-on-corpus | |
| `t6` | Black Ops II (2012) | documented (two open questions) | table, corpus-checked |
| `iw6` | Ghosts (2013) | documented | corpus-corrected, partial (SP only) |
| `s1` | Advanced Warfare (2014) | documented | corpus-corrected, partial (names) |
| `t7` | Black Ops III (2015) | verified-on-corpus | table, corpus-checked, with entity fields |
| `iw7` | Infinite Warfare (2016) | documented | corpus-corrected, partial (names) |
| `h1` | Modern Warfare Remastered (2016) | documented | corpus-corrected, partial (names) |
| `s2` | WWII (2017) | documented | |
| `t8` | Black Ops 4 (2018) | documented | table, corpus-checked |
| `iw8` | Modern Warfare (2019) | documented | corpus-corrected, partial (names) |
| `t9` | Black Ops Cold War (2020) | documented | table, corpus-checked |
| `s4` | Vanguard (2021) | documented | |
| `iw9` | Modern Warfare II (2022) | documented | |
| `jup` | Modern Warfare III (2023) | documented | table, corpus-checked (`jup-sp` for the campaign) |
| `t10` | Black Ops 6 (2024) | documented | names only (arity unchecked by the engine), no corpus |
| `t11` | Black Ops 7 (2025) | documented | table, corpus-checked |

Black Ops III is the reference target: its mod tools ship readable scripts, and its builtins and
entity fields were extracted from the executable and checked against them. Every game gets
parsing, dialect checks, formatting, the index, the language server and its hash families. The
builtin rules (`B001`, `B002`, `B003`) and the field rule (`F001`) need a catalogue and are
silent without one. Engine event catalogues (gruff-data `events/`, the events each engine notifies,
with receiver and payload) exist for 13 games: CoD4, MW2, MW3, Ghosts, Advanced Warfare, MW
Remastered, Infinite Warfare, World at War, Black Ops, Black Ops II, Black Ops III, Black Ops 4
and Cold War. None is complete yet: they feed the event rules `T002`/`T003`, hover and event
navigation, and `T001` stays silent.

Builtin catalogue levels: *table* means the executable's tables store each builtin's arity and
the engine checks it; *corpus-checked* means the game's own scripts were linted against the
catalogue (residual contradictions are listed in `tools/re/reports/calibrate.md`);
*corpus-corrected* means the arity was measured by running each handler under emulation and
then corrected where the game's scripts contradict it (the measure is kept in the data).
*Partial* catalogues miss an executable or names, so `B001` stays off for them and only `B002`
and `B003` run. Common aliases are accepted (`bo3` for `t7`, `bo2` for `t6`, `mw2` for
`iw4`...).

## Install

**Prebuilt binary.** Download the archive for your platform from the Releases page, check it
against `SHA256SUMS`, unzip it and put the folder on your `PATH`. The archive holds `gruff` (or
`gruff.exe`), the deprecated `gsc` alias, this README, the licence and a snapshot of gruff-data in
`gruff-data/`, which `gruff` finds next to itself.

**From source** (Rust 1.85 or later):

```sh
git clone https://github.com/Kenshin9977/gruff && cd gruff
cargo install --path crates/gruff         # installs `gruff` (and the `gsc` alias) into ~/.cargo/bin
cd .. && git clone https://github.com/Kenshin9977/gruff-data   # or set GRUFF_DATA
```

**Where the data comes from.** `gruff` looks for gruff-data in this order: `--data-dir`,
`$GRUFF_DATA` (or the older `$GSC_DATA`), then a `gruff-data` (or `gsc-data`) folder in or above
the working directory (the workspace, for the language server) or the binary's folder. Without it, files are read as the superset dialect and
no catalogue rule runs; the tools say so.

## Quick start

```sh
gruff check                                # lint the current directory (game from gruff.toml, else t7)
gruff check --game t6 maps/                # pick a game
gruff check --library "<BO3>/share/raw" .  # resolve names against the game's own scripts
gruff check --fix                          # apply the safe fixes
gruff check --explain L001                 # a rule's documentation
gruff format --check .                     # CI: list the files that would change
gruff format scripts/                      # format in place
gruff index --game t7 --out .gsc-index scripts/
grep -P '\tspawn\t' .gsc-index/CALLS.tsv   # every call to spawn
gruff hash t10.function main               # 162a529a3f7947b  main
gruff hash --list --game bo7               # the hash families of one game
gruff hash --reverse --game t11 --hash-package ../names function_3e93ae4f7823fe81 # name a hash
```

A library root (`--library`, or `library` in `gruff.toml`) is read to resolve names, such as the
functions the game's own scripts define, and is never reported on. Point it at your own mod
tools folder or dump.

Exit codes: 0 when nothing is left to report, 1 when diagnostics remain (or files would
change), 2 on a usage, configuration or I/O error.

## Configuration: `gruff.toml`

`gruff` reads the nearest `gruff.toml` (or `.gruff.toml`, or a `pyproject.toml` with a
`[tool.gruff]` table) at or above the first path. Every key is optional. The older names,
`gsc.toml`, `.gsc.toml` and `[tool.gsc]`, are still read; `gruff.toml` wins when a directory has
both.

```toml
game = "t7"                # game id or alias (default t7; the language server guesses from the folder)
library = ["C:/Program Files (x86)/Steam/steamapps/common/Call of Duty Black Ops III/share/raw"]
mode = "retail"            # language server: "retail" or "community" (gsc-tool, Plutonium...)
data-dir = "../gruff-data"   # language server: the gruff-data directory

[check]
select = ["E", "D", "B", "L"]   # replaces the default selection (codes, prefixes, names, ALL)
extend-select = ["B003"]        # adds to it
ignore = ["D004"]               # removes from it
preview = false                 # enable the preview rules

[check.per-file-ignores]
"*_debug.gsc" = ["B003"]
"scripts/zm/**" = ["F001"]

[format]
style = "full"             # or "minimal": whitespace only, every line break kept
max_width = 100
brace_style = "next_line"  # or "same_line"
indent = "tabs"            # or "spaces"
indent_width = 4           # with indent = "spaces"
paren_spacing = "padded"   # or "compact", "preserve"; the default depends on the engine family
```

Every `[format]` key is in [docs/formatter.md](docs/formatter.md); `// gsc-fmt: off` and
`// gsc-fmt: on` (or `gruff-fmt:`) leave the statements between them as written.

Relative paths are relative to the `gruff.toml`. The `[check]` keys may also sit at the top level.
Command-line flags are a layer on top of the file: `--select` replaces, `--extend-select` adds,
`--ignore` removes, and the most specific selector wins. Suppress a diagnostic on a line with
`// gsc: ignore[B001]`, in a whole file with `// gsc: ignore-file[B001]`; unused suppressions are
reported as `S100`.

## Rules

| Prefix | Category |
|---|---|
| `E` | syntax errors |
| `D` | dialect: syntax the selected game lacks |
| `B` | engine builtins (unknown, arity, dev-only) |
| `L` | loops and control flow |
| `F` | entity fields |
| `T` | threads and events (preview): unknown or misspelled event, `waittill` past the payload, player loop without `endon` |
| `S` | suppression comments |

Every rule has a page in [docs/rules](docs/rules/README.md), also printed by
`gruff check --explain CODE`; `gruff check --list-rules` lists them. New rules start in preview
(`--preview`) for at least one minor release.

**A preview rule reports a hint, not an error.** Preview means its precision is not established
yet, so its findings are shown and counted apart (`Found 3 errors and 12 hints.`), carry SARIF
level `note`, appear as hints in an editor, and never set a non-zero exit code on their own. Only
the stable rules can fail a build, which is what makes `--preview` safe to leave on in CI: you see
what the unproven rules think without letting them stop you. Each rule's page records what has
been measured about its precision, and a rule leaves preview on that measurement.

## Output formats

`gruff check --output-format <format>`:

| Format | Use |
|---|---|
| `text` (default) | `path:line:col: CODE message` with a source snippet |
| `concise` | one line per diagnostic |
| `json` | a JSON array (code, name, message, file, range, fix), for scripts |
| `github` | GitHub Actions workflow commands: annotations on pull requests |
| `sarif` | SARIF 2.1.0, for code-scanning dashboards |

`--statistics` prints the counts per code instead. `gruff format --diff` prints a unified diff.

## Editor setup

**VS Code** (and VSCodium, Cursor, Windsurf): install the `.vsix` of the release
(`code --install-extension gruff-<version>.vsix`). It bundles the `gruff` binary and adds
highlighting for `.gsc`, `.csc` and `.gsh`. Settings: `gruff.game`, `gruff.dataDir`,
`gruff.libraryRoots`, `gruff.mode`, `gruff.server.path`. See [editors/vscode](editors/vscode/README.md).

**Neovim** (0.11 or later):

```lua
vim.lsp.config("gruff", { cmd = { "gruff", "lsp" }, filetypes = { "gsc", "csc", "gsh" },
  root_markers = { "gruff.toml", ".git" },
  init_options = { game = "t7", libraryRoots = { "/path/to/share/raw" } } })
vim.lsp.enable("gruff")
```

**Helix** (`languages.toml`):

```toml
[language-server.gruff]
command = "gruff"
args = ["lsp"]
config = { game = "t7" }

[[language]]
name = "gsc"
scope = "source.gsc"
file-types = ["gsc", "csc", "gsh"]
language-servers = ["gruff"]
```

Server settings, capabilities and measurements: [docs/lsp.md](docs/lsp.md).

**Other editors and the web.** `editors/tree-sitter-gsc` is a tree-sitter grammar for the
whole dialect superset (highlights, locals, folds, indents queries; a `.wasm` build), for
Neovim's tree-sitter, Helix, Zed or GitHub-style highlighting. `gsc_wasm` runs the parser,
single-file check, formatter and hashes in WebAssembly; `editors/playground` is a static page
on it. See [docs/editors.md](docs/editors.md) and [docs/wasm.md](docs/wasm.md).

## Documentation

The documentation book is in [`docs/`](docs/SUMMARY.md); build it with `mdbook build docs`. It
gathers the rules, the language server, the hash families, the dialect matrix and the
contribution guide.

## Contributing

Contributions are welcome under the
[Developer Certificate of Origin](CONTRIBUTING.md#2-developer-certificate-of-origin)
(`git commit -s`). Never commit game files, scripts, dumps or name databases: tests read them
from local paths given in environment variables. See [CONTRIBUTING.md](CONTRIBUTING.md).

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Acknowledgements

Gruff builds on the work of the GSC tooling community.
[gscode](https://github.com/Blakintosh/gscode) by Blakintosh inspired the dialect-as-data
design, the rule that a missed diagnostic costs less than a false positive, and the policy on
proprietary files. [GSCLSP](https://github.com/Lierrmm/GSCLSP) by Lierrmm,
[gsc-tool](https://github.com/xensik/gsc-tool) by xensik and acts are cited as sources in
gruff-data, with their licences recorded entry by entry.

## Licence

GPL-3.0-or-later; see [LICENSE](LICENSE). One exception: the tree-sitter grammar
`editors/tree-sitter-gsc` is MIT (its own [LICENSE](editors/tree-sitter-gsc/LICENSE)), so that
editors can vendor it. The data in gruff-data is CC0-1.0 unless an entry says otherwise.
