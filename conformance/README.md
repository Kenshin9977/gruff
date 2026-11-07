# Conformance suites

One tiny script per behaviour, run in the game's own script VM (emulated offline from the
executable, `tools/re/vm/`), never in a running game. Only our own `.gsc` sources live here;
their compiled output stays in `tools/re/out/` (gitignored).

    py -3.11 -I tools/re/vm/t7_suite.py [name ...] [--developer]

## Header lines

| Line | Meaning |
|---|---|
| `// question: ...` | what the test asks |
| `// fact: ...` | the answer the golden trace shows, in one sentence (copied into `gruff-data/semantics/<game>.toml`) |
| `// claim: <source>: ...` | what a document or another tool says, for comparison |
| `// expect: f<time> T<n> <text>` | the golden trace, in order |
| `// expect error: <substring>` | the run must end with this error (load failure, Com_Error, harness limit) |
| `// expect compile error: <substring>` | the official compiler must reject the file with this message |
| `// unsupported: <reason>` | run and recorded, not judged: the harness cannot vouch for the result yet |
| `// with: other.gsc` | load another file of the suite with it (`#using scripts\zz_harness\other;`) |
| `// helper: yes` | a file loaded by tests, not a test |
| `// entry: name` | entry function (default `main`, started with `Scr_ExecThread`, self = level) |

Trace lines: `f<time>` is the VM's own time (`gScrVarPub.time`, one 50 ms server frame per unit);
`T0` is the entry thread, `T1`, `T2`... the threads in the order they are created; the text is
the arguments of `logprint()` joined. Script errors the VM throws appear in the trace as
`script error[ (terminal)]: <message>`.

## Results

`PASS` (the trace is the golden), `FAIL` (it differs: shown), `UNSUPPORTED|<reason>` (the harness
cannot run or vouch for it, or the behaviour cannot exist in the game). Nothing is skipped silently.
The goldens are per game: a different answer on another game is a dialect fact, not a failure of
that game.

**Compiler refusals.** One rule for every suite: a behaviour whose script the game's compiler (or
the official linker) refuses because the form the behaviour is about does not exist in the game
(a keyword such as `childthread`, a syntax such as `&name` parameters or `waittill("a", "b")`, a
literal such as `#"hash"`, a builtin the game lacks such as `waitframe`) is **UNSUPPORTED**: the
script keeps `// expect compile error: <message>` (recorded as `compile_error` in the facts) and
`// unsupported: <reason>`, with no `// fact:` and no `// expect:` lines. A refusal is a PASS only
when it is itself the answer to a name-resolution question about a well-formed script
(`thread_call_builtin_only`, `func_ref_builtin_only`: does a threaded call or a reference fall back
to the builtin? no, "unknown function"). A header with both `expect compile error` and `expect`
lines fails the suite (a refused script runs nothing).

The golden traces come from the emulated VM, which runs this build's own code; the second
measure is the user's own run of the same scripts in the offline mod tools client (agents never
launch a game), and the static reading cited in `tools/re/reports/vm_t7.md`.

## t8 and t9

    py -3.11 -I tools/re/vm/t89_suite.py --game t8|t9 [name ...] [--script-errors continue]

The T7 scripts in T8 syntax (`iprintln` for `logprint`, hash events, `waittill` returning a
struct...) plus T8-only behaviours; t9 runs the same scripts. They are compiled by acts (no
official linker exists), so `expect compile error` would be acts' message. Two more header lines:

| Line | Meaning |
|---|---|
| `// vs t7: same` or `// vs t7: <the other answer>` (t8), `// vs t8: ...` (t9) | the dialect fact: whether the same test answers the same in the other game (`dialect` in the semantics file) |
| `// acts-flags: ...` | extra `acts gscc` options for the test (`--no-devcall-inline`) |

`f<n>` is the VM's frame counter (`scrVmPub.frame`; the harness adds 50 ms of level time per
frame), and the second measure is the static reading in `tools/re/reports/vm_t8.md` and
`vm_t9.md` (neither game can be run). Script errors show their 32-bit code (`#12e690eb`).

## t4 and iw3

    py -3.11 -I tools/re/vm/old_suite.py --game t4|iw3 [name ...] [--developer]

The T7 scripts and the T8-only ones in the World at War / CoD4 dialect (no namespaces, no
`function` keyword, `#include zz\other;`, `::f`; t4 traces `iprintln`, iw3 `logprint`). The
game's own compiler compiles each script at load, so `expect compile error` is the game's message.
A behaviour whose form cannot exist in the dialect (`&name`, `#"hash"`, `waitframe`...) keeps its
script, is marked `// unsupported:`, still runs, and records the compiler's refusal; the suite
says when such a run differs from its own expectation. `// vs t7:` (t4) and `// vs t4:` (iw3)
give the dialect fact. Reports: `tools/re/reports/vm_t4.md`, `vm_iw3.md`.

## iw5, iw6, s1, h1 and iw7

    py -3.11 -I tools/re/vm/iw_suite.py --game iw5|iw6|s1|h1|iw7 [name ...] [--jobs 2]

The IW3 scripts in the dialect of the x64 IW games (`#include zz\other;`, `logprint`). None of
these games ships a compiler (its ScriptFile assets are bytecode compiled offline): the scripts
are compiled by gsc-tool and linked by the game's own loader, so `expect compile error` is
gsc-tool's message, and a bare call's resolution (decided at compile time: the bytecode calls a
builtin by id and a script function by code position) is not measured; the tests about it are
`unsupported` (gsc-tool refuses a function named like a builtin). These builds keep no runtime
error text: a script error is `script error: <opcode name> (opcode 0x..)`, the name from
gsc-tool's opcode table of the game. `f<n>` counts scrVarPub.time from the entry thread's start.
`// vs iw3:` (iw5), `// vs iw5:` (iw6), `// vs iw6:` (s1), `// vs s1:` (h1, iw7) give the
dialect fact. Report: `tools/re/reports/vm_iw.md`.

## t6 and t5

    py -3.11 -I tools/re/vm/t56_suite.py --game t6|t5 [name ...] [--developer]

The same scripts in the Black Ops II / Black Ops dialect (no namespaces, `#include
maps\mp\zz_harness\other;` in t6, `zz_harness\other` in t5, `::f`, `logprint`). T6 is compiled by
gsc-tool (no official BO2 compiler exists), so a T6 `expect compile error` is gsc-tool's message;
T5 is compiled at load by the game's own compiler (`Server script compile error`). Refused forms
follow the rule above. `// vs t7:` (t6) and `// vs t6:` (t5) give the dialect fact (a note that
starts with `same` counts as the same answer). Reports: `tools/re/reports/vm_t6.md`, `vm_t5.md`.
