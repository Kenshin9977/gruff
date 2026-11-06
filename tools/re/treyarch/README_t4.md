# T4 (World at War): builtin catalogue

Shared extractor for the three 32-bit Treyarch games: `treyarch/old_builtins.py` (profiles `t4`,
`t5`, `t5mp`, `t6`, `t6zm`, `t6sp`) and `treyarch/build_old.py --game t4|t5|t6`. Results:
`reports/t4_builtins.md`, `gruff-data/builtins/t4.toml`.

    py -3.11 -I treyarch/old_builtins.py extract --game t4      # about 3 minutes
    py -3.11 -I treyarch/build_old.py --game t4

Executable: `codwaw_flat.exe`, the unwrapped SP build (`CoDWaW LanFixed.exe`, same
`.data` as the Steam `CoDWaW.exe` 1.7) flattened; x86, image base 0x400000. The MP exe is
SteamStub-wrapped and not read: MP scripts are checked against the SP catalogue (the report
gives their agreement apart).

## What T4 looks like, and how each piece is found

**Static tables, names as strings.** Each entry is 12 bytes `{char *name, void (*handler)(),
int developer}`; there is no hash and no arity. Lookups (`Scr_GetFunction`-style) loop over one
table each with an inline strcmp (`mov ecx, [esi + table]` ... `add esi, 0xc; cmp esi, N*12`),
return the handler and write the developer int and the table's own name pointer through their
`(const char **pName, int *pType)` arguments. Some are composites that call the per-subsystem
lookups in turn (the GSC method lookup: 7 tables, actor, threat bias, vehicle, hud element,
helicopter... the developer flag is only returned by the last one; `*pType` is zeroed first).

**Roles from the compiler.** The script compiler is in the executable. Its two call emitters
(function call and method call) branch on the script instance (`cmp esi, 0` / `test ebx, ebx`:
0 = GSC, 1 = CSC) and call the lookups. `run_role` emulates each emitter from that branch to the
join with the instance set, calls skipped and recorded: GSC function = [threat-bias functions,
main functions], CSC function = 1 table, GSC method = composite (7 tables), CSC method = 1
table. Anchor check: the emitter's developer-call error ("return value of developer command can
not be accessed if not in a /# ... #/ comment") is referenced right after the join.

**Tables, two measures.** Each lookup is emulated with an absent name: the 4-byte reads at the
12-byte stride, grouped by the reading instruction (adjacent tables are scanned by different
loops), give each table and its count; the static loop bound gives the count again (equal for
all 11 tables). Round trip: every name is looked up again through its role's lookups and must
return its handler and its own name pointer.

**Function or method, second measure.** Method handlers are `void(scr_entref_t)` (the entref
by value: `{u16 entnum, u16 classnum}` for GSC; CSC methods also read a second word): the run
records whether the handler reads its first stack argument. Function tables: 0 of 477; method
tables: most entries (the others are stubs or ignore self).

**Developer flag.** The lookup's `*pType` is what the emitter uses: it turns it into the cached
function type (12 = developer codepos, else 7) and refuses a developer call's return value
outside `/# #/`. `dev_only` is that returned value (entries of the sub-lookups that do not
return it are not developer-only to the compiler, whatever their table field says; the evidence
says when the two differ).

**Arity: handler emulation (weakest measure).** No table holds an arity and the compiler does
not check one. `scrVmPub` is read off Scr_GetInt's bound check (`imul eax, eax, 0x4320;
cmp esi, [eax + 0x3bd471c]` = outparamcount; `mov eax, [eax + 0x3bd4710]` = top); accessors
take the instance in eax and the index in ecx here (LTCG), so they are not hooked: the real
accessors run on a fake value stack, and `Scr_Error` (message in ecx; found after the formatter
in Scr_GetInt's type-error path) and the runtime error it ends in are hooked to record the
message and *return* (continue mode). `va()` and the CRT's `_vsnprintf` are formatted in
Python; kernel32 imports the CRT needs are stubbed, any other import ends the run. Types: a run
with 16 undefined parameters names each read slot's type ("type undefined is not an int",
"cannot cast undefined to string"); the slot then gets a value of that type and the run is
repeated (up to 6 passes, the type ids from the VM's own type-name table). Then runs with N = 0..8,
12, 16, 24 decide min/max (`build_old.arity_from_runs`): a run is rejected by a missing parameter
("parameter %d does not exist" with index >= N) or the handler's own count error; min = smallest
accepted N; max = largest accepted N, or 1 + last index read when 24 is accepted (extra
arguments ignored), or variadic when that run reads index >= 16. Methods run with a fake self:
entity 0 of the arrays their entref resolver indexes, every dword pointing at a zeroed block.
