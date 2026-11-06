# T6 (Black Ops II): builtin catalogue

Same extractor as T4/T5 (`treyarch/old_builtins.py`, see `README_t4.md`), profiles `t6`
(t6mp, the primary), `t6zm` and `t6sp`; `treyarch/build_old.py --game t6` merges them. Results:
`reports/t6_builtins.md`, `gruff-data/builtins/t6.toml`.

    py -3.11 -I treyarch/old_builtins.py extract --game t6     # and t6zm, t6sp
    py -3.11 -I treyarch/build_old.py --game t6

Executables: `t6_dump_flat.exe` (t6mp unpacked, flattened) and
`t6zm.exe.unpacked.exe`, `t6sp.exe.unpacked.exe` (file layout). x86, image base
0x400000.

## What T6 looks like

**Static tables with arity.** 0x18-byte entries `{char *name, u32 id, u32 min_args,
u32 max_args, handler, u32 developer}`; max 0x7fffffff is variadic, min = max = 0xffffffff no
arity. Lookups compare names (inline strcmp) and return the handler and, through
`(const char **pName, int *pDev, int *pMin, int *pMax)`, the developer flag, min and max (the
sub-lookups of a composite that receive only pName/pMin/pMax leave the flag 0). The `id` word is
not read by the lookups: it is unique per name, ascending in alphabetical order for most names
then appended (a builtin numbering of the build pipeline, not proven further); it is only
recorded in the evidence.

**Roles from the linker.** There is no compiler; the import linker resolves each import of a
compiled script by name. Its kind (`flags & 0xf`): 2 = function call, patched to opcode 0x29,
4 = method call, patched to 0x2a (1 = function reference, 3/5 script threads). In each case
`test ebp, ebp` on the instance picks the GSC or CSC lookup; `run_role` emulates from there
(the case is anchored on its developer check, "**** Dev only calls must be wrapped in a devblock
/#...#/ : ", and on the opcode patch). The same case checks the call's argument count against
the returned min/max (below min: "Unresolved external"; above max: an error): T6 arity is the
table's, enforced at link time, no emulation needed.

**Handlers** run for parameter types only (N = min, max, max + 1). Accessors: cdecl
`Scr_GetInt(inst, index)`; VariableValue is `{type, u}` (type first, int = 7); the missing-
parameter error is inlined (the message is stored in scrVarPub's slot, then the runtime error
is called), so the runtime error is hooked too.

**Intrinsics.** gruff-data's `profiles/t6.toml` opcode_builtins (gsc-tool's arc list) plus
isdefined: measured against the linker's lookups; a name absent from every table can only be an
opcode, and is listed as `[[intrinsic]]`.
