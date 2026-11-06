# Black Ops Cold War (T9) script VM, run offline: method, stubs, results, dialect against T8

Phase 5, step 5.6. 2026-11-06, 16:10-17:08 (one agent, after T8). Dump:
`BlackOpsColdWar_dump.exe` (PE timestamp 1778629159; sha256 in `gruff-data/semantics/t9.toml`).
Nothing was launched or attached; Cold War cannot be run (Ricochet): the second measure is the
static reading of the handlers. Addresses are RVAs of this dump. Facts:
`gruff-data/semantics/t9.toml`; scripts: `conformance/t9/` (the T8 scripts, same syntax).

## 1. What runs

The same harness as T8 (`tools/re/vm/t89vm.py`, profile `t9`), the same steps:

| Step | Entry (RVA) | Notes |
|---|---|---|
| `MT_Init` | replicated | obfuscated in this dump: its tail is a chunk in the second `.text` (0x1befa645) reached through a pushed address. The harness writes what it writes (both pool pointers 0xfed8d78/0xfed8d70 = the static pool 0xeed8cd0, flag byte 0xfed8d80) and calls its tail `MT_Alloc(0x10, 0)` (0x1b5da20) |
| `ScrVar_Init(0)` | `0x1b88040` | paged variables: 32 pages of 0x2222 variables ("pg%d%d"), page table 0x10022760 (stride 0x228) |
| `Scr_InitVM(0)`, `Scr_StartVM(0, 0, 0)` | `0x1bb4e60`, `0x1bb4f70` | same globals as T8 |
| `Scr_BeginLoadScripts`, `Scr_GscLink(0, hash, 0)`, `Scr_EndLoadScripts(0, 1)` | `0x1b5d4c0`, `0x1b60320`, `0x1b5d580` | Scr_GscLink and parts of the loader are Arxan-obfuscated (an opaque `jns` at the entry, chunks in the second `.text`); they run as they are under Unicorn |
| asset lookup | `0xb1e78f0` (hooked) | type 0x44, the name hash by value; ScriptParseTree `{u64 name, buffer, len}` |
| `Scr_ExecThread`, `VM_SetTime`, `Scr_RunCurrentThreads` | `0x1bb0b60`, `0x1bb4c90`, `0x1bb77f0` | G_RunFrame (0x64cc4f0): SetTime, RunCurrentThreads, ..., RunCurrentThreads (0x64def77) |
| trace | `iprintln` handler; `ScrVm_Error` `0x1bb06e0` | scrVmPub[inst] at 0x10022bc0 (stride 0x1cbe70): time +0xa804c, frame +0xa8050, and a pointer (+0xa8058) to the stack block: top +8, function_stack_t +0x20, outparamcount +0xe96c |

T9 VM type numbering has `finalization` at 7 (the game's own type-name table, 0xe773570), so
every type after `int` is one higher than T8's.

## 2. Stubs beyond T8's

| Stub | Why |
|---|---|
| `MT_Init` | replicated (see above), logged |
| memory allocators `0x9f32160`, `0x9f32260` -> bump heap; pool `reserve` `0x9f32130` -> 1 | ScrVar_Init allocates from a "scrmem" pool of the "main" allocator, set up at engine start (absent: `Sys_Error 0x8af19f48 ,scrmem,main`) |
| `__isa_available` (0xece0158) = 1 | 5 (AVX2) in the dump, taken from the running game; memmove (0xd450b40) would use AVX, which Unicorn lacks |
| CRT `vsnprintf_s` / `vsprintf` thunks (0xd46906c, 0xd469314), `longjmp` (0xd451410) | as T8 |
| `SL_Obfuscate` (0xc8704a0) -> plain copy | as T8 |
| strings of the test objects untagged before loading | acts writes each Cold War string as `{0x8b, length + 1, 0, text in clear}`; the game copies a tagged string with strcpy (the 0 byte cuts it) and decodes it with one of 64 ciphers (0xc86bc30), which turns acts' clear text into an empty string. `T89VM.untag` rewrites the tags as clear text (the same object acts writes for T8). This is acts' encoding not matching this build, not the VM |
| `Com_Printf` (0x56b5e40, a no-op), `Sys_Error` (0xcb27920), `Com_Error` (0x56463a0) | as T8 |

## 3. Results (both modes: PASS 57)

`dialect` compares with T8: 39 answers are T8's, 18 differ:

| Test(s) | T9 | T8 (and T7) |
|---|---|---|
| `notify_wakes_waiter_before_notifier_continues`, `notify_wake_order_two_waiters`, `notify_reentrant` and 10 others whose order changes | **notify runs each woken waiter inside the call**, before the notifier's next statement; a notify inside a woken waiter nests (got a, got b, after notify b, after notify a) | queued, run after the notifier yields |
| `notify_wake_order_three_waiters` | still c, b, a | same order |
| `waittilltimeout` | a 100 ms timeout fires after **2** frames | 3 frames |
| `thread_call_assign_result` | `x = thread f()` is a value of type 7 (`finalization`), defined even when f waits; `"y=" + y` is a script error (#12e690eb) giving undefined | f's return value / undefined |
| `array_ref_param_non_array` | a non-array to `&name` is accepted silently (`x=5`) | script error, recovery loop |
| `undefined_arithmetic` | non-terminal error #9ad8e396, the thread goes on (both modes) | T8: Com_Error(0x20) on every error |

The rest is T8's: FIFO same-time waits, childthread copies endons, hash/string event identity,
waittill struct results, positional notify values dropped, payload object as result, hash
equality without error, array copy rules, bare-call and `&f` resolution, fatal `Sys_Error
0x6394f836` on unresolved externals and on a dev call outside a devblock, no loop guard.

## 4. Static readings

- **Notify runs waiters inline**: the Notify handler (0x1b70290; acts' t9_38 map, 99.37 %, with
  the main-loop tables 0xe773830/0xe783830) calls `VM_Notify` (0x1bb5b10), which calls
  `VM_Resume` (0x1bb76f0 -> `VM_Execute` 0x1bb0df0) for each woken waiter, and `Scr_ExecThread`
  (0x1bb0b60) for callbacks. T8's VM_Notify (0x2774090) only queues the waiters (0x2774ca0).
- **Timeouts**: VM_SetTime (0x1bb4c90) fires a timeout when time >= deadline (`comisd` + `jb` at
  0x1bb4dff); T8 uses `jbe` (strictly after).
- **Script errors**: the reporter (0x1bb78e0) calls Com_Error (0x56463a0) only on the terminal
  path (`test bl` at 0x1bb7c5c); ScrVm_Error (0x1bb06e0) then longjmps into VM_Execute's
  recovery, as T7. T8's reporter calls Com_Error(0x20) for every error.

## 5. Limits

As T8 (server VM only, no entities, acts as the compiler, error texts as codes), plus: the
obfuscated code ran unchanged, but MT_Init is replicated rather than run, and the test objects'
strings are untagged before loading. The `thread_call_assign_result` value type (`finalization`)
is the game's own type name; what that value is (a thread handle?) is not read.
