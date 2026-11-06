#!/bin/sh
# Build the playground: gsc_wasm (wasm-bindgen, --target web) into pkg/, and tree-sitter-gsc
# with the web-tree-sitter runtime into vendor/. Needs the wasm32-unknown-unknown target,
# wasm-bindgen-cli at the version of crates/gsc_wasm/Cargo.toml (WASM_BINDGEN overrides the
# command), and npm for tree-sitter. wasm-opt is used when found (WASM_OPT overrides it).
# SPDX-License-Identifier: GPL-3.0-or-later
set -eu

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
ts="$repo/editors/tree-sitter-gsc"
target="${CARGO_TARGET_DIR:-$repo/target}"
bindgen="${WASM_BINDGEN:-wasm-bindgen}"

# Size over speed for the web build; the native release profile is left alone.
CARGO_PROFILE_RELEASE_OPT_LEVEL=s CARGO_PROFILE_RELEASE_LTO=true \
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_PROFILE_RELEASE_PANIC=abort \
  cargo build --manifest-path "$repo/Cargo.toml" -p gsc_wasm --release \
  --target wasm32-unknown-unknown

rm -rf "$here/pkg"
"$bindgen" --target web --no-typescript --out-dir "$here/pkg" \
  "$target/wasm32-unknown-unknown/release/gsc_wasm.wasm"

opt="${WASM_OPT:-$(command -v wasm-opt || true)}"
if [ -n "$opt" ]; then
  "$opt" -Oz --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
    --enable-mutable-globals --enable-reference-types --enable-multivalue \
    "$here/pkg/gsc_wasm_bg.wasm" -o "$here/pkg/gsc_wasm_bg.wasm"
fi

# tree-sitter-gsc for highlighting.
if [ ! -d "$ts/node_modules" ]; then
  (cd "$ts" && npm ci)
fi
if [ ! -f "$ts/tree-sitter-gsc.wasm" ] || [ "$ts/src/parser.c" -nt "$ts/tree-sitter-gsc.wasm" ] \
  || [ "$ts/src/scanner.c" -nt "$ts/tree-sitter-gsc.wasm" ]; then
  (cd "$ts" && npx tree-sitter build --wasm -o tree-sitter-gsc.wasm)
fi
mkdir -p "$here/vendor"
cp "$ts/tree-sitter-gsc.wasm" "$ts/queries/highlights.scm" "$here/vendor/"
cp "$ts/node_modules/web-tree-sitter/web-tree-sitter.js" \
  "$ts/node_modules/web-tree-sitter/web-tree-sitter.wasm" "$here/vendor/"

ls -l "$here/pkg/gsc_wasm_bg.wasm" "$here/vendor/tree-sitter-gsc.wasm"
