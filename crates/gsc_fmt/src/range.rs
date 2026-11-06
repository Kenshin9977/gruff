//! Range formatting: format the whole file, then take only the statements that touch a range.
//!
//! The formatted file has the same lexemes as the input, so lexeme `i` of one is lexeme `i` of
//! the other. The edit replaces the input from the first to the last lexeme of the selected
//! statements by the same lexemes of the formatted file, whitespace between them included; the
//! text around the edit keeps its own whitespace. The result is verified like a full format.

use gsc_syntax::{Dialect, SyntaxKind, SyntaxNode, TextSize};
use SyntaxKind::*;

use crate::{format, trim_hspace_end, verify, FormatError, Options};

/// Replace bytes `start..end` of the input by `text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RangeEdit {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// The byte spans of the lexemes of `text` (see [`crate::lexemes`]).
fn spans(text: &str, dialect: &Dialect) -> Vec<(usize, usize)> {
    let (tokens, _) = gsc_syntax::tokenize(text, dialect);
    let mut out = Vec::with_capacity(tokens.len() / 2);
    let mut pos = 0usize;
    for t in tokens {
        let s = &text[pos..pos + t.len as usize];
        let start = pos;
        pos += t.len as usize;
        match t.kind {
            WHITESPACE => {}
            COMMENT => out.push((start, start + trim_hspace_end(s).len())),
            _ => out.push((start, pos)),
        }
    }
    out
}

/// Is the list member: a declaration, a statement, a class member or a `case`.
fn is_list(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SOURCE_FILE
            | BLOCK
            | SWITCH_STMT
            | SWITCH_CLAUSE
            | CLASS_DEF
            | DEV_BLOCK_STMT
            | DEV_BLOCK_ITEM
    )
}

/// The byte range of the innermost statement (or declaration) holding the token at `offset`.
fn member_at(root: &SyntaxNode, offset: usize) -> Option<(usize, usize)> {
    let tok = root
        .token_at_offset(TextSize::from(offset as u32))
        .right_biased()?;
    let node = tok
        .parent_ancestors()
        .find(|n| n.parent().is_some_and(|p| is_list(p.kind())))?;
    let r = node.text_range();
    Some((r.start().into(), r.end().into()))
}

fn starts_line(text: &str, at: usize) -> bool {
    text[..at]
        .chars()
        .rev()
        .find(|c| !matches!(c, ' ' | '\t'))
        .is_none_or(|c| c == '\n' || c == '\r')
}

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind(['\n', '\r']).map_or(0, |i| i + 1)
}

/// Format the statements of `text` that touch bytes `start..end`. Returns `None` when they are
/// already formatted.
pub fn format_range(
    text: &str,
    dialect: &Dialect,
    opts: &Options,
    start: usize,
    end: usize,
) -> Result<Option<RangeEdit>, FormatError> {
    let formatted = format(text, dialect, opts)?;
    if formatted == text {
        return Ok(None);
    }
    let a = spans(text, dialect);
    let b = spans(&formatted, dialect);
    if a.len() != b.len() {
        return Err(FormatError::Unsafe("lexeme counts differ".to_owned()));
    }
    let (start, end) = (start.min(text.len()), end.min(text.len()).max(start));
    // The lexemes touching the range, then the statements holding them.
    let first = a.partition_point(|s| s.1 <= start);
    let last = a.partition_point(|s| s.0 < end.max(start + 1));
    if first >= a.len() || last == 0 || first >= last {
        return Ok(None);
    }
    let last = last - 1;
    let root = gsc_syntax::parse(text, dialect).syntax();
    let lo = member_at(&root, a[first].0).map_or(a[first].0, |m| m.0.min(a[first].0));
    let hi = member_at(&root, a[last].0).map_or(a[last].1, |m| m.1.max(a[last].1));
    let i = a.partition_point(|s| s.0 < lo);
    let j = a.partition_point(|s| s.1 <= hi);
    if j == 0 || i >= j {
        return Ok(None);
    }
    let j = j - 1;
    let (mut in_s, mut out_s) = (a[i].0, b[i].0);
    if starts_line(text, in_s) && starts_line(&formatted, out_s) {
        in_s = line_start(text, in_s);
        out_s = line_start(&formatted, out_s);
    }
    let in_e = a[j].1;
    let new_text = &formatted[out_s..b[j].1];
    if &text[in_s..in_e] == new_text {
        return Ok(None);
    }
    let mut result = String::with_capacity(text.len());
    result.push_str(&text[..in_s]);
    result.push_str(new_text);
    result.push_str(&text[in_e..]);
    verify(text, &result, dialect).map_err(FormatError::Unsafe)?;
    Ok(Some(RangeEdit {
        start: in_s,
        end: in_e,
        text: new_text.to_owned(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_only_the_touched_statement() {
        let d = Dialect::superset();
        let text = "f()\n{\nx=1;\ny=2;\n}\n";
        let opts = Options::default();
        let at = text.find("y=2").unwrap();
        let e = format_range(text, &d, &opts, at, at + 1).unwrap().unwrap();
        let mut out = text.to_owned();
        out.replace_range(e.start..e.end, &e.text);
        assert_eq!(out, "f()\n{\nx=1;\n\ty = 2;\n}\n");
        // Already formatted: nothing to do.
        let again = format_range(&out, &d, &opts, at, at + 1).unwrap();
        assert!(again.is_none_or(|e| out[e.start..e.end] == e.text));
    }
}
