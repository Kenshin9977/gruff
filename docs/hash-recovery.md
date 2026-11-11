# Hash recovery: UNKNOWNS.tsv and VOCAB.tsv

`gruff index --unknowns` writes two extra tables, for recovering the names of hashed identifiers
that no hash package and no clear name of the workspace resolves.

Only a game that hashes its script identifiers has anything to put in them: BO3 (`t7.canon`), BO4
and Cold War (`t89.scr` for identifiers, `t8.fnv64` for `#"..."` strings and script paths), MWIII
(`iw9.scr`, `iw9.xhash`, `iw.asset`) and BO6/BO7 (the `t10.*` set). For every other title the two
files come out empty, which is the right answer rather than an error. BO3 is a special case worth
knowing: its decompiled scripts keep their names, so a 2,459-file corpus holds 153 hashed tokens in
all and the tables are a formality.

## Why context, and not a bigger dictionary

A hashed identifier is recovered by guessing its name and hashing the guess. The hashing is free;
what costs is the false positives:

```
expected false hits = candidates * targets / 2^bits
```

The width is the whole game. BO4 and Cold War hash `function`, `var`, `namespace`, `class` and
`event` with `t89.scr`, **32 bits**, so a wide pass is noisy where a BO7 60-bit pass would be
silent: ~20 000 unnamed BO4 function hashes against a million candidates carry **4.8** expected
false hits. Scoping the pass to the ~50 targets of a single file, against that same million, carries
**0.012** — a factor of 400 for free, and the candidates get better at the same time because a
file's own vocabulary is a far better prior than a corpus-wide word list.

Measured on the BO4 corpus (2 633 files, 2026-11-11):

| pass | candidates | names | expected false hits |
|---|---:|---:|---:|
| every clear name of the other titles, untargeted | 1 479 226 | 1 471 | ~16 |
| only the names of the same-basename file elsewhere | 8 635 | 3 420 | ~0 |

Same hash, same names, incomparable certainty. That difference is what these two tables serve.

## UNKNOWNS.tsv

One row per `(role, value)` the workspace spells as a hash and nothing names. The rule is the one
`gruff hash --coverage` counts with, so its `distinct - distinct_named` and these rows are the same
set — plus the bare 16-hex-digit strings a decompiler writes inside a plain string literal.

| column | what it holds |
|---|---|
| `hash` | the decompiler spelling (`function_5436a07f`): greppable in the scripts as written |
| `value` | the masked hash, hex |
| `role` | `function`, `var`, `namespace`, `class`, `string` |
| `family`, `bits` | the family to hash a candidate with, and its width (the collision budget) |
| `sites`, `files` | occurrences, and how many files hold them |
| `scope` | the file it belongs to: where it is defined, else where it is used most |
| `def_at` | `path:line` of its definition, for a function or class |
| `namespace` | the namespace of that definition, recovered when a name is known |
| `params`, `flags` | the signature and `autoexec`/`private`/… of that definition |
| `object` | a field: `level:12,self:3`; an event: its receivers |
| `ops` | a field: `read:9,write:3`; an event: `notify:4,waittill:2` |
| `callers` | named callers (a function), else the named functions that touch it |
| `callees` | what it calls, named only |

A neighbour nobody can read is noise, not context, so the two list columns hold only names that are
clear or recovered. They are capped at 8 with `+n` for the rest.

## VOCAB.tsv

`file kind source count tokens`: the names a file **uses** -- what it defines, calls, reads or
notifies. `kind` is `function`, `namespace`, `field`, `event`, `string` or `path`; `source` is
`clear` for what the file spells out and `recovered` for what a hash package, or the workspace's
own clear names, give back for a hashed spelling.

Both sources matter, and on a decompiled corpus the second is the larger half: BO4 writes 17,261
distinct names in clear while the packages restitute 39,391 more. Dropping those left every block
generating from under a third of the words its own file uses. This is the vocabulary a generator
should draw on for the unknowns whose `scope` is that file, and the reason UNKNOWNS.tsv carries a
`scope` column instead of repeating these words on each of its rows.

## Using them

1. Group UNKNOWNS.tsv by `scope`, and take each group's targets with its VOCAB.tsv row.
2. Generate candidates from that vocabulary, hash with the row's `family`, keep the hits.
3. Do not expect coherence to filter what the budget did not: a false hit drawn from a file's own
   words reads as sensibly as a true one. The one check that is not an opinion is a second family
   — the same name hitting an unknown target of another width — which on BO4 corroborated 1.1% of
   the 32-bit hits, enough to confirm those and not enough to licence a larger pass.
4. Size the pass before launching it with the formula above, and say so when a batch carries more
   than about one expected false hit.
