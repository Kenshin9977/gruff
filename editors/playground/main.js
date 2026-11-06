// GSC playground: gsc_wasm (diagnostics, syntax tree, format, hash) and tree-sitter-gsc
// (highlighting), all local files. SPDX-License-Identifier: GPL-3.0-or-later

import init, { Engine, version, embeddedGames, hash, hashFamilies } from './pkg/gsc_wasm.js';

const SAMPLE = `#using scripts\\shared\\callbacks_shared;

#insert scripts\\shared\\shared.gsh;

#namespace demo;

function autoexec __init__()
{
	callback::on_spawned( &on_player_spawned );
}

function on_player_spawned()
{
	self endon( "disconnect" );
	self thread watch_health();
	level.demo_spawns = isdefined( level.demo_spawns ) ? level.demo_spawns + 1 : 1;
}

function watch_health()
{
	while ( true )
	{
		self waittill( "damage", amount );
		if ( self.health < 30 )
			self iprintlnbold( &"DEMO_LOW_HEALTH" );
		wait 0.05;
	}
}

function spin()
{
	for ( ;; )
	{
		x = abs( 1, 2 );
	}
}
`;

const $ = id => document.getElementById(id);
const state = { engine: null, profile: null, builtins: null, tsParser: null, tsQuery: null };

function setStatus(text) {
  $('status').textContent = text;
}

function makeEngine() {
  const game = $('game').value;
  try {
    state.engine?.free();
    state.engine = new Engine(game, state.profile ?? undefined, state.builtins ?? undefined);
    const data = state.engine.hasBuiltins ? 'builtins loaded' : 'no builtin catalogue';
    setStatus(`${state.engine.game}: ${data}`);
  } catch (e) {
    state.engine = null;
    setStatus(String(e.message ?? e));
  }
}

function select(start, end) {
  const area = $('source');
  area.focus();
  area.setSelectionRange(start, end);
}

function renderDiagnostics(text) {
  const list = $('diagnostics');
  list.replaceChildren();
  let diags;
  try {
    diags = JSON.parse(state.engine.check(text, 'main.gsc', $('select').value || undefined,
      $('preview').checked));
  } catch (e) {
    const li = document.createElement('li');
    li.textContent = String(e.message ?? e);
    list.append(li);
    $('count').textContent = '';
    return;
  }
  $('count').textContent = diags.length ? `(${diags.length})` : '';
  if (!diags.length) {
    const li = document.createElement('li');
    li.className = 'muted';
    li.textContent = 'All checks passed.';
    list.append(li);
  }
  for (const d of diags) {
    const li = document.createElement('li');
    li.className = /^E/.test(d.code) ? 'error' : 'warning';
    const code = document.createElement('span');
    code.className = 'code';
    code.textContent = d.code;
    const at = document.createElement('span');
    at.className = 'at';
    at.textContent = `${d.start.line}:${d.start.col}`;
    li.append(code, at, d.message + (d.fix ? `  [fix: ${d.fix.message}]` : ''));
    li.title = d.name;
    li.addEventListener('click', () => select(d.start.utf16, d.end.utf16));
    list.append(li);
  }
}

function renderTree(text) {
  $('tree').textContent = state.engine.syntaxTree(text);
}

function renderFormat(text) {
  const r = JSON.parse(state.engine.format(text, $('style').value));
  $('format-status').textContent = r.ok
    ? (r.changed ? 'Formatted.' : 'Already formatted.')
    : `Left alone: ${r.error}`;
  $('formatted').textContent = r.ok ? r.text : '';
  $('apply-format').disabled = !r.ok || !r.changed;
}

function captureClass(name) {
  const parts = name.split('.');
  return parts.map((_, i) => 'hl-' + parts.slice(0, i + 1).join('-')).join(' ');
}

function renderHighlight(text) {
  const out = $('highlighted');
  if (!state.tsParser) {
    out.textContent = 'tree-sitter-gsc is not loaded (run build.sh).';
    return;
  }
  const tree = state.tsParser.parse(text);
  const captures = state.tsQuery.captures(tree.rootNode);
  // Outer captures first; an inner one inside an emitted range is skipped.
  captures.sort((a, b) => a.node.startIndex - b.node.startIndex
    || b.node.endIndex - a.node.endIndex);
  const frag = document.createDocumentFragment();
  let at = 0;
  for (const c of captures) {
    const { startIndex: s, endIndex: e } = c.node;
    if (s < at || e <= s) continue;
    if (s > at) frag.append(text.slice(at, s));
    const span = document.createElement('span');
    span.className = captureClass(c.name);
    span.textContent = text.slice(s, e);
    frag.append(span);
    at = e;
  }
  frag.append(text.slice(at));
  out.replaceChildren(frag);
  tree.delete();
}

function renderHash() {
  const text = $('hash-input').value;
  try {
    $('hash-output').textContent = `0x${hash($('family').value, text)}`;
  } catch (e) {
    $('hash-output').textContent = String(e.message ?? e);
  }
  const rows = JSON.parse(hashFamilies()).map(f => {
    const tr = document.createElement('tr');
    for (const v of [f.id, `0x${hash(f.id, text)}`, f.kinds]) {
      const td = document.createElement('td');
      td.textContent = v;
      tr.append(td);
    }
    return tr;
  });
  $('hash-all').replaceChildren(...rows);
}

let timer = 0;
function update() {
  clearTimeout(timer);
  timer = setTimeout(() => {
    if (!state.engine) return;
    const text = $('source').value;
    renderDiagnostics(text);
    const tab = document.querySelector('nav [aria-selected="true"]').dataset.tab;
    if (tab === 'tree') renderTree(text);
    if (tab === 'format') renderFormat(text);
    if (tab === 'highlight') renderHighlight(text);
  }, 150);
}

function showTab(name) {
  for (const b of document.querySelectorAll('nav button')) {
    b.setAttribute('aria-selected', String(b.dataset.tab === name));
  }
  for (const t of document.querySelectorAll('.tab')) t.hidden = t.id !== `tab-${name}`;
  if (name === 'hash') renderHash();
  update();
}

async function readFile(input) {
  const f = input.files?.[0];
  return f ? await f.text() : null;
}

async function loadTreeSitter() {
  try {
    const { Parser, Language, Query } = await import('./vendor/web-tree-sitter.js');
    await Parser.init({ locateFile: () => 'vendor/web-tree-sitter.wasm' });
    const lang = await Language.load('vendor/tree-sitter-gsc.wasm');
    const parser = new Parser();
    parser.setLanguage(lang);
    const highlights = await (await fetch('vendor/highlights.scm')).text();
    state.tsQuery = new Query(lang, highlights);
    state.tsParser = parser;
  } catch (e) {
    console.warn('tree-sitter-gsc unavailable:', e);
  }
}

async function main() {
  await init();
  $('version').textContent = `gruff ${version()}`;
  const games = ['superset', ...JSON.parse(embeddedGames())];
  for (const g of games) $('game').append(new Option(g, g));
  $('game').value = games.includes('t7') ? 't7' : 'superset';
  for (const f of JSON.parse(hashFamilies())) $('family').append(new Option(f.id, f.id));
  $('family').value = 't7.canon';

  $('source').value = SAMPLE;
  makeEngine();
  await loadTreeSitter();

  $('source').addEventListener('input', update);
  $('source').addEventListener('keydown', e => {
    if (e.key === 'Tab' && !e.ctrlKey && !e.metaKey) {
      e.preventDefault();
      document.execCommand('insertText', false, '\t');
    }
  });
  for (const id of ['preview', 'select', 'style']) $(id).addEventListener('input', update);
  $('game').addEventListener('change', () => { makeEngine(); update(); });
  $('profile').addEventListener('change', async () => {
    state.profile = await readFile($('profile'));
    makeEngine();
    update();
  });
  $('builtins').addEventListener('change', async () => {
    state.builtins = await readFile($('builtins'));
    makeEngine();
    update();
  });
  for (const b of document.querySelectorAll('nav button')) {
    b.addEventListener('click', () => showTab(b.dataset.tab));
  }
  $('apply-format').addEventListener('click', () => {
    const r = JSON.parse(state.engine.format($('source').value, $('style').value));
    if (r.ok) {
      $('source').value = r.text;
      update();
    }
  });
  for (const id of ['family', 'hash-input']) $(id).addEventListener('input', renderHash);
  update();
}

main().catch(e => setStatus(`failed to start: ${e.message ?? e}`));
