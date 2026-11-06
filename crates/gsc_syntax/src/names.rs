//! E0301: decompiler names that no tokenizer can read as one name.
//!
//! A decompiler that resolves a hash through a string table may pick a colliding string with
//! spaces or a `:` in it (`init heli sounds max_accuracy = x;`, `example: level anim_hatch;`:
//! 11 files of the BO3 dumps). The text is not GSC, and the parser cannot recover the intended
//! name. In decompiler output only, the syntax errors of a line holding such a name become one
//! E0301, so that tools can tell these files from parser bugs.

use rowan::{TextRange, TextSize};

use crate::lexer::Token;
use crate::{SyntaxError, SyntaxKind};
use SyntaxKind::*;

/// Words that legally follow a receiver with only a space between: call modifiers.
const MODIFIERS: &[&str] = &["thread", "childthread", "call", "threadendon", "builtin"];

fn is_receiver(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        IDENT | SELF_KW | LEVEL_KW | GAME_KW | ANIM_KW | WORLD_KW
    )
}

/// The ranges of the spaced names in `text`: two words with only blanks between them that are
/// not a call (`self foo()`), or a word glued to a `:` followed by a blank and a word.
fn spaced_names(text: &str, tokens: &[Token]) -> Vec<TextRange> {
    // Non-trivia tokens with their offsets, and whether only blanks precede them.
    let mut toks: Vec<(SyntaxKind, usize, usize, bool)> = Vec::new();
    let mut pos = 0usize;
    let mut gap_blank = false;
    let mut gap_any = false;
    for t in tokens {
        let end = pos + t.len as usize;
        if t.kind.is_trivia() {
            let s = &text[pos..end];
            gap_any = true;
            gap_blank = t.kind == WHITESPACE && !s.contains(['\n', '\r']);
        } else {
            toks.push((t.kind, pos, end, gap_any && gap_blank));
            gap_any = false;
            gap_blank = false;
        }
        pos = end;
    }
    let word = |i: usize| &text[toks[i].1..toks[i].2];
    let modifier = |i: usize| MODIFIERS.iter().any(|m| word(i).eq_ignore_ascii_case(m));
    let mut out = Vec::new();
    for i in 1..toks.len() {
        let (k, _, end, spaced) = toks[i];
        let (pk, pstart, pend, _) = toks[i - 1];
        let next = toks.get(i + 1).map(|t| t.0);
        // `init heli`: two words, the second not called.
        if spaced
            && k == IDENT
            && is_receiver(pk)
            && !modifier(i)
            && !modifier(i - 1)
            && !matches!(next, Some(L_PAREN | COLON2))
        {
            out.push(range(pstart, end));
        }
        // `example: level`: a `:` glued to a word, then a blank and a word.
        if k == COLON && pk == IDENT && pend == toks[i].1 {
            if let Some(&(nk, _, _, nspaced)) = toks.get(i + 1) {
                if nspaced && (is_receiver(nk) || nk.is_keyword()) {
                    out.push(range(pstart, spaced_words_end(&toks, i + 1)));
                }
            }
        }
        // `level.+ level.x`, `level.*** client`: a field name starting with an operator.
        if pk == DOT && pend == toks[i].1 && k.is_punct() && !matches!(k, L_PAREN | L_BRACK) {
            let mut j = i;
            while toks
                .get(j + 1)
                .is_some_and(|t| t.0.is_punct() && !t.3 && t.1 == toks[j].2)
            {
                j += 1;
            }
            if let Some(&(nk, _, _, nspaced)) = toks.get(j + 1) {
                if nspaced && (is_receiver(nk) || nk.is_keyword()) {
                    out.push(range(toks[i].1, spaced_words_end(&toks, j + 1)));
                }
            }
        }
    }
    out
}

/// The end of the run of blank-separated words starting at `toks[i]`.
fn spaced_words_end(toks: &[(SyntaxKind, usize, usize, bool)], mut i: usize) -> usize {
    let mut end = toks[i].2;
    while let Some(&(k, _, e, spaced)) = toks.get(i + 1) {
        if !spaced || !(is_receiver(k) || k.is_keyword()) {
            break;
        }
        end = e;
        i += 1;
    }
    end
}

fn range(start: usize, end: usize) -> TextRange {
    TextRange::new(TextSize::from(start as u32), TextSize::from(end as u32))
}

fn line_of(starts: &[usize], offset: usize) -> usize {
    starts.partition_point(|&s| s <= offset) - 1
}

/// Replace the syntax errors of every line that holds a spaced name by one E0301.
/// `items` are the ranges of the top-level items of the tree.
pub(crate) fn reclassify(
    text: &str,
    tokens: &[Token],
    items: &[TextRange],
    errors: &mut Vec<SyntaxError>,
) {
    let names = spaced_names(text, tokens);
    if names.is_empty() {
        return;
    }
    let mut starts = vec![0usize];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    let mut by_line: Vec<(usize, TextRange)> = Vec::new();
    for r in names {
        let l = line_of(&starts, usize::from(r.start()));
        match by_line.last_mut() {
            Some((pl, pr)) if *pl == l => *pr = pr.cover(r),
            _ => by_line.push((l, r)),
        }
    }
    let flagged = |l: usize| by_line.binary_search_by_key(&l, |(x, _)| *x).ok();
    let mut hit = vec![false; by_line.len()];
    errors.retain(|e| {
        if e.code.starts_with("E01") && e.code != "E0102" {
            return true;
        }
        match flagged(line_of(&starts, usize::from(e.range.start()))) {
            Some(i) => {
                hit[i] = true;
                false
            }
            None => true,
        }
    });
    // Later errors of an item (a function) broken by such a name are its consequences.
    let first_hit_in = |at: TextSize| {
        let item = items.iter().find(|r| r.contains(at))?;
        by_line
            .iter()
            .zip(&hit)
            .filter(|(_, h)| **h)
            .map(|((_, r), _)| r.start())
            .find(|s| item.contains(*s))
    };
    errors.retain(|e| {
        if e.code.starts_with("E01") && e.code != "E0102" {
            return true;
        }
        first_hit_in(e.range.start()).is_none_or(|s| s > e.range.start())
    });
    for (i, (_, r)) in by_line.iter().enumerate() {
        if hit[i] {
            let name = &text[*r];
            errors.push(SyntaxError::new(
                "E0301",
                format!(
                    "decompiler name not tokenizable: `{name}` (a hash lookup resolved to text \
                     with spaces or `:`)"
                ),
                *r,
            ));
        }
    }
    errors.sort_by_key(|e| (e.range.start(), e.range.end()));
}

#[cfg(test)]
mod tests {
    use crate::{parse, Dialect};

    fn codes(text: &str) -> Vec<&'static str> {
        let d = Dialect::superset().with_decompiled(true);
        parse(text, &d).errors().iter().map(|e| e.code).collect()
    }

    #[test]
    fn spaced_names_become_e0301() {
        assert_eq!(
            codes("f()\n{\n    init heli sounds max_accuracy = x;\n    wait example: level anim_hatch;\n}\n"),
            ["E0301", "E0301"]
        );
        assert_eq!(
            codes("f()\n{\n    level.+ level.x = undefined;\n    level.*** client : _y = 0;\n}\n"),
            ["E0301", "E0301"]
        );
        // Later errors of the broken function are absorbed; other functions keep theirs.
        assert_eq!(
            codes("f()\n{\n    if ( a b c ) { x(); }\n    else\n    { y(); }\n}\ng()\n{\n    x = ;\n}\n"),
            ["E0301", "E0201"]
        );
    }

    #[test]
    fn calls_and_modifiers_are_not_names() {
        assert!(codes(
            "f()\n{\n    self foo();\n    level thread bar();\n    self childthread baz();\n}\n"
        )
        .is_empty());
        // A real syntax error on a line without a spaced name stays what it is.
        assert_eq!(codes("f()\n{\n    x = ;\n}\n"), ["E0201"]);
    }

    #[test]
    fn source_mode_is_untouched() {
        let d = Dialect::superset();
        let p = parse("f()\n{\n    init heli sounds = x;\n}\n", &d);
        assert!(p.errors().iter().all(|e| e.code != "E0301"));
    }
}
