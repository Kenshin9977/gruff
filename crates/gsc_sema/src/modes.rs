//! Game modes (`sp`, `mp`, `zm`). Games with one executable per mode register a different
//! builtin set in each; a catalogue entry's `modes` says which executables hold it. A file's mode
//! comes from its path ([`mode_of_path`]) or from `gruff.toml` (`game-mode`, `[game-modes]`).

use std::fmt;

/// One game mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Mode {
    Sp,
    Mp,
    Zm,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Sp, Mode::Mp, Mode::Zm];

    /// `sp`, `mp`, `zm`: the catalogue's spelling.
    pub fn label(self) -> &'static str {
        match self {
            Mode::Sp => "sp",
            Mode::Mp => "mp",
            Mode::Zm => "zm",
        }
    }

    /// A catalogue or configuration spelling (`sp`, `mp`, `zm`, any case).
    pub fn parse(s: &str) -> Option<Mode> {
        match s.to_ascii_lowercase().as_str() {
            "sp" => Some(Mode::Sp),
            "mp" => Some(Mode::Mp),
            "zm" => Some(Mode::Zm),
            _ => None,
        }
    }

    fn bit(self) -> u8 {
        1 << self as u8
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A set of modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modes(u8);

impl Modes {
    pub const NONE: Modes = Modes(0);

    pub fn contains(self, m: Mode) -> bool {
        self.0 & m.bit() != 0
    }

    pub fn insert(&mut self, m: Mode) {
        self.0 |= m.bit();
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The modes in `sp`, `mp`, `zm` order.
    pub fn iter(self) -> impl Iterator<Item = Mode> {
        Mode::ALL.into_iter().filter(move |m| self.contains(*m))
    }

    /// `mp, zm`.
    pub fn labels(self) -> String {
        self.iter().map(Mode::label).collect::<Vec<_>>().join(", ")
    }
}

impl FromIterator<Mode> for Modes {
    fn from_iter<I: IntoIterator<Item = Mode>>(it: I) -> Modes {
        let mut s = Modes::NONE;
        for m in it {
            s.insert(m);
        }
        s
    }
}

/// The games whose scripts sit in `maps` and `clientscripts` folders, the multiplayer ones in
/// their `mp` subfolder.
const MAPS_LAYOUT: &[&str] = &[
    "iw1", "iw1-uo", "iw2", "iw3", "iw4", "iw5", "codol", "iw6", "s1", "h1", "s2", "t4", "t5", "t6",
];

/// A path component (or the file stem) naming zombies: `zombies`, a `zm_` or `_zm` prefix, a
/// `_zm` suffix, `_zm_` or `zombie` inside; in IW games also Extinction's `alien` folder (a
/// co-op mode of the multiplayer executable).
fn names_zombies(c: &str, iw: bool) -> bool {
    c == "zombies"
        || c.starts_with("zm_")
        || c.starts_with("_zm")
        || c.ends_with("_zm")
        || c.contains("_zm_")
        || c.contains("zombie")
        || (iw && c == "alien")
}

/// The mode a script of `game` (a dialect id) runs in, from its root-relative path; `None` for
/// the shared folders (`common_scripts`, `animscripts`...) and other layouts. Anchored at the
/// first `maps` or `clientscripts` component. Conventions: `docs/modes.md`.
pub fn mode_of_path(game: &str, rel: &str) -> Option<Mode> {
    if !MAPS_LAYOUT.contains(&game) {
        return None;
    }
    let iw = !game.starts_with('t');
    let lower = rel.replace('\\', "/").to_ascii_lowercase();
    let comps: Vec<&str> = lower.split('/').filter(|c| !c.is_empty()).collect();
    let (last, dirs) = comps.split_last()?;
    let stem = last.rsplit_once('.').map_or(*last, |(s, _)| s);
    if dirs.iter().any(|c| names_zombies(c, iw)) || names_zombies(stem, iw) {
        return Some(Mode::Zm);
    }
    let anchor = dirs
        .iter()
        .position(|c| *c == "maps" || *c == "clientscripts")?;
    if dirs.get(anchor + 1) == Some(&"mp") {
        Some(Mode::Mp)
    } else {
        Some(Mode::Sp)
    }
}

/// `gruff.toml`'s modes for the project's own files: `[game-modes]` globs (first match wins),
/// then `game-mode`; `None` inside is `any` (no mode).
#[derive(Clone, Debug, Default)]
pub struct ModeOverrides {
    /// `(glob, mode)` in file order, matched against the path relative to its root.
    pub globs: Vec<(String, Option<Mode>)>,
    /// `game-mode = "..."`.
    pub all: Option<Option<Mode>>,
}

impl ModeOverrides {
    pub fn is_empty(&self) -> bool {
        self.globs.is_empty() && self.all.is_none()
    }

    /// The overriding mode of a file, if a rule applies (`Some(None)`: no mode).
    pub fn mode_of(&self, rel: &str) -> Option<Option<Mode>> {
        self.globs
            .iter()
            .find(|(g, _)| crate::vfs::glob_match(g, rel))
            .map(|(_, m)| *m)
            .or(self.all)
    }
}

/// A configuration value: a mode, or `any` (no mode).
pub fn parse_setting(s: &str) -> Result<Option<Mode>, String> {
    if s.eq_ignore_ascii_case("any") {
        return Ok(None);
    }
    Mode::parse(s)
        .map(Some)
        .ok_or_else(|| format!("unknown mode `{s}` (expected sp, mp, zm or any)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_from_paths() {
        let m = |g, p| mode_of_path(g, p);
        assert_eq!(m("t6", "maps/_utility.gsc"), Some(Mode::Sp));
        assert_eq!(m("t6", "maps/mp/_utility.gsc"), Some(Mode::Mp));
        assert_eq!(m("t6", "clientscripts/mp/_fx.csc"), Some(Mode::Mp));
        assert_eq!(m("t6", "clientscripts/_fx.csc"), Some(Mode::Sp));
        assert_eq!(m("t6", "maps/mp/zombies/_zm_utility.gsc"), Some(Mode::Zm));
        assert_eq!(m("t6", "maps/mp/zm_transit.gsc"), Some(Mode::Zm));
        assert_eq!(
            m("t6", "maps/mp/gametypes_zm/_zm_gametype.gsc"),
            Some(Mode::Zm)
        );
        assert_eq!(m("t6", "aitype/zm_transit_basic.gsc"), Some(Mode::Zm));
        assert_eq!(m("t6", "common_scripts/utility.gsc"), None);
        assert_eq!(m("t6", "animscripts/utility.gsc"), None);
        assert_eq!(m("t5", "maps/_zombiemode.gsc"), Some(Mode::Zm));
        assert_eq!(m("t4", "maps/nazi_zombie_sumpf.gsc"), Some(Mode::Zm));
        assert_eq!(m("s1", "maps/mp/zombies/_zombies.gsc"), Some(Mode::Zm));
        assert_eq!(m("iw6", "maps/mp/alien/_utility.gsc"), Some(Mode::Zm));
        assert_eq!(m("iw5", "maps/so_survival.gsc"), Some(Mode::Sp));
        // a root above the scripts, Windows separators, any case
        assert_eq!(m("t6", "mymod\\Maps\\MP\\_load.gsc"), Some(Mode::Mp));
        // games without the layout: no mode
        assert_eq!(m("t7", "scripts/zm/_zm.gsc"), None);
        assert_eq!(m("iw7", "scripts/mp/utility.gsc"), None);
    }

    #[test]
    fn sets_and_overrides() {
        let s: Modes = [Mode::Zm, Mode::Mp].into_iter().collect();
        assert!(s.contains(Mode::Mp) && !s.contains(Mode::Sp));
        assert_eq!(s.labels(), "mp, zm");
        let o = ModeOverrides {
            globs: vec![("zm_*".into(), Some(Mode::Zm)), ("*.csc".into(), None)],
            all: Some(Some(Mode::Mp)),
        };
        assert_eq!(o.mode_of("maps/mp/zm_x.gsc"), Some(Some(Mode::Zm)));
        assert_eq!(o.mode_of("shared/b.csc"), Some(None));
        assert_eq!(o.mode_of("maps/x.gsc"), Some(Some(Mode::Mp)));
        assert_eq!(ModeOverrides::default().mode_of("x.gsc"), None);
        assert_eq!(parse_setting("ANY"), Ok(None));
        assert_eq!(parse_setting("zm"), Ok(Some(Mode::Zm)));
        assert!(parse_setting("coop").is_err());
    }
}
