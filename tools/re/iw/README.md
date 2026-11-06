# tools/re/iw: builtin catalogues of the Infinity Ward / Sledgehammer engines

Phase 2 of the RE track (T7 was phase 1, `../README.md`): the builtin functions and methods of
every IW-engine game the user has, from the game's own code, never from a running game.
Method: GrayWolf's `docs/new-game-methodology.md` (the code is the only reference, run under
Unicorn rather than read; two measures per fact; a step that cannot prove its result says so).
Built on `../common`: `image.py` extends common's PE image, `iwemu.py` (its own emulator: dirty-page restore, import stubs) shares common's memory helpers, `iwcorpus.py` common's tokenizer pieces.

Run with `py -3.11 -I` (isolated; each script puts its own directory and the user site on
`sys.path` explicitly):

    py -3.11 -I extract.py <game>      # out/<game>.json (addresses and measures, not committed)
    py -3.11 -I build.py <game> ...    # gruff-data/builtins/<game>.toml, reports/iw/<game>.md

`<game>` is one of `iw1 iw3 iw4 iw5 iw6 s1 h1 iw7 s2 iw8 jup jup_sp`; `iw5_sp s1_sp h1_sp
s2_sp` extract a single-player executable that `build.py` merges into the game's catalogue.
The executables (Steam installs read-only, Cordycep dumps) are listed in `extract.py`'s GAMES.

## Files

| File | Role |
|---|---|
| `image.py` | common/image.py's PE image under the IW names (`mem`, `pdata`), plus imports, entropy and a bytes `cstr`. |
| `iwemu.py` | Unicorn harness: x64 (gs TEB) and x86 (fs through a GDT); code pages read+exec, data read+write; dirty pages restored after every run; import slots pointed at stubs (stdcall argument counts for x86); a `ret` at address 0 (a call through a null pointer returns); static TLS blocks. |
| `nametables.py`, `named.py` | The builds that keep names (IW1 gamex86.dll, IW3): `{name, handler, developer}` runs; each lookup emulated with an absent name (its reads at the entry stride give the table and its count), the loop bound `cmp reg, N*12` as the second measure, every name looked up again (round trip); roles from the two chains that call the lookups (Scr_GetFunction: one lookup; Scr_GetMethod: several). |
| `idreg.py` | The registration builds (IW4/IW5 64-bit rebuilds, IW6, S1, H1, IW7, S2): the registrar found as the call that receives the tables' handlers inside the loops that load them; every caller of it emulated with it hooked; the registrar itself run on each call to see which of its two arrays (functions, methods) it stores into. |
| `descreg.py` | IW8: descriptors `{entries, count}`, one registrar per descriptor storing into the VM context's function or method array; the context's init found and run first. JUP: `{hash, handler, u8 type, u8 min, u8 max}` entries appended by two adders to the context's function and method lists. |
| `vm.py` | The VM's parameter interface, found from the handlers alone (the IW4+ builds strip every error string): the parameter count G and stack top T, the accessors, their accepted types, the raise switch, the error raisers; then every handler run for N parameters. |
| `typenames.py` | The VM's value type names (`var_typename`), when the build keeps them. |
| `tablescan.py`, `leascan.py`, `probe.py`, `xdis.py` | Exploration helpers (function-pointer runs, registrars in code, strings and their references, cross-references and disassembly). |
| `iwcorpus.py` | Second measure: argument counts of the call sites in `corpus/<game>` (counts only, nothing stored). |
| `gsctool.py` | gsc-tool's id/hash tables (GPL-3.0), read locally: names and the corpus cross-check only. A name from it makes the entry GPL-3.0-only in the data. |
| `iw1vm.py` | CoD1: the VM is in the packed engine, reached through a table of pointers the engine fills at load; the slots are classified from the game code (the count: its result compared right after the call; the accessors: called after `push index`) and stubbed, so the handlers can be run all the same (no parameter types). |
| `match.py` | IW4 names: each IW4 handler matched to the one IW5 handler of the same code shape (mnemonics, accessor calls as their type) and arity; unique matches only, recorded as `name_source = "iw5-handler-match"`. |
| `build.py` | Names, merge of the SP executable, corpus validation, the TOML and the report. |
| `summary.py` | `reports/iw/README.md`, one table over every game. |

## How arity is measured (no IW table stores it before JUP)

`vm.py`, from the handlers only:

1. **G and T.** Among the globals the most-called callees of the handlers reference, G (the
   count of parameters, scrVmPub.outparamcount) is the dword most compared, T (scrVmPub.top)
   the pointer most loaded; the pair kept is the one for which most callees read a fake stack
   placed at T only when G is large enough. IW8 and JUP keep the VM state in a context passed in
   rcx: G and T are fields of a heap context there.
2. **Accessors.** Each callee (and, in the global builds, each function referencing T) is run
   with G = 12, T -> a fake stack, every candidate argument location holding a distinct small
   value: a function that reads exactly the slot its index names is an accessor, and that
   location is its index (rcx on x64; IW3's whole-program-optimised x86 code passes it in eax,
   ecx or on the stack). Its accepted types: run once per value type, every dword of the stack
   = t; the block path most types take is the mismatch path, the other types are accepted.
3. **The raise switch.** The stripped builds return a default on a script error unless a thread
   is running: the global whose setting turns an accessor's error path into an unwind is found
   by trying each global that path reads. **Error raisers**: the last direct call of an accessor
   on a mismatch, never made on a match (on IW3 the first one found is `va`, whose result feeds
   the next call: it is recognised as the formatter, stubbed, and the raiser measured again).
4. **Handler runs.** Accessors hooked at entry (index and type recorded, a neutral value
   returned), error raisers hooked, N = 16 first, then N = 0 up to two past the highest index
   read. ok(N): no script error and no index >= N read; a run the emulation cuts short (a fault
   in engine code, the instruction limit) passes when it had read every parameter given.
   min = the least ok N, max = the greatest index read in an ok run + 1; `max_checked` when a
   larger N fails (an explicit usage check), `variadic` when the N = 16 run reads index 15.
   A handler that raises before reading anything checks some state first: the data it read
   last before the error is set in turn (a pointer to zeros, or 1) until it gets past
   (`arity_mode = "strict+state"`); failing that, a lenient run (errors return; the errors
   every N raises are the baseline, more is a failure). A run failing on an error after reading
   its parameters is run again with 1 / 1.0 as the numeric neutral value (a value check on 0).

## What differs by generation

- **IW1 (CoD1)**: the builtins live in the game module `gamex86.dll`: a 121-entry
  `{name, handler, developer}` function table and `{name, handler}` method tables, their
  lookups handed to the engine by pointer (no direct caller) or chained; CoDMP.exe is packed
  and holds the VM (see `iw1vm.py`). **IW2 (CoD2)**: CoD2MP_s.exe is packed
  (0xb3a00 raw bytes for a 0x15ae000 .text) and no unpacked copy exists: not extracted.
- **IW3**: x86, `{name, handler, developer}` tables, one function lookup and a five-lookup
  method chain, error strings kept (`parameter %d does not exist`, `type %s is not a float`).
  Its handlers reach the static CRT (`va`, `_getptd`), which the emulation cannot finish in a
  process the CRT never initialised: `va` is stubbed (an empty string), `_getptd_noexit` too
  (a zeroed block), and the TLS imports keep what is stored.
- **IW4/IW5 (the 2026 x64 rebuilds)**: no builtin name and no script error string. IW5
  registers `{id, handler, developer}` through one registrar into a function array (ids 1..455)
  and a method array (ids from 0x8000); IW4 appends `{handler, developer}` to one array, the
  index being the registration order (function init first, then method init). IW4 has neither
  names nor ids in the executable.
- **IW6, S1, H1, IW7**: the IW5 scheme (ids, one registrar, two arrays), dumps from
  Cordycep. **S2 (WWII)**: `{handler, id}` tables are in the dump, but no code references them
  by an address the scan can follow (the registration is not found: no table loop, no
  descriptor reference), so neither the roles nor the table bounds can be proven. A
  static-scan fallback (`static` in `extract.py`) gives 993 entries of which only 39 carry an id
  gsc-tool names and 70% of the corpus call sites agree: not good enough to publish, so no
  `s2.toml` is written.
- **IW8**: descriptor tables `{entries, count}` registered into the VM context's two arrays;
  the context's init sets the id ranges (functions from 1, methods from 0x8000).
- **JUP**: `{u64 name hash, handler, u8 developer, u8 min, u8 max}`: the arity is stored
  (97% of the corpus call sites agree); an entry storing 0/0 records none, and the handler runs
  in this obfuscated dump agree with only 57% (MP) / 75% (SP) of the call sites on those, so
  they are published without an arity (the runs stay in the evidence). Names are 64-bit
  hashes (`x = 0x10000000233 * (byte ^ x)` from 0x79D6530B0BB9B5D1 over the lower-cased name);
  the two lists' split is the method adder's `add rdx, 0x7d0` (slot 0x7d0 onwards).

Results per game: `../reports/iw/<game>.md`.
