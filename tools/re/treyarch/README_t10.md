# T10 (Black Ops 6) and T11 (Black Ops 7) builtins

One module for both games (`t10_builtins.py`, profiles `t11`, `t10`, `t10sp`) and one builder
(`build_t10.py`): the engine is the same in the three executables.

    py -3.11 -I treyarch/t10_builtins.py extract --game t11     (BO7, about 15 min with handlers)
    py -3.11 -I treyarch/t10_builtins.py extract --game t10     (BO6 multiplayer)
    py -3.11 -I treyarch/t10_builtins.py extract --game t10sp --no-handlers   (BO6 campaign)
    py -3.11 -I treyarch/build_t10.py

## What the engine looks like, and how each piece is found

**Static tables, handed out as spans.** An entry is 0x18 bytes: `u64 hash, u64 handler, u8 type,
u8 min, u8 max, 5 x 0`. The hash is the full 64-bit salted FNV-1a of the name (`t10.function`
is its low 60 bits; the top nibble takes every value). A table reaches the engine as a span
`{ptr, count}`: a getter `lea rax, table; mov [rcx], rax; mov qword [rcx+8], N; ret` (BO7, BO6
MP) or a span in .data (BO6 SP). Scanning for the layout finds runs that merge adjacent tables;
the spans are what count.

**Builtin sets.** A builder clears a set object and appends spans with one of two append functions
(`{u64 hash, entry*}` pairs into the function array at +0 or the method array, counts after the
arrays: BO7 +0x18380/+0x18384, BO6 MP +0x16a80/+0x16a84, BO6 SP +0x15180/+0x15184), then sorts
both (qsort import). Found from the code: the linker is the function that loads the
"Unresolved external" error; its two lookups are the calls whose 16-bit result it tests; their
bodies give the count and array offsets; the append functions are the `inc dword [rcx+count]`
loops; the builders are their callers, the set object the `lea rcx` before the call (inline) or
before the call to the builder. Every builder is emulated on a zeroed object (qsort, bsearch and
the memset import done in Python) and the result must equal the dump's live set, pair for pair.

**Roles.** `vm->builtins (+8) = vm->sets[GetMode()]` (BO7 `sets` at VM+0x4ae58, BO6 MP +0x10),
read off the code after the call to GetMode. VMs: the game's `{"server", "client"}` name array
and the VM pointers beside it (BO7: two VMs, BO6 MP: server only). Modes: BO7's own name table
(`none`, `sp`, `mp`); BO6's mode-info array (`sp` -> 1; `mp`, `cp`, `br`, `dmz` -> 2). Function vs
method: the method lookup tags its id `| 0x8000` and searches the second array; method handlers
take `(vm, entref*)`, functions `(vm)`. BO7: 4 sets (server sp/mp, client sp/mp). BO6 MP: 6 sets,
of which the server VM holds 2; the other 4 (no VM of the dump holds them; their hashes overlap
BO7's client sets) are reported, not written. BO6 SP: 2 sets, its own hash (name + salt, not
t10.function), report only.

**Second measures.** Each lookup run with an absent key: its single bsearch call must get exactly
the set's array, count and stride 16. Round trip: every hash of every set through the emulated
lookup; the id returned must name an element with that hash (a duplicate hash in one set:
only one is returned, the other is `shadowed`). The comparator is emulated on unsigned pairs.

**Linker checks.** Dev only: `type == 1` and the devblock error (both games). Arity: BO7 checks
`min <= nparams <= max` (u8, so max 255 admits any count: written variadic); BO6's linker has no
such path, so although its entries store min/max bytes (some with min > max) the TOML writes them
as unchecked and the evidence quotes the bytes.

**Parameter types.** No type names survive: script errors are `DEV ERROR 0x%08X` codes, so the
common error-string labelling cannot run. Scr_GetNumParam is the handlers' most called
`mov eax, [rcx+N]; ret` (N = outparamcount of the VM context at VM+0x10); the accessors are the
callees that bound-check their index against it; Scr_Error is the call after the formatter in
their error paths. Each accessor is run with one value of each tag (byte +8 of a stack value)
and labelled by the tags it accepts (`tag7`, `any`...); the handlers then run through
`common.handlers.HandlerRunner` (VM context as scrVmPub, every IAT slot pointed at a
`mov rax, rcx; ret` stub).

**Corpus.** BO7: the decompiled archive (`function_<hex>` = t10.function). BO6: none (server-side
scripts); T10 is cross-checked against T11 by hash, VM and kind.
