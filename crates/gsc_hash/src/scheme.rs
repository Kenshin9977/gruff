//! How a game names script identifiers: which family hashes each kind of identifier, and how a
//! decompiler spells a hash it could not name (`function_1a2b3c4d`, `#"hash_..."`).
//!
//! In BO4 and later most identifiers reach the decompiler as hashes. A definition the decompiler
//! named by hash (`function_1a2b3c4d`) and a call it printed by name (`foo()`) are the same symbol
//! when `foo` hashes to `0x1a2b3c4d`: [`Scheme::key`] gives both the same key.

use crate::family::Family;
use crate::game::Game;
use crate::lookup::mask;

/// What an identifier is, for choosing its hash family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Role {
    /// Functions and methods (`function_<hex>`).
    Function,
    /// Fields and local variables (`var_<hex>`).
    Var,
    /// Namespaces (`namespace_<hex>`).
    Namespace,
    /// Classes (`class_<hex>`).
    Class,
    /// Events given as identifiers (`event_<hex>`); event strings are [`Role::String`].
    Event,
    /// `#"..."` script hashes (`#"hash_<hex>"`).
    String,
    /// Script file names (`script_<hex>`).
    Script,
    /// `@"..."` dvar hashes.
    Dvar,
}

impl Role {
    /// Every role.
    pub const ALL: &'static [Role] = &[
        Role::Function,
        Role::Var,
        Role::Namespace,
        Role::Class,
        Role::Event,
        Role::String,
        Role::Script,
        Role::Dvar,
    ];

    /// A short label (`function`, `var`...).
    pub fn label(self) -> &'static str {
        match self {
            Role::Function => "function",
            Role::Var => "var",
            Role::Namespace => "namespace",
            Role::Class => "class",
            Role::Event => "event",
            Role::String => "string",
            Role::Script => "script",
            Role::Dvar => "dvar",
        }
    }

    /// The decompiler prefix of an unnamed hash of this role.
    pub fn prefix(self) -> &'static str {
        match self {
            Role::Function => "function_",
            Role::Var => "var_",
            Role::Namespace => "namespace_",
            Role::Class => "class_",
            Role::Event => "event_",
            Role::String | Role::Dvar => "hash_",
            Role::Script => "script_",
        }
    }
}

/// Reads a decompiler's spelling of an unnamed hash: `prefix` then 1 to 16 hex digits. Words that
/// happen to be hex (`function_facade`) are not hashes: a value under 8 digits must hold a decimal
/// digit.
pub fn parse_spelling(written: &str, prefix: &str) -> Option<u64> {
    let hex = written
        .get(..prefix.len())
        .filter(|p| p.eq_ignore_ascii_case(prefix))
        .map(|_| &written[prefix.len()..])?;
    if hex.is_empty() || hex.len() > 16 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    if hex.len() < 8 && !hex.bytes().any(|b| b.is_ascii_digit()) {
        return None;
    }
    u64::from_str_radix(hex, 16).ok()
}

/// Any decompiler spelling of a hash: its role and value (`hash_` reads as [`Role::String`]).
pub fn parse_any_spelling(written: &str) -> Option<(Role, u64)> {
    [
        Role::Function,
        Role::Var,
        Role::Namespace,
        Role::Class,
        Role::Event,
        Role::String,
        Role::Script,
    ]
    .iter()
    .find_map(|&r| parse_spelling(written, r.prefix()).map(|v| (r, v)))
}

/// The families of one game's script identifiers.
#[derive(Clone, Copy, Debug)]
pub struct Scheme {
    pub game: Game,
    families: [Option<&'static Family>; 8],
}

fn fam(id: &str) -> Option<&'static Family> {
    crate::family(id)
}

impl Scheme {
    /// The scheme of a game whose scripts hash identifiers (BO3 and BO4 to BO7, MWII, MWIII), or
    /// `None` for a game that keeps them in clear or uses token ids.
    pub fn for_game(game: Game) -> Option<Scheme> {
        use Role::*;
        let mut s = Scheme {
            game,
            families: [None; 8],
        };
        let mut set = |roles: &[Role], id: &str| {
            for r in roles {
                s.families[*r as usize] = fam(id);
            }
        };
        match game {
            Game::T7 => set(
                &[Function, Var, Namespace, Class, Event, String],
                "t7.canon",
            ),
            Game::T8 | Game::T9 => {
                set(&[Function, Var, Namespace, Class, Event], "t89.scr");
                set(&[String, Script], "t8.fnv64");
            }
            Game::Iw9 | Game::Jup => {
                set(&[Function, Var, Namespace, Class], "iw9.scr");
                set(&[String, Event], "iw9.xhash");
                set(&[Script], "iw.asset");
            }
            Game::T10 | Game::Bo7 => {
                set(&[Function, Class], "t10.function");
                set(&[Var], "t10.var");
                set(&[Namespace], "t10.namespace");
                set(&[Event], "t10.event");
                set(&[String], "t10.xhash");
                set(&[Script], "t10.script");
                set(&[Dvar], "t10.dvar");
            }
            _ => return None,
        }
        Some(s)
    }

    /// The scheme of a gruff-data profile id (`t8`, `jup`, `t11`...).
    pub fn for_profile(id: &str) -> Option<Scheme> {
        Game::from_profile_id(id).and_then(Scheme::for_game)
    }

    /// The family that hashes identifiers of `role`, if the game hashes them.
    pub fn family(&self, role: Role) -> Option<&'static Family> {
        self.families[role as usize]
    }

    /// The distinct families of the scheme, in role order.
    pub fn families(&self) -> Vec<&'static Family> {
        let mut out: Vec<&'static Family> = Vec::new();
        for f in self.families.iter().flatten() {
            if !out.iter().any(|o| o.id == f.id) {
                out.push(f);
            }
        }
        out
    }

    /// The hash an identifier stands for: the value of a decompiler spelling (`function_1a2b`),
    /// else the hash of the name. Both are masked to the family's width, so a definition named by
    /// hash and a call written by name compare equal.
    pub fn key(&self, role: Role, written: &str) -> Option<u64> {
        let f = self.family(role)?;
        Some(match parse_spelling(written, role.prefix()) {
            Some(v) => v & mask(f.bits),
            None => f.hash(written),
        })
    }

    /// The value of a decompiler spelling of `role`, masked to the family's width; `None` for a
    /// name written in clear.
    pub fn spelled(&self, role: Role, written: &str) -> Option<u64> {
        let f = self.family(role)?;
        parse_spelling(written, role.prefix()).map(|v| v & mask(f.bits))
    }

    /// `#1a2b3c4d`: a hash as the toolchain displays it.
    pub fn display_hash(&self, role: Role, value: u64) -> String {
        let m = self.family(role).map_or(u64::MAX, |f| mask(f.bits));
        format!("#{:x}", value & m)
    }

    /// How a decompiler would spell `value` (`function_1a2b3c4d`).
    pub fn spelling(&self, role: Role, value: u64) -> String {
        format!("{}{value:x}", role.prefix())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings() {
        assert_eq!(
            parse_spelling("function_1a2b3c4d", "function_"),
            Some(0x1a2b_3c4d)
        );
        assert_eq!(parse_spelling("FUNCTION_1A2B", "function_"), Some(0x1a2b));
        assert_eq!(parse_spelling("function_facade", "function_"), None);
        assert_eq!(
            parse_spelling("function_deadbeef", "function_"),
            Some(0xdead_beef)
        );
        assert_eq!(parse_spelling("function_", "function_"), None);
        assert_eq!(parse_spelling("function_name", "function_"), None);
        assert_eq!(parse_spelling("var_12345678901234567", "var_"), None);
        assert_eq!(
            parse_any_spelling("namespace_5a1b"),
            Some((Role::Namespace, 0x5a1b))
        );
    }

    #[test]
    fn a_name_and_its_spelling_share_a_key() {
        for game in [
            Game::T7,
            Game::T8,
            Game::T9,
            Game::Jup,
            Game::T10,
            Game::Bo7,
        ] {
            let s = Scheme::for_game(game).unwrap();
            let h = s.key(Role::Function, "main").unwrap();
            assert_eq!(
                s.key(Role::Function, &s.spelling(Role::Function, h)),
                Some(h)
            );
            assert_eq!(s.spelled(Role::Function, "main"), None);
        }
        assert!(Scheme::for_game(Game::Iw5).is_none());
        let bo7 = Scheme::for_profile("t11").unwrap();
        assert_eq!(bo7.game, Game::Bo7);
        assert_eq!(bo7.family(Role::Var).unwrap().id, "t10.var");
        // 64-bit spellings are cut to the family's 60 bits.
        let v = bo7.key(Role::Function, "main").unwrap();
        let wide = format!("function_{:x}", v | 0xf000_0000_0000_0000);
        assert_eq!(bo7.key(Role::Function, &wide), Some(v));
        assert_eq!(bo7.display_hash(Role::Function, 0x0abc), "#abc");
    }
}
