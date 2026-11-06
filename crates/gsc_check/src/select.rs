//! Rule selection with Ruff's precedence: `select`, `extend-select` and `ignore`, in layers
//! (configuration file, then command line), the most specific selector winning inside a layer.

use crate::registry::{self, RULES};

/// A rule selector: `ALL`, a code prefix (`B`, `B00`, `E02`), a full code or a rule name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selector {
    All,
    /// Upper-case code prefix, the full code included.
    Prefix(String),
}

impl Selector {
    /// Parse a selector. Names resolve to their code; anything else must be a known prefix.
    pub fn parse(s: &str) -> Result<Selector, String> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("ALL") {
            return Ok(Selector::All);
        }
        if let Some(r) = RULES.iter().find(|r| r.name == s) {
            return Ok(Selector::Prefix(r.code.to_owned()));
        }
        let up = s.to_ascii_uppercase();
        let valid = !up.is_empty()
            && up.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && up[1..].chars().all(|c| c.is_ascii_digit())
            && (RULES.iter().any(|r| r.code.starts_with(&up))
                || (matches!(&up[..1], "E" | "D") && up.len() <= 5));
        if valid {
            Ok(Selector::Prefix(up))
        } else {
            Err(format!("unknown rule selector `{s}`"))
        }
    }

    pub fn matches(&self, code: &str) -> bool {
        match self {
            Selector::All => true,
            Selector::Prefix(p) => code.starts_with(p.as_str()),
        }
    }

    /// `ALL` < category < longer prefix < full code.
    pub fn specificity(&self) -> usize {
        match self {
            Selector::All => 0,
            Selector::Prefix(p) => p.len(),
        }
    }
}

impl std::fmt::Display for Selector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Selector::All => f.write_str("ALL"),
            Selector::Prefix(p) => f.write_str(p),
        }
    }
}

/// Parse a list of selectors.
pub fn parse_list<S: AsRef<str>>(items: &[S]) -> Result<Vec<Selector>, String> {
    items.iter().map(|s| Selector::parse(s.as_ref())).collect()
}

/// One layer of selection (a configuration file or the command line).
#[derive(Clone, Debug, Default)]
pub struct Layer {
    /// Replaces everything enabled by the layers below.
    pub select: Option<Vec<Selector>>,
    pub extend_select: Vec<Selector>,
    pub ignore: Vec<Selector>,
}

/// The resolved selection.
#[derive(Clone, Debug, Default)]
pub struct Selection {
    pub layers: Vec<Layer>,
    pub preview: bool,
}

impl Selection {
    /// Is `code` enabled? Stable rules are on by default, preview rules only with `preview`.
    pub fn enabled(&self, code: &str) -> bool {
        let preview_rule = registry::is_preview(code);
        if preview_rule && !self.preview {
            return false;
        }
        let mut on = true;
        for layer in &self.layers {
            let base = if layer.select.is_some() { false } else { on };
            let picks = layer
                .select
                .iter()
                .flatten()
                .chain(&layer.extend_select)
                .map(|s| (s, true))
                .chain(layer.ignore.iter().map(|s| (s, false)));
            // Most specific wins; on a tie, `ignore` wins.
            let best = picks
                .filter(|(s, _)| s.matches(code))
                .max_by_key(|(s, enable)| (s.specificity(), !*enable));
            on = best.map_or(base, |(_, enable)| enable);
        }
        on
    }
}

/// A glob over `/`-separated paths: `*` and `?` stay inside one component, `**` spans any number
/// of them. A pattern without `/` matches the file name anywhere.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let pattern = pattern.replace('\\', "/");
    let path = path.replace('\\', "/");
    if !pattern.contains('/') {
        let name = path.rsplit('/').next().unwrap_or(&path);
        return match_component(pattern.as_bytes(), name.as_bytes());
    }
    let pat: Vec<&str> = pattern.trim_start_matches("./").split('/').collect();
    let comps: Vec<&str> = path.trim_start_matches("./").split('/').collect();
    match_parts(&pat, &comps)
}

fn match_parts(pat: &[&str], comps: &[&str]) -> bool {
    match pat.split_first() {
        None => comps.is_empty(),
        Some((&"**", rest)) => (0..=comps.len()).any(|i| match_parts(rest, &comps[i..])),
        Some((p, rest)) => comps.split_first().is_some_and(|(c, crest)| {
            match_component(p.as_bytes(), c.as_bytes()) && match_parts(rest, crest)
        }),
    }
}

fn match_component(p: &[u8], s: &[u8]) -> bool {
    match p.split_first() {
        None => s.is_empty(),
        Some((b'*', rest)) => (0..=s.len()).any(|i| match_component(rest, &s[i..])),
        Some((b'?', rest)) => !s.is_empty() && match_component(rest, &s[1..]),
        Some((c, rest)) => s
            .split_first()
            .is_some_and(|(d, srest)| c.eq_ignore_ascii_case(d) && match_component(rest, srest)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(v: &[&str]) -> Vec<Selector> {
        parse_list(v).unwrap()
    }

    #[test]
    fn parses_selectors() {
        assert_eq!(Selector::parse("all").unwrap(), Selector::All);
        assert_eq!(
            Selector::parse("unknown-builtin").unwrap(),
            Selector::Prefix("B001".into())
        );
        assert_eq!(
            Selector::parse("b0").unwrap(),
            Selector::Prefix("B0".into())
        );
        assert_eq!(
            Selector::parse("E02").unwrap(),
            Selector::Prefix("E02".into())
        );
        assert!(Selector::parse("X1").is_err());
        assert!(Selector::parse("no-such-rule").is_err());
    }

    #[test]
    fn ruff_precedence() {
        let config = Layer {
            select: Some(sel(&["B"])),
            ignore: sel(&["B002"]),
            ..Layer::default()
        };
        let s = Selection {
            layers: vec![config.clone()],
            preview: false,
        };
        assert!(s.enabled("B001"));
        assert!(!s.enabled("B002"));
        assert!(!s.enabled("L001"));
        // More specific select beats a broader ignore in the same layer.
        let cli = Layer {
            extend_select: sel(&["B002", "L001"]),
            ignore: sel(&["B"]),
            ..Layer::default()
        };
        let s = Selection {
            layers: vec![config, cli],
            preview: false,
        };
        assert!(s.enabled("B002"));
        assert!(s.enabled("L001"));
        assert!(!s.enabled("B001"));
    }

    #[test]
    fn cli_select_replaces_config() {
        let s = Selection {
            layers: vec![
                Layer {
                    select: Some(sel(&["L"])),
                    ..Layer::default()
                },
                Layer {
                    select: Some(sel(&["B001"])),
                    ..Layer::default()
                },
            ],
            preview: false,
        };
        assert!(s.enabled("B001"));
        assert!(!s.enabled("L001"));
    }

    #[test]
    fn preview_rules_need_preview() {
        let mut s = Selection {
            layers: vec![Layer {
                select: Some(sel(&["B003"])),
                ..Layer::default()
            }],
            preview: false,
        };
        assert!(!s.enabled("B003"));
        s.preview = true;
        assert!(s.enabled("B003"));
        let default = Selection {
            layers: vec![],
            preview: false,
        };
        assert!(default.enabled("E0201"));
        assert!(!default.enabled("B003"));
    }

    #[test]
    fn globs() {
        assert!(glob_match("*.csc", "scripts/zm/a.csc"));
        assert!(glob_match("scripts/**/*.gsc", "scripts/zm/x/a.gsc"));
        assert!(glob_match("scripts/**/*.gsc", "scripts/a.gsc"));
        assert!(!glob_match("scripts/*.gsc", "scripts/zm/a.gsc"));
        assert!(glob_match("scripts\\zm\\*", "scripts/zm/a.gsc"));
    }
}
