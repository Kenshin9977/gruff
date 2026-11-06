# Black Ops II (T6) script VM, run offline: method, stubs, results, dialect against T7

Phase 5.7, part 1. 2026-11-06, 17:21-18:55 (one agent, T6 then T5). Executable: `t6_dump_flat.exe`
(t6mp.exe unpacked with Steamless, flattened; x86, image base 0x400000, PE timestamp 1786983181;
sha256 in `gruff-data/semantics/t6.toml`). Nothing was launched or attached: the game's code runs under
Unicorn (`tools/re/vm/t56vm.py`, profile `t6`). Addresses are VAs. Facts:
`gruff-data/semantics/t6.toml`; scripts: `conformance/t6/`.

## 1. What runs

| Step | What the game's own code does | Entry |
|---|---|---|
| init (Com_Init's order) | `SL_Init()` (MT_Init 0x7df080, string hash table), `Scr_InitVariables(0)` (four pools from PMem_Alloc), `Scr_Init(0)` (Scr_VM_Init 0x7eaaf0, bInited), `Scr_Settings(0, dev, dev, 0)` | `0x7e3890`, `0x7e4d60`, `0x7eac20`, `0x7eac80` |
| G_InitGame's part | `Scr_InitSystem(0, 1)` (time array, pause array, level...), `Scr_SetLoading(0, 1)`, `Scr_AllocGameVariable(0)` | `0x7ec340`, `0x7ead10`, `0x7e76f0` |
| load (GScr_LoadScripts 0x622ca0) | `Scr_BeginLoadScripts(0, 1)` (its first call de-duplicates the 2,580 canonical names: ~200M instructions, 1-2 s), `Scr_LoadScript(0, name)` -> `DB_FindXAssetHeader(0x30, "<name>.gsc")` (hooked), header check, the game's linker 0x7e0f00 / import resolver 0x7e08b0, `Scr_GetFunctionHandle(0, file, func, 0, 1)`, `Scr_EndLoadScripts(0)`, `Scr_SetLoading(0, 0)` | `0x7de700`, `0x7de2f0`, `0x7dfef0`, `0x7de300` |
| run | `Scr_ExecThread(0, handle, 0)` + `Scr_FreeThread` (the StartGameType runner 0x639990's pair); per frame as G_RunFrame (0x6015b0) does: `Scr_RunCurrentThreads(0)` before the entity loop, `Scr_IncTime(0)` (runner again, time += 1) after it | `0x7f1970`, `0x7ec310`, `0x7f2810`, `0x7f2950` |
| trace | `logprint`'s handler (0x628bf0, from `builtins/t6.toml`) replaced by a recorder; errors at `Scr_ErrorInternal` (0x7ead30: gScrVarPub.error_message, scrVmPub terminal byte) | |

Time unit: gScrVarPub.time (+0x10), one 50 ms server frame (the Wait opcode multiplies seconds by 20
on the server instance, 60 on the client). Threads: T0 the entry thread, T1... in creation order (the
return of AllocThread 0x7e5900 at its call sites, **except** 0x7ef673: the EndOn opcode allocates a
thread-typed object as the endon's record; counted, it renumbered later threads in
`multiple_endons`, caught and fixed). The running frame is gFs.localId (0x2d45f88 + 8, written back
by VM_Execute at every opcode); a script call runs in a child_thread object whose type word holds
the parent id from bit 8; the thread is the chain's root (checked: a called function's logprint is
named after its caller's thread).

**Compiler.** BO2 has no official compiler: the tests are compiled by gsc-tool (xensik, 1.5.1,
`-m comp -g t6 -s pc32`; located beside `gsctool_src`), cached in `tools/re/out/vm_gscc_t6/`. A bare
call is an import with an empty namespace; the retail bytecode (30 objects of the decompressed
common_mp zone, read only) encodes calls to same-file functions the same way (36 such calls), so
the bare-call results below hold for Treyarch's encoding too.

## 2. Stubs (each run's JSON lists every one with its hit count)

| Stub | Why |
|---|---|
| imports | x86 stdcall stubs that pop their argument count (a table; an import missing from it stops the run with its name and a backtrace); heap, TLS, FLS, Interlocked*, VirtualAlloc handled |
| `PMem_Alloc` (0x75b860) | bump heap, never freed |
| CRT `_vsnprintf` (0xa230ad), `sprintf` (0xa23292), `va` (0x75d410) | Python formatter (CRT locale and per-thread buffers absent) |
| CRT `longjmp` (0xa28540) | direct `_JUMP_BUFFER` restore (no SEH unwind) |
| `DB_FindXAssetHeader` (0x59a120) | our ScriptParseTree `{name, len, buffer}`, any other asset not found |
| `Com_Printf` (0x6c2670), `Com_PrintError` (0x6c2790) | message recorded |
| `Com_Error` (0x6c2a50) | message recorded, the run stops (load errors end here) |
| `LoadScriptGDB` (0x7dfc50) -> 0 | opens `scriptgdb.clump` for line numbers; reached only from the link error reporter (prints "line 1" without it) |
| 0x7dfab0 not called | GScr_LoadScripts' debug-info loader (`scriptgdb.clump`) |
| gsc-tool's object name | the header name `t6\maps\...` (gsc-tool writes its output folder in front) cut to `maps\...` before loading |

## 3. Results (retail and developer 1 give the same traces): PASS 45, UNSUPPORTED 12

46 of 57 behaviours answer as T7: notify defers woken waiters (last registered first), endons kill
before a waittill wakes, lost early notifies, `wait 0` / `waittillframeend` order, last-in-first-out
same-time waits, threads run to their first wait, `x = thread f()`, a thread on a released struct
lives on, no loop guard, case-sensitive strings, struct references, `undefined + 1` non-terminal.

Differences from T7:

| Test | T6 | T7 |
|---|---|---|
| `array_copy_on_assign`, `array_assign_shared_reverse`, `array_in_struct_field` | **arrays are values, copied on write**: `b = a` then a write through either leaves the other unchanged | `b = a` shares |
| `array_param_plain(_nested)` | same answer (the callee's write copies), by another mechanism | copy at the call |
| `bare_call_local_shadows_builtin` | **a bare call reaches the builtin even when the file defines the name**; `path::f()` reaches the file's function | current namespace first |
| `bare_call_using_file_other_namespace` | a bare call reaches a function of an `#include`'d file | never another namespace |
| `hash_equality` | `#"x"` exists (GetHash); `==` on hashes or string vs hash is a script error whose result is **undefined** | same error, result false |
| `notify_string_vs_hash_event` | events must be strings: waittill/notify with a hash is a script error and the statement is skipped (the waiter runs on at once) | (no T7 test) |
| `dev_builtin_outside_devblock` | as T7: the linker refuses ("Dev only calls must be wrapped in a devblock") | same |
| `thread_call_builtin_only` | as T7: unresolved external at load | same |

UNSUPPORTED (12): the forms T6 does not have, refused by gsc-tool (`&name` parameters x3,
`childthread` x2, `endon(a, b)`, `waittill("a", "b")`, `s = waittill(...)`) or by the game's linker
(`waitframe`, `waittilltimeout`: unresolved external); and `func_ref_builtin_only` /
`func_ref_shadows_builtin`: the linker's reference case (kind 1, 0x7e0926) shares the call case's
builtin-first lookup but patches no opcode, so `::spawnstruct` becomes a function value holding the
builtin's **native handler address**; ScriptFunctionCallPointer (0x7ee91c) then runs that machine code
as bytecode (here a getdvar error on garbage, then "unhandled script vm exception error recovery
case 106"). A real game behaviour with no meaningful golden.

## 4. Static readings behind the surprising answers

- **Copy on write**: the array-reference path 0x7e9640 (every write through an array): when the
  array object's reference count is not 0, RemoveRefToObject (0x7e7cb0), Scr_AllocArray
  (0x7e58d0) and CopyArray (0x7e5e30) give the variable its own copy (0x7e9882). SafeCreateLocalVariables
  (opcode 0x17, 0x7edc7f) reads one aligned u16 name per parameter, no flag byte: no by-reference
  parameter exists.
- **Bare calls**: the import resolver (0x7e08b0): kinds 1 (reference) and 2 (call) with an empty
  namespace look up the builtins first (0x636f80 functions, 0x639530 methods), the script resolver
  0x7df5f0 (the object, then its includes) only when no builtin has the name; kind 3/5 (threads)
  only the script resolver.
- **No inline wake**: VM_Execute (0x7ed7e0) is entered only from VM_Resume (0x7f15c0, called only by
  VM_SetTime 0x7f1b60) and the three Scr_Exec*Thread; no notify path runs a waiter.
- **Developer calls**: the resolver checks the lookup's developer flag against the import's bit 0x10
  (0x7e098f).

## 5. Limits

Server VM only; no entities; the compiler is gsc-tool's; logprint's real handler is not run (an
`undefined` argument shows as `undefined`); the second measure (the user's run of the scripts in a
real BO2 session, e.g. Plutonium) is owed.
