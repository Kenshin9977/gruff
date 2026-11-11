# Black Ops 4 (T8) script VM, run offline: method, stubs, results, dialect against T7

Phase 5, step 5.6. 2026-11-06, 15:18-16:10 for T8, until 17:08 with T9 (one agent). Dump:
`BlackOps4_dump.exe` (PE timestamp 1780932985). Nothing was launched
or attached; BO4 cannot be run (Ricochet), so the second measure is the static reading of the
handler or function behind each surprising answer. Addresses are RVAs of this dump. Facts:
`gruff-data/semantics/t8.toml`; scripts: `conformance/t8/`.

## 1. What runs

| Step | What the game's own code does | Entry (RVA) |
|---|---|---|
| builtin tables | static in `.rdata`; the bsearch index arrays (zero in the dump) are filled with the hash order of their table, places from phase 2's `out/t8_extract.json` | 8 arrays |
| init | `SL_Init` (memory tree `MT_Init` 0x2744610, string hash table), `ScrVar_Init(0)` (five pools into scrVarGlob 0x8305230), `Scr_InitVM(0)` (stack, frames, jmp_buf, execution queues), `Scr_StartVM(0, 0, 0)` (global objects world, mission, level, game, anim, classes; time, frame, the running flag the frame functions test) | `0x2757220`, `0x275dff0`, `0x27737f0`, `0x2773990` |
| load | `Scr_BeginLoadScripts(0)`, `Scr_GscLink(0, &XHash, 0)`: the asset lookup (hooked) returns our ScriptParseTree, the game's loader (0x2747210) loads includes through the same lookup, the linker (0x2747980: strings, globals, then imports 0x2744d30) resolves against the loaded exports and the builtin tables; `Scr_EndLoadScripts(0, 1)` | `0x2743d60`, `0x2746eb0`, `0x2743e30` |
| run | the entry's code address read off the loaded object's export table; `Scr_ExecThread(0, codepos, 0, 0, level)`; per frame as `G_RunFrame` (0x2d072c0) does: `VM_SetTime(0, levelTime += 50)`, `Scr_RunCurrentThreads(0)`, then (after the entity loop) `Scr_RunCurrentThreads(0)` again | `0x276e9e0`, `0x2773570`, `0x2775700` |
| trace | `iprintln`'s handler (logprint does not exist in T8) is a recorder: VM frame, thread, arguments off the VM stack (`scrVmPub.top` 0x83134e8, outparamcount +0x8818). Script errors at `ScrVm_Error` (0x276e530: code, inst, terminal) | |

Threads: T0 is the entry thread, T1, T2... in creation order (the return of `AllocThread`
0x2758ab0 at its 14 direct call sites). A script call runs in a `child_thread` variable whose
parent id is at `ScrVar_t+0x14` (measured); the thread is the root of that chain. `f<n>` is
`scrVmPub.frame` (+0x880c from top): VM_SetTime adds one per server frame; `wait` counts
milliseconds against the level time (`scrVmPub.time`, +0x8808).

```sh
py -3.11 -I tools/re/vm/t89_run.py --game t8 conformance/t8/notify_reentrant.gsc [--script-errors continue]
py -3.11 -I tools/re/vm/t89_suite.py --game t8 [--script-errors continue]
py -3.11 -I tools/re/vm/t89_facts.py --game t8                 # gruff-data/semantics/t8.toml
```

A test takes about 8 s (the dump is mapped per process; no static-init cache is needed); the
suite, 57 tests with 2 processes, about 4 minutes.

**Compiler.** BO4 has no official mod tools linker: the tests are compiled by acts
(`acts gscc -g bo4`, VM 0x36, located by `GSC_ACTS` or `acts` in `paths.local.json`), output
cached in `tools/re/out/vm_gscc_t8/` (gitignored). acts encodes a bare call or `&f` as an import of
`<current namespace>::f` with the get-call flag 0x20, and by default wraps a dev-only call in a
devblock itself (the dev test compiles with `// acts-flags: --no-devcall-inline`). What the game's
linker does with that encoding is measured; what Treyarch's own compiler emits is not (the retail
bytecode, `bytecode-t8-imports`, names builtins with the empty namespace).

**Syntax ported from T7.** `logprint` -> `iprintln`; events as hashes (`#"go"`); `waittill`
returns a struct (no bound names), `waittillmatch({#field:value}, e)`, `waittillframeend()`.
Nine new tests cover T8-only forms (`waittill` result struct, several events, `endon(a, b)`,
`waitframe`, `waittilltimeout`, string vs hash events, payload object, childthread endons).

## 2. Stubs (each run's JSON lists every one, with its hit count)

| Stub | Why | Replaces |
|---|---|---|
| imports (all) | logged, return 0; heap/TLS/FLS handled (`vmcore.py`, shared with no game) | kernel32 etc. |
| `PMem_Alloc` (0x3c3fea0) | bump heap, never freed | the engine's pools |
| `DB_FindXAssetHeader` (0x2eb58b0) | ScriptParseTree `{XHash, buffer, len}` of the test scripts, 0 for anything else (type 0x30) | the asset database |
| `SL_Obfuscate` (0x3c52500) | plain copy: script strings are otherwise stored obfuscated with a `rand()` key and decoded in place on each read (0x3c4c570); the table's hash is computed before, on the clear text | string obfuscation |
| `rand` (0x4856d58) | MS LCG from seed 1 (deterministic); not reached once the obfuscation is a copy | CRT |
| CRT `vsnprintf_s` (0x4855608), `vsprintf` (0x4855754) | Python printf (locale and per-thread data are not set up in a dump; hash-to-string casts and error texts go through them) | CRT |
| CRT `longjmp` (0x4844058) | direct `_JUMP_BUFFER` restore (the CRT one checks the stack against the TEB and unwinds through RtlUnwindEx) | CRT |
| `Com_Printf` (0x288e770) | a no-op in this build; the loader and linker still call it: recorded (`vm_messages`) | console |
| `Sys_Error` (0x3d34e70) | fatal: the run stops with its code and details (link errors end here) | |
| `Com_Error` (0x2889710) | code 0x20 (script error): recorded, then the run stops (`stop`, default) or Com_Error returns at once (`--script-errors continue`); any other code stops the run | error handling |
| `iprintln` handler | the trace recorder | |

## 3. Results (both modes; 56 PASS, 1 UNSUPPORTED)

`dialect` gives the T7 answer when it differs. 39 behaviours answer as T7 (array sharing and
deep copy on plain parameters, `&name` sharing, bare-call and `&f` resolution order, threads run
to their first wait, deferred notify wakes in reverse registration order, endon kills before a
waittill wakes, lost early notifies, `wait 0` / `waittillframeend` order, no loop guard,
struct references, case-sensitive string equality...). The 18 differences:

| Test | T8 | T7 |
|---|---|---|
| `wait_same_duration_order` | waits ending in the same frame resume **first in, first out** | last in, first out |
| `thread_outlives_parent` | the child (waiting first) runs before the parent in their common frame | parent first |
| `childthread_dies_with_parent` | `childthread` exists but the child does **not** die with the parent | not a keyword |
| `childthread_inherits_endons` | a childthread copies the endons active in its starter: they kill it too | — |
| `hash_equality` | `#"GoAway" == #"goaway"` and `"goaway" == #"goaway"` are true, no error | script error, false |
| `notify_string_vs_hash_event` | a string event is the same event as its hash | strings only |
| `waittill_returns_struct` | waittill returns a struct: payload fields + `_notify` (the event hash) | binds names |
| `waittill_more_args_sent` | plain values after the event are **dropped**: the waiter gets a struct with only `_notify` | extra values dropped, first ones bound |
| `notify_payload_object_is_result` | an object payload *is* the waiter's result, `_notify` written into it | — |
| `waittillmatch` | matches on payload fields `({#name:"x"}, e)` | first value vs a string |
| `waittill_any_of_events`, `endon_multiple_events` | several events per waittill / endon | one |
| `waitframe_one` | `waitframe(n)` = n frames | — |
| `waittilltimeout` | a 100 ms timeout fires after **3** frames (150 ms) | — |
| `undefined_arithmetic` | the error reporter always ends in Com_Error(0x20): in the dump's state it aborts the run; when Com_Error returns, the thread goes on with undefined (as T7) | non-terminal, goes on |
| `bare_call_using_file_other_namespace`, `thread_call_builtin_only` | the unresolved external is a fatal `Sys_Error 0x6394f836` | Com_Error |
| `dev_builtin_outside_devblock` | same message, through `Sys_Error` | Com_Error |

UNSUPPORTED: `array_ref_param_non_array` (error #f5ef3c53 at the T7 position; continue mode then
repeats it 61 times and ends with the terminal #38685e3b, the pattern T7's recovery shows).

Error messages are 32-bit codes (`#12e690eb`...): their texts are in a string table asset,
decoded through the asset database (0x289aa20), so offline the game prints "Dangerously Missing
Some Bagels". The facts keep the codes.

## 4. Static readings behind the surprising answers (the second measure)

- **Notify defers, endons first**: the Notify handler (0x2752160) calls `VM_Notify` (0x2774090),
  which first walks the endon table (kills the endon'd threads, and runs endon *callbacks* at once
  through AllocThread + VM_ExecuteInternal), then for each waiter of the event writes its result
  struct (0x27747c0) and queues it in the frame queue of the current frame (0x2774ca0). No waiter
  runs inside the notify.
- **Scheduler**: sorted execution queues in scrVmPub (+0x18 time, +0x2020 frame, +0x4028
  timeouts) replace T7's per-time variable lists; VM_SetTime (0x2773570) and
  Scr_RunCurrentThreads (0x2775700) take due entries from the head (FIFO among equal times).
  Timeouts fire only when the time is past the deadline (`jbe` at 0x2773760).
- **childthread**: ScriptThreadCallEndOn (0x274ac00) = ScriptThreadCall (0x274c910) + 0x2749ac0,
  which copies the caller's endons (endon table 0x83114d0) onto the new thread (0x2774940).
- **Payload**: the Notify handler keeps a pointer payload as is; any other value after the event
  is replaced by a new empty struct (AllocObject 0x2758a40 at 0x2752252); a string event becomes
  its hash (0x2757b60).
- **Arrays**: SafeCreateLocalVariables (0x274a3d0): plain parameter + array argument ->
  AllocArray (0x2758530) + CopyArray (0x2759520), as T7.
- **Equality**: the evaluator (0x275bbe0) coerces the pair (0x2760230) before comparing.
- **Script errors**: ScrVm_Error (0x276e530) -> reporter (0x27758b0) -> Com_Error(0x20) always;
  Com_Error (0x2889710) returns for 0x20 only when a session byte (0x8a57023, not identified) is
  set, else it aborts to the main loop. Which one a retail BO4 session does is not known: both are
  recorded (`trace` = stop, `alt_trace` = continue).

## 5. Limits

- Only the server VM (inst 0); no entities (level and structs only); no engine-side notifies.
- The compile side is acts', not Treyarch's (no official BO4 compiler exists).
- Error texts are codes. The infinite-loop answer depends on the harness budget.
- The `stop`/`continue` choice for script errors is a harness parameter, not a measured fact.

## 6. Profile contradictions (gruff-data `profiles/t8.toml`, owned by another agent; not edited)

- `waittill_return_value = false` (assumed): measured **true** (a struct).
- `childthread = true`: true, but its semantics is "inherits the starter's endons", not "dies
  with the parent".
- `bare_call_resolution = namespace-first` (documented): now **measured in the engine** for the
  acts encoding (current namespace, then builtin for a get-call; threads never fall back).
- `array_argument_passing = copy-unless-ref-param` (assumed): **measured** (deep copy; `&name`
  shares).
