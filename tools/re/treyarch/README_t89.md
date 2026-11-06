# T8 (Black Ops 4) and T9 (Black Ops Cold War)

One profile module for both engines, `treyarch/t89_builtins.py` (a `PROFILES` dict, every value
checked at run time), and one builder, `treyarch/build_t89.py`:

    py -3.11 -I treyarch/t89_builtins.py extract --game t8   # BlackOps4_dump.exe
    py -3.11 -I treyarch/t89_builtins.py extract --game t9   # ...\BlackOpsColdWar_dump.exe
    py -3.11 -I treyarch/build_t89.py --game t8
    py -3.11 -I treyarch/build_t89.py --game t9

Outputs: `out/<game>_extract.json` (+ `.handlers.jsonl`, the handler runs), `reports/<game>_builtins.md`,
`gruff-data/builtins/<game>.toml`. Results are in the reports.

## What the engines look like, and how each piece is found

**Static tables.** The entries are T7's 0x20-byte layout, `{u32 hash, u32 min, u32 max, u32 0,
u64 handler, u32 type, u32 0}` (`common.tables.LAYOUT_T7`), but in `.rdata` as compiled: the
hash (`t89.scr` of the lower-case name) is already there, so nothing is emulated at init.
`type` is 0 or 1 (1: dev-only); max 0x7FFFFFFF is variadic, as in T7.

**Getters.** Each table has a small leaf getter (no `.pdata`): `getter(hash, &type, &min, &max)
-> handler`, a linear scan with a `cmp eax, N` bound (the static measure), or, for a few tables,
a scan bounded by a count stored right after the table (registered tables). The large tables
are not scanned: the lookup calls the CRT `bsearch` on an `int` index array (size 4, comparator
`table[idx].hash` vs the key). **Those index arrays are zero in both dumps** (filled at run time:
in T9 by qsort initialisers that iota-fill then sort; in T8 no writer was found by any xref).
The bsearch is hooked: its comparator's `lea` names the table, its count the size; an all-zero
array is filled with the hash order of its table (the only order the comparator accepts), an
already filled one must equal it or the extraction stops.

**Roles.** The VM value reader (deserialising a value of type `api_function`: 11 in T8, 12 in
T9) reads a one-byte is-function flag and the hash, then calls one of four lookups chosen by
`inst == 0` (GSC) and the flag: exactly T7's shape, so `t7_builtins.find_roles` is reused with
the reader and its stream read as the profile's anchors. Each lookup is a chain: a few getters,
one or two bsearches, more getters; the GSC and CSC function lookups share the BG tables.

**Tables of a lookup.** The lookup is emulated with hash 0 (absent): the getters' 4-byte reads
at the 0x20 stride, split into runs in time order (two adjacent tables read by two getters are
two runs), give the linear tables; reads made inside bsearch or a comparator are not counted;
the hooked bsearch calls give the others, in call order. Second measure: the static loop bound,
where there is one, and the run of entry-shaped bytes at the table address (equal to the count,
or longer when the next table follows in `.rdata`). Third: every entry looked up again through
the engine (handler, min, max must come back; a hash in two tables of one lookup is returned
from the first only and the second is kept, `shadowed`). Every run of the static scan
(`common.tables.scan_static`) with type words 0/1 is covered by the traced tables.

**scrVmPub** is read off the animtree parameter accessor, the one accessor that still loads its
message text (`expected a animtree name, got %s, param %d`): `imul r, inst, stride`, then the
`cmp edx, [...]` bound against outparamcount and the `top` load. T8: scrVmPub[inst] is image
data (top at rva 0x83134e8, outparamcount at +0x8818, stride 0x16868). T9: a pointer per inst
(slot at rva 0x100cac18, stride 0x1cbe70) to a structure with top at +8 and outparamcount at
+0xe96c; the handler runner points it at a fake structure. The same accessor loads the VM's
type-name table (`undefined, pointer, string, vector, hash, float, int, ...`).

**Parameter accessors.** The error texts are hashed (`mov ecx, <message id>; call Scr_Error`),
so `common.handlers.find_accessors` (by strings) finds nothing. An accessor here is a handler
callee whose first instructions compare the index (edx) with outparamcount (plus short wrappers
of one); Scr_Error is the call made with a message id in ecx by at least three accessors. Each
accessor is then run alone with a value of every VM type in slot 0: the types it returns on are
its label (`int`, `float|int`, `vector`, `string|...`), named by the game's own type table.
Handlers are `void(inst)` and `void(inst, scr_entref_t *)` (the method's entref is a 16-byte
value passed by pointer); the entref resolvers are the first call of many method handlers.

**Crashes.** Unicorn crashes the host process (access violation inside the emulator) on a few
obfuscated handlers. The handler runs therefore go in child processes; the run in flight when
one dies is recorded as `emulator crash` (no types) and a new child goes on.

## Names and intrinsics

Names: a hash is named only by a string that hashes to it with `t89.scr`. Candidates, in
attribution order: the T7 catalogue's names (keeping the T7 entry's own `name_source`), the
identifiers of the game's decompiled corpus, the BO4 glossary, GrayWolf's `.wni` name indexes,
cod-hash-recovery, the local hash lists (`hash_lists` in `paths.local.json`; `hash-list`); each source is also counted alone.
With a 32-bit hash and millions of candidates a few chance matches are expected: the report
gives the estimate.

Intrinsics: acts writes the opcodes by name and almost every table builtin as `function_<hash>`
(a few methods it knows, delete, hide, linkto..., by name). The candidate opcode names (T7's list
plus the wait/cast forms) that the corpus calls and that have no table entry of the kind they are
called as are listed as `[[intrinsic]]`, with their call sites counted apart.
