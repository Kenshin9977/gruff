# The language server

`gruff lsp` speaks the Language Server Protocol on standard input and output. It is a thin layer
(`crates/gsc_lsp`) over the LSP-agnostic core: `gsc_sema` answers the editor queries,
`gsc_check` produces diagnostics and fixes, `gsc_fmt` formats. The VS Code client lives in
[`editors/vscode`](editors.md); any LSP client works:

```lua
-- Neovim
vim.lsp.config("gruff", { cmd = { "gruff", "lsp" }, filetypes = { "gsc", "csc", "gsh" },
  root_markers = { "gruff.toml", ".git" },
  init_options = { game = "t7", libraryRoots = { "/path/to/share/raw" } } })
vim.lsp.enable("gruff")
```

```toml
# Helix: languages.toml
[language-server.gruff]
command = "gruff"
args = ["lsp"]
config = { game = "t7" }
```

## Why lsp-server

The transport is [`lsp-server`](https://crates.io/crates/lsp-server), rust-analyzer's
synchronous scaffold (message types, stdio framing, the initialize and shutdown handshakes),
with `lsp-types` for the protocol structures. The alternative, `tower-lsp-server`, is async: it
brings tokio and runs handlers concurrently, which buys nothing here. The core is synchronous,
its queries take milliseconds, and the one slow job (linting) is easier to debounce and drop on
a plain thread than to cancel inside a runtime. With `lsp-server` the server owns its main loop,
so batching edits and cancelling queued requests are a few lines rather than a framework
feature, and the binary stays free of an async runtime.

## Architecture

- **Main loop.** It drains every message waiting on the connection into a batch. A
  `$/cancelRequest` cancels the requests of its batch (answered with `RequestCancelled`). Edits
  update the open document's text at once, but reach the workspace (`Workspace::set_text`, one
  re-parse per file) only just before the next request, or at the end of the batch, so a burst
  of keystrokes costs one re-parse. Requests run on this thread under a read lock.
- **Loader.** The workspace loads on a background thread, reporting `$/progress` (settings and
  data, parsing, resolving). It also runs the first check, then frees the syntax trees of the
  library roots (see below). Until it is ready, requests answer `null`. A settings change, or a
  change to `gruff.toml`, starts a new load; the old workspace serves until the new one is in.
- **Lint worker.** Every change bumps a generation counter and wakes the worker, which waits
  for 75 ms without changes (`debounceMs` in the initialization options), then updates its
  incremental check session under a read lock: only the files an edit can affect are checked
  again (next section). When the generation moved during the run, publishing waits for the next
  run (already queued); otherwise it publishes the diagnostics that changed, per file, and keeps
  them (with a hash of the text) for code actions. Library roots are never reported on.

## Incremental diagnostics

`gsc_check::incremental::Session` keeps the last check of the workspace and, on each update,
re-checks only the files whose diagnostics may have changed. Its results always equal a fresh
`gruff check` of the same workspace: `crates/gsc_check/tests/incremental.rs` asserts it after
scripted edits across every dependency and after 1,200 pseudo-random edits (three seeds), and
`crates/gsc_lsp/tests/incremental_corpus.rs` does the same on a real corpus (ignored by
default).

An update hashes every file's text (xxh3, 4 ms for the 48 MB of T7) to find the changed files,
and recomputes their check facts. A file's diagnostics depend on its own text and, across files,
on:

- **Exported names**: definitions (kind, namespace, name, parameters, flags), through name
  resolution and the builtin rules. A changed definition re-checks the files that mention one of
  the changed names: any word of their text, lower case, which is a superset of what a lookup can
  use. A change of imports, namespace, file key or classes, or a file added or removed, re-checks
  every file.
- **Yield summaries** (the loop rule): the "may yield" bit of the functions and macros a file's
  sync calls reach. The summaries are a fixpoint over the call graph; an update solves again only
  the *cone* of the changed nodes (every node that reaches one, through a reverse-edge index), and
  re-checks the files that reach a node whose bit flipped. When definitions change, the changed
  files' nodes move to the end of the numbering and the edges into them are renumbered, so only
  the files that mention a changed name resolve their calls again; past a quarter of dead slots,
  everything is renumbered.
- **Fields written** (the field rule): a file is re-checked when it mentions a field whose
  "written somewhere" status flipped.
- **Events** (`T001`-`T004`): a file's event sites, event-like strings and references (for the
  thread-start index), without positions. A change re-checks the files that mention a changed
  name, the files waiting for a name within two edits of a newly (un)notified one (`T002`
  suggestions), and, when a pattern or dynamic notify changed, every file with event sites.

The language server also frees the syntax trees of library files after the first check (most of
a game's memory: 230 MiB of live heap on T7); a query that needs one (field references in a
library file, a hover on a library function) parses the file again without keeping the tree.

## Capabilities

| LSP | What it does |
|---|---|
| `textDocument/didOpen`, `didChange` (incremental), `didClose` | Open buffers override the disk; closing reverts to the disk text. Files outside every folder get a root of their own. |
| `workspace/didChangeWatchedFiles` | Created, changed and deleted scripts; `gruff.toml`, `.gruff.toml`, `pyproject.toml` reload the project. |
| `workspace/didChangeConfiguration` | `{ "gsc": settings }` or the settings object; reloads when they change. |
| `textDocument/publishDiagnostics` | Syntax (`E`), dialect (`D`) and lint rules, filtered by the `gruff.toml` selection and suppression comments. `E`/`D` are errors, `S100` a hint marked unnecessary, the rest warnings. |
| `textDocument/definition`, `references`, `hover` | `Workspace::definition`, `references`, `hover`. A bare name resolves in the game's order (the profile's `bare_call_resolution`): in BO3 and later the current namespace, then the builtins, never a function of another namespace even when its file is `#using`'d, so such a call has no definition (rule [B004](rules/B004.md)); bare-name completion offers no other namespace's functions there either. In Black Ops II (`builtin-strict`) a bare call to a builtin's name goes to the builtin even when the file defines a function of that name, which only `path::f` and `thread f()` reach. |
| Hashed names | BO3, BO4 to BO7, MWIII: a definition named by hash (`function_1a2b`) and a call written by name meet (`gsc_sema::hashing`); hover, completion, document symbols and diagnostics show a hash the hash packages or the workspace's clear names name as `name (#1a2b)`, an unknown one as `#1a2b`, and hover gives the hash of the name and its family. See [Hash packages](hash-packages.md). |
| Events | On the string of a `waittill`, `waittillmatch`, `endon` (or `waittill_any`...), definition jumps to its notifiers; on a `notify` string, to its waiters and `endon`s; references list every site of the event (any case, `"prefix_" + n` patterns included); hover shows who notifies it, on what, with how many values, who waits, and what the engine catalogue says. The tables behind them are patched per file on every edit (`gsc_sema::events`). |
| `textDocument/documentSymbol`, `workspace/symbol` | Outline (methods under their class); fuzzy search over every definition. |
| `textDocument/completion` | Triggers `.`, `:` and `>` (a lone `:` or `>` returns nothing; `::` and `->` complete). |
| `textDocument/signatureHelp` | Script functions: parameters as written. Builtins: parameter types from the catalogue's `param_types`, optional ones in brackets, else `argN` placeholders; the arity and VM in the documentation. |
| `textDocument/prepareCallHierarchy`, `callHierarchy/incomingCalls`, `outgoingCalls` | Calls and function pointers; builtin callees are left out. |
| `textDocument/formatting` | `gsc_fmt::format` with the game's style and `[format]` of gruff.toml (full style unless `style = "minimal"`); one minimal edit. Refused, with the reason, on syntax errors or unsafe output. |
| `textDocument/rangeFormatting` | `gsc_fmt::format_range`: the statements (or declarations) that touch the range, formatted as in the whole document; one edit, verified like a full format. Same refusals. |
| `textDocument/codeAction` | `quickfix` per gsc_check fix (unsafe ones titled so, never preferred); `source.fixAll.gsc` applies every safe fix. Only offered on diagnostics computed on the current text. |
| `textDocument/prepareRename`, `rename` | Locals, parameters, functions and methods, workspace-wide. Refused: builtins, fields, event names, files, macros, definitions or uses in library roots, a new name that is not an identifier, is a keyword or a builtin. |
| `textDocument/semanticTokens/full` | Types `namespace function method parameter variable property macro class hashLiteral`, modifiers `declaration readonly defaultLibrary` (builtins are `function` + `defaultLibrary`). |
| `gsc/status` (request) | Game, where it came from, mode, file counts, builtins, warnings. |
| `gsc/ready` (notification) | The workspace loaded: total milliseconds and per phase. |

Positions are UTF-16 unless the client offers `utf-8` in `general.positionEncodings`.

## Settings

`initializationOptions` and `workspace/didChangeConfiguration` carry the same object (either the
settings or `{ "gruff": settings }`; the older `{ "gsc": settings }` is read too):

| Key | Meaning |
|---|---|
| `game` | Game id or alias; empty or `auto` for `game` in gruff.toml, else detection. |
| `dataDir` | The gruff-data directory (or its `profiles` folder). |
| `libraryRoots` | Library roots, absolute or relative to the first workspace folder. |
| `hashPackages` | Hash packages (files or directories), absolute or relative to the first workspace folder; adds to `hash_packages` in gruff.toml and `$GSC_HASH_PACKAGES`. See [Hash packages](hash-packages.md). |
| `mode` | `retail` (default) or `community`. |

The game comes from the setting, then `game` in `gruff.toml`, then the folder: a game id or alias
in its path (`bo3`, `t6`, `iw4x`...), else the scripts' imports (`#using` means T7 or later,
`#include` with `.csc` files T6, `#include` alone IW5), else T7. gruff-data comes from the setting,
then `data-dir` in `gruff.toml`, `$GRUFF_DATA` (or `$GSC_DATA`), then a `gruff-data` (or `gsc-data`)
folder next to or above a workspace
folder, the working directory or the binary; without it files are read as the superset dialect,
with no catalogue, and the server says so. `mode` may also be set at the top of `gruff.toml`.

## Tests and measurements

`cargo test -p gsc_lsp` runs the unit tests and `tests/lsp.rs`, which drives the server over
in-memory pipes with framed JSON messages, on the fixtures of `tests/fixtures` (a two-file mod,
a made-up catalogue): initialize with progress, diagnostics, completion (`::`, general, refused
triggers), definition, hover, references, signature help, symbols, call hierarchy, rename
(function, local, refused builtin, bad name), formatting, code actions, semantic tokens,
incremental edits, cancellation, close-reverts-to-disk, files outside the folders, shutdown.

`tests/bench_t7.rs` is a scripted session against the release binary on the T7 corpus:

```sh
cargo build --release
cargo test --release -p gsc_lsp --test bench_t7 -- --ignored --nocapture
```

It reads `GSC_CORPUS` (default `../corpus`, using `t7/source-modtools` as the folder and
`t7/decompiled` as a library root), `GRUFF_DATA` (default `../gruff-data`) and `GSC_BIN` (default
`target/release/gruff`). On the development machine (Windows 11; 3,437 files, 978 of them reported), warm file cache:

| Measure | Result |
|---|---|
| initialize to ready (loaded, resolved, first check) | 0.75 to 0.9 s (settings and data 110 ms, discovery and parsing 480 ms, resolution 17 ms, first check 150 ms) |
| initialize to first diagnostics | 0.85 to 1.0 s |
| memory (working set) | about 750 MiB after the first check, 650 MiB once the freed library trees are returned (was 700 and 760) |
| completion, general (1,508 items) | median 10 ms |
| completion after `level.` | median 70 ms (every field of every file) |
| completion after `ns::` | median 1.5 ms |
| definition | median 0.4 ms |
| hover | median 0.5 ms |
| semantic tokens of the 4,200-line `_globallogic.gsc` (3,790 tokens) | median 7 ms |
| document symbols of the same file | median 2 ms |
| references of a function (30 uses) | median 4 ms |
| references of a builtin (190 uses) | median 15 ms |
| completion / definition / references right after an edit | median 24 / 12 / 37 ms (the edit's re-parse and the lazy re-resolution) |

With a cold file cache, or while other builds use the cores, parsing took up to 3.8 s (ready in
3.3 to 3.9 s); every other figure moved by less than a factor of two.

`tests/bench_corpus.rs` measures diagnostics latency and memory on any folder
(`GSC_BENCH_FOLDER`, `GSC_BENCH_LIBRARY`, empty for none, `GSC_BENCH_GAME`, `GSC_BENCH_FILE`,
default: the largest file): ten edits that change no name (a comment) and ten that add a
function, each timed from `didChange` to the end of the check it triggers, debounce included.
Before (0.1.0: a full check after a 200 ms pause) and after (incremental, 75 ms):

| Corpus | Measure | Before | After |
|---|---|---|---|
| T7 mod tools (3,437 files, 978 reported; edits in `_zm.gsc`) | edit to diagnostics, comment / new function | 381 / 381 ms | 109 to 121 / 115 to 128 ms |
| | the check itself | 164 / 166 ms | 21 / 26 ms (1 file re-checked) |
| | working set after the session | 761 MiB | 646 to 652 MiB |
| T6 decompiled (3,864 files, all reported; edits in `maps/_utility.gsc`) | edit to diagnostics, comment / new function | 518 / 517 ms | 121 / 142 ms |
| | the check itself | 299 / 300 ms | 27 / 49 ms (1 file re-checked) |
| | working set after the session | 790 MiB | 740 MiB (no library root: no tree freed) |

On the T7 corpus, `incremental_corpus.rs` (80 random line insertions, results compared with a
full check after each) gave a median update of 7 to 10 ms against 230 to 260 ms for the full
check: 2 to 5 ms when no name changes, 15 to 20 ms for a new or renamed function, up to 150 ms
when an edit breaks the syntax of a large file and changes dozens of names.

## Known gaps

- An edit arriving during a check waits for it (the workspace has one copy, behind a
  read-write lock): usually 5 to 30 ms, up to 150 ms when an edit changes many names.
- Memory follows the core, which keeps every primary file's tree and every file's facts and
  resolution; the allocator returns the freed library trees to the system only gradually, so
  the peak working set stays near 750 MiB on T7.
- Semantic tokens are full-document only (no delta, no range); no folding ranges from the server
  (the client's language configuration folds by markers and indentation).
- Rename does not touch macros, fields, class names or files; it does not check that the new name
  collides with another function of the namespace.
- `$/progress` reports phases, not per-file progress.
