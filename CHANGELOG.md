# Changelog

All notable changes to Gruff are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html): while the version is 0.x, a minor
release may add stable rules or change defaults. gruff-data is versioned on its own; each release
records the data snapshot it bundles.

## [Unreleased]

### Fixed

- **A file the game's own compiler refuses is no longer linted.** A syntax error makes a GSC
  compiler reject the whole file, so nothing in it runs and a finding about its meaning describes
  code the game never sees. Gruff suppressed its rules around a syntax error, range by range, and
  so still reported on the parts that happened to parse. Now the syntax errors are reported and
  the rules stop, as Ruff does on an unparsable file; S100 stops too, or one refusal would become
  one report per suppression comment. `E0301` (decompiler output no tokenizer can read) is not
  such an error and still lets the rules run. World at War's `maps/_ai_supplements.gsc`, a dead
  file whose trailing `*/` the game refuses, went from 6 findings to the one that is true.
- **L001 understands a wait spelled as a call.** Black Ops 4 and later compile `waittill` and its
  family to opcodes rather than keywords, and a decompiler writes them back as ordinary calls, so
  `while (true) { self waittill(#"hash_...") }` looked like a loop that never waits. On the
  corpora: Black Ops 4 510 findings to 1, Cold War 486 to 1, Modern Warfare III 435 to 3, while
  Black Ops III (5) and Black Ops II (6), which have the keyword, do not move.
- **B001 reports a foreign file once.** Ten or more unknown names in one file, outnumbering the
  calls that resolve, is a file from another game or another version rather than a file with
  typos: one report names the file and its first few names instead of one per call site. World at
  War's `maps/_destructible_types.gsc`, a Call of Duty 4 leftover, went from 1,319 reports to one.
  Below the thresholds nothing changes.
- The Modern Warfare III campaign builtin catalogue now declares its own incompleteness
  (`[coverage] complete = false`, which `calibrate.py` writes from the note added for it): 932 of
  its roughly 1,902 functions were extracted, so B001 cannot tell an unknown name from one the
  catalogue lacks and is silent, as in every other partial catalogue. Its corpus goes from 768
  findings to 24, B001 482 to 0 and B002 15 to 6.

- `tools/re/calibrate.py` puts a catalogue back as it found it when a run stops part way. It
  writes the file without the intrinsics `restore` took out, so that `gruff check` can read it,
  and only then decides whether to write them back; anything that failed in between left them
  gone for good, since a partial catalogue can never derive them again (B001 is silent there, so
  no call site is unresolved). Found by doing it: a wrong `--gsc` path cost the Modern Warfare III
  campaign its eight intrinsics.

Measured over the 17 corpora that have a builtin catalogue, the four together take the findings
from 3,917 to 423, a fall of 89%. Nine of the 17 do not move at all, which is the point: what
went was a handful of systematic false positives, not a rule's reach. Five corpora account for
almost all of it: World at War (1,368 to 45), Modern Warfare III's campaign (768 to 24), Black
Ops 4 (536 to 27), Cold War (491 to 6) and Modern Warfare III multiplayer (467 to 35); Call of
Duty loses one finding, in a file it cannot parse.

## [0.4.0] - 2026-11-11

The script VMs of the x64 Infinity Ward games measured offline, builtins split per game mode, and
two questions closed by measurement rather than by code: whether block comments nest (they do not,
in any of the four games whose official compiler can be asked) and whether B001 should speak at all
when a mod is opened without the game's scripts (it should not). No hash of a game executable is
left in the data. gruff-data snapshot `ae83b5a`.

### Added

- **VM semantics of the x64 IW games.** `tools/re/vm/iw_run.py`, `iw_suite.py`, `iw_facts.py`
  and `iwvm.py` run the conformance suite in the script VMs of Modern Warfare 3 (the x64
  re-release), Ghosts, Advanced Warfare, Modern Warfare Remastered and Infinite Warfare, emulated
  offline (gsc-tool compiles, the game's own loader links): 44-45 PASS and 12-13 UNSUPPORTED
  each, `conformance/{iw5,iw6,s1,h1,iw7}/`, report `tools/re/reports/vm_iw.md`. Runtime behaviour
  is IW3's; childthread inherits its starter's endons and outlives it; the loop guard kills a
  thread silently after 5 s; S1 and H1 have `waitframe()`. Bare-call resolution is decided at
  compile time in these games and stays unmeasured. Modern Warfare 2's x64 re-release has no
  compiler or loader (its scripts are compiled into the executable) and is not run.
  `tools/re/vm/pycrt.py` does a dynamically linked build's C runtime imports in Python.
- **Builtins per game mode.** Games that ship one executable per mode (Black Ops II campaign,
  multiplayer and zombies; Black Ops; Modern Warfare 3, Advanced Warfare and Modern Warfare
  Remastered campaign and multiplayer) register different builtins in each. Every file now has
  a game mode, from its path (`maps/mp/` multiplayer, zombies files by name, other `maps/` and
  `clientscripts/` files campaign, shared folders none; `docs/modes.md`) or from `gruff.toml`
  (`game-mode`, `[game-modes]` globs). B001, B002, B003, B005, N001's `::f`, name resolution,
  hover and completion see only the builtins of the file's mode, and B002 holds a file to its
  own executable's arity where the executables differ (Black Ops II `playfx`: 2 to 4 arguments
  in multiplayer, 2 to 6 in the campaign). B001 names a builtin of another mode ("`x` is a
  builtin function of mp, zm scripts only"). Needs gruff-data with `modes`. On the corpora:
  Black Ops II B002 16 to 8 (the campaign's own `spawnfx`, `playloopedfx` and `playfx` calls
  among them), Black Ops B002 11 to 10; World at War B001 0 to 1,355, its catalogue now being
  complete for campaign and zombies scripts. Those are calls to functions no file of the corpus
  defines although every import resolves: 1,319 `destructible_*` builders in the Call of Duty 4
  leftover `maps/_destructible_types.gsc` (World at War's `maps/_destructible.gsc` has none of
  them), 31 Der Riese calls to `maps/_zombiemode_utility` functions of a later patch than the
  corpus's copy, and 5 in `maps/_ai_supplements.gsc`, a leftover whose trailing `*/` the game's
  own compiler refuses, so the game never loads the file at all (measured; see the block-comment
  entry below).
- B001 no longer reports a name that no builtin has when the file or a file it imports has an
  import missing from the workspace (a mod opened without the game's scripts): the name may be a
  function of the missing file, and I003 reports the import once a library root is set. Builtins
  keep their kind, VM, mode and arity checks. Same in every game (T7 reported them before too).
  Corpora: Black Ops B001 17 to 12, Black Ops mod tools 623 to 478, Black Ops II 4 to 3.
- **B006 builtin-shadowed-function** (preview, `builtin-strict` games: Black Ops II): a script
  function named like a builtin of its mode, whose bare calls in its file and in the files that
  `#include` it link to the builtin instead. 10 functions on the game's 3,864 scripts (15
  without the game modes, 5 of them campaign functions such as `spawnfx`).
- `tools/re/modes.py`: the `modes`, `mode_args`, `[extraction] modes` and (through
  `calibrate.set_coverage`) `complete_modes` of the catalogues, from one extraction per
  executable (T5, T6, IW5, S1, H1), run after any build like `calibrate.py`;
  `reports/modes.md`.
- **Block comments do not nest, measured.** `conformance/{t4,iw3,t5,t7}/nested_block_comment.gsc`
  and `conformance/t4/stray_block_comment_close.gsc` ask four games, each through its own official
  compiler or linker: `/* a /* b */ */` is refused in all four, and so is a lone `*/`. The Black
  Ops III linker names what it lexed at the second `*/` ("syntax error, unexpected
  TOKEN_MULTIPLY"), so the comment had already closed at the first one. The question came from a
  World at War script that reads as if comments nested; they do not, and such a file is refused at
  load rather than read as commented out. Gruff's lexer already ended a block comment at the first
  `*/`, so it is unchanged: the measurement closes the question instead of opening work.
- CI: the release workflow reads gruff-data over SSH when the secret `GRUFF_DATA_SSH_KEY` holds a
  read-only deploy key of that repository, which is what a private gruff-data needs. Unset, the
  checkout is token-authenticated as before, which is what a public one needs.

### Changed

- **V002 thread-call-value** now reports in MW3, Ghosts, AW, MWR and IW, whose profiles measure
  `thread_call_value = "callee-result"` (decompiled corpora: 68, 51, 52, 6 and 68 findings).
- **No hash of a game executable in the data any more.** A `[meta] dump` of an event catalogue or
  a semantics file named the executable and its sha256; it now names the file only, and the event
  and semantics schemas no longer accept a digest. Which build an extraction read stays in its own
  output under `tools/re/out/`, which is never committed, so nothing is lost for replaying a run.
  `events_build.py` and the five `*_facts.py` generators write the shorter form, the `reports/`
  that quoted a digest no longer do, and `tools/validate.py` passes on all 24 profiles, 19 builtin,
  1 field, 15 event and 12 semantics files. The reverse-engineering addresses in the data
  (`handler_rva` and the `evidence` prose) are untouched and still an open question.

## [0.3.0] - 2026-11-07

The first release under the Gruff name: the rename, GSCode editor parity, 25 new rules and VM
semantics measured in seven games' emulated VMs. gruff-data snapshot `1c62a68`.

### Added

- **Language server: GSCode editor parity.** Folding ranges (declarations, blocks, comment runs,
  dev blocks, import runs, `region`/`endregion` comments), selection ranges, document highlights
  (writes marked), document links on `#using`/`#include`/`#insert`/`#inline`, "N references"
  code lenses (functions, methods, classes, macros; autoexec says so), inlay hints (parameter
  names, macro parameter names, the type of a local at its first assignment), go to
  implementation (method overrides, a field's `=` writes and the functions they bind), go to
  type definition (the class a local or field holds, the function it points to), type hierarchy
  for classes. Class members are navigable (a bare name in the class's methods reaches its
  `var`), fields go to their writes, and both rename; class references include `new C()` and
  base lists. Hover and signature help show what a macro expands to at the use. Completion
  offers known event names inside strings and, from three characters on, functions of files not
  imported, adding `#using` or `#include` as the game's profile says (`ns::f` in the namespace
  games). Code actions: add a missing import, remove a duplicate one, organize imports (unused
  ones are judged conservatively). Each family can be turned off in the initialization options
  (`inlayHints.parameterNames`, `inlayHints.inferredTypes`, `inlayHints.macroParameterNames`,
  `codeLens.references`, `completion.autoImport`, `completion.literals`); the VS Code extension
  adds a Gruff submenu to the editor's context menu and opens the references of a lens.

### Renamed

- **The project is now Gruff** (formerly gsc-tools; the data repository is now gruff-data, formerly
  gsc-data). The binary is `gruff` (`gruff check`, `gruff format`...); `gsc` remains for one
  version as a deprecated alias (a second binary built from the same code, which prints a notice
  except for `lsp` and `mcp`). The Rust package of the command line is `gruff` (was `gsc_cli`,
  directory `crates/gruff`); the library crates keep their `gsc_*` names, which name the language.
- **Configuration**: `gruff.toml`, `.gruff.toml` and `[tool.gruff]` are read, before `gsc.toml`,
  `.gsc.toml` and `[tool.gsc]`, which still work (`gruff.toml` wins in the same directory).
- **Data**: `$GRUFF_DATA` (preferred) or `$GSC_DATA`; discovery finds a `gruff-data` folder as
  well as `gsc-data`. Release archives ship the snapshot as `gruff-data/`.
- **Language server**: diagnostics' `source` is `gruff`, `serverInfo.name` is `gruff`; settings
  are read from a `gruff` section (a `gsc` section still works). The VS Code extension is `gruff`
  ("Gruff — GSC language tools"): settings `gruff.*` (were `gsc.*`), commands
  `Gruff: Restart Language Server` and `Gruff: Show Server Status`; it runs the bundled `gruff`,
  else `gruff` on `PATH`, else `gsc`.
- **MCP server**: `serverInfo.name` is `gruff` (the tools keep their `gsc_*` names).
- **Output**: SARIF `driver.name` and GitHub annotation titles say `gruff`; `gruff report`'s
  `toolchain.name` is `gruff` (the format id stays `gsc-corpus-report`).
- Suppression comments stay `// gsc: ignore[...]`; `// gruff: ignore[...]` is accepted too, as is
  `// gruff-fmt: off` / `on` beside `// gsc-fmt: off` / `on`.

### Added
- **25 rules catching up with gscode's checks** (Blakintosh/gscode, GPL-3.0; logic ported where
  noted on each page). New categories `I` imports, `M` macros, `N` names and declarations, `U`
  local variables. What the BO3 compiler refuses was measured with its linker
  (`tools/linker/probes/lint_*.gsc`); the rules it refuses with no finding in the shipped
  sources are stable: L005 `non-constant-case-label`, M001 `macro-arg-count`, M003
  `duplicate-macro-parameter`, N005 `duplicate-function`, N007 `misplaced-vararg`, N008
  `inheritance-cycle`, N009 `constructor-parameters`, V003 `read-only-assignment`, V005
  `useless-expression`. In preview: I001 `duplicate-import` (safe fix), I002 `unused-import`
  (safe fix; 1,312 reports on the shipped sources, none found used on cross-check), I003
  `unresolved-import` (needs a library root), L002 `unreachable-code`, L003
  `duplicate-case-label`, L004 `assignment-as-value`, M002 `macro-redefined`, N001
  `unresolved-script-function`, N002 `namespace-not-imported` (unsafe fix adds the `#using`),
  N003 `private-function-call`, N004 `script-arg-count`, N006 `duplicate-parameter`, U001
  `unused-variable`, U002 `uninitialized-variable`, V002 `thread-call-value` (gated by the new
  gruff-data feature `thread_call_value`: callee-result in CoD4 to BO4, silent in Cold War where
  the value is a `finalization`), V004 `division-by-zero`.
- `#insert`/`#inline` resolve to the header (`.gsh`) of a path before a script of the same name.

- **Rules B004 `bare-call-other-namespace` and B005 `thread-builtin`** (stable, BO3 and later):
  a bare call or `&f` that only a function of another namespace defines (an unsafe fix writes
  `ns::f` when one namespace has it), and `thread` on a builtin. The game refuses to load both.
  On the corpus: 0 reports on the 973 BO3 mod-tools scripts (the official linker builds them),
  0 on the decompiled BO3, BO4 and Cold War scripts, which the rules skip: acts writes a call
  bare whatever its import's namespace. Forced on, they give 0, 0 and 11 B004: 5 are qualified
  imports acts wrote bare, 6 a get-call method of the script's own namespace that the Cold War
  builtin catalogue lacks (`function_87c48367`). B004 skips `/# #/` blocks, where the retail
  catalogues lack dev-only builtins (BO4: 48 reports there, all `function_8ed414ef`, a dev-call
  method import). 0 everywhere else: the rules only run for `namespace-first` games.
- **Rule V001 `hash-equality`** (preview): `==` with a `#"..."` literal in a game whose profile
  says `hash_equality = "script-error"` (BO3, measured in the emulated VM: a script error, and
  false even for equal hashes; BO4 and Cold War compare hashes by value). 0 reports on the BO3
  mod tools and the decompiled BO3 scripts, which hold no such comparison.

### Changed

- **VM semantics from the profiles.** `gsc_sema::Config` reads the gruff-data features measured
  in the emulated VMs (`waittill_return_value`, `waittill_multiple_events`,
  `endon_multiple_events`, `notify_wakes_waiters`, `hash_equality`). In BO4 and later every
  argument of `waittill` and `endon` is an event (a computed one is a dynamic site), nothing is
  bound (type inference, local navigation and completion no longer take them for variables),
  and `waittillmatch( { #f: v }, "e" )` waits for its second argument; BO3 binds every name
  after the event; games whose VM has not been run keep the old guess (string arguments are
  events). T003 in games whose `waittill` returns the payload struct reads the result instead:
  `r = self waittill( "e" )` then `r.f` that no notify of `"e"` sends (struct-literal payloads,
  see `docs/rules/T003.md`). Hover on an event says when the woken waiters run (inside the
  `notify` in Cold War, after the notifier yields in BO3 and BO4). Corpus: T001-T004 give
  byte-identical reports before and after on all 20 games (BO3 mod tools: T002 2; BO4/Cold War
  T003 0, with one wait each whose payload is fully known); `gruff index` lists 52 (BO4) and 72
  (Cold War) more event sites, the computed events of multi-event waits and `waittillmatch`'s
  events. A notify-before-waittill rule (a wait whose event only its own function notifies) was
  tried and dropped: 64 reports on the corpus, the ones read all queues or handshakes between
  two instances of the same function.

- **Bare-name resolution follows the game's order** (`bare_call_resolution` in the profile).
  `namespace-first` (T7, measured in the game's VM; T8 to T11, now `namespace-first` in gruff-data
  after BO4 and Cold War bytecode and acts' BO4 linker): the current namespace (the same file
  first), then the builtins, never a function of another namespace even when its file is
  `#using`'d; a threaded call never reaches a builtin. Before, a bare name still reached a
  `#using`'d file's functions. Go-to-definition, references, hover, call hierarchy and the index
  follow it, and bare-name completion no longer offers other namespaces' functions. On the BO3
  mod tools this moves 32 references: `TIMEOUT( t )` had been read as `util::timeout`, it is the
  macro. A reference at file scope (a `REGISTER_SYSTEM( ..., &__init__ )` before the first
  `#namespace`) resolves in the namespace in effect there (`FileFacts::ns_switches`).
  `builtin-first` games (IW, MWIII; measured in none) keep their order: the same file, the
  builtins, the imports, despite the name.
- **Black Ops II bare calls: `builtin-strict`** (measured in the emulated T6 VM). The game's
  linker tries the builtins before any script function, even one of the same file (only
  `path::f` reaches it), then the same file, then the `#include`d files; a threaded call skips
  the builtins; `::f` takes the same order, builtin first (the game then stores the builtin's
  native handler: B005 reports it). Only a builtin of the call's kind comes first (a method call
  looks among the builtin methods). B002 no longer counts a script function as shadowing a
  builtin there. On the decompiled BO2 scripts (`gruff index`), 935 of 358,126 CALLS rows change:
  906 `::f` references now reach the `#include`d file's function (unresolved before: `::f`
  looked only in its own file), 29 calls to a same-file function named like a builtin now go to
  the builtin (21 `spawn` in the singleplayer `maps/_utility.gsc`, 2+2 `vectorcross`, 3
  `distance2dsquared`, 1 `self suicide()`). `gruff check --select ALL --preview` gains 3 B002, on
  singleplayer calls of `spawnfx`/`playloopedfx` with 6 and 8 arguments: the singleplayer
  scripts define these names and call the builtins `codespawnfx`/`codeplayloopedfx`, and the
  T6 catalogue, extracted from the multiplayer executable plus the singleplayer extras, does not
  say which multiplayer builtins singleplayer lacks.

## [0.2.0] - 2026-11-06

Phases 3 and 4: engine events, the event graph and its rules, an MCP server, incremental
diagnostics, corpus parameter types, the full formatter, tree-sitter and WebAssembly, hashed-name
games with hash packages, and community corpus reports.

### Added

- **Event graph** (`gsc_sema::events`): every `notify`, `waittill`, `waittillmatch` and `endon`
  site (and the `waittill_any`, `delay_notify`, `wait_endon` wrappers) is a file fact; the
  workspace knows who notifies what, on which receiver side and VM, and how each function is
  started. The engine event catalogue `gruff-data/events/<game>.toml` is loaded when present. In
  the language server, definition goes from a waiter to its notifiers and back, references list
  every site of an event, and hover sums up notifiers, payload, waiters and the engine
  catalogue.
- **Rules T001-T004** (preview): unknown event (needs a complete engine catalogue), misspelled
  event, `waittill` binding past the payload, player-threaded infinite loop without `endon`.
  Measured on the 20-game corpus with the engine catalogues: T002 67 reports, 72% true typos
  (64% without the catalogues); T003 4 reports, 1 true; T004 80; T001 would give 2,921 reports
  if the catalogues were read as complete, so it stays gated on `[coverage] complete`. All four
  stay in preview; `docs/rules/T00*.md` hold the classified reports.
- **`gruff report`**: a deterministic JSON report of a local script tree (construct counts per
  dialect feature, calls no script defines with argument-count histograms and literal argument
  types, event and field names), facts only: no code, no string values, script names hashed by
  default. `tools/re/merge_reports.py` merges reports into drafts under `gruff-data/community/`;
  from a game's own corpus report alone, the drafts reach a name precision of 99.5% (BO2) and
  99.7% (MW3) and recall nearly every called builtin. See `docs/community-reports.md`.
- `gruff index` and `gruff types` take `--hash-package` (and read `$GSC_HASH_PACKAGES`).
- **tree-sitter-gsc** (`editors/tree-sitter-gsc`, **MIT**, the one part of Gruff that is
  not GPL-3.0-or-later, so that editors can vendor it): a tree-sitter grammar for the dialect
  superset, CoD1 to BO7, source and decompiler output, with an external scanner (disabled
  `#else` branches, macro statements without `;`, acts' encoded animation names, names glued
  with `/`) and highlights, locals, tags, folds and indents queries. 99.92% of the 40,649
  corpus files parse without `ERROR`, as with the Rust parser; `tree-sitter-gsc.wasm` for the
  web. See `docs/editors.md`.
- **WebAssembly** (`gsc_wasm`): parse (diagnostics and syntax tree), single-file check, format
  and hash from JavaScript, with the BO3 profile and catalogues embedded when gruff-data is found
  at build time, or any game's TOML passed in. `editors/playground`: a static page on it (no
  CDN) and a Node smoke test. See `docs/wasm.md`.

- **Full formatter** (`gsc_fmt`, `style = "full"`, now the default of `gruff format` and of the
  language server): the tree is printed through a Wadler/Prettier-style document IR (`group`,
  `indent`, `line`, `softline`, `fill`) that breaks long calls, parameter lists, operator chains,
  ternaries, array and struct literals and conditions at `max_width` (100). Comments keep their
  place (trailing comments stay trailing and aligned, own-line comments on their own line);
  directives, preprocessor lines, items or statements holding `#if` regions, and
  `// gsc-fmt: off` ... `// gsc-fmt: on` regions are copied as written. New `[format]` keys:
  `style`, `max_width`, `tab_width`, `brace_style` (`next_line`, the style of every studio
  source; `same_line`), `bracket_spacing`, `keyword_space`, `wrap` (`fill`, `one_per_line`),
  `max_blank_lines`; per-game defaults measured on the corpus. `gruff format --style
  minimal|full --max-width N`. Corpus gate: 40,649 files, 0 unsafe results, 0 idempotence
  failures. See `docs/formatter.md`.
- **Range formatting**: `gsc_fmt::format_range` and `textDocument/rangeFormatting` in the
  language server (the statements touching the range, verified like a whole-file format).
- **Hashed-name games** (BO3, BO4, Cold War, MWIII, BO6, BO7): names unify through hashes
  (`gsc_sema::hashing`, `gsc_hash::Scheme`). A definition the decompiler named by hash
  (`function_1a2b...`) and a call written by name are one symbol, in bare, namespace-qualified
  and path-qualified calls; `namespace_<hex>` meets a clear `#namespace`; `#using script_<hex>`
  reaches the file kept under its clear path (and the reverse); a builtin spelled
  `function_<hex>` is the builtin. Unresolved calls on the corpora: BO4 109,416 to 1,342, Cold
  War 162,061 to 4,201, MWIII 270,619 to 22,524, BO7 351,940 to 99,551 (BO7's are calls into
  scripts its archive lacks); unresolved imports BO4 18,628 to 0, MWIII 28,079 to 22. B001 on
  MWIII drops from 2,778 to 13 (the definitions-by-hash residue), Cold War 22 to 5.
- **Hash packages** (`docs/hash-packages.md`, format in `gruff-data/schema/hash-package.md`): name
  lists, plain or `hash<TAB>name`, hashed by the toolchain with each game's families, loaded from
  `hash_packages` in gruff.toml, `--hash-package`, the `gsc.hashPackages` editor setting and
  `$GSC_HASH_PACKAGES`. Hover, completion, document symbols and diagnostics show `name (#hash)`,
  an unknown hash as `#hash`. `gruff hash --reverse` names values from the packages, `gruff hash
  --coverage` measures a tree; `tools/hash_packages/convert.py` builds packages from existing
  dictionaries (bo7-gsc-archive, GrayWolf `.wni`, CSV lists, script trees). With local packages,
  identifiers with a name: BO4 36.7% to 76.3%, Cold War 37.5% to 75.4%, MWIII 43.6% to 87.1%,
  BO7 26.6% to 90.5%. gruff-data ships no package.
- `gsc_hash`: `iw9.xhash` (MWIII `#"hash_..."`, full 64-bit FNV-1a TA); `iw9.scr` checked on the
  MWIII corpus (spot-checked); `Game::from_profile_id` (`t11`, `jup-sp`...); `Scheme`, `Role`,
  `Package`, `Dictionary`. `gruff mcp`'s hash tool reads the packages.

- **`gruff mcp`** (`gsc_mcp`): a Model Context Protocol server for coding agents on stdio, with ten
  read-only tools (workspace summary, symbol search, definition, references, callers, callees,
  check, events, builtin lookup, hash and reverse hash) answering in compact, paginated text.
  Dual-era: protocol revision 2026-07-28 (per-request `_meta`, `server/discover`) and the
  `initialize` handshake of 2025-11-25 and earlier. See `docs/mcp.md`.
- **Incremental diagnostics** (`gsc_check::incremental`): the language server re-checks only the
  files an edit can affect, with results equal to a full check (asserted by randomized tests);
  edit-to-diagnostics on T7 drops from about 380 ms to 110-130 ms, debounce now 75 ms. The
  server frees the syntax trees of library roots after the first check.

- **`gruff types`**: the argument types the scripts pass to each builtin, as JSON
  (`gsc_sema::types`: literals, known builtin results, local flow, script parameters and
  results to a workspace fixpoint). `tools/re/types_corpus.py` writes it into the catalogues
  as `corpus_arg_types`/`corpus_param_types`; signature help shows a corpus type as `~int`
  where the handler measure gives none.

### Changed

- `gruff hash` takes `--reverse`, `--coverage`, `--game` and `--hash-package`; strings follow the
  family as before.
- The gruff-data lookup (`--data-dir`, `$GRUFF_DATA`, a `gruff-data` folder upwards) is one function,
  `gsc_sema::data::find_profiles_dir`, shared by the CLI and the language server.
- In hashed games the callee of a resolved namespace call is the definition's spelling
  (`gruff index` CALLS.tsv), so both spellings of a call count as one callee.
- `gruff format` and the language server's formatting use the full style by default; the 0.1.0
  behaviour (whitespace only, every line break kept) is `--style minimal` or
  `[format] style = "minimal"`.

### Data (gruff-data snapshot `8b19740`)

- **Engine event catalogues** for 13 games (IW3, IW4, IW5, IW6, S1, H1, IW7, T4, T5, T6, T7, T8,
  T9; 1,530 events): the events each engine notifies, with receiver and payload, extracted from
  the executables by running every `Scr_Notify` call site under emulation from one anchor per
  game (`tools/re`, `README.md`, "Engine-emitted events"). Each has a `[coverage]` table (sites
  resolved, how many of the corpus's `waittill` names the engine explains); none is complete,
  so T001 stays silent. Not done: IW8, MWIII, BO6, BO7 (their notify core is still to be found).
- **Corpus parameter types**: `corpus_arg_types` / `corpus_param_types` in every builtin
  catalogue with a corpus (`gruff types` + `tools/re/types_corpus.py`); builtins with at least one
  known parameter type 47.0% to 52.0% over all games (BO3 71% to 81%).
- Community drafts for CoD2, WWII and CoD Online (`community/`), from corpus reports; read by no
  tool.
- The hash package format (`schema/hash-package.md`); no package is shipped.

## [0.1.0] - 2026-11-06

First public release.

### Added

- **`gsc` binary** with six commands: `check`, `format`, `index`, `parse`, `hash`, `lsp`.
- **Parser** (`gsc_syntax`): a modal lexer and a hand-written resilient parser producing a
  lossless CST and a typed AST generated from `grammar/gsc.ungram`. It reads the superset of every
  dialect from CoD1 to Black Ops 7 and reports what the selected game lacks from its dialect
  profile: feature gates (casts, `builtin` calls, goto and labels, `istrue`...), contextual
  keywords (`function`, `thisthread`), retail and community compiler modes, `#define` macro
  expansion, decompiler output detection.
- **Semantic core** (`gsc_sema`): a workspace of files and library roots, per-file facts, name
  resolution across files (`#using`, `#include`, namespaces), the gruff-data loaders (profiles,
  builtin and field catalogues) and the IDE queries the language server uses. Compiler intrinsics come
  from the catalogue's `[[intrinsic]]` list, the one source the tools read; `--game jup-sp`
  selects the MWIII campaign catalogue.
- **`gruff check`**, a Ruff-style linter with 22 rules: syntax errors (`E01xx`, `E02xx`),
  decompiler names (`E0301`), dialect rules (`D001` to `D006`), builtin rules (`B001` unknown
  builtin with did-you-mean, `B002` argument count, `B003` dev-only builtin, preview), `L001`
  infinite loop without a wait, `F001` unknown entity field (preview), `S100` unused suppression.
  Selection by code, prefix or name (`select`, `extend-select`, `ignore`, `per-file-ignores`),
  `--preview`, suppression comments, safe and unsafe fixes, `--explain`, `--list-rules`,
  `--statistics`, and the `text`, `concise`, `json`, `github` and `sarif` output formats.
  Configuration in `gruff.toml`, `.gruff.toml` or `pyproject.toml` (`[tool.gruff]`).
- **`gruff format`**, a gofmt-style whitespace formatter with per-family defaults and a `[format]`
  table (`indent`, `indent_width`, `paren_spacing`); `--check`, `--diff`, standard input. It
  refuses files with syntax errors and any output that would change the token stream.
- **`gruff index`**, a grep-able TSV index: `SYMBOLS`, `CALLS`, `FIELDS`, `INCLUDES`, `EVENTS` and
  `EVENTS_SUMMARY`, including calls, events and fields inside `#define` bodies at their uses.
- **`gruff hash`** (`gsc_hash`): every identifier hash family of the series, CoD1 to Black Ops 7,
  with stable ids, game lists, input canonicalisation and widths, the 63-then-60-bit lookup and
  CSV / WNI name-database readers. See `docs/hash-families.md`.
- **`gruff lsp`** (`gsc_lsp`): diagnostics with a debounced background lint, quick fixes and
  `source.fixAll.gsc`, definition, references, hover, document and workspace symbols, completion,
  signature help with builtin parameter types, call hierarchy, rename, semantic tokens,
  formatting, progress reporting, `gsc/status`. On the Black Ops III mod-tools scripts (3,437
  files) the workspace is ready in under a second.
- **VS Code extension** (`editors/vscode`, 0.1.0): TextMate grammars for `.gsc`, `.csc` and
  `.gsh`, language configuration, the bundled `gsc` server, settings (`gsc.game`, `gsc.dataDir`,
  `gsc.libraryRoots`, `gsc.mode`, `gsc.server.path`), status bar and commands. The client is
  bundled with esbuild.
- **Documentation book** (mdBook, `docs/`): getting started, configuration, editors, the language
  server, one page per rule, the dialect matrix, hash families, legal notice, contributing.
- **Release workflow** (`.github/workflows/release.yml`): binaries for Windows, Linux and macOS
  on x86_64 and aarch64, archives with a gruff-data snapshot, `SHA256SUMS`, per-platform `.vsix`,
  and a Marketplace / Open VSX publish job behind a manual approval.
- **Local tooling**, never fed game files into the repository: corpus export and manifest
  (`tools/corpus`), the official Black Ops III linker harness and its probes (`tools/linker`), and
  the builtin and field catalogue extractors that run the game executables' tables under Unicorn
  (`tools/re`).

### Data (gruff-data snapshot `f44913e`)

- Dialect profiles for 24 games, CoD1 to Black Ops 7, 73 features each, every value graded
  *verified-on-corpus*, *documented* or *assumed* and labelled with its licence.
- Profiles measured on a 20-game local corpus; eight games verified on corpus (iw1, iw2, iw3, t4,
  iw4, t5, codol, t7); the Black Ops II `foreach` question settled, and the two Black Ops III
  documentation conflicts recorded with the linker's verdict.
- Builtin catalogues for 18 games (19 files), extracted from the game executables. Per game, the
  arity source and how it was verified against the game's own scripts (B002 on that corpus
  before and after the corpus pass):

  | Game | Arity from | Verification | B002 on its corpus |
  |---|---|---|---|
  | `iw1` CoD1 | handler emulation through the engine slots | corpus-corrected; partial (SP module only) | 1166 -> 12 |
  | `iw3` CoD4 | handler emulation | corpus-corrected; partial (MP only) | 867 -> 12 |
  | `t4` World at War | handler emulation | corpus-corrected; partial (SP only) | 1934 -> 12 |
  | `iw4` MW2 | handler emulation, names by IW5 handler match | corpus-corrected; partial (names) | 34 -> 3 |
  | `t5` Black Ops | handler emulation (SP + MP) | corpus-corrected | 4866 -> 15 |
  | `iw5` MW3 | handler emulation (MP + SP) | corpus-corrected | 577 -> 12 |
  | `t6` Black Ops II | the tables (MP, ZM, SP) | corpus-checked, 22 widened | 131 -> 13 |
  | `iw6` Ghosts | handler emulation (SP dump) | corpus-corrected; partial (SP only) | 3269 -> 12 |
  | `s1` Advanced Warfare | handler emulation (MP + SP) | corpus-corrected; partial (14 unnamed) | 3516 -> 39 |
  | `t7` Black Ops III | the tables | corpus-checked; entity fields too | 0 |
  | `iw7` Infinite Warfare | handler emulation | corpus-corrected; partial (812 unnamed) | 5387 -> 20 |
  | `h1` MW Remastered | handler emulation (MP + SP) | corpus-corrected; partial (46 unnamed) | 2796 -> 24 |
  | `t8` Black Ops 4 | the tables | corpus-checked | 0 |
  | `iw8` MW 2019 | handler emulation | corpus-corrected; partial (117 unnamed) | 3126 -> 41 |
  | `t9` Cold War | the tables | corpus-checked | 0 |
  | `jup`, `jup-sp` MWIII | the tables (u8 min/max), handler runs otherwise | corpus-checked | 4; 15 -> 6 |
  | `t10` Black Ops 6 | none (stored, never checked by the engine) | names only, no corpus | - |
  | `t11` Black Ops 7 | the tables | corpus-checked (BO7 archive) | 0 |

  No catalogue yet for CoD2, United Offensive, CoD Online, WWII, Vanguard and MWII (2022).
  Every catalogue lists its compiler intrinsics (`isdefined`, `vectorscale`, and `endon`,
  `notify`, `waittill`... in T8-T11 and JUP) with their arity. An entry named from gsc-tool's
  tables is labelled GPL-3.0-only; the rest is CC0.
- The Black Ops III entity field catalogue.
- The dialect matrix (`docs/dialect-matrix.md`), JSON schemas for profiles, builtin and field
  catalogues, a validator that checks all of them, and the corpus probe.

### Known limitations

- Builtin rules need a catalogue: CoD2, United Offensive, CoD Online, WWII, Vanguard and MWII
  (2022) get syntax, dialect and loop rules only. On partial catalogues (CoD1, CoD4, World at
  War, MW2, Ghosts, Advanced Warfare, Infinite Warfare, MW Remastered, MW 2019) `B001` is off,
  since a missing name may be an unextracted or unnamed builtin. The field rule runs for Black
  Ops III only.
- Arities measured by handler emulation are corrected only where the game's scripts call the
  builtin; a builtin the corpus never calls keeps its measure. Most IW builtins ignore extra
  arguments, so `B002` reports only too few there.
- The language server re-checks the whole workspace after each pause in typing (about 200 ms on
  Black Ops III); semantic tokens are full-document only.
- `#include` chains are not followed transitively; Black Ops III load-time link order is not
  modelled.
- No thread and event (`T`) rules yet; the event index is there for them.

[Unreleased]: https://github.com/Kenshin9977/gruff/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/Kenshin9977/gruff/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/Kenshin9977/gruff/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/Kenshin9977/gruff/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/Kenshin9977/gruff/releases/tag/v0.1.0
