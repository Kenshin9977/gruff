# Game modes

Several games ship one executable per game mode, and each registers its own builtins: Black Ops
II has `t6sp.exe` (campaign), `t6mp.exe` (multiplayer) and `t6zm.exe` (zombies); Black Ops has
`BlackOps.exe` (campaign and zombies) and `BlackOpsMP.exe`; Modern Warfare 3, Advanced Warfare
and Modern Warfare Remastered a campaign and a multiplayer executable. A campaign script cannot
call a multiplayer-only builtin, and Black Ops II's campaign scripts define functions such as
`spawnfx` and `playloopedfx` that only the other executables have as builtins.

The builtin catalogues (`gruff-data/builtins/<game>.toml`) record this, from one extraction per
executable (`tools/re/modes.py`):

- `[extraction] modes` lists the modes whose executables were read, each with its executable.
  A mode missing there was not read (World at War's multiplayer, Call of Duty 4's campaign and
  Ghosts' multiplayer executables are encrypted): its scripts see every builtin, as before.
- An entry's `modes` lists the modes that have it, when that is not all of them.
- An entry's `mode_args` gives each mode's arity when the executables store different ones
  (Black Ops II: 52 entries, such as `playfx`, 2 to 4 arguments in multiplayer and 2 to 6 in
  the campaign).
- `[coverage] complete_modes`: the modes whose executables were all read, in a catalogue that
  lacks another one. World at War's catalogue is complete for its campaign and zombies scripts,
  Call of Duty 4's for its multiplayer ones, Call of Duty's for its campaign.

`gruff check` (B001, B002, B003, B005, B006, N001's `::f`), name resolution, hover and
completion use only the builtins of the file's mode, when the mode is known.

## A file's mode

From its path, in the games whose scripts sit under `maps/` and `clientscripts/` (Call of Duty
to Ghosts, Advanced Warfare, Modern Warfare Remastered, WWII, World at War, Black Ops, Black
Ops II), after the first `maps` or `clientscripts` folder of the path:

| Path | Mode |
|---|---|
| a folder `zombies`; a folder or file named `zm_*`, `_zm*`, `*_zm`, `*_zm_*` or containing `zombie` (`maps/mp/zombies/`, `maps/mp/zm_transit.gsc`, `maps/mp/gametypes_zm/`, `aitype/zm_*`, `maps/nazi_zombie_*`, `maps/_zombiemode*`); in the IW games also `alien` (Ghosts' Extinction) | `zm` |
| `maps/mp/...`, `clientscripts/mp/...` | `mp` |
| any other file under `maps/` or `clientscripts/` | `sp` |
| anything else (`common_scripts/`, `animscripts/`, `character/`...) | none: shared by several modes, every builtin |

World at War's and Black Ops' zombies run in the campaign executable, and Advanced Warfare's Exo
Zombies in the multiplayer one: their `zm` files see that executable's builtins. Later games
(Black Ops III and after, Infinite Warfare, Modern Warfare 2019 and after) ship one executable
for every mode, or a separate catalogue (`jup-sp`): no mode is derived there.

## Setting the mode in gruff.toml

A mod usually targets one mode, and its files may not follow the game's layout:

```toml
game = "t6"
game-mode = "zm"          # every file of the project (sp, mp, zm, or any for none)

[game-modes]              # glob = mode, first match wins, before game-mode
"maps/mp/gametypes/**" = "mp"
"shared/**" = "any"
```

Globs are matched against the path relative to the root the file was found under, as in
`per-file-ignores`. They apply to the project's files, not to the library roots (the game's
own scripts keep the conventions above). The top-level `mode` key is another setting: the
language server's compiler mode (`retail` or `community`).
