# grammar/

`gsc.ungram` describes the shape of the GSC concrete syntax tree in
[ungrammar](https://rust-analyzer.github.io/blog/2020/10/24/introducing-ungrammar.html) notation,
the format rust-analyzer uses for `rust.ungram`. Draft 1 (phase 1) matches the tree the parser in
`crates/gsc_syntax` builds: `cargo test -p gsc_syntax --test sourcegen` generates the typed AST
(`src/ast/generated.rs`) from it, fails when that file is stale, and checks that every node
`SyntaxKind` has a rule.

## Superset plus dialect gating

There is one grammar for all games, from CoD1 to Black Ops 7, not one grammar per game.

1. **The parser accepts the superset.** Every production in `gsc.ungram` parses in every dialect,
   so `class` in a CoD4 file, `#include` in a BO3 file or decompiler output that mixes
   families still produces a full tree. The parser itself is hand-written (recursive descent with
   a Pratt core for expressions) and never fails. Unparsable text goes into Error nodes.
2. **A dialect profile gates productions.** Each production carries a `// gate:` comment that
   names a feature of `gruff-data/schema/profile.schema.json`. After parsing, a check walks the tree.
   If a node's feature is off in the active profile (`gruff-data/profiles/<engine-id>.toml`), it
   reports a diagnostic such as "`class` requires a game with the `classes` feature (t7, t8, t9)".
   The tree is not changed.
3. **Only the lexer reads the profile directly.** The keyword set decides whether a word is a
   keyword or an identifier, so `call`, `vararg` or `function` stay legal names in games that lack
   them. The profile's string prefixes and hash spellings decide literal and identifier token kinds.
   This is Clang's model: `LangOptions` flags and per-keyword flags in `TokenKinds.def`, read by
   a single lexer, parser and Sema.

The model has three benefits. Errors are better ("not available in this game" instead of
"unexpected token"). Mixed decompiler output still parses. And adding a game means writing a data
file, not a grammar.

## Gate notation

| Comment | Meaning |
|---|---|
| `gate: f` | boolean feature `f` must be true |
| `gate: f = v` | enum feature `f` must equal `v` |
| `gate: f has v` | set or list feature `f` must contain `v` |
| no gate | universal |

A profile value of `"unknown"` or a `status = "unresolved"` entry means the check stays silent. A
missed diagnostic costs less than a false positive on shipped code.

## Retail and community modes

A feature's `value` describes the game's own (retail) compiler. Some games are also built by
community compilers or loaded by community clients (gsc-tool, Plutonium...), which accept more: BO2
has no evidenced retail preprocessor, but gsc-tool t6 accepts `#define`, `#if`, `#inline` and `.gsh`
headers. The profile records that as `features.<f>.community.value`. `Dialect::with_mode(Mode::Community)`
(`gruff parse --mode community`) makes every gate read the community value where there is one and
`value` elsewhere; a community value is its own claim, so the retail entry's `unresolved` status
does not silence it. Retail is the default.

## Diagnostic codes

| Code | Meaning |
|---|---|
| D001 | a construct whose feature is off in the active profile |
| D002 | a function declared without the `function` keyword where it is required |
| D003 | an import after the first declaration |
| D004 | `goto`, `jumpiffalse`/`jumpiftrue` or a label (`goto_labels`; also reported when unknown) |
| D005 | a `(int)x` cast (`casts`) |
| D006 | a `builtin` call (`builtin_call`) |
| E01xx | lexical errors |
| E02xx | parse errors |
| E0301 | decompiler output whose hash lookup resolved to text no tokenizer can read as one name |

E0301 only exists in decompiler mode. When a line holding a spaced name (`init heli sounds
max_accuracy = x;`, `example: level anim_hatch;`, `level.+ level.x`) has syntax errors, they become
one E0301, and the later errors of the same top-level item are dropped as consequences. The 11 BO3
dumps that fail to parse have only E0301 errors: without them the T7 decompiled corpus parses at
100%. Tools can skip such files with `SyntaxError::is_decompiler_name`.

## Macros

Macros never change the tree. `macro_expansions(root, dialect)` (or `macro_expansions_with` and a
`MacroTable` built from the headers, found with `HeaderSet` and `header_paths`) returns one
`MacroExpansion` per use: the use's range, the fully expanded tokens, and `parse()`, which parses
the expansion as items at file scope and as statements elsewhere. Object-like and function-like
macros, variadic `...`/`__VA_ARGS__`, argument pre-expansion, rescanning and `#undef` are handled;
`#` and `##` are not (no shipped script uses them). On the BO3 mod-tools sources (the only corpus
with macros): 5,410 `#define`s, 15,760 uses expanded in 710 files, all of which parse.

## What the file drives

- the typed AST views over the rowan CST (generated; seven nodes whose children are all
  expressions, such as `CallExpr`, have hand-written accessors in `src/ast/ext.rs`);
- the dialect check (`src/check.rs`), one rule per `gate:` comment;
- later: a documentation page per construct and a consistency test against the tree-sitter
  grammar's `node-types.json`.

## Changes from draft 0, and why

Measured on the BO3 mod-tools sources (978 files), the decompiled BO3 dumps and every other game
of the local corpus.

- **Preprocessor conditionals are trivia, not nodes.** Shipped BO3 code splits a function header
  across `#if`/`#else` (two signatures sharing one body), so branches are not well nested at item
  or statement level. The lexer keeps the first branch of each group active and turns every later
  branch into one `DISABLED_TEXT` trivia token, as Roslyn does. `PreprocIf`, `PreprocIfdef`,
  `PreprocElif`, `PreprocElse`, `PreprocCondition` and `PreprocBranchBody` are gone.
- **Directive paths are one token.** `#using scripts/shared/util;` (decompiler spelling) and
  `#insert scripts\shared\shared.gsh;` (extension kept) lex as one `'path'`; `ScriptPath` and the
  `('.' Name)?` suffix of `InsertDirective` are gone. `#using_animtree` takes an `ArgList`.
- **`MacroStmt`.** Mod-tools code writes macros as statements without `;` (`WAIT_SERVER_FRAME` on
  its own line). An upper-case name or call followed by a line break is a `MacroStmt`.
- **Dev blocks.** A file-scope `/#` may stay open until the end of the file (shipped BO3 code).
- **Pointer calls accept spaced brackets.** `[ [ f ] ]()` is a pointer call in shipped code, so
  `[[`/`]]` are not tokens; the parser looks for `[ [ ... ] ]` followed by `(` or `->`.
- **Calls.** The callee of `CallExpr` is any `NameRef`/`QualifiedRef` (both are `Expr`s), so
  receiver and callee accessors are hand-written. `builtin` joins the call modifiers (MWIII).
- **`for` clauses** are wrapped in `ForInit`/`ForCondition`/`ForStep` so that the three optional
  expressions stay distinguishable. `TupleAssignStmt` is gone: `[ a, b ] = v;` is an `AssignExpr`
  with an `ArrayExpr` on the left, gated by `tuple_assignment`.
- **Parameters**: `Param | VarargParam` is the enum `AnyParam`; `NamedParam` is now `Param`.
- **New nodes found in the corpus**: `CastExpr` (`(float)x`, CoD1/CoD2), `GotoStmt` and
  `LabelStmt` (acts and t7-src decompiler control flow, reported as D004), `obj.( expr )` fields
  (acts), `waittillframeend()` with parentheses, `'hash_ident'` literals (`#name:` struct keys),
  a foreach binding that is an expression (`foreach ( level.x in a )`, gsc-tool).
- **Contextual keywords.** Profiles list keywords inconsistently (BO4 lacks `self`, BO3 lacks
  `codecall`, Cold War lacks `constructor`, MWIII lists `class` but uses it as a variable name), so
  the parser recognises the structural words contextually and a keyword in name position becomes
  an identifier.

## Open points

- `QualifiedRef` used as a value without `&` (`x = ns::f;`) parses; whether it is a call or a
  pointer is left to the resolver.
- Decompiler output whose hash lookups resolved to text with spaces (`init heli sounds
  max_accuracy`, in 11 BO3 dumps) cannot be lexed as names and stays a syntax error, E0301.
- acts writes a zero hash as a bare `#` (`x ?? #;` in one BO7 file, beside the zero asset `%""`).
  It is not source syntax: the lexer accepts it as a `'hash_ident'` in decompiler mode only.
- Undecrypted BO7 animation names (`%\x83\x99...`) are one `'encoded_name'` token whose length
  comes from the third decoded byte (726 of 741 names in the BO7 archive); the others end at the
  line end.
- acts-only extensions (`detour`, lambdas, `autoexec(N)`, `#constexpr`, `#file`) are left out
  until a retail game is shown to use them.
- `CastExpr`, `builtin` and goto/labels are gated (`casts`, `builtin_call`, `goto_labels`).
