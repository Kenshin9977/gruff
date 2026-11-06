# Call of Duty 4 (IW3) script VM, run offline: method, stubs, results, dialect against T4

Phase 5.7, part 2. 2026-11-06, 18:02-18:10 (one agent, after T4, `vm_t4.md`). Executable:
`iw3mp.exe`, the Steam multiplayer build ("CoD4 MP 1.8 build win-x86", PE timestamp 1525213173,
file layout, image base 0x400000; sha256 in `gruff-data/semantics/iw3.toml`). Nothing was launched
or attached. Addresses are VAs. Facts: `gruff-data/semantics/iw3.toml`; scripts: `conformance/iw3/`
(the T4 scripts with `logprint`).

## 1. What runs

The World at War harness (`vm/t4vm.py` `OldVM`) with the IW3 profile (`vm/iw3vm.py`): WaW's VM is
CoD4's with a second instance added, so every piece has a counterpart. IW3 has one VM instance:
the functions take no inst argument.

| Step | Entry (VA) | Notes |
|---|---|---|
| CRT, engine | `_cfltcvt_init` 0x6730cc, `Sys_InitMainThread` 0x505d70, `Com_InitHunkMemory` 0x55e060 | Hunk_InitDebugMemory is inlined in Com_Init (0x4f9e14-0x4f9e81): replicated |
| init | `SL_Init` 0x512a20, `InitVariableRange(edi, eax)` 0x513c70 for [1, 0x8001) and [0x8002, 0x18000) (Com_Init 0x4fa198), `Scr_Init` 0x517c90, `Scr_InitSystem` 0x51d850 (G_InitGame 0x4ba69f) | gameId is not allocated (G_InitGame's inline code around it is not run; no test uses `game`) |
| load | `Scr_BeginLoadScripts` 0x510a60, `Scr_LoadScript(ecx)` 0x510e60 (compiler as T4), `Scr_GetFunctionHandle(file, name)` 0x510880, `Scr_EndLoadScripts` 0x510e80 | GScr_LoadScripts 0x4c5bd0 |
| run | `Scr_ExecThread(eax = handle)` 0x51d560 then the id freed (0x514a40), as G_InitGame 0x4ba771; per frame: G_RunFrame (0x4bb3f0) runs Scr_RunCurrentThreads' body inline twice (0x4bb6a1, 0x4bb7c6), then 0x516820 and `time += 1` (0x4bb87c): the harness calls the same body as a function (0x51d7e0) twice, then 0x516820, and replicates the two instructions of the increment | |
| trace | `logprint` (0x4ca1b0) and `println` (0x4c60f0) handlers as recorders (top 0x178c068, outparamcount 0x178c074); `Scr_ErrorInternal` 0x517cf0 (error_message 0x15c261c, terminal 0x178c06e); the loop guard's kill at 0x51a49e | |

Threads: the thread-call opcodes 0x54-0x57 (table 0x51b7dc) and the Exec APIs type a new thread
variable 0x6e (thread is type 14 here: the type-name table 0x71c980 has `include codepos` at 13,
which T4 dropped); a script call runs in a child thread, type 17, its caller's frame above bit 8.
`f<n>` is scrVarPub.time (0x15c2624).

```sh
py -3.11 -I tools/re/vm/old_run.py --game iw3 conformance/iw3/notify_reentrant.gsc
py -3.11 -I tools/re/vm/old_suite.py --game iw3 [--developer]
py -3.11 -I tools/re/vm/old_facts.py --game iw3
```

## 2. Stubs and replications

The T4 list (`vm_t4.md` section 2) at IW3's addresses: imports (stdcall stubs, virtual clock),
`FS_FOpenFileRead`/`FS_Read`/`FS_FCloseFile` (0x556020, 0x5567e0, 0x555a70), `_vsnprintf`
(0x669185), `Com_PrintMessage` (0x4f75e0), `Com_Error` (0x4f7ed0) and `Sys_Error` (0x571140) stop
the run, CRT `longjmp` (0x66b0dc) as a direct restore, `_getptd_noexit` (0x66f58a) as one block
filled as `_initptd` (0x66f4cb) fills it, `_osplatform`/`_winmajor` (0x7223a8, 0x7223b4), `_lock`
(0x671593) a no-op, Scr_Settings' three bytes (developer 0x15c2616, developer_script 0x15c2617,
abort_on_error 0x178c06d), plus two IW3-only replications: the inlined Hunk_InitDebugMemory and
the frame's `time += 1`. No null-page write in the suite.

## 3. Results (retail: PASS 46, UNSUPPORTED 11; developer: 3 traces differ)

**Every retail golden is T4's** (`vs t4: same` for all 57; the 11 UNSUPPORTED forms, `childthread`
included, are refused by both compilers with the same messages). So against T7, IW3 differs exactly where T4 does: arrays
are values (copy on write), `#include` merges, a bare call takes the same file's function, then the
builtin, then the #include'd files; `::f` and `thread f()` never reach a builtin (`profiles/iw3.toml`:
`bare_call_resolution = "file-first"`; 1,554 `::f` references of the CoD4 scripts name a function
only an `#include`d file defines); a dev builtin
outside `/# #/` is compiled away in retail; a 2.5 s wall-clock loop guard kills the thread. The
scheduler (deferred notify, last-in-first-out wakes and same-time waits, `wait 0`,
`waittillframeend`, endons) is T7's and T4's.

The one difference found between T4 and IW3 is in developer mode: the loop guard's error is
**terminal** in IW3 and **not terminal** in T4 (both then end in Com_Error through abort_on_error).
IW3 sets terminal_error and fills error_message itself (0x51a573); T4 sets the flag, then calls
its Scr_Error (0x69ab70), which stores its terminal argument (0) over it.

Static readings (the second measure; addresses in each test's `// evidence:`): copy on write in the
array-ref evaluator (0x516ff0, reference-count test 0x516fb5, new array 0x514990, copy 0x515040);
the call emitter (0x50ad90) looks in the current file's functions first (0x50ac10), then the
builtins (cache 0x14e1078, lookup 0x4d2f20); dev builtin bracket 0x50ac70 / 0x50af4c; VM_Notify
(0x51c760) never runs a thread, it queues at scrVarPub.time; OP_jumpback 0x51a3d9 (2500 ms).

## 4. Limits

As T4: server VM only, no entities, level and structs; MP executable (logprint exists); no trace
mode; the user's own run is the missing second measure; the gameId object is absent.
