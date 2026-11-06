# Hash families

`gruff hash --list` prints the live registry. This page records where each family comes from and how
it was checked. Constants are facts about the games; every function was reimplemented from its
description, no code was copied.

Widths: 32, 60 (`& 0x0FFFFFFFFFFFFFFF`), 63 (`& 0x7FFFFFFFFFFFFFFF`, bit 63 is an engine flag) or 64.
A 64-bit lookup tries 63 bits, then 60 (`lookup_with_fallback`, `NameTable`).

Bytes are hashed unsigned. acts hashes a signed C `char` (and its `lowerc` folds `\` to `/` in
every family); the results differ only for bytes `>= 0x80` or for backslashes, see "Open issues".

## Families

| id | games | input | bits | algorithm | status |
|---|---|---|---|---|---|
| `iw3.r_hash` | CoD4, WaW, MW2, MW3, BO1 | raw (`c \| 0x20`) | 32 | djb2 xor, seed 0 (`R_HashString`) | documented |
| `iw4.stringtable` | MW2, MW3 | lower | 32 | `h*31 + c` from 0 | documented |
| `t5.com_hashstring` | BO1, BO2 | lower | 32 | djb2 `h*33 + c` from 5381 | documented |
| `t5.com_hashkey` | BO1, BO2 | raw | 32 | `h += c*(0x77+i)`; `h ^ ((h ^ (h>>10)) >> 10)` (signed) | documented |
| `t5.snd` | BO1, BO2, BO3 | lower | 32 | SDBM from 5381, 0 -> 1 (`SND_HashName`) | documented |
| `t6.sab` | BO2, BO3 | lower | 32 | SDBM from 5381 | verified |
| `t7.canon` | BO3 | lower (no `\` fold) | 32 | FNV-1a 32 seed `0x4B9ACE2F`, then `h *= 0x1000193` | verified |
| `t89.scr` | BO4, CW | lower, `\`->`/` | 32 | acts `HashT89Scr` (below) | spot-checked |
| `t8.fnv64` | BO4, CW | lower, `\`->`/` | 63 | FNV-1a 64 TA | verified |
| `t8.localize` | BO4, CW | lower, `\`->`/` | 60 | FNV-1a 64 TA | documented |
| `t9.strings` | CW | lower, `\`->`/` | 60 | FNV-1a 64 TA | verified |
| `t8.sound_asset` | BO4 | lower, `/`->`\` | 63 | FNV-1a 64 TA | verified |
| `iw8.localize_ta` | MW2019, MWIII | lower | 63 | FNV-1a 64 TA (GrayWolf weapon display keys) | documented |
| `iw.asset` | MWII, Vanguard, MW2R, MWIII, BO6, BO7 | lower, `\`->`/` | 63 | FNV-1a 64 IW | verified |
| `iw.localize` | MWIII, BO6, BO7 | lower, `\`->`/` | 63 | FNV-1a 64 IW | verified |
| `iw.sound_alias` | Vanguard to BO7 | lower | 64 | FNV-1a 64 TA | documented |
| `iw9.bone` | MWII, MWIII | lower | 32 | FNV-1a 32 | verified |
| `iw9.scr` | MWII, MWIII | lower, `\`->`/` | 64 | FNV-1a 64, basis `0x79D6530B0BB9B5D1`, prime `0x10000000233` (acts `HashJupScr`) | spot-checked |
| `iw9.xhash` | MWII, MWIII | lower, `\`->`/` | 64 | FNV-1a 64 TA (`#"hash_..."` script hashes) | spot-checked |
| `iw.rawfile_ext` | MWIII, BO6, BO7 | lower | 32 | FNV-1a 32 | documented |
| `t10.function` `t10.var` `t10.namespace` `t10.event` `t10.bundlevar` | BO6, BO7 | lower, `\`->`/` | 60 | salted FNV-1a 64: `name[0] + "zt@f3yp(d[kkd=_@" + name[1:]`, basis `0x1C2F2E3C8A257D07`, prime `0x10000000233` | verified |
| `t10.dvar` | BO6, BO7 | lower, `\`->`/` | 60 | salted, `"q6n-+7=tyytg94_*"`, basis `0xD86A3B09566EBAAC` | verified |
| `t10.xhash` | BO6, BO7 | lower, `\`->`/` | 60 | FNV-1a 64 TA | verified |
| `t10.case` | BO6, BO7 | **raw** | 60 | FNV-1a 64 TA, prime `0x10000000193` | verified |
| `t10.assetlit` | BO6, BO7 | lower, over `"<type>:<name>"` | 63 | FNV-1a 64 IW | verified |
| `t10.script` | BO6, BO7 | path with `.gsc`, `/` | 63 (table at 60) | FNV-1a 64 IW | verified |
| `t10.bundlekey` | BO6, BO7 | raw | 64 | FNV-1a 64 TA | verified |
| `t10.entity_key` | BO6, BO7 | lower | 64 | the salted script hash | documented |
| `t10.scr_sp` | BO6 campaign | lower | 64 | FNV-1a 64 of `name + salt` (acts `HashT10ScrSP`) | documented |
| `t10.omnvar` | BO6, BO7 | lower | 64 | salted, `"gvbs9*vpm@mh@krh"`, basis `0xCBF28CE593123345`, prime `0x100000002C1` | documented |
| `t10.script_string` | BO6, BO7 | raw | 64 | XXH3-64, seed `0x100000001B3` (`"end"` = `0x6ee7ba8e155a9ca8`) | spot-checked |
| `t10.type_name` | BO6, BO7 | lower | 63 | FNV-1a 64 TA (type name beside an asset id) | documented |
| `t10.bone` | BO6, BO7 | raw | 64 | FNV-1a 64 TA | documented |
| `t10.tunable_tag` | BO6, BO7 | raw | 63 | FNV-1a 64 TA | documented |
| `t10.type_id` | BO6, BO7 | raw | 32 | FNV-1a 32 of the asset type name | documented |
| `codol.ifs_name` | CoD Online | lower, `/`->`\` | 64 | lookup3 `hashlittle2`, seeds 2, 1 | documented |
| `codol.mpq_key` | CoD Online | raw (upper-cased inside) | 32 | MPQ `HashString`, offset `0x300` | documented |

TA basis `0xCBF29CE484222325`, IW basis `0x47F5817A5EF961BA`, FNV-1a 64 prime `0x100000001B3`.
CoD1/UO/CoD2, CoD4, WaW, MW2 and BO1 keep script identifiers in clear; MW3 and Ghosts to WWII
use gsc-tool token ids and MW2019 canonical ids (lookup tables, not hashes).

## Script identifiers per game

`gsc_hash::Scheme` maps each kind of script identifier to its family (functions, fields and
locals, namespaces, classes, events, `#"..."` hashes, script file names); the table is in
[Hash packages](hash-packages.md). How each mapping was checked on the corpora (2026-11-06,
rehashing the clear names of each corpus and the local name lists, then counting the decompiler's
hashes they hit):

- BO4, Cold War: functions, fields, namespaces and events (`event_<hex>`) re-hash with `t89.scr`;
  `#"hash_..."` with FNV-1a 64 TA (63 bits, `t8.fnv64`); `#using script_<hex>` is `t8.fnv64` of
  the file path with its extension: with it all 20,323 imports of the BO4 corpus reach a file
  (18,628 did not before), and all but 9 of Cold War's 27,228.
- MWIII: functions, fields and namespaces re-hash with `iw9.scr` (acts' `HashJupScr`): 23,375
  distinct function hashes and 40,717 field and local hashes of the multiplayer corpus are named
  by BO3 and BO7 names, impossible by chance at 64 bits. `#"hash_..."` values are full 64-bit
  FNV-1a TA (`iw9.xhash`). The `script_<hex>.gsc` file names are `iw.asset` of the path with
  `.gsc`, at 60 bits (all but 22 of the corpus's 29,263 imports reach a file).
- BO6, BO7: the `t10.*` families above; BO7 `namespace_<hex>` and `function_<hex>` values are
  printed with the four top bits the 60-bit families drop, and compare at 60 bits.

## BO4 / Cold War `HashT89Scr`

Open question: FNV-1a 32 seed `0x4B9ACE2F` without T7's final multiply, or a shift/xor hash with a
final `* 0x8001`? **It is the shift/xor hash.** acts' `src/core/shared/utils/hash_mini.hpp`
(MIT) defines `HashT89Scr(str) = HashT89ScrPost(HashT89ScrPre(str))`:

```
h = 0x4B9ACE2F
for c in lowerc(str):                       # A-Z lower-cased, '\' -> '/'
    x = (c + h) ^ ((c + h) << 10)
    h = x + (x >> 6)
return 0x8001 * ((9h) ^ ((9h) >> 11))       # all 32-bit
```

acts uses it for the BO4 and Cold War linkers (`BO4LinkContext::HashScr`, `BOCWLinkContext::HashScr`)
and lists it as `h32`, "BO4/CW XHashScr". The FNV-1a variant appears nowhere in acts.

Evidence on the game: the BO4 dumped exe holds a script weapon-field table with each field's
32-bit hash next to its accessor. 56 of its entries were named in the GrayWolf weapon-stats work
(`healtime` = `var_4465ef1e`, `clipmodel` = `9fdd8e46`, `firstRaiseTime` = `1a936250`...).
All **56/56** re-hash with `t89.scr`; **0/56** with FNV-1a 32 from the T7 seed, with or
without the final multiply. Random 32-bit agreement on 56 pairs is impossible, so the algorithm
is settled; the name set is small, hence "spot-checked" rather than "verified".

## MWIII / BO6 localize keys: IW basis

GrayWolf's MWIII weapon-stats code hashes weapon display keys with the TA basis, while its docs
say localize keys use the IW basis. **The IW basis is right for localize keys.** The localize-keys
study of the GrayWolf docs (2026-10-11) matched keys taken from MW2019's clear-text localize
(99 184) against the hashed ids of the MWIII, BO6 and BO7 French zones: 15 087 MWIII,
19 858 BO6, 19 916 BO7 hits under the IW basis, and the same thirty menu keys match nothing
under TA. The resulting 74 645-name table re-hashes 74 645/74 645 with `iw.localize` at 60 bits
and 0/74 645 with the TA basis. Known pairs (60 bits): `menu/back` `c6d515be207b9a5`, `menu/ok`
`5c33da8f6ce65b0`, `menu/cancel` `78c2c5fe58603ac`. The TA lambda is kept as
`iw8.localize_ta` (documented, unverified): it may hash a different field.

## Verification (2026-11-05)

`cargo test --release -p gsc_hash --test name_databases -- --ignored --nocapture`

| Source | Family | Pass |
|---|---|---|
| bo7-gsc-archive `names/all_names.tsv` (169 503 pairs) | `t10.var` 85 473, `t10.function` 45 314, `t10.xhash` 17 462, `t10.dvar` 4 608, `t10.bundlekey` 4 053, `t10.namespace` 3 566, `t10.assetlit` 3 495, `t10.bundlevar` 2 875, `t10.script` 1 850, `t10.case` 800, `t10.event` 7 | 100 % each |
| BO3 `bo3_names_and_labels.csv` (real names) | `t7.canon` 346 884 | 100 % (21 595 `x_*` labels and 56 names holding U+FFFD skipped) |
| BO4 exe weapon-field table | `t89.scr` 56 | 100 % |
| localize names table | `iw.localize` 74 645 | 100 % |

GrayWolf `package_index` `.wni` (community data, reported, not asserted; a pair passes if one of
the file's families re-hashes it at 60 or 32 bits): `bo3_sab`, `bo6_names`, `bo7_names`,
`fnv1a_bones`, `fnv1a_localize`, `fnv1a_v2_ximages`, `fnv1a_ximages_mw5` 100 %;
`bo2_sab` 99.999 %, `fnv1a_xmodels` 99.999 %, `fnv1a_xmaterials` 99.9998 %, `fnv1a_ximages`
99.998 %, `fnv1a_xsounds` 99.998 % (TA, backslashes kept: `t8.sound_asset`), `fnv1a_v2_xanims`
99.999 %, `fnv1a_v2_xmaterials` 99.998 %, `bo3_script_hashes` 99.984 % (the same 56 U+FFFD
names), `fnv1a_string` 98.93 % (some keys there are hashed as written, matched by
`t10.bundlekey`). The rest are a few dozen mistyped or mis-filed community entries.

## Open issues

- BO3 CSV names holding U+FFFD: the original bytes were lost before the CSV was written. Neither a
  single high byte nor any one UTF-8 code point in its place reproduces the hash.
- Non-ASCII bytes: acts XORs a sign-extended `char`; this crate uses unsigned bytes. No verified
  name contains a byte `>= 0x80`, so which one the games use is unknown.
- `t89.scr` folds `\` to `/` as acts does; BO3 shows the T7 hash does **not** fold
  (346 884 names pass without folding, 342 185 with it). Identifiers rarely hold `\`.
- Unknown: BO4/CW scriptbundle 32-bit keys, the BO4 weapon asset hash, BO4/CW dvars and `case`,
  MWII/Vanguard/MW2R GSC identifiers (MWII is assumed to share MWIII's, no corpus), the family of
  `class_<hex>` names (few hits with `t89.scr` in BO4/CW), MWIII dvars (`@"hash_..."`: no family
  hit), BO6-specific checks of `case`/`assetlit`.
