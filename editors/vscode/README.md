# Gruff — GSC language tools for VS Code

Language support for Call of Duty GSC and CSC scripts, from CoD 1 to Black Ops 7, powered by the
Gruff language server (`gruff lsp`, from the [Gruff](https://github.com/Kenshin9977/gruff)
repository). Not affiliated with or
endorsed by Activision. The extension ships no game files or data.

## Features

- **Languages**: `gsc` (`.gsc`), `csc` (`.csc` client scripts) and `gsh` (`.gsh` headers).
- **Highlighting**: a TextMate grammar (comments, `/@ @/` doc comments, `/# #/` dev blocks,
  directives, Treyarch and IW function definitions, `ns::f` and `path\file::f` qualifiers,
  function pointers, hashed literals `#"..."`, `%anim`, `$ent`), refined by semantic tokens from
  the server (script functions versus builtins, namespaces, parameters, locals, fields, macros,
  hash literals).
- **Editing**: bracket and quote pairs, `/@ @/` auto-closing, folding of dev blocks, doc
  comments, block comments, `#if`/`#ifdef` sections and `// region` markers.
- **Diagnostics**: syntax errors, dialect errors (features the game lacks) and the `gruff check`
  rules (unknown builtins, wrong arity, infinite loops without a wait...), per file, honouring
  `gruff.toml` or the older `gsc.toml` (`game`, `select`/`extend-select`/`ignore`,
  `per-file-ignores`, `library`).
- **Quick fixes** for the rules that have them; unsafe fixes say so in their title and are never
  preferred. `source.fixAll.gsc` applies the safe ones.
- **Navigation**: go to definition, find references, hover (signature, doc comment, where it is
  defined, builtin arity), document outline, workspace symbols, call hierarchy.
- **Completion** of locals, parameters, functions in scope, macros and builtins; fields after
  `.`, a namespace's functions after `ns::`, methods after `->`. **Signature help** for script
  functions and builtins (arity and parameter types from gruff-data).
- **Rename** of local variables, parameters and script functions, across the workspace. Builtins
  and functions defined in library roots are refused.
- **Formatting** with `gruff format` (the same safety checks: files with syntax errors are left
  alone, and output that would change the token stream is rejected).

## Settings

| Setting | Default | What it does |
|---|---|---|
| `gruff.game` | `auto` | Game id or alias (`t7`, `bo3`, `t6`, `iw5`...). `auto`: `game` in `gruff.toml`, else a guess from the folder name, `#using` versus `#include` and client scripts. |
| `gruff.dataDir` | empty | The `gruff-data` directory. Empty: `data-dir` in `gruff.toml`, `$GRUFF_DATA` (or `$GSC_DATA`), then a `gruff-data` (or `gsc-data`) folder above the workspace or the server binary. Without it, files are read as the superset dialect, with no builtin checks. |
| `gruff.libraryRoots` | `[]` | The game's own scripts (the mod tools' `share/raw`, a decompiled dump): used to resolve names, never reported on. Relative to the first workspace folder; `${workspaceFolder}` is expanded. Adds to `library` in `gruff.toml`. |
| `gruff.hashPackages` | `[]` | Hash packages naming the hashed identifiers (BO3, BO4 to BO7, MWIII). Adds to `hash_packages` in `gruff.toml`. |
| `gruff.mode` | `retail` | `retail` (the game's compiler) or `community` (gsc-tool, Plutonium...). |
| `gruff.server.path` | empty | The `gruff` binary. Empty: the binary bundled in the extension (`server/<platform>-<arch>/`), then `gruff` on `PATH`, then the deprecated `gsc` alias on `PATH`. |
| `gruff.trace.server` | `off` | Log the LSP traffic in the *Gruff Language Server* output. |

Changing a setting (or `gruff.toml`) reloads the workspace; changing `gruff.server.path` restarts
the server. The status bar shows the game in use; click it for details.

Example `gruff.toml` for a Black Ops III mod:

```toml
game = "t7"
library = ["C:/Program Files (x86)/Steam/steamapps/common/Call of Duty Black Ops III/share/raw"]

[check]
extend-select = ["L"]
ignore = ["F001"]
```

## Commands

- **Gruff: Restart Language Server**
- **Gruff: Show Server Status**

## Building the package

```sh
cargo build --release                 # in the repository root
cd editors/vscode
npm install --ignore-scripts
npm run package                       # copy target/release/gruff[.exe], compile, esbuild, vsce package
```

`npm run package` writes `gruff-<version>.vsix`; install it with
`code --install-extension gruff-<version>.vsix`. The client and `vscode-languageclient` are
bundled by esbuild into `dist/extension.js`, so the package carries no `node_modules`. It holds
the server binary of the platform it was built on only; the release workflow builds one package
per platform with `vsce package --target <platform>` (`win32-x64`, `linux-arm64`,
`darwin-arm64`...). Nothing is published from a local build.

Tests: `npm run smoke` (plain Node: the manifest, the grammars, and the bundled server answering
over JSON-RPC) and `npm test` (the extension inside an installed VS Code, on the gsc_lsp test
fixtures; set `VSCODE_EXE`, or `GSC_TEST_DOWNLOAD=1` to download VS Code).

## Licence

GPL-3.0-or-later.
