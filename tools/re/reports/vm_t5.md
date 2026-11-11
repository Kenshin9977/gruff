# Black Ops (T5) script VM, run offline with the game's own compiler: method, stubs, dialect against T6

Phase 5.7, part 1. 2026-11-06, 18:05-18:55 (one agent, after T6). Executable: the install's
`BlackOpsMP.exe` (not encrypted, file layout; x86, image base 0x400000, PE timestamp 1320282179;
The SP exe (`exe.t5`) has no `logprint`, the trace builtin,
so the multiplayer one is used. Nothing was launched or attached (`tools/re/vm/t56vm.py`, profile
`t5`). Facts: `gruff-data/semantics/t5.toml`; scripts: `conformance/t5/`.

## 1. What runs

**No external compiler**: the plain-text `.gsc` is given to the game, whose own lexer, parser and
compiler run inside `Scr_LoadScript`. A compile error is the engine's `Com_Error 5 "Server script
compile error"` with its message (`bad syntax`, `unknown function`, `uninitialised variable ...`),
recorded as the behaviour's compile error.

| Step | Entry |
|---|---|
| Com_Init's part (0x86cf10): `Hunk_InitMemory()` (5 MB hunk through VirtualAlloc), `Hunk_InitDebugMemory()` (the debug hunk developer mode's `Scr_InitOpcodeLookup` uses), `SL_Init(0)`, `Scr_InitVariables(0)`, `Scr_Init(0)`, `Scr_Settings(dev, dev, 0, 0)` | `0x6177a0`, `0x68d790`, `0x5049e0`, `0x5cd480`, `0x4ecee0`, `0x55d010` |
| G_InitGame's part (0x5f715b): `Scr_InitSystem(0, 1)`, `Scr_SetLoading(1, 0)`, `Scr_AllocGameVariable(0)` | `0x65fe00`, `0x5fab60`, `0x4bff40` |
| GScr_LoadScripts (0x41fb20): `Scr_BeginLoadScripts(0, 1)`, `Scr_LoadScript(0, name)` -> Scr_LoadScriptInternal 0x560cf0 -> Scr_AddSourceBuffer 0x62fdc0 -> Scr_ReadFile 0x8e37a0 -> the fast-file reader 0x8e35d0 (`DB_FindXAssetHeader(0x24 rawfile, "<name>.gsc")`, inflate) -> parse, compile; `Scr_GetFunctionHandle(0, file, func)`; `Scr_EndLoadScripts(0)` | `0x624dc0`, `0x6b6120`, `0x5d5600`, `0x59c7e0` |
| run: `Scr_ExecThread(0, handle, 0)`, `Scr_FreeThread(id, 0)` (the StartGameType runner 0x4efaf0); per frame `Scr_RunCurrentThreads(0)` then `Scr_IncTime(0)` (the frame function 0x5c9ea0) | `0x69bb00`, `0x5c17c0`, `0x41c180`, `0x6bea70` |

Scripts are registered as `zz_harness/<file>`: Scr_LoadScriptInternal formats `"%s.gsc"` into a
64-byte buffer, and `maps/mp/zz_harness/notify_wakes_waiter_before_notifier_continues.gsc` (69
characters) was cut and not found (caught as a load failure with no message).

**The running thread.** T5's VM_Execute (0x8e6b90) keeps the frame (`fs`: pos, localId, ...) in its
own locals and writes gFs back only when it leaves: read from gFs, a new thread's first logprint was
named after its caller (caught on `thread_runs_until_first_wait`). The harness watches the dispatch
(0x8e6c16, every opcode) and reads fs.localId at `[ebp + 0x5c]`; with it, all thread names agree
with T6's.

## 2. Stubs beyond T6's pattern

| Stub | Why |
|---|---|
| dvar at `[0x359eb00]` (useFastFile) -> a fake dvar, value 1 | Scr_ReadFile picks the fast-file reader on it; no dvar is registered in an executable at rest |
| rawfile format | the fast-file reader inflates a `.gsc` rawfile `{u32 length, u32 compressed length, zlib}`; our source is stored that way |
| CRT `malloc` (0x9cf4d6) -> bump heap | zlib's allocations; the CRT heap is not set up (it ends in LoadLibraryA) |
| CRT `sscanf` (0x9cdbbe) -> Python (`%f %d %i %x`, any other format stops the run) | the lexer reads float literals with `%f` |
| 0xa0bbd0 -> `return 1` | called first by Scr_InitClassMap (0x61b950): an SEH-guarded routine outside the script VM that ends in RaiseException and resumes in its own handler |
| VirtualAlloc reserve -> bump heap (commit returns the address) | Hunk_InitMemory, the debug hunk |
| `PMem_Alloc` (0x4c58c0), `_vsnprintf` (0x9cae40), `va` (0x502ca0), `longjmp` (0x9d05c4), `DB_FindXAssetHeader` (0x493a60), `Com_Printf` (0x4126c0), `Com_Error` (0x627380) | as T6 |

## 3. Results (retail and developer 1 identical): PASS 47, UNSUPPORTED 11

52 of 57 behaviours answer as T6, including **arrays as values (copy on write)**, deferred notify,
same-time order, and the same refusals. Differences from T6:

| Test | T5 | T6 |
|---|---|---|
| `bare_call_local_shadows_builtin` | **the file's own function** first, then the builtin (file-first) | the builtin first |
| `func_ref_shadows_builtin` | `::spawnstruct` reaches the file's function | the builtin's native handler (broken) |
| `func_ref_builtin_only` | the compiler refuses `::spawnstruct` with no script function ("unknown function") | the native handler run as bytecode |
| `thread_call_builtin_only` | refused by the compiler ("unknown function") | refused by the linker (unresolved external) |
| `dev_builtin_outside_devblock` | `println` outside `/# #/` compiles and the script runs on | refused at link |
| `hash_equality` | `#"x"` is an int computed at compile time (case-insensitive): `#"GoAway" == #"goaway"` is true; string vs hash is a script error (string vs int), result undefined | hash type, script error |

The bare-call order matches the profile value `file-first` (same file, builtin, `#include`'d
files; `thread f()` and `::f` never reach a builtin).

UNSUPPORTED (10, all refused by the game's own compiler): `&name` parameters x3 (bad syntax),
`childthread` x2 (uninitialised variable 'childthread'), `endon(a, b)`, `waittill("a", "b")`,
`s = waittill(...)` (bad syntax), `waitframe`, `waittilltimeout` (unknown function).

## 4. Limits

No static second reading was made for T5 (the answers equal T6's except the compiler-side ones,
which the game's own compiler gives directly); server VM only, no entities; the user's run of the
scripts in a real session is owed.
