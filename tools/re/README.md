# tools/re: engine builtin catalogues from the game executable

Phase 1 of the RE track: the builtin functions and methods each engine exposes to GSC, taken from
the game's own code, never from a running game. The first engine done is Black Ops III (T7).

The method follows GrayWolf's `docs/new-game-methodology.md`: the game's code is the only
reference, it is run under Unicorn rather than read, community offsets (boiii-free) are
hypotheses, every fact has two independent measures, and a step that cannot prove its result
stops with the reason instead of dropping entries silently.

## Layout

- `common/`: the engine-neutral module, shared by every family (Treyarch here, IW in `iw/`).
  Its API is kept stable; additions only.
- `treyarch/`: one small profile and builder per Treyarch game, plus what is truly that engine's.
- `out/`: extraction JSON (addresses only, not committed). `reports/`: the generated reports.

| File | Role |
|---|---|
| `common/image.py` | PE image, x86 or x64, flat (Cordycep dump flattened) or file layout (sections placed at their RVA in memory); `.pdata` functions, function bodies (x86: linear sweep to the last `ret`), strings a function loads, rip-relative / absolute / rel32 cross-reference scans (numpy). |
| `common/emu.py` | Unicorn harness for both modes: the image at its base, a stack, a fake TEB/PEB (x64 `gs:`, x86 `fs:` through a GDT entry), a bump heap, `call(va, args)`, and the hook helpers `arg(i)` / `ret(rax)` / `map_on_unmapped()`. Pattern from graywolf-tools `zonekit/emulator.py`, reduced. |
| `common/hashes.py` | `t7.canon`, `t89.scr`, `t8.fnv64`, `t10.function` in Python; same values as `gruff hash <family>`. |
| `common/tables.py` | Entry layouts, the static table scan, a lookup's reads with an absent key, the static loop bound, the emulation of static initialisers. |
| `common/handlers.py` | Accessor discovery by error strings, Scr_Error, the handler runner (N parameters on a fake VM stack), `param_types`. |
| `common/corpus.py` | Call-site argument counts of a script corpus by name, syntax kind and file type (counts only); `function_<hex>` names of decompiled corpora are matched by hash. |
| `common/names.py` | Naming hashes from candidate strings, re-hashed (a list's claimed hash is never trusted). |
| `common/report.py` | The builtins TOML writer, the call-site validation and the shared Markdown sections. |
| `treyarch/t7_builtins.py` | T7 `extract`: the catalogue, the lookups' roles, the per-entry round trip and the handler runs, to `out/t7_extract.json` (regenerated in about 10 minutes). |
| `treyarch/officialdoc.py` | Parser of `docs_modtools/bo3_scriptapifunctions.htm`, read locally, used only as an oracle: names, method or not, mandatory/optional counts, client/server. |
| `treyarch/build_t7.py` | Names, validation, writes `gruff-data/builtins/t7.toml` and `reports/t7_builtins.md`. |
| `calibrate.py` | Run after any build: `[coverage]`, the language intrinsics, intrinsic arities and the corpus second measure of handler-measured arities (`reports/calibrate.md`). |
| `types_corpus.py` | Run after any build too: the corpus measure of the parameter types (`gruff types`: literals, known builtin results, local flow, script parameters to a fixpoint), written as `corpus_arg_types`/`corpus_param_types` beside the handler's `param_types`, which it never changes (`reports/types_<game>.md`: coverage, agreement, disagreements). |
| `merge_reports.py` | Community corpus reports (`gruff report`) into drafts under `gruff-data/community/<game>/` (builtins with `confidence = "corpus"`, events, profile proposals; provenance per entry, measured entries never overwritten); `--standalone --compare` writes `reports/community_<game>.md`, the quality estimate (see `docs/community-reports.md`). |
| `common/paths.py` | This machine's paths (`paths.local.json`, `GSC_RE_<KEY>`); see below. |

Run everything from `tools/re` with `py -3.11 -I` (isolated mode: no script or current directory
on `sys.path`; each script adds `tools/re` itself, and `common.image.user_site()` the user
site-packages for unicorn, capstone and numpy):

    py -3.11 -I treyarch/t7_builtins.py extract      # --exe PATH, or exe.t7 in paths.local.json
    py -3.11 -I treyarch/build_t7.py

## This machine's paths

No script holds a path of this machine. Executables, dumps, name lists and the BO7 corpus are
looked up by key in `common/paths.py`: the environment variable `GSC_RE_<KEY>` first (`exe.t7`
-> `GSC_RE_EXE_T7`), then `tools/re/paths.local.json` (gitignored), then a default for the two
folders that sit beside Gruff (`corpus`, `gsc_data`). Copy `paths.example.json` to
`paths.local.json` and set your own paths; an `--exe` argument still wins. A script that needs
a key nobody set stops and names it. Reports and catalogues record the executable's file name
and PE timestamp, never its path.

## Compiler intrinsics (T7)

`isdefined`, `vectorscale`, `gettime`, the `getdvar*` family, `abs`, the `anglesto*` family,
`angleclamp180`, `vectortoangles`, `getfirstarraykey`/`getnextarraykey`, `profilestart`/`stop`
and `waitrealtime` are compiled to one VM opcode each by the arc compiler (gsc-tool and acts
name the opcodes, `OP_VectorScale`, `OP_GetTime`...): the VM never looks them up. Measured in the
executable: `isdefined`, `vectorscale`, `getdvar`, the four `getdvarcolor*`, `profilestart`,
`profilestop` and `waitrealtime` have no entry in any of the four lookups; `gettime` has one in
the GSC function lookup only (so CSC `gettime` exists only as the opcode); the others also have a
table entry in both VMs. They are listed as `[[intrinsic]]` in `t7.toml`, apart from the
builtins, and the call-site check counts them separately. (gruff-data `profiles/t7.toml` already
lists them as `opcode_builtins`, which `gruff check` loads; a consumer of `builtins/t7.toml`
alone, such as `gruff index`, has to read either.)

## What T7 looks like, and how each piece is found

**The tables are built at static-init time.** In the dump every builtin entry is zeros: the
compiler did not fold the name hashes, so a dynamic initialiser lower-cases each name, hashes it
(FNV-1a, `imul r, r, 0x1000193`) and stores `{canon, min, max, handler, type}`. Searching the
image for `t7.canon("getentarray")` finds nothing; emulating the initialisers is what produces the
tables. `init_tables` emulates every `.pdata` function containing the FNV multiply (88; about 50 run to
their return, the others are not initialisers and fault early) and records every write.

Entry layout (0x20 bytes): `u32 canon, u32 min_args, u32 max_args, u32 0, u64 handler, u32 type,
u32 0`. min/max are enforced at link time (the linker's "Too many parameters" error); max
0x7FFFFFFF is variadic. type 1 marks the debug builtins (dev-only), 2 the `__protected__` ones.

**The four lookups and their roles.** The VM's value reader (the loader of a serialised variable)
has a case that reads a one-byte "is function" flag and a canon id, then calls one of four
lookups chosen by `inst == 1` (CSC) and the flag. `find_roles` emulates that reader for every
value type, inst and flag with the stream read hooked, and records which lookup each case calls:
the roles come from the game's code, not from the tables' contents.

**Each lookup's tables.** A lookup checks an inline table, then calls per-table getters (CSC
function: 10 tables; GSC method: 11). `trace_lookup` emulates the lookup with canon 0 (absent):
it scans every table it owns, and its 4-byte reads at a 32-byte stride give each table's address
and exact entry count, in lookup order. A run of reads that is not entry-shaped, or a table that
no initialiser wrote, stops the extraction.

Second measure of each table's size: the static loop bound (`lea r, [rip+table] ... cmp eax,
N`), decoded with capstone. Third: every entry is looked up again by its hash through the engine
(`round_trip`); the handler, min, max and type returned must be the entry's. A hash that sits in
two tables of one lookup is returned from the first only; the second is kept and marked
`shadowed`, not dropped.

**Parameter types.** `find_accessors` takes the functions the handlers call and keeps those that
raise "parameter %d does not exist" or a "type %s is not a X, param %d" error (their type is X),
plus short wrappers of them (Scr_GetString = GetConstString + convert), and the entref resolvers
("not an entity"). `HandlerRunner` runs each handler with `scrVmPub[inst].outparamcount = N`
(the address read off Scr_GetInt's own bound check), accessors hooked at entry (index recorded, a
neutral value returned), the first megabyte mapped as zeros so null-plus-offset reads continue,
unmapped pages mapped on demand, and Scr_Error (the call that follows the message formatter in
the accessors' error paths) stopping the run. N = min(max, 16), then N = min: the indices read
in each run, the accessor per index. This is the weakest measure: a handler that validates a
value (a dvar name that must exist) errors out before its later parameters, and a handler that
calls into another DLL stops there. `param_types` is absent rather than guessed when nothing was
read.

**Entity parameters and a believable `self`** (`treyarch/t7_self.py`). Two of the "not an entity"
functions looked alike: `0x15f5a70` takes a method's entref (a 16-byte struct passed by pointer:
entnum at +0, class at +8) and is the resolver; `0x1b2fc30(index)` calls the bound-checked value
getter (it reads `outparamcount`) with the index in ecx and the server inst implied: it is
Scr_GetEntity, a parameter accessor of type `entity` (`find_accessors(vm=...)` now tells them
apart). Before, every entity parameter was hidden. The 563 runs that stopped at Scr_Error were
mostly the handlers' own checks of their object ("not an actor" 135, "not a hud element" 27,
"Not a sentient" 26, about 60 "entity must be a player entity"). The fake self: the resolver run
unhooked maps entref {0, 0} to address 0 (the entity array is not allocated in the dump; its
obfuscated base decodes to 0), so entity 0 lives in the mapped null page; the check sites of
those messages load a qword at [entity + OFF] just before raising them (0x250 client, 0x258
actor, 0x260 sentient, 0x270), and each such slot of entity 0 (and of a heap copy that the hooked
resolver and the entity accessor return) gets a pointer to its own zeroed buffer. Each method is
run with an entity entref and with a hud-element entref (class 1); the run that read most
parameters is kept and its variant written in the evidence.

## Validation

Two oracles, both external to the executable:

- **The official doc** (`bo3_scriptapifunctions.htm`): % of its names found, and on each (name,
  side, kind) matched, min == mandatory and max == mandatory + optional, counted against the
  signature line and against the `[MANDATORY]/[OPTIONAL]` list (the two disagree in 81 entries of
  the doc itself). Every disagreement is listed in the report.
- **Mod-tools call sites** (`corpus/t7/source-modtools`): every call to a builtin of the matching
  syntax kind and file type (names the scripts define themselves excluded) must fall within the
  extracted min..max.

Results: `reports/t7_builtins.md`.

## Entity fields (T7)

The script-visible engine fields of each object kind, by the same method. Files:

| File | Role |
|---|---|
| `treyarch/t7_fields.py` | `extract`: the registrations, the tables, the read-only measures, to `out/t7_fields_extract.json` (addresses only, not committed, about 20 s). Needs `out/t7_extract.json` from `t7_builtins.py extract` for the parameter accessors. |
| `treyarch/officialfields.py` | Reader of `docs_modtools/ScriptObjectFields.xlsx` (zip + XML, standard library), read locally, used only as an oracle: names per sheet and their type column. |
| `treyarch/build_t7_fields.py` | Names, type labels, the oracle and corpus checks, the typo-lint simulation; writes `gruff-data/fields/t7.toml` and `reports/t7_fields.md`. |

    py -3.11 -I treyarch/t7_fields.py extract
    py -3.11 -I treyarch/build_t7_fields.py

**The tables are built at static-init time**, like the builtin tables: the same FNV initialisers
fill them. **Registration is the engine's own code**: `Scr_AddClassField(inst, classnum, canon,
offset)` stores each field in its class's field map (an int variable named by the canon, holding
the offset). It was found from the read-only error strings ("actor field %s is read-only" ...):
each string sits in a small setter stub; the stub's address fills the setter slot of a table
after init; the code that loads that table's address is a loop whose one call is
`Scr_AddClassField` (its body stores type 0xa and the offset: `check_anchors` verifies it). Every
caller of it is emulated from its function head with the call hooked, which gives (VM, class,
canon, offset) and, from the 4-byte read that produced the canon, the entry's address.

**Owner kinds.** The VM has six field classes (0 entity, 1 hud element, 2 path node, 3 vehicle
node, 4 client dynamic entity, 5 weapon); entity sub-kinds are offset groups that the
registration ORs in (0x2000 sentient, 0x4000 and 0xa000 actor, 0x6000 player, 0x8000 vehicle;
CSC entities use 0x0/0x2000/0x4000/0x8000). Each group is named by the read-only message its
setters raise; class 1 raises none and is named by its names (29/29 in the HudElem sheet).
There is no class for `level`: it has no engine fields.

**Second measure of the tables**: one stride per group, offset low bits = entry index, canon 0
after the last entry. **Read-only, two measures**: each entry's setter run alone on the post-init
image (read-only message before any parameter read: read-only; the value read through a hooked
accessor: writable), and the VM's object-field setter dispatch (`Scr_SetObjectField`, found as
the two functions loading "weapon is read-only") run for each (class, offset) on entity 0. On
entity 0 the dispatch does not reach the sentient/actor/player/vehicle setters, and the CSC one
reads the value before the lookup: the report says which measure decided each field.

**Validation**: the official field list (share of its names found, per kind; type labels by
majority and per-field agreement), and the mod-tools scripts: every `x.name` access split into
engine field / script-defined (assigned or `var`-declared somewhere) / neither, plus a simulation
of a "likely typo of an engine field" lint with 20 hand-classified hits. Results:
`reports/t7_fields.md`.

## Engine-emitted events (phase 3)

The events the engine notifies (the names scripts `waittill`), per game: `gruff-data/events/<game>.toml`
(schema `gruff-data/schema/events.schema.json`) and `reports/events_<game>.md`.

    py -3.11 -I treyarch/events.py extract --game t7     # t4 t5 t6 t7 t8 t9 t10 t10sp t11
    py -3.11 -I treyarch/events.py build --game t7       # t10 merges out/events_t10sp.json
    py -3.11 -I iw/events.py extract --game iw5          # iw3 iw4 iw5 iw6 s1 h1 iw7

| File | Role |
|---|---|
| `common/events.py` | Call index (every rel32 call/jmp by target), function decoding from its start (x86 and x64 leaves: from a call target), the scr_const slots (init emulated, SL_GetString hooked: slot -> string), the VM type-name table, `Probe` (one Unicorn for every run; code write-protected, imports as `ret` pages), the wrapper discovery. |
| `common/events_sites.py` | Push cores (static), pushers (each Scr_Add* call run with its inner calls skipped), the guided site measure. |
| `common/events_run.py` | The extraction driver (a profile in, `out/events_<game>.json` out). |
| `common/events_build.py`, `common/events_corpus.py` | Naming, merging per event, the corpus measure, the TOML and the report. |
| `treyarch/events.py`, `iw/events.py` | One profile per game: the anchors, each with how it was found. |

**Method.** One anchor per game, the VM's core notify (`Scr_NotifyNum` / `VM_Notify`), found
by hand once from the engine's own constants: the reads of the `trigger`/`damage` scr_const slots
(string-id engines) or the hashes of corpus event names stored as immediates (T7: t7.canon;
T8/T9: t8.fnv64), followed down their callee chain. From the core, every small caller is run
under Unicorn with sentinel arguments: when a sentinel (or a C string it gets, hashed; or an SL
id it converts) reaches the core's event parameter, the caller is a wrapper, with its event,
count, object and receiver (object id from a call: entity; from a global: level). Every call site
of every notifier outside the wrappers is then emulated from its function start (or the root of
the paths that reach it) to the notifier's entry, each conditional branch with only one side
reaching the site forced, every inner call skipped and recorded. At the entry: the event (a
scr_const token, a C string, a hash; several when a branch picks it), the count, and the pushes
on the path after the previous notify. A push is a skipped call that, run alone with its own
inner calls skipped except the push cores, advances the VM stack top by one value; the type tag
it writes is the type (the core's static tag is the second measure), an object id from
Scr_GetEntityId/FindEntityId makes it an entity. Arguments are kept in waittill order (the
reverse of push order: checked on T7 `grenade_fire`, IW5 `damage`) and only where the count
argument equals the pushes seen (the second measure per site).

**Corpus measure.** Every literal event of `waittill`/`waittillmatch` in the game's corpus is
split into: notified by some script literal, sent by the engine (this catalogue), unexplained.
The unexplained ones are classed in the report: also written as a string elsewhere (a script
`notify( var )` or a builtin's notify parameter can send them), `waittillmatch`-only
(notetracks), written only in waittill/endon (data: notetracks, map KVPs, menus, or engine code
not reached).

**BO6 and BO7** (`reports/events_t10.md`, `events_t11.md` say how each anchor was found). BO7
passes events as the full 64-bit FNV-1a of the name (`iw9.xhash`, as its corpus spells them);
BO6 as 32-bit string ids, two notify paths (both are cores: `extra_cores`). Both fill their
scr_const slots through a getter `(&out, char*, ...)` the inits call ~2,900 times
(`const_out_arg`); the VM context comes from `[gScrVm] + off` (`gptr` specs, `vm_of_ctx`);
pushers advance the top behind a pointer (BO7 `vmpub_ctx`) or by a store (BO6 `top_store`) and
tag a byte (`tag_width`); the builds keep no type names, the tags are named from the push cores'
code (`T10_TAGS`, `T10SP_TAGS`). BO6 has no script corpus: its report measures the catalogue
against the BO7 corpus instead (`cross_corpus`); the campaign dump is merged into `t10`.

**Not done**: iw8, jup. Both cores are found (IW8 0x3afd0f0, JUP 0x3d5a430: (ctx, object id,
32-bit string id, count), behind Scr_NotifyNum 0x3afd1f0 / 0x3d5a530), but their events cannot
be named from these dumps. JUP: the scr_const inits call the getter 0x3d41bd0 2043 + 372 + 62
times, but the strings they pass are not text in the dump (618 carry the encrypted-string flag
and the engine's own decryptor 0x46a9c70 run under Unicorn returns no text; the others carry no
flag and are not text either), so the dump's string pool is not in its run-time state. IW8: the
~2,800 slots the notify sites read (0x13491axx-0x13494xxx) and the VM context are zero in the
dump and no code stores into them by a rip-relative, image-relative or pointer-table write; its
string literals are baked records {u64 key, u32 length | 0x01000000, text XOR key} decrypted in
place on first use (`maxrank`, `null`...), not the slots' init. No MWII (iw9) dump on this
machine. The S1 catalogue is read from the campaign dump: the multiplayer dump holds almost none
of its strings in clear.

## Script VM, run offline (phase 5, T7)

The game's own script VM (init, loader and linker, scheduler, opcode handlers) run under Unicorn
over the dump, to answer what the VM does with a script: `reports/vm_t7.md`, facts in
`gruff-data/semantics/t7.toml`, scripts in `conformance/t7/` (header format:
`conformance/README.md`).

    py -3.11 -I vm/t7_run.py ../../conformance/t7/notify_reentrant.gsc --notify   # one trace
    py -3.11 -I vm/t7_suite.py [--developer]                                     # the suite
    py -3.11 -I vm/t7_facts.py                                                   # the facts
    py -3.11 -I vm/t7_run.py x.gsc --trace x.jsonl [--calls] [--ops]             # structured trace
    py -3.11 -I vm/trace_view.py x.jsonl [--html x.html]                         # its timeline

| File | Role |
|---|---|
| `vm/trace.py` | The gsc-trace format (`docs/trace-format.md`), shared by every producer: writer, reader, code address -> source map. Keep it stable. |
| `vm/t7_trace.py` | T7 trace mode: hooks on the dispatch, thread alloc/remove and throw; decodes waits, notifies, endons, builtins from the VM stack; maps code to lines with the linker's `.gdb`. |
| `vm/trace_view.py` | Text timeline with thread lanes, or a self-contained HTML timeline. |
| `vm/examples/round_loop.gsc` | A small zombies-like round loop (our own script) for the trace demo. |
| `vm/t7vm.py` | `T7VM`: the dump mapped with import stubs (logged), TLS, the static-init builtin tables (cached in `out/`), `boot` (MT, variables, Scr_Init, global objects), `load`, `exec_thread`, `run_frame`, the trace hooks (logprint, script errors, thread names). Every address with how it was checked. |
| `vm/t7_run.py` | One script: compile with the official linker (`tools/linker/gsc_link.py --gscc-out`, cached in `out/vm_gscc/`), load, run frames until idle, print the trace. |
| `vm/t7_suite.py` | The suite: PASS/FAIL/UNSUPPORTED per script, one process per test, at most 3 at once. |
| `vm/t7_facts.py` | The suite's two runs (developer 0 and 1) to `gruff-data/semantics/t7.toml`. |

BO4 (T8) and Cold War (T9), the same way (`reports/vm_t8.md`, `reports/vm_t9.md`; scripts compiled
by acts, `GSC_ACTS` or `acts` in `paths.local.json`):

    py -3.11 -I vm/t89_run.py --game t8 ../../conformance/t8/notify_reentrant.gsc
    py -3.11 -I vm/t89_suite.py --game t9 [--script-errors continue]
    py -3.11 -I vm/t89_facts.py --game t9

| File | Role |
|---|---|
| `vm/vmcore.py` | `GameVM`: the game-independent harness (dump under Unicorn, logged import stubs, TLS, bump heap, Python printf, `hook`/`watch`/`hook_longjmp`, `call` raising `Fault`). |
| `vm/t89vm.py` | `T89VM` and the `t8`/`t9` profiles: every address with how it was found; boot, asset lookup, `Scr_GscLink`, export lookup, frames, trace hooks. |
| `vm/t89_run.py`, `vm/t89_suite.py`, `vm/t89_facts.py` | One script; the suite (two script-error modes); the facts with `dialect` against T7 (t8) or T8 (t9). |

World at War (T4) and Call of Duty 4 (IW3), 32-bit, plain-text GSC compiled at load by the game's
own compiler (`reports/vm_t4.md`, `reports/vm_iw3.md`):

    py -3.11 -I vm/old_run.py --game t4 ../../conformance/t4/notify_reentrant.gsc [--developer]
    py -3.11 -I vm/old_suite.py --game t4|iw3 [--developer]
    py -3.11 -I vm/old_facts.py --game t4|iw3

| File | Role |
|---|---|
| `vm/vmcore32.py` | `GameVM32`: the x86 counterpart of vmcore.py (stdcall import stubs, TLS, virtual clock, x86 longjmp, register-argument calls, null-page write log). |
| `vm/t4vm.py` | `OldVM` and the `T4` profile: every address with how it was found; boot, file reads, load, frames, trace hooks. `vm/iw3vm.py`: the IW3 profile. |
| `vm/old_run.py`, `vm/old_suite.py`, `vm/old_facts.py` | One script; the suite (retail / developer); the facts with `dialect`. |

Black Ops II (T6, bytecode compiled by gsc-tool: `gsctool` / `GSC_TOOL` or beside `gsctool_src`) and
Black Ops (T5, BlackOpsMP.exe, plain text compiled at load by the game), `reports/vm_t6.md`,
`reports/vm_t5.md`:

    py -3.11 -I vm/t56_run.py --game t6|t5 ../../conformance/t6/notify_reentrant.gsc [--developer] [--verbose]
    py -3.11 -I vm/t56_suite.py --game t6|t5 [--developer]
    py -3.11 -I vm/t56_facts.py --game t6|t5

| File | Role |
|---|---|
| `vm/t56vm.py` | its own x86 `GameVM32` (written in parallel with `vmcore32.py`) and `T56VM` with the `t6`/`t5` profiles: every address with how it was found, the boot order and call signatures, stubs, trace hooks. |
| `vm/t56_run.py`, `vm/t56_suite.py`, `vm/t56_facts.py` | One script (`--verbose` lists a T6 object's imports as the linker reads them); the suite (one process at a time by default); the facts with `dialect` against T7 (t6) or T6 (t5). |

Modern Warfare 3 (IW5, the x64 re-release), Ghosts (IW6, single-player dump), Advanced Warfare
(S1), Modern Warfare Remastered (H1) and Infinite Warfare (IW7), x64, ScriptFile bytecode
compiled by gsc-tool (none of these games ships a compiler) and linked by the game's own loader
(`reports/vm_iw.md`):

    py -3.11 -I vm/iw_run.py --game iw5 ../../conformance/iw5/notify_reentrant.gsc [--verbose]
    py -3.11 -I vm/iw_suite.py --game iw5|iw6|s1|h1|iw7 [--jobs 2]
    py -3.11 -I vm/iw_facts.py --game iw5|iw6|s1|h1|iw7

| File | Role |
|---|---|
| `vm/iwvm.py` | `IWVM` on `vmcore.GameVM` and the five profiles: every address with how it was found; boot, the ScriptFile asset hook, load, frames, trace hooks (the logprint table slot pointed at a stub of ours, script errors at VM_ExecuteInternal's setjmp landing, named after gsc-tool's opcode table), and the replications a dump needs (hunk users, memory tree, builtin registration). |
| `vm/pycrt.py` | The C runtime imports of a dynamically linked build (Ghosts: VCRUNTIME140 and the UCRT) in Python: setjmp/longjmp, mem*/str*, printf, floor. |
| `vm/iw_run.py`, `vm/iw_suite.py`, `vm/iw_facts.py` | One script (compiled by gsc-tool, helpers first, cached in `out/vm_gscc_<game>/`); the suite; the facts with `dialect` against the previous game. |

Modern Warfare 2 (IW4, the x64 re-release) is not run: the build has no GSC compiler and no
loader, every script is compiled ahead of time into the executable (see `reports/vm_iw.md`).

Unicorn is GPL-2.0-only: it stays in these Python research tools, which write data files; it is
never linked into the Rust crates.

## Porting to another engine

What carries over: the entry-shape hunt, emulating the static initialisers, tracing a lookup's
reads with an absent key, the round trip, the accessor discovery by error strings and the handler
runs. What changes per engine, and sits in the `T7` dict of `t7_builtins.py` (each value checked
at run time):

- the value reader (or another caller) that gives the lookups their roles, and its stream read;
- `scrVmPub`'s address, stride and the `top`/`outparamcount` offsets (read off Scr_GetInt);
- the entry layout and the hash (T8+: 64-bit hashes and other layouts; IW engines: name strings
  and a single table of function ids), and whether the tables are static data (then step 1 is a
  plain scan) or built at init;
- the calling convention of handlers (T7: `void(inst)` and `void(inst, entref)`).
