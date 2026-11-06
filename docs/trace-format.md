# Script VM trace format (gsc-trace, version 1)

A **gsc-trace** records what a GSC script VM did during one run: threads, waits, notifies,
endons, builtin calls, script errors and, optionally, every script call and every opcode, each
with the VM time and the source position. It is written by the emulated Black Ops III VM today
(`tools/re/vm/t7_run.py --trace`) and is meant to be written the same way by any later
producer: the T8/T9 emulators, a Rust tracer, a DAP debugger adapter, another game's VM.
`tools/re/vm/trace_view.py` reads it (text timeline or a self-contained HTML page).

The shared Python implementation is `tools/re/vm/trace.py` (writer, reader, address to source
map); this page is the contract.

## File

- UTF-8, one JSON object per line (JSONL), LF line ends.
- Line 1 is the `header` record, the last line is the `end` record, the events are in between
  **in the order the VM produced them**. That order is the point of the format: within one
  frame it is the order of execution, so "the waiter ran before the notifier continued" is
  read straight from the file.
- Every event has `seq` (0, 1, 2... in file order), `t` (VM time) and `ev` (the kind).
- A reader must ignore keys and event kinds it does not know. Producer-specific data goes in
  keys starting with `x_`.

### Versioning

`header.version` is an integer, the major version. Within a version, changes are additive only
(new optional keys, new event kinds). Removing or renaming a key, or changing what one means,
bumps the version; a reader refuses a version newer than it knows (`trace.read` does). The
format history is at the end of this page.

## Header

```json
{"ev":"header","format":"gsc-trace","version":1,"game":"t7",
 "producer":"gruff/tools/re/vm/t7_run.py (emulated BO3 VM, bo3_flat.exe)",
 "time_unit":"frame","frame_ms":50,
 "sources":[{"id":0,"path":"scripts/zz_harness/wait_0.gsc","file":"conformance/t7/wait_0.gsc",
             "lines":true,"crc":"0xc2e8f90d"}],
 "options":{"calls":false,"ops":0,"entry":"main","developer":false}}
```

| Key | Meaning |
|---|---|
| `format`, `version` | `"gsc-trace"`, `1` |
| `game` | the dialect id used elsewhere in Gruff (`t7`, `t8`, `iw5`...) |
| `producer` | free text: tool, VM, build |
| `time_unit`, `frame_ms` | what `t` counts: `"frame"` (a server frame, `frame_ms` long) or `"ms"` |
| `sources` | the loaded scripts: `id` (referenced by `loc.src`), `path` (the VM's script name), `file` (local source file, if known), `lines` (whether `loc.line` is available), any extra keys (`crc`...) |
| `options` | producer options (what was recorded) |

## Common keys

| Key | Meaning |
|---|---|
| `seq` | record number, file order |
| `t` | VM time when the event happened (for T7 `gScrVarPub.time`, one 50 ms frame per unit) |
| `ev` | event kind (table below) |
| `th` | the thread the event belongs to: `T0`, `T1`... in order of creation, never reused within a run (the VM's own id may be; it is in `thread_create.id`) |
| `loc` | where in the script: `{"src": 0, "off": 182, "func": "zz_test::main", "line": 13}` |

`loc`: `src` indexes `header.sources`; `off` is the byte offset of the instruction in the
compiled script; `func` is the qualified name of the function containing it; `line` (1-based)
is present when the producer has a line table. A position outside every known script is
`{"addr": "0x..."}`. A DAP adapter maps `loc` to a `Source` + `line`.

Values (arguments, durations, objects) are **display strings in GSC syntax**, so that every
producer can write them without sharing a type system:

| Value | Text |
|---|---|
| undefined | `undefined` |
| int, float | `5`, `0.05` |
| string | `"text"` (quoted) |
| hash | `#"name"`, or `#"#1a2b3c4d"` when the name is unknown |
| vector | `(1, 2, 3)` |
| object | `level`, `game`, `anim`, `world`, or `<kind>#<id>` (`struct#54`, `array#12`, `entity#3`) |
| function | `&ns::name` |
| other | `<type name>` |

Event names (`event` keys) are plain names (`"death"`), or `#xxxxxxxx` when only the hash is
known.

## Events

| `ev` | Keys | When |
|---|---|---|
| `frame` | | the VM time is about to advance a frame (the producer's frame loop) |
| `thread_create` | `th`, `id`, `parent`, `kind`, `entry`, `self`, `loc`, `entry_loc` | a thread starts. `kind`: `exec` (started by the engine/harness, no parent), `thread` (`thread f()`), `method_thread` (`obj thread f()`); `entry` the function; `self` the object for a method thread; `loc` the call site in the parent; `entry_loc` the first instruction. Written when the new thread runs its first instruction, so it follows the parent's call |
| `thread_end` | `th`, `reason`, `by`, `obj`, `event`, `loc` | a thread ends. `reason`: `return` (its entry function returned), `endon` (killed by an endon: `by` is the `seq` of the notify, `obj`/`event` its object and event), `error` (after a script error), `killed` (any other cause). `loc` is the last position the thread ran |
| `wait_start` | `th`, `kind`, `duration`, `obj`, `event`, `match`, `loc` | a thread blocks. `kind`: `wait` (`duration`), `waittill` (`obj`, `event`), `waittillmatch` (`obj`, `event`, `match`: the values to match), `waittillframeend`, `wait_other` (another timed wait opcode, `duration`) |
| `wait_end` | `th`, `kind`, `since`, `wait`, `by`, `loc` | the thread runs again after its wait (it was resumed). `since`: `t` of the wait start; `wait`: its `seq`; `by`: the `seq` of the notify that woke a waittill, when known; `loc` the resume point |
| `notify` | `th`, `obj`, `event`, `args`, `loc` | `obj notify(event, args...)` from script |
| `endon` | `th`, `obj`, `event`, `loc` | `obj endon(event)` registered for `th` |
| `builtin` | `th`, `name`, `method`, `self`, `args`, `loc` | an engine builtin is called (`method: true` and `self` for a method call) |
| `error` | `th`, `msg`, `terminal`, `loc` | the VM raises a script error (the engine's own message). Producers may add detail keys (T7: `at` and `opcode` for an invalid opcode) |
| `call` | `th`, `func`, `loc`, `from` | optional (`options.calls`): a script function is entered; `from` is the caller's position (absent for a thread's entry function) |
| `return` | `th`, `func`, `loc` | optional: a script function returns (a thread killed by an endon has no `return`) |
| `op` | `th`, `op`, `loc`, `recovery` | optional (`options.ops`, capped): an opcode is dispatched; `op` is the producer's opcode name; `recovery: true` for a re-dispatch by the VM's error recovery |
| `end` | `status`, `error`, `frames`, `counts`, `dropped` | last record: `status` `ok` or `error`, the run's error (load failure, engine error, harness limit), frames run, records per kind, records dropped by caps |

Producers that cannot see something leave it out rather than guess: a `wait_end` without `by`,
a `loc` without `line`, a `thread_end` with `reason: killed`.

A minimal producer writes `header`, `thread_create`, `thread_end`, `wait_start`, `wait_end`,
`notify` and `end`; everything else is optional. For a debugger the same records are the
natural stop points: `thread_create`/`thread_end` are DAP thread events, `call`/`return` build
the stack, `error` is an exception stop, and a breakpoint is a `loc` match on `op`.

## Example

`conformance/t7/endon_during_wait.gsc` in the emulated BO3 VM (frames 2-4 had no event):

```
{"seq":1,"t":0,"ev":"thread_create","th":"T1","id":11,"parent":"T0","kind":"thread","entry":"zz_test::sleeper","loc":{"src":0,"off":172,"func":"zz_test::main","line":9},"entry_loc":{"src":0,"off":256,"func":"zz_test::sleeper","line":16}}
{"seq":2,"t":0,"ev":"endon","th":"T1","obj":"level","event":"stop","loc":{"src":0,"off":266,"func":"zz_test::sleeper","line":18}}
{"seq":4,"t":0,"ev":"wait_start","th":"T1","kind":"wait","duration":"0.1","loc":{"src":0,"off":296,"func":"zz_test::sleeper","line":20}}
{"seq":9,"t":1,"ev":"notify","th":"T0","obj":"level","event":"stop","args":[],"loc":{"src":0,"off":206,"func":"zz_test::main","line":11}}
{"seq":10,"t":1,"ev":"thread_end","th":"T1","reason":"endon","by":9,"obj":"level","event":"stop","loc":{"src":0,"off":296,"func":"zz_test::sleeper","line":20}}
```

`trace_view.py` shows it as:

```
        T0                                T1
-- f0 --------------------------------------------------------------
        [0] + main() L7
        => T1                             [1] + sleeper() L16
        |                                 [2] # endon level stop L18
        |                                 [3] logprint("waiting") L19
        |                                 [4] ~ wait 0.1 L20
        [5] ~ wait 0.05 L10               :
-- f1 --------------------------------------------------------------
        [8] ^ woke +1f L11                :
        [9] ! level notify stop L11       :
        |                                 [10] x endon level stop by [9]
        [11] logprint("sent stop") L12
        [12] ~ wait 0.2 L13
   (f2-f4: nothing)
-- f5 --------------------------------------------------------------
        [17] ^ woke +4f L14
        [18] x return L14
```

## Producers

### Black Ops III, emulated (`tools/re/vm/t7_run.py --trace FILE [--calls] [--ops [N]]`)

The game's own VM runs offline under Unicorn (`tools/re/reports/vm_t7.md`); the tracer
(`tools/re/vm/t7_trace.py`) only watches it:

- opcodes at the dispatch calls of `VM_Execute` and of its error-recovery re-dispatch; the
  opcode name is the one acts gives for the handler the game's table selects;
- waits, waittills, endons, notifies, builtin calls and thread calls are decoded from the
  operands and the VM stack at their dispatch, before the handler runs;
- thread creation at `ScrVar_AllocThread`'s return in the thread-call handlers; thread end
  where the VM turns the thread variable into `removed_thread`; a wait ends at the thread's next
  opcode; errors where the VM throws;
- `line` comes from the line table the official linker writes beside each compiled script
  (`<path>.gsc.gdb`, matched to the `.gscc` by its source CRC); `func` from the `.gscc` exports.

Not recorded yet: notifies sent by engine code (not by script), entity threads (the harness has
no entities), the client VM.

## History

- **1** (2026-11-06): first version.
