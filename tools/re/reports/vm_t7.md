# Black Ops III (T7) script VM, run offline: method, stubs and the first conformance suite

Phase 5, steps 5.1-5.4. 2026-11-06, 12:51-13:46 (one agent); step 5.5 (trace mode, section 8)
15:18-15:55. Dump: `bo3_flat.exe`
(PE timestamp 1765634846). Nothing was launched or attached: the
game's code runs under Unicorn over the flat dump (`tools/re/vm/`). Addresses are RVAs of this
dump. Facts: `gruff-data/semantics/t7.toml`; scripts: `conformance/t7/`.

## 1. What runs

| Step | What the game's own code does | Entry (RVA) |
|---|---|---|
| static init | the builtin tables are built by the FNV static initialisers (phase 2); kept: the tables only (places from `out/t7_extract.json`), every other byte they wrote is restored | 88 initialisers |
| 5.1 init | `MT_Init` (memory tree that holds SL strings), `ScrVar_Init(0)` (variable pool 130,000 x 0x40 + name hash), `Scr_Init(0)` (loader state, class map, VM stack and frames, `bInited`), `Scr_InitObjects(0)` (timeArray, pauseArray, world, classes, level, anim, game) | `0x12c8f00`, `0x12df230`, `0x12ec2e0`, `0x12ec340` |
| 5.2 load | `Scr_BeginLoadScripts`, `Scr_LoadScript(name)`: asset lookup (hooked), the game's linker resolves imports against the loaded exports and the builtin tables, reports link errors and calls Com_Error; `Scr_GetFunctionHandle`; `Scr_EndLoadScripts` | `0x12c7e10`, `0x12c8410`, `0x12cc110`, `0x12c7f40` |
| 5.3 run | `Scr_ExecThread(inst, handle, 0, 0, self=level)`, then per frame as `G_RunFrame` (`0x19e1c30`) does: `Scr_RunCurrentThreads`, then `Scr_RunFrame` (runner again, `gScrVarPub.time += 1`) | `0x12ea790`, `0x12ecc30`, `0x12ec2a0` |
| trace | `logprint`'s handler (`0x1aa3630`) is replaced by a recorder: VM time, thread, arguments read off the VM stack. Script errors are read where the VM throws (`0x12ea4e0`, after `Scr_Error`). Notifies at `VM_Notify` (`0x12d36c0`, `--notify`) | |

A run boots in about 0.3 s (the static-init result is cached in `out/`, gitignored, 48 s the
first time) and a test takes 1-4 s; the whole suite, 49 tests, about 3 minutes with 3 processes.

```sh
py -3.11 -I tools/re/vm/t7_run.py conformance/t7/notify_reentrant.gsc --notify
py -3.11 -I tools/re/vm/t7_suite.py [--developer]       # PASS/FAIL/UNSUPPORTED per test
py -3.11 -I tools/re/vm/t7_facts.py                     # gruff-data/semantics/t7.toml
```

The test scripts are compiled by the official mod tools linker (`tools/linker/gsc_link.py
--gscc-out`, new option). Their `.gscc` stay in `tools/re/out/vm_gscc/` (gitignored); only the
`.gsc` sources are committed.

**Trace conventions.** `f<n>` is `gScrVarPub.time` (one 50 ms server frame per unit). `T0` is the
entry thread, `T1`, `T2`... threads in creation order (the return of `ScrVar_AllocThread`,
`0x12d9960`, at its call sites in the thread-call handlers and `Scr_Exec*Thread`). A thread is the
root of the `localId` chain: each script call allocates a `child_thread` variable whose parent id
is at +0x38 (measured; the first guess, +0x34, labelled callees as new threads).

## 2. Stubs (every one is logged per run in the JSON output, `stubs`)

| Stub | Why | Replaces |
|---|---|---|
| imports (all) | the dump's import slots point at our stub page: each call is logged by name and returns 0, except the handled ones | kernel32 etc. |
| `HeapAlloc`/`HeapReAlloc`/`HeapFree`/`HeapSize`/`GetProcessHeap` | CRT allocations land in a bump heap | Windows heap |
| `Fls*`/`Tls*`, `Encode/DecodePointer` | CRT per-thread data | |
| TLS block | `ThreadLocalStoragePointer[_tls_index]` gets a copy of the `.tls` template (MT_Init and the profiler read it) | the loader's TLS setup |
| `PMem_Alloc` (`0x20c5fb0`) | bump heap, never freed | the engine's physical memory pools |
| `DB_FindXAssetHeader` (`0x1420ef0`) | returns a `ScriptParseTree {name, len, buffer}` for the test scripts, 0 for anything else | the asset database (fast files) |
| `LoadScriptGDB` (`0x12cb120`) | returns at once; only reached when a link error is reported (it opens `scriptgdb.zip`) | file system |
| `vsnprintf` (`0x2bc44c0`), `sprintf` (`0x2bc3fb0`), `va` (`0x227d160`) | a Python printf (the CRT's locale and `va`'s per-thread buffers are not set up in a dump) | CRT |
| `Com_PrintfV` (`0x20f0720`) | message recorded; a `Com_ERROR:` banner stops the run and becomes its error (Com_Error itself is obfuscated in this build and longjmps to a frame the harness never set) | console |
| `logprint` handler | the trace recorder (the real handler writes `g_log`, absent) | |
| none | dvars, the file system and time were never reached by the VM paths used; `timeGetTime` and `QueryPerformanceCounter` return 0 (the runner only uses them for statistics) | |

The harness also maps a zero page on any unmapped read or write and records it (`unmapped`):
no test needed one.

## 3. Results (retail mode, developer 0; developer 1 gives the same traces)

| Test | Result | Developer mode | Answer |
|---|---|---|---|
| `array_assign_shared_reverse` | PASS | same | assignment shares the array both ways |
| `array_copy_on_assign` | PASS | same | `b = a` shares the array: a write through b is seen through a (no copy on assignment, no copy on write) |
| `array_in_struct_field` | PASS | same | a local read from a struct field shares the field's array |
| `array_param_plain` | PASS | same | an array passed to a plain parameter is copied on entry |
| `array_param_plain_nested` | PASS | same | that copy is deep: an inner array written through the copy is not seen by the caller |
| `array_param_ref` | PASS | same | an array passed to a `&name` parameter is shared with the caller |
| `array_param_ref_field` | PASS | same | `level.arr` passed to a plain parameter is copied, to a `&name` parameter shared |
| `array_ref_param_non_array` | UNSUPPORTED | differs | a non-array given to `&name` is the script error "Var ByRef only supported for array types"; the error recovery then resumes inside `SafeCreateLocalVariables`' operands and hits a terminal "Invalid opcode" (developer: Com_Error "server script error"; retail: the game re-throws forever, stopped by the harness after 64 errors). Understood (section 8.3): the game's behaviour, no golden possible |
| `bare_call_local_shadows_builtin` | PASS | same | a bare call reaches the current namespace's function before a builtin of the same name; `sys::` reaches the builtin |
| `bare_call_using_file_other_namespace` | PASS | same | a bare call never reaches another namespace, even a `#using`'d one: the game's linker reports an unresolved external |
| `bare_call_using_file_shadows_builtin` | PASS | same | a same-named function in a `#using`'d file of another namespace does not shadow the builtin |
| `childthread_dies_with_parent` | UNSUPPORTED | same | `childthread` is not a T7 keyword: the official linker reads it as an uninitialised local variable and refuses the script (compiler-refused forms are UNSUPPORTED, conformance/README.md) |
| `dev_builtin_outside_devblock` | PASS | same | the compiler accepts `println()` outside `/# #/`, the game's linker refuses the script: "Dev only calls must be wrapped in a devblock" |
| `endon_during_wait` | PASS | same | an endon kills a thread in a timed wait |
| `endon_during_waittill` | PASS | same | an endon kills a thread blocked in waittill on another event |
| `endon_other_object` | PASS | same | an endon is bound to its object |
| `endon_vs_waittill_same_event` | PASS | same | endon and waittill on the same event: killed, not woken |
| `endon_vs_waittill_same_event_reversed` | PASS | same | control: without the endon the waiter wakes |
| `func_ref_builtin_only` | PASS | same | `&spawnstruct` with no script function falls back to the builtin, and calling it works |
| `func_ref_shadows_builtin` | PASS | same | `&f` reaches the current namespace's function first |
| `hash_equality` | PASS | same | `==` between two hashes, or a string and a hash, is a non-terminal script error ("unmatching types 'hash' and 'hash'") and yields false |
| `infinite_loop` | PASS | same | VM_Execute has no loop guard: a loop with no wait runs until the harness budget (20M instructions) |
| `level_vs_struct_notify_isolation` | PASS | same | a waittill is not woken by the same event on another object |
| `multiple_endons` | PASS | same | each of several endons kills the thread |
| `notify_before_waittill_same_frame` | PASS | same | a notify sent before the waittill is registered is lost, even in the same frame |
| `notify_no_waiter` | PASS | same | a notify nobody waits for is silent |
| `notify_reentrant` | PASS | same | a notify sent by a woken waiter is deferred the same way |
| `notify_wake_order_three_waiters` | PASS | same | waiters wake in reverse order of registration (c, b, a) |
| `notify_wake_order_two_waiters` | PASS | same | the last registered waiter wakes first, both after the notifier yields |
| `notify_wakes_waiter_before_notifier_continues` | PASS | same | **notify does not run the waiter inside the call**: the notifier runs on to its next wait or its end, then the woken threads run in the same frame |
| `string_equality_case` | PASS | same | `==` on strings is case-sensitive |
| `struct_reference_on_assign` | PASS | same | structs are references |
| `thread_call_assign_result` | PASS | same | `x = thread f()` compiles; x is f's return value when f ends without waiting, undefined when f waits first |
| `thread_call_builtin_only` | PASS | same | `thread spawnstruct()` with no script function: the script does not load (a threaded call never falls back to a builtin) |
| `thread_call_return_value` | PASS | same | a direct call returns its value; a threaded call as a statement drops it |
| `thread_call_shadowing_builtin` | PASS | same | `thread f()` reaches the current namespace's function when a builtin has the same name |
| `thread_on_released_struct` | PASS | same | a thread whose self struct nobody references keeps running; self stays defined |
| `thread_outlives_parent` | PASS | same | a plain thread outlives the thread that started it |
| `thread_runs_until_first_wait` | PASS | same | `thread f()` runs f at once up to its first wait, then the caller continues |
| `thread_without_wait_runs_to_end` | PASS | same | a thread with no wait runs to its end before the caller continues |
| `undefined_arithmetic` | PASS | same | `undefined + 1` is a non-terminal script error: the thread goes on, the result is undefined |
| `undefined_field_read` | PASS | same | reading an unset field gives undefined, no error |
| `wait_0` | PASS | same | `wait 0` resumes in the same frame, before threads in `waittillframeend` |
| `wait_005_is_one_frame` | PASS | same | wait 0.05 / 0.1 / 0.15 take 1 / 2 / 3 frames |
| `wait_same_duration_order` | PASS | same | waits ending in the same frame resume in reverse order of the wait calls |
| `waittill_fewer_args_sent` | PASS | same | waittill names with no value sent stay undefined |
| `waittill_more_args_sent` | PASS | same | extra notify values are dropped without error |
| `waittillframeend_order` | PASS | same | `waittillframeend` resumes in the same frame after threads that do not wait |
| `waittillmatch` | PASS | same | waittillmatch skips a notify whose first argument differs |

STATUS t7: PASS 47, UNSUPPORTED 3 (both modes; `childthread_dies_with_parent` was PASS with its compile error until the
suites took one rule for compiler-refused scripts: UNSUPPORTED with the message. `nested_block_comment`,
added 2026-11-11, is the third: the linker closes a block comment at the first `*/` and lexes the
second as `*` then `/`, "syntax error, unexpected TOKEN_MULTIPLY"; see `vm_t4.md` section 4).

**Where my first guesses were wrong** (the goldens are the VM's answers, not my expectations):
notify wakes are deferred (I expected the waiter to run inside the notify, as in CoD4's
`VM_Notify`); wake and same-time resume order is last-in-first-out; `wait 0` does not yield
past waittillframeend threads; array assignment shares; `==` on hashes is an error; a threaded
call can return a value; `childthread` does not exist in the T7 compiler.

## 4. The two ROADMAP contradictions

**Bare-call lookup order (`bare_call_resolution`).** The BO3 GSC doc says "the current
namespace first, then sys"; gscode's dialect facts say "the builtin first; threaded calls and `&`
references mean the script function". Measured in the game's own linker at load:

- bare `f()`: the current namespace's script function if it exists, else the builtin
  (`bare_call_local_shadows_builtin`, `bare_call_using_file_shadows_builtin`);
- never a function of another namespace, even `#using`'d: unresolved external, the script does
  not load (`bare_call_using_file_other_namespace`);
- `thread f()`: the script function; no builtin fallback, the script does not load
  (`thread_call_shadowing_builtin`, `thread_call_builtin_only`);
- `&f`: the script function first, else the builtin (`func_ref_*`).

The doc is right; gscode's "builtin first" is wrong for T7 bare calls and its "`&` means the
script function" misses the builtin fallback; "threaded calls mean the script function" is
right. acts' BO4 linker rule (namespace first, builtin fallback only for get-call imports, never
for threads) matches T7 exactly. The compiler side was already measured (every bare call is an
import of `<current namespace>::f`), so the whole answer is the load-time linker's.
Profile: `namespace-first`, now `measured-in-engine`; the `[[unresolved]] t7-bare-call` block is
removed.

**Array copy (`array_argument_passing`).** The BO3 doc says "by copy unless the parameter is
`&name`"; gscode's profiles say "BO3 passes arrays by reference". Measured: a plain parameter
gets a deep copy, a `&name` parameter shares (`array_param_*`). The doc is right; gscode is right
only for `&name`. Static second measure: `SafeCreateLocalVariables` (`0x12ce890`) calls
`AllocArray` + `CopyArray` (`0x12da210`, recursive for child arrays) for a parameter without
flag 0x1, keeps the pointer with it, and raises "Var ByRef only supported for array types" for
a non-array. Neither source says it, but plain assignment `b = a` **shares** the array (no copy
on write: `ScrVar_GetArrayRef`, `0x12de5b0`, never copies); a linter that models arrays as values
must model locals as references and only parameters as copies. Profile:
`copy-unless-ref-param`, `measured-in-engine`; `[[unresolved]] t7-array-passing` removed.

## 5. Measures and their limits

1. The emulated run: the game's own code for every step from init to the opcode handlers.
2. A static reading of the function that decides the behaviour, for the surprising answers
   (`evidence` in the facts): notify deferral (`VM_Notify` `0x12d36c0` calls
   `ScrVar_SetWaitTime` and never `VM_Resume` or `VM_Execute`), array copy and sharing, the
   missing loop guard (Jump handler `0x12cd9b0`, no "infinite loop" string in the build).
3. Two frame models (end-of-frame runner only, and `G_RunFrame`'s two calls) and two VM modes
   (developer 0 and 1) give identical traces, except `array_ref_param_non_array` (UNSUPPORTED).
4. **Still owed (the methodology's second measure):** the same scripts run once by the user in
   the offline mod tools client ("launch with mod", developer on, `logprint` goes to the game
   log, or `iprintln`), and the logs compared to the goldens. Agents never do this. Until then
   the facts say "measured in the emulated VM of this build", not "observed in a running game".

Also not covered: entities (only `level` and structs; entity delete/death and `self` on a removed
entity need the entity system), the client VM (`inst 1`), `waittill` on several events,
`waittill_any`-style script helpers, `notify` from the engine side (`Scr_Notify` wrappers).

## 6. Licence

Unicorn is GPL-2.0-only. As in the rest of `tools/re`, it is imported by a separate Python
research tool that writes data files (traces, `semantics/t7.toml`, CC0 facts); nothing links it
into the GPL-3.0-or-later Rust crates. Whether shipping these Python tools in the same
repository as GPL-3.0 code is fine (they are separate programs that do not combine with the
crates) is a question for the user before any release; icicle-emu (MIT/Apache-2.0) is the
alternative if an emulator must ever live inside a crate.

## 7. Open issues

- `array_ref_param_non_array`: understood with the trace (section 8.3): the game's own error
  recovery resumes inside the operands of `SafeCreateLocalVariables`. It stays UNSUPPORTED only
  because a run that never ends has no golden trace.
- The infinite-loop answer depends on the harness budget; a watchdog outside the VM (another
  thread, a hitch detector) would not be seen here.
- `thread_on_released_struct` shows the struct is kept alive; whether it is freed when the thread
  ends is not measured (needs a variable-pool walk).
- Error messages in the trace are the engine's own short texts; they are kept as facts (like event
  names). Tell me if they should be reduced to codes.
- The builtin table places come from phase 2's `out/t7_extract.json`; the harness refuses to run
  without it (build mismatch included).

## 8. Trace mode (step 5.5)

`t7_run.py --trace run.jsonl [--calls] [--ops [N]]` writes a **gsc-trace** (format spec,
versioned: `docs/trace-format.md`; shared writer/reader: `vm/trace.py`, meant for the T8/T9
emulators, a Rust tracer or a DAP adapter too). `trace_view.py run.jsonl` prints a per-frame
timeline with one lane per thread; `--html` writes a self-contained page (swimlanes with run and
wait bars and notify-to-wake/kill links, plus the event table).

### 8.1 Where each event comes from

| Event | Hook (RVA) | Decoded from |
|---|---|---|
| every opcode (`op`, `--ops`) | the dispatch calls of `VM_Execute` (`0x12edd2c` table `0x3287350`, bit 13 set; `0x12edd36` table `0x3267350`) and of the error-recovery re-dispatch `0x12ee0f0` (`0x12ee1f2` / `0x12ee1fc`, tables `0x3297350` / `0x3277350`) | `function_state.pos` is past the u16 opcode; the name is acts' name of the handler the normal table selects (125 handlers, one name each) |
| `wait_start` | dispatch of `Wait`, `Wait2`, `WaitTillFrameEnd`, `IW_SingleWaitTill`, `WaitTillMatch2` | stack top: duration; or object (top), event (top-1), match values below (`WaitTillMatch2`'s operand byte is their count) |
| `wait_end` | the waiting thread's next dispatch | `by`: the last notify of the same (object, event) |
| `notify`, `endon` | dispatch of `IW_Notify`, `IW_SingleEndon` | object (top), event (top-1), notify arguments down to the `PreScriptCall` marker (type precodepos) |
| `builtin` | dispatch of `CallBuiltinFunction` / `CallBuiltinMethod` | operands: u8 argument count, then the handler pointer at the next 8-byte boundary (named with `gruff-data/builtins/t7.toml`); arguments from the top; for a method `self` is the top value (`0x12ce220` pops it) |
| `thread_create` | `ScrVar_AllocThread`'s return at the thread-call sites (`ALLOC_THREAD_SITES`), written at the new thread's first opcode | entry function = where that opcode is; parent and `self` from the thread-call dispatch |
| `thread_end` | `0x12e0c60(inst, id)`: turns the thread variable into removed_thread (`0x1a`) | measured by watching the thread variables' writes: it runs when a thread ends and when an endon kills it (inside the Notify handler); a waiting thread only becomes a time_thread (`0x13`) |
| `error` | `Scr_Throw` (`0x12ea4e0`) | `gScrVarPub.error_message`, terminal flag |
| `call` / `return` (`--calls`) | dispatch at an exported function's first instruction / of `End`, `Return` | |

**Source lines.** The official linker writes a line table beside each compiled script, in the
mod's `zone_source/all/scriptgdb/<path>.gsc.gdb`; `gsc_link.py --gscc-out` now copies it next to
the `.gscc` (cache entries without it are linked again when a trace is asked for). Measured
layout: magic `80 'GDB' 0d 0a 00` + version byte `0x13`; u32 0; the source CRC (the same as the
`.gscc` header's); `{u32 offset, u32 count}` for a line table, a second table (always empty in
our scripts) and a string table (file stem, namespace, function and builtin names). Line-table
entry *i* (8 bytes: u32 code offset, u32 0) is where line *i*+1's code starts; a line with no
code repeats the next line's offset, so the line of an instruction is the last line starting at
or before it. Checked against acts' disassembly of `wait_0.gsc` (every statement on its line).

Cost: a trace run takes about as long as a plain one (1-6 s); the opcode hook is one Python call
per opcode.

### 8.2 Timelines

Four conformance tests (`notify_wakes_waiter_before_notifier_continues`,
`notify_wake_order_three_waiters`, `endon_vs_waittill_same_event`, `waittillframeend_order`)
and `wait_0`, `waittillmatch`, `endon_during_wait` traced; their timelines show the facts of
section 3 directly (deferred wake, last-in-first-out wake order with `by` pointing at the
notify, waittillframeend after the frame's other threads). One thing the logprint traces could
not show: **an endon kill happens inside the notify**, before the notifier's next statement,
while a waittill woken by the same kind of notify only runs after the notifier yields:

```
-- f1 --------------------------------------------------------------
        [8] ^ woke +1f L11                :
        [9] ! level notify stop L11       :
        |                                 [10] x endon level stop by [9]
        [11] logprint("sent stop") L12
```

`vm/examples/round_loop.gsc` (our own script: rounds, zombie threads on structs, a round watcher
on `waittill("zombie_died", id)`, a `waittillmatch("start_of_round", 3)` nuke that kills the
living zombies through their `endon("death")`, `game_over` ending every loop) runs 61 frames,
18 threads, 296 records. The nuke at f19, in the text view (lanes cut):

```
        :              [150] ^ woke +2f L74
        :              [151] logprint("nuke") L74
        :              [152] ! struct#43 notify death L81
        :              |                         [153] x endon struct#43 death by [152]
        :              [154] ! struct#105 notify death L81
        :              |                                     [155] x endon struct#105 death by [154]
```

The same timeline shows a bug of the script that its log does not: the nuke's
`level notify("round_cleared")` (`[158]`, f19) is lost, because the round loop is still in its
spawn loop (`wait 0.05`, `[149]`) and not yet in `waittill("round_cleared")`; round 3 only ends
when the last zombie, spawned after the nuke, dies at f28.

### 8.3 `array_ref_param_non_array` explained

With `--ops` the trace shows the sequence (offsets in `change`):

```
[10] . SafeCreateLocalVariables L9            change+0
[11] E Var ByRef only supported for array types L9
[12] . SafeCreateLocalVariables L9 (recovery)
[13] E TERMINAL Invalid opcode L9             at change+2, opcode 0x6a01
[14] E TERMINAL Invalid opcode L9             ... the same, until the harness stops at 64
```

Static reading of why: `VM_Execute` saves `pos` before each opcode (`0x12edc63`) and, after the
longjmp of a script error, restores it and calls the recovery `0x12ee0f0`, which re-dispatches
the same opcode through the error tables; every opcode's error variant sits just before its
handler and skips that opcode's operands (`Plus`: pops one value; `ScriptFunctionCall`
`0x12cee40`: skips the 8-byte operand, pops the arguments, pushes undefined). The error variant
of `SafeCreateLocalVariables` (`0x12ce880`) is a bare `ret`: its operands (u8 count, a padding
byte, then per variable an aligned u32 name hash and a flag byte) are not skipped, so the next
dispatch reads the count `0x01` and the padding byte `0x6a` as opcode `0x6a01`, an invalid one.
The invalid opcode throws again from the same saved position; in retail `Scr_Throw` longjmps
with no terminal check and the recovery clears the terminal flag before `RuntimeErrorInternal`
(`0x12d4e40`, which then does nothing in retail), so the loop never ends: a hang in the game, not
a harness artefact. In developer mode `Scr_Throw` reports the terminal error and Com_Error ends
the run. The padding byte is the compiler's (the same in three separate compilations of this
file); a value that makes a valid opcode would run that handler instead.

So the ByRef check (its error string is in `SafeCreateLocalVariables`, `0x12ce890`) has no
working recovery. The test keeps its golden first line and stays UNSUPPORTED (a run that never
ends has no golden); its `// fact:` and `// evidence:` say the above, and `semantics/t7.toml`
was regenerated (suite: PASS 48, UNSUPPORTED 1 in both modes, unchanged).

### 8.4 Limits

- Notifies sent by engine code (not by a script `notify`) are not recorded; nor are entity
  threads (no entities in the harness) or the client VM.
- `thread_end` reasons other than `return` and `endon` are inferred (`error` after a script
  error in that thread, else `killed`).
- `Wait2` (another timed-wait opcode) is recorded as `wait_other`; which script construct
  compiles to it was not checked.
- The opcode names are acts'; 125 handlers, each with one name. The two `Nop` handlers include
  the default handler of unused opcode values.
- The second measure is still owed for everything here (section 5.4): the timelines are the
  emulated VM's.
