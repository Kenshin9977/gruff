//! The games a hash family applies to.

use std::fmt;

/// A game (or engine generation). The id is the short engine code used across the toolchain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Game {
    /// Call of Duty (2003), United Offensive, Call of Duty 2.
    Iw1Iw2,
    /// Call of Duty 4: Modern Warfare.
    Iw3,
    /// World at War.
    T4,
    /// Modern Warfare 2 (2009).
    Iw4,
    /// Modern Warfare 3 (2011).
    Iw5,
    /// Black Ops.
    T5,
    /// Black Ops II.
    T6,
    /// Ghosts, Advanced Warfare, MW Remastered, Infinite Warfare, WWII (token-id games).
    Iw6ToS2,
    /// Black Ops III.
    T7,
    /// Black Ops 4.
    T8,
    /// Black Ops Cold War.
    T9,
    /// Modern Warfare (2019).
    Iw8,
    /// Modern Warfare II (2022).
    Iw9,
    /// Vanguard.
    S4,
    /// Modern Warfare 2 Campaign Remastered.
    H2,
    /// Modern Warfare III (2023).
    Jup,
    /// Black Ops 6.
    T10,
    /// Black Ops 7.
    Bo7,
    /// Call of Duty Online (IW4-based client, IFS archives).
    Codol,
}

impl Game {
    /// Every game, oldest first.
    pub const ALL: &'static [Game] = &[
        Game::Iw1Iw2,
        Game::Iw3,
        Game::T4,
        Game::Iw4,
        Game::Iw5,
        Game::T5,
        Game::T6,
        Game::Iw6ToS2,
        Game::T7,
        Game::T8,
        Game::T9,
        Game::Iw8,
        Game::Iw9,
        Game::S4,
        Game::H2,
        Game::Jup,
        Game::T10,
        Game::Bo7,
        Game::Codol,
    ];

    /// The short id (`t7`, `jup`, `bo7`...).
    pub fn id(self) -> &'static str {
        self.info().0
    }

    /// The common name.
    pub fn name(self) -> &'static str {
        self.info().1
    }

    /// (short id, common name).
    fn info(self) -> (&'static str, &'static str) {
        match self {
            Game::Iw1Iw2 => ("iw1", "CoD1 / UO / CoD2"),
            Game::Iw3 => ("iw3", "CoD4"),
            Game::T4 => ("t4", "World at War"),
            Game::Iw4 => ("iw4", "MW2 (2009)"),
            Game::Iw5 => ("iw5", "MW3 (2011)"),
            Game::T5 => ("t5", "Black Ops"),
            Game::T6 => ("t6", "Black Ops II"),
            Game::Iw6ToS2 => ("iw6-s2", "Ghosts to WWII"),
            Game::T7 => ("t7", "Black Ops III"),
            Game::T8 => ("t8", "Black Ops 4"),
            Game::T9 => ("t9", "Cold War"),
            Game::Iw8 => ("iw8", "MW (2019)"),
            Game::Iw9 => ("iw9", "MWII (2022)"),
            Game::S4 => ("s4", "Vanguard"),
            Game::H2 => ("h2", "MW2 Remastered"),
            Game::Jup => ("jup", "MWIII (2023)"),
            Game::T10 => ("t10", "Black Ops 6"),
            Game::Bo7 => ("bo7", "Black Ops 7"),
            Game::Codol => ("codol", "CoD Online"),
        }
    }

    /// Looks a game up by its id, case-insensitively.
    pub fn from_id(id: &str) -> Option<Game> {
        Game::ALL
            .iter()
            .copied()
            .find(|g| g.id().eq_ignore_ascii_case(id))
    }

    /// Looks a game up by a gruff-data profile id or a common alias as well (`t11` and `bo7`,
    /// `jup-sp`, `iw6`, `bo4`...), case-insensitively.
    pub fn from_profile_id(id: &str) -> Option<Game> {
        if let Some(g) = Game::from_id(id) {
            return Some(g);
        }
        let id = id.to_ascii_lowercase();
        Some(match id.as_str() {
            "t11" => Game::Bo7,
            "jup-sp" | "jup_sp" | "mwiii" | "mw3-2023" => Game::Jup,
            "bo6" => Game::T10,
            "bo4" => Game::T8,
            "cw" | "bocw" | "coldwar" => Game::T9,
            "bo3" => Game::T7,
            "iw1-uo" | "iw2" => Game::Iw1Iw2,
            "iw6" | "iw7" | "s1" | "h1" | "s2" => Game::Iw6ToS2,
            _ => return None,
        })
    }
}

impl fmt::Display for Game {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}
