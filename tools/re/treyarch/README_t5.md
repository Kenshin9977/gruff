# T5 (Black Ops): builtin catalogue

Same extractor as T4 (`treyarch/old_builtins.py`, see `README_t4.md` for the method), profiles
`t5` (SP and zombies) and `t5mp`; `treyarch/build_old.py --game t5` merges them. Results:
`reports/t5_builtins.md`, `gruff-data/builtins/t5.toml`.

    py -3.11 -I treyarch/old_builtins.py extract --game t5
    py -3.11 -I treyarch/old_builtins.py extract --game t5mp
    py -3.11 -I treyarch/build_old.py --game t5

Executables: `blackops_flat.exe` (Steam `BlackOps.exe`, flattened) and the install's
`BlackOpsMP.exe` (not encrypted, read in its file layout, read-only). Both x86, image base
0x400000. The catalogue is the SP exe's; the (name, VM, kind) only the MP exe has are added with
`table = "BlackOpsMP.exe only"`, and the arities that differ between the two are listed in the
report (the corpus holds SP, zombies and MP scripts).

## Differences from T4

- Same 12-byte entries `{char *name, handler, int developer}`, same compiler emitters (both
  branch with `cmp esi, ebp` on the instance, ebp = 0), same lookup convention
  `(const char **pName, int *pType)`, all cdecl. The MP exe's emitters are found by that code
  shape (`find_emitters`), not by fixed addresses.
- The name strings sit packed among other data (not NUL-preceded): tables are found from the
  lookups, never by searching the strings.
- Accessors are real cdecl functions `Scr_GetInt(unsigned int index, scriptInstance_t inst)`
  (index first); `Scr_Error(inst, const char *msg, bool)`; scrVmPub at 0x32c8740 (top +8,
  outparamcount +0x14, stride 0x4320 per instance), all read off Scr_GetInt at run time.
- Handlers are still `void(void)` / `void(scr_entref_t)` with the instance hard-coded in their
  accessor calls (CSC handlers pass 1).
