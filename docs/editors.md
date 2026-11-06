# Editors

`gruff lsp` is a standard language server on standard input and output, so any LSP client can use
it. The VS Code extension bundles the binary; for other editors, put `gruff` on your `PATH`.

## Neovim

Neovim 0.11 or later:

```lua
vim.lsp.config("gruff", { cmd = { "gruff", "lsp" }, filetypes = { "gsc", "csc", "gsh" },
  root_markers = { "gruff.toml", ".git" },
  init_options = { game = "t7", libraryRoots = { "/path/to/share/raw" } } })
vim.lsp.enable("gruff")
vim.filetype.add({ extension = { gsc = "gsc", csc = "csc", gsh = "gsh" } })
```

## Helix

In `languages.toml`:

```toml
[language-server.gruff]
command = "gruff"
args = ["lsp"]
config = { game = "t7" }

[[language]]
name = "gsc"
scope = "source.gsc"
file-types = ["gsc", "csc", "gsh"]
comment-token = "//"
language-servers = ["gruff"]
```

## tree-sitter

The grammar is **MIT**-licensed (`editors/tree-sitter-gsc/LICENSE`), unlike the rest of
Gruff (GPL-3.0-or-later), so that Neovim, Helix, Zed and grammar collections can vendor it.

`editors/tree-sitter-gsc` is a tree-sitter grammar for the whole dialect superset (CoD1 to BO7,
source and decompiler output), with `highlights`, `locals`, `tags`, `folds` and `indents` queries
in Neovim capture names. On the 20-game corpus 99.92% of the files parse without an `ERROR` node,
the same share as the server's own parser (per-game table in the grammar's README). It is for
highlighting, folding and structural selection; diagnostics stay with `gruff lsp`.

Helix, in `languages.toml` beside the language above:

```toml
[[grammar]]
name = "gsc"
source = { path = "/path/to/gruff/editors/tree-sitter-gsc" }
```

then `hx --grammar build` and copy `queries/*.scm` to `runtime/queries/gsc/`. Neovim
(nvim-treesitter): register a parser with `install_info = { url = "/path/to/gruff/editors/
tree-sitter-gsc", files = { "src/parser.c", "src/scanner.c" } }` and copy the queries to
`queries/gsc/`. Zed and Emacs (`treesit`) load the same `src/` directory.

## Other clients

Start `gruff lsp` (the `--stdio` flag is accepted and ignored). The settings object the server reads
from `initializationOptions` and `workspace/didChangeConfiguration` is described in
[The language server](lsp.md#settings).

{{#include ../editors/vscode/README.md}}
