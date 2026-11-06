# Development

## Layout

| Path | What it is |
|---|---|
| `crates/gsc_hash` | Every identifier hash family, CoD1 to BO7, with stable ids, game lists, input canonicalisation and widths; name-database readers. |
| `crates/gsc_syntax` | Modal lexer, resilient hand-written parser, lossless CST (rowan), typed AST, dialect checks. |
| `crates/gsc_sema` | The shared semantic core: workspace, file facts, name resolution, the data loaders, IDE queries. |
| `crates/gsc_check` | `gruff check`: rules, selection, suppression, fixes, output formats. |
| `crates/gsc_fmt` | `gruff format`. |
| `crates/gsc_index` | `gruff index`. |
| `crates/gsc_lsp` | `gruff lsp`, a thin layer over the core. |
| `crates/gruff` | The `gruff` binary (package `gruff`), and its deprecated `gsc` alias (`src/bin/gsc.rs`). |
| `grammar/gsc.ungram` | The grammar, the single source of the AST. |
| `editors/vscode` | The VS Code extension. |
| `tools/corpus`, `tools/linker`, `tools/re` | Local-only tooling: corpus export, the official BO3 linker harness, the catalogue extractors. They read the user's own installs and never store game files. |

## Checks

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The default tests use fixtures written by hand. Tests that need game data are `#[ignore]`d and
read local paths from environment variables, so nothing is copied into the repository:

```sh
# hash families against your own name databases
GSC_BO7_NAMES_DIR=<bo7 names dir> GSC_BO3_NAMES_CSV=<bo3 names csv> \
GSC_WNI_DIR=<GrayWolf package_index> \
cargo test --release -p gsc_hash --test name_databases -- --ignored --nocapture

# parser, formatter and language server on your local corpus (see tools/corpus/README.md)
GSC_CORPUS=<corpus dir> GRUFF_DATA=<gruff-data dir> cargo test --release -- --ignored --nocapture
```

## The book

```sh
cargo install mdbook
mdbook build docs          # writes target/book; needs ../gruff-data for the dialect matrix
mdbook serve docs
```

Rule pages (`docs/rules/*.md`) are also embedded in the binary for `gruff check --explain`, so they
keep LF line endings.

## The VS Code extension

```sh
cargo build --release
cd editors/vscode
npm ci
npm run package            # compile + esbuild bundle + the server binary, then vsce package
npm run smoke              # Node only: manifest, grammars, the bundled server over JSON-RPC
npm test                   # inside an installed VS Code (VSCODE_EXE, or GSC_TEST_DOWNLOAD=1)
```

The client is bundled by esbuild into `dist/extension.js`; the `.vsix` carries no
`node_modules`. Release builds package one `.vsix` per platform with `vsce package --target`.

## Releases

`.github/workflows/release.yml` runs on a `v*` tag or by hand. It builds the six targets, makes
the archives (binary, README, licence, a gruff-data snapshot) and `SHA256SUMS`, packages the
per-platform `.vsix` files and drafts a GitHub release. Publishing to the VS Code Marketplace and
Open VSX is a separate job behind the `marketplace` environment, which needs a manual approval
and the `VSCE_PAT` and `OVSX_PAT` secrets.

Versioning is SemVer 0.x: a minor release may add stable rules or change defaults; the
configuration schema freezes at 1.0. gruff-data is versioned on its own.
