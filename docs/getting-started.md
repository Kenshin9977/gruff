# Getting started

## Install

**Prebuilt archives.** Each release has one archive per platform:

| Platform | Archive |
|---|---|
| Windows x64 | `gsc-<version>-x86_64-pc-windows-msvc.zip` |
| Windows ARM64 | `gsc-<version>-aarch64-pc-windows-msvc.zip` |
| Linux x64 | `gsc-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| Linux ARM64 | `gsc-<version>-aarch64-unknown-linux-gnu.tar.gz` |
| macOS Intel | `gsc-<version>-x86_64-apple-darwin.tar.gz` |
| macOS Apple silicon | `gsc-<version>-aarch64-apple-darwin.tar.gz` |

Check the archive against `SHA256SUMS` (`sha256sum -c SHA256SUMS --ignore-missing`, or
`Get-FileHash` in PowerShell), unpack it and put the folder on your `PATH`. The archive holds the
binary, the README, the licence and a snapshot of gruff-data in `gruff-data/`.

**From source**, with Rust 1.85 or later:

```sh
cargo install --path crates/gruff
```

## The data

gruff-data holds the dialect profiles and the catalogues. `gruff` finds it, in order, from:

1. `--data-dir <dir>` (or the `gruff.dataDir` setting of the editor, `data-dir` in `gruff.toml` for
   the language server);
2. the `GRUFF_DATA` environment variable (or the older `GSC_DATA`);
3. a `gruff-data` folder (or the older `gsc-data`) in or above the working directory or workspace;
4. a `gruff-data` folder next to or above the binary (the release archives put it there).

Without gruff-data, files are read as the superset of every dialect, and the builtin and field
rules do not run.

## First commands

```sh
gruff check                                # lint the current directory
gruff check --game t6 maps/                # choose the game (default: gruff.toml, else t7)
gruff check --library "<BO3>/share/raw" .  # resolve names against the game's own scripts
gruff check --statistics                   # counts per rule
gruff check --fix                          # apply the safe fixes
gruff check --explain B001                 # a rule's page
gruff format --diff .                      # what the formatter would change
gruff format .                             # format in place
gruff index --game t7 --out .gsc-index .   # SYMBOLS.tsv, CALLS.tsv, FIELDS.tsv...
gruff hash --list --game t10               # hash families of Black Ops 6
gruff hash t7.canon player_is_in_laststand
```

A **library root** is a folder read to resolve names (the functions the game's own scripts
define, for example) and never reported on. Without the game's scripts, a call into them looks
like an unknown function; `B001` already skips any name defined in the workspace, so give it the
library when you have it.

## In CI

```yaml
- run: gruff check --output-format github .
- run: gruff format --check .
```

`--output-format sarif` produces SARIF 2.1.0 for code-scanning uploads. Exit codes: 0 clean,
1 diagnostics remain or files would change, 2 usage, configuration or I/O error.
