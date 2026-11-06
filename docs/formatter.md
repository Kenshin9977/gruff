# The formatter

`gruff format` rewrites whitespace and line breaks, nothing else. Every result is checked before it
is written: it must parse, and its tokens other than whitespace (code, comments, preprocessor
lines, inactive `#if` text) must equal the input's. Files with syntax errors are left alone.

```sh
gruff format scripts/               # format in place
gruff format --check .              # CI: list the files that would change, exit 1 if any
gruff format --diff scripts/foo.gsc # print a unified diff
gruff format --style minimal .      # whitespace only, keep every line break
gruff format --max-width 120 .
cat foo.gsc | gruff format - --stdin-filename foo.gsc
```

## Styles

**`full`** (the default) prints the code with its own line breaks. Statements go one per line,
braces on their own line (`brace_style = "next_line"`, the style of every studio source in the
corpus) or at the end of the header (`"same_line"`), and long lines break at `max_width` (100):

```gsc
self thread spawn_wave( level.spawn_points[0], "zombie_wave_group_alpha", 1.5, ( 0, 0, 1 ),
	undefined, self.origin, 5000 );
if ( isdefined( self.target_entity ) &&
	self.target_entity.health > 0 &&
	!level.paused &&
	isdefined( level.x ) )
{
	value = ( isdefined( self.override_value )
		? self.override_value
		: level.default_value_for_testing_long );
}
```

Arguments fill lines (`wrap = "fill"`) or go one per line (`wrap = "one_per_line"`); `&&` and `||`
chains break one operand per line, other operator chains fill; arrays of plain values fill, other
literals break one element per line.

**`minimal`** keeps every line break of the source and only normalises horizontal whitespace
(around operators, after commas, inside parentheses) and the indentation of statement lines. It
was the only style of 0.1.0.

## Comments and regions

Comments keep their place: a comment that ended a line still ends it (aligned trailing comments
stay aligned), a comment on its own line stays on its own line. Directives (`#using`, `#define`...),
preprocessor lines and inactive `#if` branches are copied as written, and so is every function or
statement that holds a `#if` region.

To leave code alone:

```gsc
// gsc-fmt: off
level.table = [  1, 0, 0,
                 0, 1, 0,
                 0, 0, 1  ];
// gsc-fmt: on
```

`// gruff-fmt: off` and `// gruff-fmt: on` work the same.

The comments must stand between statements or declarations; the statements that start between
them are copied as written. An `off` without `on` runs to the end of its block.

## Options

In the `[format]` table of `gruff.toml` (see [Configuration](configuration.md)):

| Key | Values | Default |
|---|---|---|
| `style` | `full`, `minimal` | `full` |
| `max_width` | 20 to 1000 | 100 |
| `tab_width` | 1 to 16: the width of a tab when measuring a line | 4 |
| `indent`, `indent_width` | `tabs`, or `spaces` and 1 to 16 | tabs |
| `brace_style` | `next_line`, `same_line` | `next_line` |
| `paren_spacing` | `padded` (`f( a )`), `compact` (`f(a)`), `preserve` | padded; compact for CoD1, United Offensive, CoD2 |
| `bracket_spacing` | `padded` (`a[ i ]`), `compact` (`a[i]`), `preserve` | compact for iw1, iw2, t6, t7, iw6 to iw8, s1, s2, h1; padded elsewhere |
| `keyword_space` | `space` (`if (`), `none` (`if(`), `preserve` | none for t4 and t5; space elsewhere |
| `wrap` | `fill`, `one_per_line` | `fill` |
| `max_blank_lines` | 0 to 10 | 1 |

The defaults per game come from measurements on the studios' sources; the counts are in
`crates/gsc_fmt/README.md`.

## In editors

The language server formats documents (`textDocument/formatting`) and ranges
(`textDocument/rangeFormatting`: the statements that touch the selection) with the same options.
It refuses, with the reason, files with syntax errors.
