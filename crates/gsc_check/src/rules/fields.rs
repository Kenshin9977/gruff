//! F001 unknown-field (preview): a field read whose name neither the engine
//! (`gruff-data/fields/<game>.toml`) nor any script of the workspace ever sets, and that is one or
//! two edits away from a name that is set: a likely typo. Names set outside scripts (map and
//! GDT key-value pairs) are why the rule only reports near misses.

use std::collections::HashMap;

use gsc_syntax::SyntaxKind::FIELD_EXPR;

use super::{in_error, Ctx};
use gsc_sema::data::levenshtein;

use crate::workspace::{field_name, is_field_write, is_hashed_name};
use crate::{Applicability, Edit, Fix};

/// `origin1` and `origin2`: numbered siblings, not typos of each other.
fn numbered_siblings(a: &str, b: &str) -> bool {
    let strip = |s: &str| s.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let (la, lb) = (strip(a), strip(b));
    (la < a.len() || lb < b.len()) && a[..la] == b[..lb]
}

/// Typo candidates: same length, one substituted letter or two swapped ones. Inserted or
/// dropped characters (`_off`/`off`, `sound_ent`/`sound_ents`) are naming variants far more
/// often than typos, so they do not count.
fn suggest<'a>(name: &str, known: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut best: Vec<(usize, &str)> = known
        .filter(|k| k.len() == name.len() && *k != name && !numbered_siblings(name, k))
        .filter(|k| {
            let differ: Vec<(u8, u8)> = name
                .bytes()
                .zip(k.bytes())
                .filter(|(a, b)| a != b)
                .collect();
            // A changed underscore is a naming variant, `offset_x`/`offset_y` are siblings.
            let axis = |c: &u8| matches!(c, b'x' | b'y' | b'z');
            !differ
                .iter()
                .any(|(a, b)| *a == b'_' || *b == b'_' || (axis(a) && axis(b)))
        })
        .filter_map(|k| levenshtein(name, k, 1).map(|d| (d, k)))
        .collect();
    best.sort();
    best.dedup_by_key(|(_, k)| *k);
    let Some(&(top, _)) = best.first() else {
        return Vec::new();
    };
    best.into_iter()
        .take_while(|(d, _)| *d == top)
        .map(|(_, k)| k)
        .collect()
}

pub fn check(ctx: &mut Ctx) {
    let Some(cat) = ctx.fields else { return };
    let written = ctx.sums.fields_written();
    let mut memo: HashMap<String, Vec<String>> = HashMap::new();
    let reads: Vec<_> = ctx
        .root
        .descendants()
        .filter(|n| n.kind() == FIELD_EXPR)
        .collect();
    for n in reads {
        if is_field_write(&n) || in_error(&n) {
            continue;
        }
        let Some(name) = field_name(&n) else { continue };
        if name.len() < 5 || name.starts_with("script_") || is_hashed_name(&name) {
            continue;
        }
        if cat.contains(&name) || written.contains(&name) {
            continue;
        }
        if ctx.near_syntax_error(&n) {
            continue;
        }
        let sugg = memo
            .entry(name.clone())
            .or_insert_with(|| {
                let known = cat
                    .names
                    .iter()
                    .map(String::as_str)
                    .chain(written.iter().map(String::as_str));
                suggest(&name, known)
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            })
            .clone();
        if sugg.is_empty() {
            continue;
        }
        let Some(range) = ast_field_range(&n) else {
            continue;
        };
        let list: Vec<String> = sugg.iter().map(|s| format!("`{s}`")).collect();
        let fix = match sugg.as_slice() {
            [only] => Some(Fix {
                applicability: Applicability::Unsafe,
                message: format!("Replace with `{only}`"),
                edits: vec![Edit {
                    range,
                    content: only.clone(),
                }],
            }),
            _ => None,
        };
        ctx.report(
            "F001",
            format!(
                "field `{name}` is never set by the engine or any script here; did you mean {}?",
                list.join(" or ")
            ),
            range,
            fix,
        );
    }
}

fn ast_field_range(n: &gsc_syntax::SyntaxNode) -> Option<gsc_syntax::TextRange> {
    use gsc_syntax::ast::AstNode;
    let fe = gsc_syntax::ast::FieldExpr::cast(n.clone())?;
    Some(fe.field()?.syntax().text_range())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions() {
        let known = ["health", "maxhealth", "origin1", "angles"];
        assert_eq!(suggest("helath", known.iter().copied()), vec!["health"]);
        assert!(suggest("origin2", known.iter().copied()).is_empty());
        assert!(suggest("zzzzzz", known.iter().copied()).is_empty());
    }
}
