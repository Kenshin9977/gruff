# WebAssembly and the playground

`crates/gsc_wasm` compiles the parser, the checker, the formatter and the hash families to
WebAssembly (`wasm32-unknown-unknown`, wasm-bindgen). `editors/playground` is a static page built
on it, with tree-sitter-gsc for highlighting. Everything is served from local files: no CDN.

## Building

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked   # the version gsc_wasm pins
sh editors/playground/build.sh     # pkg/ (gsc_wasm) and vendor/ (tree-sitter-gsc, web-tree-sitter)
node editors/playground/smoke-test.mjs
cd editors/playground && python -m http.server 8000         # then open http://localhost:8000
```

`build.sh` builds with `opt-level = "s"`, full LTO and one codegen unit (environment overrides;
the native release profile is untouched) and runs `wasm-opt -Oz` when it is on the `PATH`
(`WASM_OPT` names another one; `tree-sitter build --wasm` downloads one into its cache). When
gruff-data is found (`GRUFF_DATA`, or `../gruff-data` next to the repository) the build embeds the BO3
(`t7`) profile, event catalogue and builtin catalogue, the last reduced to names, hashes and
arities.

Sizes (0.1.0): `gsc_wasm_bg.wasm` is 780 KB (323 KB gzipped) without embedded data and 1.26 MB
(380 KB gzipped) with the T7 data; `tree-sitter-gsc.wasm` is 298 KB (33 KB gzipped).

## API

```js
import init, { Engine, version, embeddedGames, hash, hashFamilies } from './pkg/gsc_wasm.js';
await init();
const t7 = new Engine('t7');                     // embedded data
const other = new Engine('t9', profileToml, builtinsToml, eventsToml);
const any = new Engine();                        // the superset dialect, no data
JSON.parse(t7.parse(text, /* tokens */ false));  // {decompiled, errors, tree}
t7.syntaxTree(text);                             // indented text
JSON.parse(t7.check(text, 'scripts/a.gsc', /* select */ undefined, /* preview */ false));
JSON.parse(t7.format(text, 'full'));             // {ok, changed, text} or {ok: false, error}
hash('t7.canon', 'main');                        // hex at the family's width
```

Positions carry `offset` (bytes), `line` and `col` (1-based, characters) and `utf16` (offsets
into a JavaScript string). In Node, load the module with
`initSync({ module: readFileSync('pkg/gsc_wasm_bg.wasm') })`.

`check` sees one file: names defined in other files (`#using`) do not resolve, so the rules that
need them stay quiet on such names, as for a file opened alone in the language server.

## The playground

`editors/playground/index.html`: an editor with live diagnostics (click one to select its range),
the tree-sitter highlighting of the text, the `gsc_syntax` tree, the formatter's output (`full` or
`minimal`, with a button to replace the source) and the hash of a string under every family. The
game list holds `superset` and the embedded games; a profile and a builtin catalogue TOML of
another game can be loaded from disk.
