# gsc_fmt

`gruff format`. Two styles:

- **`full`** (the default) reprints the tree through a Wadler/Prettier-style document IR
  (`src/doc.rs`: `group`, `indent`, `line`, `softline`, hard line, `fill`) and breaks long lines
  at `max_width`.
- **`minimal`** (`--style minimal`, `style = "minimal"`) is the cautious phase-1 normaliser: it
  keeps every line break and only rewrites horizontal whitespace and statement indentation.

Both keep every token, comment, string and preprocessor line, skip files with syntax errors, and
verify every changed result before returning or writing it: the output must parse, and its tokens
other than whitespace (code, comments, preprocessor text, inactive `#if` text) must equal the
input's. The user documentation is `docs/formatter.md`.

## Full style

Layout:

- One statement per line; one declaration per item. Blank lines between statements and
  declarations are kept up to `max_blank_lines` (default 1); blank lines at the start and end of a
  block go.
- Braces: `brace_style = "next_line"` (Allman, the default) or `"same_line"` (K&R, with
  `} else {` and `} while (...)`). `case` labels one level inside their `switch`, their
  statements one level further; a braced `case` body sits at the label's level.
- A control statement's unbraced body goes on the next line, indented. `else if` stays on one
  line.
- Argument lists, parameter lists and vectors: flat when they fit; otherwise `wrap = "fill"` (the
  default) puts as many arguments per line as fit, continuation lines one level deeper and the
  `)` after the last argument (the corpus style); `wrap = "one_per_line"` puts each on its own
  line and the `)` alone.
- Operator chains (`a + b + c`, `a && b && c`): one group per run of one precedence class, the
  operator at the end of the line (the corpus majority, 45 to 88% of the broken chains, most
  sources above 58%). `&&` and `||` chains break every operand onto its own line; other chains
  fill.
- Ternaries break before `?` and `:`. Array and struct literals break one element per line, or
  fill when every element is a literal or a name.
- `lhs = value`: a value without break points of its own (a name, a field, a function reference)
  moves to the next line when the line is too long.
- Comments keep their place. A comment that ended a line still ends it (the blanks before it are
  kept, so aligned trailing comments stay aligned) and does not count when measuring the line; a
  comment on its own line stays on its own line, re-indented. A line comment always ends its
  line, breaking the groups around it.
- `/# ... #/` dev blocks are formatted as code: a one-statement block written on one line stays on
  one line when it fits, otherwise the statements go one level inside.
- Copied as written: directives (`#using`, `#include`, `#insert`, `#define` with its
  continuation lines...), preprocessor lines, inactive `#if` text, every top-level item or
  statement holding a `#if` region or a `\` line splice, and the statements or declarations
  between `// gsc-fmt: off` and `// gsc-fmt: on` (also `/* gsc-fmt: off */`, and `gruff-fmt:` for `gsc-fmt:`). The directives must
  stand between statements or declarations; an `off` without `on` runs to the end of its block.
- Spacing: one space around binary, assignment and ternary operators and after commas; none
  before `,` `;` or the `(` of a call; none around `.` `::` `->` or after a prefix operator;
  `paren_spacing` inside the parentheses of calls, parameters, control statements, parenthesised
  expressions, vectors and literals; `bracket_spacing` inside index brackets; `keyword_space`
  between `if`/`while`/`for`/`foreach`/`switch` and `(`. Anything else keeps whether the source
  had a gap, so tokens written together stay together (decompiler spellings such as `%anim@1`).
- Lines that cannot break (a long string, a long name) may stay longer than `max_width`.

Safety rules of the printer, on top of the verification:

- Two tokens are never glued when the source had a gap and gluing them could lex differently.
- A keyword used as a name (`function` as a parameter in decompiler output) never starts a
  continuation line: the parser reads such a word as a keyword after a line break.
- Preprocessor lines start their own line and keep their original indentation.

## Options

`gruff.toml`:

```toml
game = "t7"                # dialect and default style; `[format] game` wins
[format]
style = "full"             # or "minimal"
max_width = 100            # 20 to 1000
tab_width = 4              # the width of a tab when measuring a line
indent = "tabs"            # or "spaces"
indent_width = 4           # with indent = "spaces"
brace_style = "next_line"  # or "same_line"
paren_spacing = "padded"   # "compact", "preserve"
bracket_spacing = "compact"     # index brackets: "padded", "compact", "preserve"
keyword_space = "space"    # "none", "preserve"
wrap = "fill"              # or "one_per_line"
max_blank_lines = 1        # 0 to 10
```

`gruff format --style minimal|full --max-width N` override the file. The language server reads the
same table; `textDocument/rangeFormatting` formats the statements that touch the range
(`format_range`: the whole file is formatted, then only the lexemes of those statements are
replaced, and the result is verified again).

## Defaults, measured on the corpus

Brace placement does not split the families: every studio source puts the `{` of a function, a
control statement and a `switch` on its own line (98.5 to 100% in iw1, iw2, iw3, iw4, t4, t5, t7
and codol), and never cuddles `} else` (at most 0.2%). So `next_line` is the default everywhere and
`same_line` is the option for community code. Unbraced bodies sit on the next line in 98.7 to
99.7% of the cases.

What does split them (share of the first form, comments and strings excluded):

| source | `f( a )` | `a[ i ]` | `if (` | `( a + b )` | `( 0, 0, 1 )` |
|---|---:|---:|---:|---:|---:|
| iw1 CoD1 source | 2.4% | 0.0% | 61.9% | 1.9% | 1.4% |
| iw2 CoD2 source | 6.3% | 0.0% | 64.8% | 7.3% | 0.5% |
| iw3 CoD4 source | 81.0% | 74.1% | 74.3% | 60.8% | 47.2% |
| iw4 MW2 source | 96.4% | 92.5% | 86.4% | 91.7% | 93.6% |
| codol source | 96.7% | 99.4% | 92.4% | 100% | 100% |
| t4 WaW source | 68.2% | 66.2% | 41.6% | 55.8% | 93.1% |
| t5 BO1 source | 63.8% | 69.6% | 42.2% | 43.6% | 91.7% |
| t5 BO1 mod-tools source | 65.7% | 50.8% | 53.2% | 57.5% | 85.8% |
| t7 BO3 mod-tools source | 86.1% | 19.5% | 63.6% | 70.0% | 50.6% |

Decompiler output follows its decompiler: gsc-tool (t6, iw5 to iw8, s1, s2, h1) writes `a[i]`,
acts (t7, t8, t9, jup) writes `a[ i ]`; both pad calls and vectors and always write `if (`.

So: `paren_spacing` padded except `compact` for iw1, iw1-uo and iw2 (unchanged from phase 1);
`bracket_spacing` compact for iw1, iw1-uo, iw2, t6, t7, iw6, iw7, iw8, s1, s2 and h1, padded
elsewhere; `keyword_space` none for t4 and t5, space elsewhere. Lines longer than 100 columns
(tab = 4) are 1.4 to 3.3% of the studio source lines (2.5 to 7.8% of decompiler output), so the
width is 100. `cargo test -p gsc_fmt --release --test corpus measure_style -- --ignored
--nocapture` prints these counts.

The minimal style's measurements (phase 1): on the t7 BO3 mod-tools sources it changes 15.7% of the
lines with `padded`; `case` labels sit one level inside their `switch` in 78% of the t7 switches;
indentation is tabs in every studio source.

## Corpus gate

`cargo test -p gsc_fmt --release --test corpus -- --ignored --nocapture` formats every file of
the local corpus in memory, formats the result again, and compares the token streams
(`GSC_FMT_STYLE=minimal` for the minimal style). Full style, 2026-11-06:

| folder | files | skipped (syntax) | changed | lines rewritten | unsafe | not idempotent |
|---|---:|---:|---:|---:|---:|---:|
| codol/source | 211 | 0 | 209 | 4.0% | 0 | 0 |
| h1/decompiled | 1,277 | 1 | 1,256 | 72.9% | 0 | 0 |
| iw1/source | 809 | 1 | 303 | 34.1% | 0 | 0 |
| iw2/source | 622 | 2 | 390 | 34.1% | 0 | 0 |
| iw3/source | 823 | 2 | 818 | 40.1% | 0 | 0 |
| iw4/source | 1,422 | 2 | 1,369 | 17.5% | 0 | 0 |
| iw5/decompiled | 1,986 | 0 | 1,982 | 70.1% | 0 | 0 |
| iw6/decompiled | 1,778 | 0 | 1,773 | 74.1% | 0 | 0 |
| iw7/decompiled | 3,590 | 0 | 3,575 | 71.6% | 0 | 0 |
| iw8/decompiled | 1,079 | 0 | 1,059 | 70.3% | 0 | 0 |
| jup/decompiled | 3,083 | 2 | 3,078 | 73.6% | 0 | 0 |
| jup_sp/decompiled | 1,788 | 0 | 1,773 | 72.1% | 0 | 0 |
| s1/decompiled | 2,058 | 0 | 2,041 | 73.4% | 0 | 0 |
| s2/decompiled | 2,108 | 0 | 2,074 | 73.9% | 0 | 0 |
| t4/source | 1,429 | 8 | 1,417 | 51.5% | 0 | 0 |
| t5/source | 2,486 | 1 | 2,471 | 51.1% | 0 | 0 |
| t5/source-modtools | 1,337 | 1 | 1,332 | 48.6% | 0 | 0 |
| t6/decompiled | 3,864 | 0 | 3,850 | 76.8% | 0 | 0 |
| t7/decompiled | 2,459 | 11 | 2,425 | 67.7% | 0 | 0 |
| t7/source-modtools | 978 | 0 | 920 | 23.6% | 0 | 0 |
| t8/decompiled | 2,633 | 1 | 2,580 | 70.9% | 0 | 0 |
| t9/decompiled | 2,829 | 0 | 2,777 | 72.1% | 0 | 0 |
| total | 40,649 | 32 | 39,472 | 65.4% | 0 | 0 |

"Lines rewritten" counts the input lines a line diff does not keep. Decompiler output is indented
with four spaces, so nearly every one of its indented lines changes to tabs; the studio sources
change mostly by spacing (t4 and t5 mix `if(` and `if (`, the iw1/iw2 files mix both paren
styles). Most iw1 aitype and character files (generated code) are already in the full style.

Throughput of `gruff format --check` (parse, format, re-lex and re-parse to verify), release build:
t7 mod-tools sources (10.4 MB) 2.01 s on one thread (5.2 MB/s; minimal style 1.42 s) and 0.29 s on
24 threads; t9 decompiled (50.7 MB) 6.58 s on one thread (7.7 MB/s), 0.90 s on 24 threads. The
gate above (format twice, compare, diff) runs over the 608 MB corpus in 56 s on 24 threads.

Tests: snapshot tests over the hand-written fixtures of `tests/fixtures` (both styles and
several options), property tests on generated programs and whitespace-shuffled fixtures under
random options of both styles (token equivalence and idempotence), unit tests of the printer
(`src/doc.rs`) and of range formatting.
