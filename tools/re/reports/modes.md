# Builtins per game mode (tools/re/modes.py)

Each executable's extraction read again; an entry's `modes` are the modes whose executables hold it (absent: all the modes read). Generated file.

| game | modes read | not read | executables (entries) | entries per modes | mode_args |
|---|---|---|---|---|---|
| t4 | sp, zm | CoDWaWmp.exe is SteamStub-encrypted (.text entropy 8.0) | one executable | n/a |  |
| t5 | sp, mp, zm |  | BlackOps.exe sp/zm: 1510; BlackOpsMP.exe mp: 1331 | sp+mp+zm 967, sp+zm 544, mp 365 |  |
| t6 | sp, mp, zm |  | t6mp.exe mp: 2032; t6zm.exe zm: 2033; t6sp.exe sp: 1927 | sp+mp+zm 1492, mp+zm 576, sp 471, zm 1 | 52 |
| iw1 | sp | the multiplayer game module was not extracted | one executable | n/a |  |
| iw3 | mp | iw3sp.exe is SteamStub-encrypted | one executable | n/a |  |
| iw4 | mp | iw4sp.exe is readable, but IW4 entries have neither a name nor an id: its entries cannot be keyed to the multiplayer ones | one executable | n/a |  |
| iw5 | sp, mp |  | iw5mp.exe mp: 777; iw5sp.exe sp: 1078 | sp+mp 621, mp 156, sp 457 |  |
| iw6 | sp | iw6mp64_ship.exe (multiplayer and Extinction) is encrypted | one executable | n/a |  |
| s1 | sp, mp, zm |  | s1_mp64_ship.exe mp/zm: 1564; s1_sp64_ship.exe sp: 1610 | sp+mp+zm 1095, mp+zm 469, sp 515 |  |
| h1 | sp, mp |  | h1_mp64_ship.exe mp: 1606; h1_sp64_ship.exe sp: 1662 | sp+mp 1132, mp 474, sp 530 |  |
