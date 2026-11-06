// Node smoke test of the web build: `node smoke-test.mjs` after `build.sh`.
// SPDX-License-Identifier: GPL-3.0-or-later

import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const wasm = await import('./pkg/gsc_wasm.js');
wasm.initSync({ module: readFileSync(join(here, 'pkg/gsc_wasm_bg.wasm')) });

const SOURCE = 'function f()\n{\n\twhile ( 1 )\n\t{\n\t\tx = 1;\n\t}\n}\n';

assert.match(wasm.version(), /^\d+\.\d+\.\d+/);
assert.equal(wasm.hash('t10.function', '0exp1j983'), 'ec28edb5ed47500');
assert.ok(JSON.parse(wasm.hashFamilies()).some(f => f.id === 't7.canon'));
assert.throws(() => wasm.hash('no.such', 'x'));

const games = JSON.parse(wasm.embeddedGames());
const superset = new wasm.Engine();
assert.equal(superset.game, 'superset');

const parsed = JSON.parse(superset.parse(SOURCE, true));
assert.deepEqual(parsed.errors, []);
assert.equal(parsed.tree.kind, 'SOURCE_FILE');
const broken = JSON.parse(superset.parse('f()\n{\n\tx = ;\n}\n', false));
assert.ok(broken.errors.length > 0);
assert.equal(broken.errors[0].start.line, 3);
assert.match(superset.syntaxTree(SOURCE), /FUNCTION_DEF@/);

const diags = JSON.parse(superset.check(SOURCE));
assert.ok(diags.some(d => d.code === 'L001'), JSON.stringify(diags));
assert.deepEqual(JSON.parse(superset.check(SOURCE, 'a.gsc', 'B')), []);

const formatted = JSON.parse(superset.format('f(){x=1;}'));
assert.equal(formatted.ok, true);
assert.equal(formatted.changed, true);
assert.equal(JSON.parse(superset.format('f( {')).ok, false);

if (games.includes('t7')) {
  const t7 = new wasm.Engine('t7');
  assert.equal(t7.hasBuiltins, true);
  const d = JSON.parse(t7.check('function f()\n{\n\tx = abs( 1, 2, 3 );\n}\n'));
  assert.ok(d.some(x => x.code === 'B002'), JSON.stringify(d));
}
assert.throws(() => new wasm.Engine('no_such_game'));

// tree-sitter-gsc through web-tree-sitter.
const vendor = join(here, 'vendor');
if (existsSync(join(vendor, 'tree-sitter-gsc.wasm'))) {
  const { Parser, Language, Query } = await import('./vendor/web-tree-sitter.js');
  await Parser.init();
  const lang = await Language.load(readFileSync(join(vendor, 'tree-sitter-gsc.wasm')));
  const parser = new Parser();
  parser.setLanguage(lang);
  const tree = parser.parse('function f()\n{\n\tself thread g( &"A", #"b", %anim );\n}\n');
  assert.equal(tree.rootNode.hasError, false);
  assert.equal(tree.rootNode.child(0).type, 'function_definition');
  const query = new Query(lang, readFileSync(join(vendor, 'highlights.scm'), 'utf8'));
  assert.ok(query.captures(tree.rootNode).some(c => c.name === 'function.call'));
  tree.delete();
  console.log('tree-sitter-gsc: ok');
}

console.log(`gsc_wasm ${wasm.version()}: ok (embedded games: ${games.join(', ') || 'none'})`);
