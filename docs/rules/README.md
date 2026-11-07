# gruff check rules

`gruff check` is a Ruff-like linter. Every rule has a code (a category letter plus digits) and a
readable name; both work in `--select`, `--ignore`, `--extend-select`, `gruff.toml` and
suppression comments. `gruff check --explain CODE` prints the rule's page; `--list-rules` lists
them all.

| Prefix | Category |
|---|---|
| `E` | syntax errors (parser) |
| `D` | dialect: syntax the selected game lacks |
| `B` | engine builtins |
| `L` | loops and control flow |
| `F` | entity fields |
| `I` | imports (`#using`, `#include`, `#insert`) |
| `M` | macros (`#define`) |
| `N` | names and declarations: calls to script functions, parameters, classes |
| `S` | suppression comments |
| `T` | threads and events |
| `U` | local variables |
| `V` | values (VM semantics measured in the game's VM) |

| Code | Name | Notes |
|---|---|---|
| [B001](B001.md) | `unknown-builtin` | unsafe fix |
| [B002](B002.md) | `builtin-arg-count` |  |
| [B003](B003.md) | `dev-only-builtin` | preview |
| [B004](B004.md) | `bare-call-other-namespace` | unsafe fix; namespace-first games |
| [B005](B005.md) | `thread-builtin` | namespace-first, file-first and builtin-strict games |
| [B006](B006.md) | `builtin-shadowed-function` | preview; builtin-strict games |
| [D001](D001.md) | `unavailable-feature` |  |
| [D002](D002.md) | `missing-function-keyword` |  |
| [D003](D003.md) | `import-after-declaration` |  |
| [D004](D004.md) | `decompiler-control-flow` |  |
| [D005](D005.md) | `unavailable-cast` |  |
| [D006](D006.md) | `unavailable-builtin-call` |  |
| [E0101](E0101.md) | `unterminated-string` |  |
| [E0102](E0102.md) | `unexpected-character` |  |
| [E0103](E0103.md) | `unterminated-block-comment` |  |
| [E0104](E0104.md) | `unbalanced-conditional` |  |
| [E0201](E0201.md) | `expected-token` |  |
| [E0202](E0202.md) | `unexpected-token` |  |
| [E0203](E0203.md) | `nesting-too-deep` |  |
| [E0204](E0204.md) | `vector-arity` |  |
| [E0299](E0299.md) | `parser-stuck` |  |
| [E0301](E0301.md) | `decompiler-name` |  |
| [F001](F001.md) | `unknown-field` | preview, unsafe fix |
| [I001](I001.md) | `duplicate-import` | preview, safe fix |
| [I002](I002.md) | `unused-import` | preview, safe fix |
| [I003](I003.md) | `unresolved-import` | preview; needs a library root |
| [L001](L001.md) | `infinite-loop-without-wait` | unsafe fix |
| [L002](L002.md) | `unreachable-code` | preview |
| [L003](L003.md) | `duplicate-case-label` | preview |
| [L004](L004.md) | `assignment-as-value` | preview |
| [L005](L005.md) | `non-constant-case-label` |  |
| [M001](M001.md) | `macro-arg-count` |  |
| [M002](M002.md) | `macro-redefined` | preview |
| [M003](M003.md) | `duplicate-macro-parameter` |  |
| [N001](N001.md) | `unresolved-script-function` | preview |
| [N002](N002.md) | `namespace-not-imported` | preview, unsafe fix; `#namespace` games |
| [N003](N003.md) | `private-function-call` | preview |
| [N004](N004.md) | `script-arg-count` | preview |
| [N005](N005.md) | `duplicate-function` |  |
| [N006](N006.md) | `duplicate-parameter` | preview |
| [N007](N007.md) | `misplaced-vararg` |  |
| [N008](N008.md) | `inheritance-cycle` |  |
| [N009](N009.md) | `constructor-parameters` |  |
| [S100](S100.md) | `unused-suppression` | safe fix |
| [T001](T001.md) | `unknown-event` | preview; needs a complete engine event catalogue |
| [T002](T002.md) | `misspelled-event` | preview |
| [T003](T003.md) | `waittill-arg-count` | preview |
| [T004](T004.md) | `missing-endon` | preview |
| [U001](U001.md) | `unused-variable` | preview |
| [U002](U002.md) | `uninitialized-variable` | preview |
| [V001](V001.md) | `hash-equality` | preview; BO3 (`hash_equality = "script-error"`) |
| [V002](V002.md) | `thread-call-value` | preview; games with `thread_call_value = "callee-result"` |
| [V003](V003.md) | `read-only-assignment` |  |
| [V004](V004.md) | `division-by-zero` | preview |
| [V005](V005.md) | `useless-expression` |  |

## Selection

Stable rules are on by default; preview rules need `--preview` (with it, they are on by default
too). Selection follows Ruff: the configuration file is one layer and the command line another,
applied in that order. In a layer, `select` replaces what the layers below enabled,
`extend-select` adds to it and `ignore` removes from it; when several selectors match a code the
most specific wins (`ALL` < `B` < `B0` < `B001`), and on a tie `ignore` wins.

## Configuration

`gruff check` reads the nearest `gruff.toml` (or `.gruff.toml`, or a `pyproject.toml` with a
`[tool.gruff]` table) at or above the first path, or the file given with `--config`. Keys are
accepted at the top level and in a `[check]` table:

```toml
game = "t7"
[check]
select = ["E", "D", "B", "L"]
extend-select = ["B003"]
ignore = ["D004"]
preview = false
[check.per-file-ignores]
"*_debug.gsc" = ["B003"]
```

Per-file-ignore globs are relative to the configuration file: `*` and `?` stay in one path
component, `**` spans several, and a pattern without `/` matches the file name anywhere.

## Suppression comments

- `// gsc: ignore[B001, L001]` suppresses those codes on its line; alone on its own line, it
  also covers the next line.
- `// gsc: ignore-file[B001]` anywhere suppresses them in the whole file.
- Without a list (`// gsc: ignore`), every code is suppressed.
- `gruff:` may replace `gsc:` (`// gruff: ignore[B001]`).
- Unused suppressions are reported as [S100](S100.md).

## Fixes and exit codes

`--fix` applies safe fixes; `--fix --unsafe-fixes` also applies unsafe ones. Fixed files keep
their encoding. Exit codes: 0 when nothing is left to report, 1 when diagnostics remain, 2 on a
usage, configuration or I/O error.
