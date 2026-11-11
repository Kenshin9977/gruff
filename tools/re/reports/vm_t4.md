# World at War (T4) script VM, run offline: method, stubs, results, dialect against T7

Phase 5.7, part 2. 2026-11-06, 17:22-18:02 (one agent; IW3 followed until 18:10, `vm_iw3.md`). Executable: `codwaw_flat.exe`, the
unwrapped single-player build (PE timestamp 1445666630, image base 0x400000, x86). Nothing was launched or attached: the game's code runs under
Unicorn over the executable (`tools/re/vm/`). Addresses are VAs. Facts: `gruff-data/semantics/t4.toml`;
scripts: `conformance/t4/`. Names follow the Quake 3 / CoD4 lineage as labels only; each one is
pinned by what calls it and what it does when run (comments of `T4` in `vm/t4vm.py`).

## 1. What runs

Unlike T7-T9, T4 compiles plain-text GSC at load with its own compiler: the test script is a file
the game reads, and the game's lexer, parser, compiler and VM do the rest. No external compiler.

| Step | What the game's own code does | Entry |
|---|---|---|
| CRT | `_cfltcvt_init` (the float conversion table `sscanf("%f")` uses for float literals) | `0x7af575` |
| engine | `Sys_InitMainThread` (TLS thread data: the `va()` buffers), `Com_InitHunkMemory`, `Hunk_InitDebugMemory` (Com_Init's order) | `0x5a2f90`, `0x5e3ca0`, `0x5e4650` |
| init | `SL_Init(0)` (memory tree + string table), `InitVariableRange` x2 (as Com_Init 0x59d46f/0x59d483), `Scr_Init(0)`, `Scr_InitSystem(0)` (time array, pause array, level, anim), gameId as G_LoadLevel (0x503300) allocates it | `0x68d9f0`, `0x68ef90`, `0x693c20`, `0x699860`, `0x68fe20`+`0x692d70` |
| load | `Scr_BeginLoadScripts`, `Scr_LoadScript(name)`: `"%s.gsc"`, the source read, parse (`0x69aec0`), compile (call resolution, `#include`s loaded the same way), `Scr_GetFunctionHandle`, `Scr_EndLoadScripts` (GScr_LoadScripts' sequence, 0x514870) | `0x689660`, `0x689c60`, `0x6894b0`, `0x689c80` |
| run | `Scr_ExecThread(handle, 0)` (self = level) then the thread id freed (G_LoadLevel 0x503366); per frame, as G_RunFrame (0x503ab0) with no entities: `Scr_RunCurrentThreads` (0x503c4d), then the tail at 0x503e48 = `0x69ade0`: runner, 0x692160, `time += 1` | `0x699560`, `0x6997e0`, `0x69ade0` |
| trace | `iprintln`'s handler (0x515e10; SP has no logprint) and `println`'s (0x5155e0) are recorders: time, thread, arguments off the VM stack (scrVmPub 0x3bd4700: top +0x10, outparamcount +0x1c). Script errors at `Scr_ErrorInternal` (0x693cf0: scrVarPub.error_message, terminal flag 0x3bd4716); the loop guard's thread kill at 0x69639c | |

Threads: T0 is the entry thread, T1, T2... in creation order (the sites that type a new thread
variable 0x6d: the four thread-call opcodes 0x54-0x57 and the Exec APIs). A script call runs in a
child-thread variable (type 16) whose type word carries the caller's frame above bit 8 (measured:
a call from `f` in frame 9 has type 0x970); the thread is the root of that chain. First guess
wrong: opcode 0x7a (endon) types its endon record 0x6d the same way, which named endons as
threads (`multiple_endons` showed `T4` for the second thread); that site was removed.

`f<n>` is scrVarPub.time (0x3882b84, one 50 ms frame per unit; `wait 0.05` = 1).

```sh
py -3.11 -I tools/re/vm/old_run.py --game t4 conformance/t4/notify_reentrant.gsc [--developer] [--verbose]
py -3.11 -I tools/re/vm/old_suite.py --game t4 [--developer]      # one process at a time
py -3.11 -I tools/re/vm/old_facts.py --game t4                     # gruff-data/semantics/t4.toml
```

A run takes about 0.4 s (boot included); the suite, 57 tests, about 25 s.

**Modes.** Retail: developer 0, developer_script 0. Developer: what `Scr_Settings` (0x59c840)
writes for the dvars developer 1 and developer_script 1: scrVarPub.developer and
developer_script set, and abort_on_error, which follows `developer`, set too.

## 2. Stubs and replications (each run's JSON lists every one, with its hit count)

| Stub | Why | Replaces |
|---|---|---|
| imports (all) | stdcall stubs that pop their arguments, logged; an import of unknown arity stops the run (`vmcore32.py`); heap, `VirtualAlloc` (a reservation is a fresh block of the bump heap, a commit returns its address), TLS/FLS, Interlocked* handled | kernel32 etc. |
| `timeGetTime`, `GetTickCount` | a virtual clock: +1 ms per read, +50 ms per frame | wall clock |
| `FS_FOpenFileRead` / `FS_Read` / `FS_FCloseFile` (0x5dbd20, 0x5dbdf0, 0x5db060) | the test scripts only (`zz/<name>.gsc`), not found otherwise | file system |
| `_vsnprintf` (0x7aa87b) | Python printf (4-byte slots, doubles two) | CRT |
| `Com_PrintMessage` (0x59a170) | recorded (`vm_messages`) | console |
| `Com_Error` (0x59ac50), `Sys_Error` (0x5fe8c0) | recorded, the run stops (compile errors end here with "script compile error") | error handling |
| CRT `longjmp` (0x7ad57c) | direct `_JUMP_BUFFER` restore (ebp, ebx, edi, esi, esp, eip, SEH head): the script error unwind into VM_ExecuteInternal's `_setjmp3` | CRT (SEH local unwind) |
| CRT `_getptd_noexit` (0x7b1577) | one per-thread block filled as `_initptd` (0x7b14b8) fills it | CRT thread init (never ran: it recursed through `_decode_pointer`) |
| CRT `_osplatform`/`_winmajor` (0x8e7650 = 2, 0x8e765c = 6) | set as the CRT start-up sets them (else the pointer-encoding probe recursed) | CRT start-up |
| CRT `_lock` (0x7b35bc) | returns at once (one emulated thread; its lock table was never set up) | CRT |
| Scr_Settings' bytes | replicated (6 bytes: developer, developer_script, abort_on_error for inst 0 and 1) | Scr_Settings from dvars |
| gameId | G_LoadLevel's two calls replicated (AllocObject + 0x692d70) | G_LoadLevel (game code around them) |

The first MiB is mapped as zeros so null-plus-offset reads continue; a write there is engine state
the harness did not set up and is recorded per run (`null_writes`, none in the suite). It found
one: without `Sys_InitMainThread`, `va()` formatted into the null page that other null-based
reads share, and the second error message of a run crashed the formatter.

The compiler formats `<name>.gsc` into a 64-byte buffer, so the harness folder is `zz/` (with
`maps/zz_harness/` the longest test name was cut and "Scr_LoadScript returned 0").

## 3. Results (retail: PASS 46, UNSUPPORTED 13; developer: 3 traces differ)

`vs t7` (`dialect` in the facts): 39 answers are T7's, 18 differ, 10 of them because the form
cannot exist in T4 (UNSUPPORTED below; `childthread_dies_with_parent`, also UNSUPPORTED, is the
same refusal as T7's) and `notify_payload_object_is_result`, which T7 does not
run. The differences and the notable sames:

| Test | T4 | T7 |
|---|---|---|
| `array_copy_on_assign`, `array_assign_shared_reverse`, `array_in_struct_field` | **arrays are values**: `b = a` then a write through either leaves the other unchanged (copy on write) | shared both ways |
| `array_param_plain`, `_nested` | the caller never sees the callee's writes (same answer, other mechanism: copy on write, no copy at the call) | deep copy on entry |
| `bare_call_using_file_other_namespace` | `#include` merges: a bare call reaches a function only an included file defines | unresolved external |
| `bare_call_local_shadows_builtin` | same file's function, then the builtin, then the #include'd files (so an #include'd `spawnstruct` does not shadow the builtin, as T7) | namespace, then builtin |
| `func_ref_builtin_only` | `::spawnstruct` with no script function: compile error "unknown function" | falls back to the builtin |
| `thread_call_builtin_only` | compile error "unknown function" | unresolved external at load |
| `dev_builtin_outside_devblock` | compiled; retail compiles the call away, developer_script 1 runs it | the linker refuses the script |
| `infinite_loop` | **a 2.5 s wall-clock guard**: retail kills the thread ("potential infinite loop in script - killing thread."), developer raises it as a terminal error | no guard |
| `childthread_dies_with_parent` | UNSUPPORTED: compile error "uninitialised variable 'childthread'" | same (its linker's message; UNSUPPORTED there too) |
| `notify_payload_object_is_result` | ported to `waittill("e", s)`: the object arrives as the same object | (T7: same binding form) |

The same as T7: notify defers the woken threads to after the notifier yields, wake order last
registered first (two and three waiters), reentrant notifies deferred the same way, a lost notify
before the waittill, endon kills in wait and waittill and before a waittill on the same event, per
object; waits ending in the same frame resume last-in-first-out; `wait 0` resumes in the same
frame before `waittillframeend` threads; `thread f()` runs to the first wait; `x = thread f()`
compiles (f's value without a wait, undefined with one); structs are references; `==` on strings
is case-sensitive; `undefined + 1` is a non-terminal error (retail: silent and the thread goes on;
developer: abort_on_error makes it a Com_Error "script runtime error"); unset fields are undefined.

UNSUPPORTED (the form cannot exist in T4; each is run, the game's compiler refuses it, and the
message is in the facts): `array_param_ref`, `array_ref_param_non_array` (`&name`: bad syntax),
`hash_equality`, `notify_string_vs_hash_event` (`#"..."`: bad token '#'), `childthread_dies_with_parent`,
`childthread_inherits_endons`,
`endon_multiple_events`, `waittill_any_of_events`, `waittill_returns_struct` (bad syntax),
`waitframe_one`, `waittilltimeout` (unknown function). `array_param_ref_field` keeps the T7
script's plain half (PASS). `func_ref_builtin_only` and `thread_call_builtin_only` stay PASS with
their compile error: the scripts are well-formed T4 and the refusal ("unknown function") is the
answer to their name-resolution question.

Profile (`profiles/t4.toml`): `bare_call_resolution = "file-first"` (the order above: same file,
builtin, `#include`d files; `::f` and `thread f()` never reach a builtin), `hash_equality = "n/a"`
(no hash literals), `childthread = false` citing the emulation without a behaviour id (both
childthread tests are UNSUPPORTED). The shipped scripts agree with `::f` reaching the
`#include`d files: 275 `::f` references of the WaW scripts name a function only an `#include`d file
defines, and the game compiles them.

Developer mode differs in `dev_builtin_outside_devblock` (println runs), `infinite_loop` (the
error, then Com_Error; recorded as not terminal: the guard sets terminal_error, then its Scr_Error
0x69ab70 stores its argument 0 over it; IW3 keeps it terminal, `vm_iw3.md`) and `undefined_arithmetic` (Com_Error after the error: abort_on_error), and the
compile errors carry the source line.

## 4. Static readings behind the surprising answers

- **Copy on write**: the array-ref evaluator (0x692850) tests the array's reference count (+4) at
  0x692ab6; when shared it drops its reference (0x690040), allocates a new array (0x68ff60) and
  copies the elements (0x690770) before the write. The emulated `b = a; b[0] = 2` reaches both
  between the two statements and nowhere else.
- **Call resolution** (the compiler, 0x682ca0): the current file's function map (0x3701fe8 +
  inst*0x1d8, FindVariable 0x690260 at 0x682cf2) first; then the builtin cache (0x36bff84) and
  lookup (0x5676f0); a name neither knows becomes a script call resolved over the #include'd files.
- **Dev builtins**: the call emitter brackets a developer builtin with 0x682b30 / 0x682ba0; with
  developer_script (0x3882b77) clear the second moves the code buffer back to where the first
  recorded it, so the call's code is discarded.
- **Notify defers**: VM_Notify (0x698670) never calls VM_Execute (0x6992e0), VM_ExecuteInternal
  (0x693e80) or VM_Resume (0x6990e0); it reads scrVarPub.time (0x698a39, 0x698b22) to queue the
  woken thread in the time array.
- **Loop guard**: OP_jumpback (opcode 0x63, 0x6962b0): `timeGetTime - start - lastReset >= 2500`
  (0x6962e8) → debugger: warn and reset; abort_on_error clear: print "killing thread" and unwind the
  thread (0x69639c); else Scr_Error "potential infinite loop in script", terminal (0x696486).
  **Caught by double-checking**: with `timeGetTime` stubbed to 0 as first written, the guard never
  fired and `infinite_loop` agreed with T7 (no guard); the strings `potential infinite loop` in the
  executable led to the handler. The clock is now virtual (section 2): the iteration count at which
  the guard fires is the harness's, the outcome the game's.
- **Block comments do not nest** (`nested_block_comment`, `stray_block_comment_close`,
  2026-11-11). Asked because a shipped script reads as if they did: an outer `/*` over nearly the
  whole file, an inner one over an older copy of a function, and a final `*/`. They do not. `/* a
  /* b */ */` is refused ("bad syntax"), a lone `*/` is refused, and the same file with one plain
  comment compiles and traces. Same answer in IW3, T5 and T7, each asked through its own official
  compiler or linker; the T7 linker names what it lexed at the second `*/` ("syntax error,
  unexpected TOKEN_MULTIPLY"), which is what pins the mechanism: the comment had already closed at
  the first one. A file written that way is therefore refused at load, not read as all-commented:
  handing the shipped file to this VM stops at "bad syntax", and the same file with its trailing
  `*/` removed gets past the lexer and on to resolving its `#include`s. Gruff's lexer already ends
  a block comment at the first `*/`, so nothing changed in it.

## 5. Limits

- Server VM (inst 0) only, no entities (level and structs), no engine-side notifies; the SP
  executable (the MP one is SteamStub-wrapped).
- The second measure is the static reading above; the user's own run of these scripts in the game
  (developer 1, `iprintln` on screen) would be the methodology's second measure, not done.
- No trace mode (`gsc-trace`) for T4 yet: the hooks exist (thread sites, errors, builtins), the
  opcode table is at 0x697558 (0..0x86), not mapped to names.
