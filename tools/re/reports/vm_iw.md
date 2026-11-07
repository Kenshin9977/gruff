# The x64 IW script VMs (MW3, Ghosts, AW, MWR, IW), run offline; MW2 not run

2026-11-07, 00:48-02:09 (one agent). Five games' script VMs run offline under Unicorn from their
x64 executables or dumps: Modern Warfare 3 (`iw5mp.exe`, the Steam x64 re-release), Ghosts
(`iw6sp64` dump, single-player: no MP dump exists), Advanced Warfare (`s1_mp64_ship` dump),
Modern Warfare Remastered (`h1_mp64_ship` dump) and Infinite Warfare (`iw7_ship` dump). Nothing
was launched or attached. Addresses are RVAs; each profile in `vm/iwvm.py` lists them with how
they were found. Facts: `gruff-data/semantics/{iw5,iw6,s1,h1,iw7}.toml`; scripts:
`conformance/{iw5,iw6,s1,h1,iw7}/`. Modern Warfare 2 is not run (section 5).

```sh
py -3.11 -I tools/re/vm/iw_run.py --game iw5 conformance/iw5/notify_reentrant.gsc [--verbose]
py -3.11 -I tools/re/vm/iw_suite.py --game iw5|iw6|s1|h1|iw7 [--jobs 2]
py -3.11 -I tools/re/vm/iw_facts.py --game iw5|iw6|s1|h1|iw7
```

## 1. What runs

The same chain in every game, found from the strings the builds keep ("script stack overflow
(too many embedded function calls)" in VM_Execute, "%s.gsc" in Scr_LoadScriptInternal, "Could
not find script '%s'" in the loader, "unknown function" in the linker, "loading_sre_fatal" in the
VM init), then by the callers and callees of those functions, decompiled with a headless Ghidra
(no analysis on the dumps: the functions are created and decompiled on demand).

| Step | What the game's own code does |
|---|---|
| init | Com_Init's order: SL_Init, the hunk, Scr_InitHunkUsers, Scr_InitVariables (the object pools), Scr_Init (VM stack, function frames, the error jmp_buf depth) |
| load | GScr_LoadScripts' calls: Scr_BeginLoadScripts (the builtin tables registered), Scr_LoadScript(name) -> the loader reads the ScriptFile asset (`DB_FindXAssetHeader(type, name)` hooked: our gsc-tool output as `{name, compressedLen, len, bytecodeLen, buffer, bytecode}`), inflates the stack, walks the bytecode (strings to SL ids, far calls recorded), loads the #include'd files the same way, links (`unknown function` if a far call has no target); Scr_GetFunctionHandle(name, id of `main`); Scr_EndLoadScripts |
| G_InitGame | Scr_InitSystem (time array, level, ...; it runs VM_SetTime's loop once, so the level starts at time 1), then Scr_ExecThread(handle, level, 0) and the thread id freed |
| frames | G_RunFrame with no entities: Scr_RunCurrentThreads before and after the entity loop, then VM_SetTime (the threads waiting on this time, then time + 1) |
| trace | `logprint`'s slot in the builtin function table pointed at a stub of ours (the retail handler is an empty function shared by every stripped builtin, so the handler itself cannot be hooked); script errors at VM_ExecuteInternal's setjmp landing |

The bytecode lives in the 2 MiB block whose base Scr_BeginLoadScripts reads as programBuffer
(where the fastfile loader puts script bytecode): Scr_GetFunctionHandle only returns code
positions inside it, so the test bytecode is placed there.

Threads: T0 the entry thread, T1... in creation order (the return of AllocThread and of the
childthread allocation; OP_endon also calls AllocThread for its record, and a childthread copies
its starter's endons the same way: those returns are left out, as T6's 0x7ef673 was). The running
thread is the root of the localId chain (child objects of type 0x12, parent above bit 8).
`f<n>` is scrVarPub.time counted from the entry thread's start.

**Compiler.** None of these games ships a compiler; gsc-tool (1.5.1) compiles the tests
(`-g iw5 -s pc64`, `-g iw6|s1|h1|iw7 -s pc`), helpers first (it reads an include's compiled
file). The game's loader and linker then do their own work on that output.

**No error text.** The x64 builds keep no runtime error messages. A script error is recorded at
the landing with the opcode that raised it, named from gsc-tool's opcode table of the game:
`script error: OP_plus (opcode 0x5b)`.

## 2. Stubs and replications (each run's JSON lists every one with its hit count)

| Game | What the dump or build needs |
|---|---|
| all | imports logged (VirtualAlloc gives heap pages), `DB_FindXAssetHeader`/`DB_IsXAssetDefault` for ScriptFile only, inflate in Python (zlib), CRT longjmp as a direct `_JUMP_BUFFER` restore, Com_Error stops the run, timeGetTime / Sys_Milliseconds as a virtual clock (+1 ms a read, +50 ms a frame) |
| iw6 | the dynamic CRT (VCRUNTIME140 and the UCRT api sets): setjmp/longjmp, mem*/str*, `__stdio_common_vsprintf`, floor... in Python (`vm/pycrt.py`); no Scr_EndLoadScripts in the build (inlined in Scr_FreeScripts): its steps replicated; SL_Init and the hunk set-up are Arxan stubs that run (1-4 s) |
| s1 | the VM init's Dvar_RegisterBool returns a zeroed dvar (no dvar system); the static CRT's `_getptd_noexit` returns one zeroed block (its FLS pointers are the running process's) |
| h1 | SL_Init and the hunk set-up are encrypted at rest in the dump (invalid code): not run, their state is the dump's; the script-string memory tree, full in the dump, is reset as IW5's MT_Init does it (bitmap, size masks, search starts, at this build's addresses read off MT_AllocIndex); Hunk_AllocateTempMemory and the CRT sprintf/Q_vsnprintf in Python |
| iw7 | as h1 (memory tree reset at 0x65b9600); the hunk users made by the harness; the builtin registration goes through a run-time interface object: skipped, the dump's tables restored and every handler of phase 2's static extraction (`iw/out/iw7.json`) written in; Scr_LoadScript's temporary block from the harness's heap |

## 3. Results

| Game | PASS | UNSUPPORTED | Differs from the previous game |
|---|---|---|---|
| iw5 | 44 | 13 | vs IW3: childthread exists; the loop guard (below); bare-call resolution not measured |
| iw6 | 44 | 13 | vs IW5: none (opcode numbering only) |
| s1 | 45 | 12 | vs IW6: `waitframe()` exists (a statement, one frame) |
| h1 | 45 | 12 | vs S1: none |
| iw7 | 44 | 13 | vs S1: no `waitframe` (gsc-tool refuses it) |

Every runtime behaviour is IW3's and T7's: notify only queues the woken threads (they run once
the notifier yields, in the same frame, last registered first); same-duration waits wake last
registered first; `wait 0` resumes in the same frame; `waittillframeend` runs after the threads
that do not wait; a notify before the waittill is lost; endons, waittillmatch, `x = thread f()`
(f's return value when f ends without waiting, undefined once it waits), arrays copy on write
(a callee or another holder never sees a write), structs by reference, undefined + 1 a
non-terminal error, string `==` case-sensitive.

- **childthread** (IW5 on): the child does not stop when its starter ends; it inherits the
  endons active in the starter (the childthread allocation copies the starter's endon entries).
- **Loop guard**: OP_jumpback compares the time since VM_Execute's start with **5000 ms** (IW3:
  2500); past it, with the script debugger off, the thread is unwound **silently** (no error,
  nothing printed: the build has no text), and the VM goes on.
- **Name resolution is not measured.** The x64 IW bytecode resolves at compile time: a builtin
  call carries the builtin's id, a local call a code position, only a far call (`file::f`, or a
  call into an #include'd file) is linked by the game. The retail compiler is not available, and
  gsc-tool refuses a script function named like a builtin, so the bare-call tests are
  `unsupported`; `bare_call_resolution` stays as documented in the profiles. What is measured: a
  bare call to a function only an #include'd file defines reaches it (far call, linked by the
  game), a threaded call never reaches a builtin (no thread-call opcode takes a builtin), and
  `::builtin` is a builtin function value that `[[ ]]()` refuses (OP_ScriptFunctionCallPointer).

## 4. Profiles and Gruff

Profiles iw5, iw6, s1, h1, iw7 now carry, measured in their VMs (passing behaviours only):
childthread, waittillframeend, waittill_return_value = false, waittill_multiple_events = false,
array_argument_passing = copy, notify_wakes_waiters = deferred, thread_call_value =
callee-result, and waitframe = true for s1 and h1.

Gruff on the decompiled corpora (`gruff check --select ALL --preview`, before and after): the only
change is V002 thread-call-value, now active in these games: MW3 68, Ghosts 51, AW 52, MWR 6,
IW 68 findings. No other count moves (T001-T004, B002 and the rest are identical).

## 5. Modern Warfare 2 (IW4): not run

The Steam x64 re-release of MW2 has no GSC compiler and no script loader: every script is
compiled ahead of time into the executable. Scr_BeginLoadScripts sets programBuffer to a static
block of `.rdata` (0x505400), GScr_LoadScripts takes its function handles in order from a table
(0x505280, read by 0x215050) instead of loading anything, and the builtin tables keep no names
(its catalogue is named only by IW5 handler matches; `spawnstruct` and `logprint` are not among
them). The VM (VM_ExecuteInternal 0x21dfe0, VM_Notify 0x2210b0, Scr_ExecThread 0x21bfb0) uses
IW5's opcode numbering but other operand layouts (CreateLocalVariable takes a u16 name, for
one), so gsc-tool's iw5 output does not run as is. Running the suite would need an IW4-format
assembler and the builtin indices of the names the tests call; MW2's profile keeps its
assumptions.

## 6. Limits

- One mode only (retail, script debugger off): no developer switch changes these behaviours, so
  `modes_agree` is true by construction.
- The compiler is gsc-tool: compile-time questions are about gsc-tool, not the retail compiler.
- Ghosts is the single-player executable; AW and MWR the multiplayer dumps; IW's dump serves every
  mode.
- H1 and IW7 run on dumps whose string table keeps the running game's strings; the memory tree
  reset frees their storage too. Every test whose trace prints or compares strings gives the same
  answer as in S1, IW5 and IW6, which are reset by their own code.
- The second measure (a run in the game) is the user's, not done here.
