# tree-sitter-gsc

A [tree-sitter](https://tree-sitter.github.io/) grammar for GSC and CSC, the Call of Duty game
script language: one grammar for the superset of every dialect from Call of Duty (2003) to Black
Ops 7, hand-written source and decompiler output (gsc-tool, acts) alike. It is the editor tier of
Gruff: highlighting, folding, indentation, code navigation and structural selection in Neovim,
Helix, Zed, Emacs and the web. Diagnostics and dialect gating stay with the language server
(`gruff lsp`), whose hand-written parser in `crates/gsc_syntax` is the reference; this grammar
follows the same tree shape (`grammar/gsc.ungram`).

## What it covers

- Directives: `#using`, `#include`, `#insert`, `#inline`, `#namespace`, `#precache`,
  `#using_animtree`; `#define` (object and function-like, `\` continuations), `#undef`.
- Preprocessor conditionals as the Rust parser reads them: `#if`/`#ifdef`/`#ifndef`/`#endif`
  lines are extras and the first branch is active; each `#elif`/`#else` branch up to its `#endif`
  is one `preproc_disabled` node (so branches that split a function header still parse).
- Functions with `function`, `autoexec`, `private`, `codecall`, `event_handler[ name ]`; default,
  reference (`&x`), pointer (`*x`) and vararg (`...`) parameters; classes with `var`,
  `constructor`, `destructor` and bases; IW file constants; file-scope macro calls.
- Dev blocks `/# ... #/` at file scope (may stay open to the end of the file) and in statements.
- Statements: `if`, `while`, `do`, `for`, `foreach ( k, v in a )`, `switch`, `wait`,
  `waitrealtime`, `waittillframeend`, `const`, profiler words, `breakpoint`, `goto`/labels and acts'
  `jumpiffalse( c ) LOC;`, and upper-case macros used as statements without `;`.
- Expressions: the full operator table (with `??`, `===`, `?.`), calls with receivers and the
  `thread`/`childthread`/`call`/`threadendon`/`builtin` modifiers, `ns::f` and `path::f`
  qualification, `&f` and `::f` function pointers, `[[ f ]]( a )` and `[[ obj ]]->m( a )` calls,
  vectors, arrays, structs, `new`, `is`, casts, `&"istring"`, `#"hash"`, `#name`, `%anim`,
  `%tree::anim`, `#animtree`, `$name`, `thisthread`. `notify`, `endon`, `waittill` and `isdefined`
  are calls, as in the Rust parser.
- Keywords are case-folded (`If`, `Function` occur in shipped files). Words that are statement
  keywords in one place and ordinary names elsewhere (`goto`, `new`, `call`, `foreach`, `int`...)
  parse both ways.
- Decompiler spellings: `function_1a2b`, `@name`, names glued with `/` (`dlc5/zmhd/fx( 1 )`,
  `scripts/mp/x.gsc::f`), names starting with a digit, `!name`, acts' encoded animation names
  (`%\x83...`) and its bare `#` zero hash.

An external scanner (`src/scanner.c`) handles disabled branches, encoded animation names, macro
statements, slash-glued decompiler names and the end of `#define` lines.

## Corpus coverage

Share of files whose tree holds no `ERROR` or `MISSING` node, measured with
`script/corpus_stat.py` on the local 20-game corpus (40 649 readable files; never committed),
beside the Rust parser's error-free share on the same files (`cargo test -p gsc_syntax --test
corpus every_game_round_trips -- --ignored`):

| game | files | tree-sitter | Rust parser |
|---|---:|---:|---:|
| codol | 211 | 100.00% | 100.00% |
| h1 | 1277 | 99.92% | 99.92% |
| iw1 | 809 | 100.00% | 99.88% |
| iw2 | 622 | 99.68% | 99.68% |
| iw3 | 823 | 99.76% | 99.76% |
| iw4 | 1422 | 99.86% | 99.86% |
| iw5 | 1986 | 100.00% | 100.00% |
| iw6 | 1778 | 100.00% | 100.00% |
| iw7 | 3590 | 100.00% | 100.00% |
| iw8 | 1079 | 100.00% | 100.00% |
| jup | 3083 | 99.94% | 99.94% |
| jup_sp | 1788 | 100.00% | 100.00% |
| s1 | 2058 | 100.00% | 100.00% |
| s2 | 2108 | 100.00% | 100.00% |
| t4 | 1429 | 99.44% | 99.44% |
| t5 | 3823 | 99.95% | 99.95% |
| t6 | 3864 | 100.00% | 100.00% |
| t7 | 3437 | 99.65% | 99.68% |
| t8 | 2633 | 99.96% | 99.96% |
| t9 | 2829 | 100.00% | 100.00% |
| all | 40649 | 99.92% | 99.92% |

The remaining files are broken in the same places for both parsers: decompiler names that no
lexer can read (`init heli sounds max_accuracy = x;`, `level.+ level.x`), a missing `;` or `}` in
shipped source, `include` without `#`, an unterminated block comment.

## Building

```sh
npm install               # tree-sitter-cli and web-tree-sitter, into node_modules/
npx tree-sitter generate  # src/parser.c from grammar.js
npx tree-sitter test      # test/corpus/*.txt
npx tree-sitter build --wasm -o tree-sitter-gsc.wasm
python script/corpus_stat.py /path/to/corpus [game ...]
```

`src/` (the generated parser) is committed, as editors expect; CI regenerates it and fails when it
is stale. `tree-sitter build --wasm` downloads a wasi-sdk into the tree-sitter cache on first use.

## Queries

`queries/highlights.scm`, `locals.scm`, `tags.scm`, `folds.scm` and `indents.scm` use the
Neovim capture names.

## Licence

MIT (see [LICENSE](LICENSE)), so that editors and grammar collections can vendor it. The
rest of Gruff is GPL-3.0-or-later. The grammar is original work: it describes the GSC syntax,
including the output formats of gsc-tool and acts, without code from either or from gscode;
the headers in `src/tree_sitter` are tree-sitter's own (MIT).
