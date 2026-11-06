# Gruff playground

A static page running Gruff in the browser: live diagnostics (`gsc_wasm`), tree-sitter-gsc
highlighting, the syntax tree, the formatter and the hash families. No CDN, no server code.

```sh
sh build.sh              # pkg/ and vendor/ (not committed)
node smoke-test.mjs      # the same module under Node
python -m http.server 8000
```

See [docs/wasm.md](../../docs/wasm.md) for requirements, sizes and the JavaScript API.
