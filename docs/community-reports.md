# Community corpus reports

Some games have no builtin catalogue, no measured profile and no event list in gruff-data because
nobody on the project can measure them: CoD: United Offensive (`iw1-uo`), Vanguard (`s4`),
Modern Warfare II (`iw9`), and every game whose executable resists extraction. Players who own
those games can still help. `gruff report` measures the scripts of a local install and writes a
**report**: counts and identifiers, no code. Merged reports become draft gruff-data entries, each
one marked with where it came from.

There is no submission channel yet. Nothing is uploaded, and nothing in the tools sends
anything anywhere. You make a report on your own machine, read it, and keep it until a channel
opens (see [Submitting, later](#submitting-later)).

## Making a report

You need the game's scripts as files: a source folder, or the output of a decompiler
(gsc-tool, acts, GrayWolf). Then run:

```sh
gruff report path/to/scripts --game s4 -o s4-report.json
```

| Option | Effect |
|---|---|
| `--game <id>` | Profile id or alias (`iw1-uo`, `s4`, `iw9`, `vg`...). Required. |
| `-o <file>` | Write the JSON there and print a summary. Without it, the JSON goes to standard output and the summary to standard error. |
| `--scripts hash\|names\|drop` | How scripts are named: `hash` (default) lists one hash per script, `names` lists the relative paths, `drop` lists nothing. |
| `--one-word-events` | Leave out event names of several words (`single anim`). |
| `--decompiled auto\|on\|off` | Whether the files are decompiler output (default: detected per file). |
| `--mode retail\|community` | The compiler semantics the profile describes (see `gruff parse --mode`). |
| `--data-dir <dir>` | The gruff-data folder, when it is not found on its own. |
| `-j <n>` | Worker threads. The report is the same for any value. |

Several folders can be given (for example the multiplayer and campaign scripts). The report is
deterministic: the same scripts, profile and options always give the same file, byte for byte,
whatever the machine or the thread count. It holds no clock time.

The report does not read gruff-data's builtin catalogues, on purpose. It lists every call that no
script defines, so it does not depend on what the project already knows.

## What a report holds

This list is the whole contract. The report repeats a short form of it in its `privacy` field.

| Section | Contents |
|---|---|
| `toolchain`, `profile` | The Gruff version, the profile id, an xxh3 of the profile file (which profile version measured it) and the mode. |
| `corpus` | File counts (gsc, csc, headers, decompiled, with syntax errors), total bytes and lines, function count, `#include`/`#using` count and how many found no file, and the **corpus fingerprint**: one hash of the sorted script hashes. Two equal installs have equal fingerprints. |
| `scripts` | Per script, its id and VM. With `hash`: an xxh3-64 of the lower-case relative path. With `names`: the relative path under the folder you gave (`maps/mp/_utility.gsc`). With `drop`: empty. When several folders are given they are labelled `root0`, `root1`..., never by their names. |
| `features` | Per dialect construct (`foreach`, `` `#include` ``, `a `&"..."` localized string`...): uses and files. The counts come from a parse with every profile feature off, so each construct the parser can gate is counted once per use. |
| `calls` | Call-site totals: to scripts, external, unresolved qualified, dynamic. |
| `builtins` | Per name that the scripts call and never define, with VM (gsc, csc) and kind (function, method): call sites, files, threaded sites, a histogram of argument counts, receiver kinds of method calls (`self`, `level`, `player`, `other`...), and per argument index the **type** of each literal argument (`int`, `float`, `string`, `istring`, `hash`, `vector`, `anim`, `function`, `undefined`...; `expr` for anything computed). Only names made of letters, digits and `_` are kept. |
| `events` | Per literal event name of a `notify`, `waittill`, `waittillmatch`, `endon` or event wrapper (`waittill_any`...): counts per class and operation, receiver kinds, files, and the payload count of `notify`. A name built by concatenation becomes a pattern (`trigger_*`). Computed names are counted, not named. Only names of at most 48 characters made of letters, digits and `_ + - . : *` are kept; spaces are allowed between up to 6 words (CoD2 writes `single anim`) unless `--one-word-events` is given. Others are counted in `skipped`. |
| `fields` | Per field name and object kind (`level`, `self`, `game`, `anim`, `other`): reads, writes and files. Identifier names only. |

### What a report never holds

- No source text: no statement, no expression, no comment, no line or column number.
- No string literal, with one exception: event names, under the rules above. The value of an
  argument is never written, only its type (`string`, not `"vox_intro"`).
- No path except the relative script names, and those only with `--scripts names`. No
  absolute path, no folder name, no user name, no machine name, no time stamp.
- Nothing from the executable or any other game file: the report reads scripts only.

### What hashing does and does not do

Script hashes are unsalted. They let the merge tool see that two reports come from the same
scripts without listing names. They do not make the names secret: anyone can hash a guessed path
(`maps/mp/_utility.gsc`) and compare. For retail scripts that does not matter, since the names
are the same for every player. For your own mod, use `--scripts drop`. The fingerprint still
lets two equal reports be recognised, and nothing else about the files is shown.

Builtin, field and event names are identifiers the engine and the game's scripts share. A mod's
own function names can also appear when the mod calls a function defined in a file you did not
include. Report a game's own scripts, not a mod in progress.

### Reading your report before you keep it

The JSON is plain text with one entry per line. Check the summary first, then look at the
`events` names and `builtins` names. Anything you would not publish can be removed by hand. The
merge tool reads what remains; it does not check the `report_id` hash.

## Merging reports into gruff-data

`tools/re/merge_reports.py` (Python 3.11, no dependencies) reads reports and writes drafts:

```sh
py -3.11 -I tools/re/merge_reports.py s4-a.json s4-b.json            # gruff-data/community/s4/
py -3.11 -I tools/re/merge_reports.py t6.json --standalone --compare --out local/drafts/t6
```

| Output | Contents |
|---|---|
| `community/<game>/builtins.toml` | A draft catalogue in the builtins schema's shape, with `draft = true` and `[coverage] complete = false`. Every entry has `confidence = "corpus"`, `name_source = "corpus-scripts"`, `min_args`/`max_args` from the observed counts, `max_checked = false` (scripts never passing more say nothing about the engine's bound), `observed_args`, `corpus_arg_types` (the literal types), sites, files, `reports` (report ids) and `independent_reports`. Below `--min-sites` (3) sites or `--min-files` (2) files the entry gets `arity_unknown = true`. Keyword calls, and names the profile's `opcode_builtins` lists, go to `[[intrinsic]]`. |
| `community/<game>/events.toml` | Per event name: notify / waittill / endon counts, files, operations, receiver kinds, notify payload counts, provenance. `script_notified = false` marks waited events that no reported script notifies: the candidates for engine events. |
| `community/<game>/profile.toml` | Per profile feature: the proposed value, the profile's current value and confidence, `agrees`, the construct counts and provenance. A use proposes the feature (or the enum value it shows); zero uses propose nothing, because absence of use proves nothing. The profile itself is never written. |

Rules the merge keeps:

- **Measured entries win.** An entry that `gruff-data/builtins/<game>.toml` already holds (same
  name, VM and kind, or an intrinsic of that name) is left out of the draft. The draft never
  overwrites or shadows it. `--standalone` ignores the measured catalogue, for validation only.
- **Independent reports.** Reports with the same `report_id` count once. Reports with the same
  corpus fingerprint are the same data: the first one is used and the others confirm it.
  Across fingerprints each count is the maximum, not the sum, because installs overlap (a
  multiplayer-only export and a full export share most scripts). `independent_reports` is the
  number of distinct fingerprints that hold the entry.
- **Provenance per entry.** Every builtin, intrinsic, event and feature proposal lists the
  report ids it comes from. Each file lists the reports it was built from (`[[report]]`: id,
  fingerprint, toolchain version, profile hash, mode, file counts).
- **Regenerated, not edited.** A run rewrites the three files from the reports given. To
  change a draft, change the set of reports.
- Profile proposals know two traps of the probe count. `goto_labels` is decompiler output, so
  a use proves nothing. In an `#include` game, a one-word qualifier may be a one-component path,
  so `namespace_qualified_calls` is not proposed there.

A draft is a lead, not a measure. To promote an entry to `builtins/<game>.toml`, a reviewer
copies it, keeps `confidence = "corpus"` and the provenance, and states the evidence.

## How good is a draft? (validation on owned games)

The whole loop was run on two games that also have a catalogue extracted from the executable
(Black Ops II `t6`, decompiled with acts; Modern Warfare 3 `iw5`, decompiled with gsc-tool). A
report was made from the game's own corpus, a draft was built from that report alone
(`--standalone`), and the draft was compared with the measured catalogue. Full tables:
`tools/re/reports/community_t6.md` and `community_iw5.md`.

| | t6 | iw5 |
|---|---:|---:|
| Draft builtin keys (name, vm, kind) | 1,838 | 1,043 |
| Precision (keys in the measured catalogue) | 99.5% | 99.7% |
| Precision, entries seen in at least 2 files | 100.0% | 99.8% |
| Recall, every measured entry | 73.0% | 84.3% |
| Recall, measured entries the corpus calls | 99.9% | 100.0% |
| Entries with an arity (≥ 3 sites in ≥ 2 files) | 1,211 | 750 |
| Arity exact (vs extraction) | 85.1% | 78.2% |
| Minimum equal (vs extraction) | 91.6% | 87.0% |
| Observed counts inside the measured range (vs extraction) | 97.5% | 94.6% |
| Profile proposals that agree with the profile | 19 / 19 | 16 / 16 |

How to read it:

- **Names are reliable.** Almost every name scripts call without defining it is a builtin. The
  few others are script functions of files the corpus lacks.
- **Recall is bounded by use.** A builtin no script calls cannot be found from scripts: a
  quarter of T6's catalogue and a sixth of IW5's is never called by the shipped scripts. Every
  builtin the scripts do call is found.
- **Arities are lower and upper bounds, not values.** The observed minimum is too high when
  scripts never omit an optional argument (8-11% of entries). The observed maximum is too low
  when they never pass one (3-4%). The draft therefore sets `max_checked = false`. The
  remaining 2.5-5% are calls outside the extracted range: script bugs the engine tolerates, or
  extraction limits (`tools/re/calibrate.py` already corrects these in the measured catalogues;
  "vs extraction" compares with the values before that correction).
- **One report already gives most of it.** These numbers come from a single complete corpus.
  Partial installs (one mode, one DLC) lower recall, not precision. More independent reports
  raise recall and the number of entries with an arity.

Drafts built this way from this machine's corpus also exist for Call of Duty 2 (`iw2`), WWII
(`s2`) and Call of Duty Online (`codol`), whose executables were not extracted:
`gruff-data/community/<game>/`.

## Submitting, later

There is no channel yet: no upload, no form, no address to send reports to. When one opens, a
report will be attached to an issue that follows `.github/ISSUE_TEMPLATE/corpus_report.md`: the
game, the version or store build, how the scripts were obtained (source, which decompiler and
version), the `gruff report` command line, and the summary. The JSON is the only file to attach.
Never attach scripts, decompiler output, fastfiles or executables.
