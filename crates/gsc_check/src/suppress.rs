//! Suppression comments and S100 unused-suppression.
//!
//! `// gsc: ignore[B001, L001]` suppresses those codes on its line; a comment alone on its line
//! also covers the next line. `// gsc: ignore-file[B001]` covers the whole file. Without a list
//! (`// gsc: ignore`) every code is suppressed. Selectors may be codes, prefixes or names.
//! `// gruff: ignore[...]` is the same comment under the tool's name.

use gsc_syntax::{LineIndex, SyntaxKind, SyntaxNode, TextRange, TextSize};

use crate::select::Selector;
use crate::{Applicability, Diagnostic, Edit, Fix};

/// One selector in a suppression comment.
#[derive(Clone, Debug)]
struct Item {
    raw: String,
    selector: Option<Selector>,
    used: bool,
}

/// A parsed suppression comment.
#[derive(Clone, Debug)]
struct Suppression {
    /// The comment token.
    range: TextRange,
    file_level: bool,
    /// `None`: every code.
    items: Option<Vec<Item>>,
    used_bare: bool,
    /// 1-based lines it covers (ignored for file-level ones).
    lines: (u32, u32),
    /// Byte range of the text between `[` and `]` inside the comment.
    list_range: Option<TextRange>,
    alone_on_line: bool,
}

/// The text between `[` and `]`, with its byte offsets in the comment.
type List = Option<(String, usize, usize)>;

/// The prefixes of a suppression comment: `// gsc: ignore` or `// gruff: ignore`.
const PREFIXES: &[&str] = &["gsc:", "gruff:"];

/// Parse `// gsc: ignore[...]` / `// gsc: ignore-file[...]` (or `gruff:`): (file level, list).
fn parse_comment(text: &str) -> Option<(bool, List)> {
    let body = text.strip_prefix("//")?;
    let lead = body.len() - body.trim_start().len();
    let (rest, prefix_len) = PREFIXES.iter().find_map(|p| {
        let r = body.trim_start().strip_prefix(p)?;
        Some((r, p.len()))
    })?;
    let after_colon = 2 + lead + prefix_len;
    let lead2 = rest.len() - rest.trim_start().len();
    let rest = rest.trim_start();
    let (file_level, tail, kw_len) = if let Some(t) = rest.strip_prefix("ignore-file") {
        (true, t, "ignore-file".len())
    } else if let Some(t) = rest.strip_prefix("ignore") {
        (false, t, "ignore".len())
    } else {
        return None;
    };
    if let Some(t) = tail.strip_prefix('[') {
        let close = t.find(']')?;
        let start = after_colon + lead2 + kw_len + 1;
        return Some((
            file_level,
            Some((t[..close].to_owned(), start, start + close)),
        ));
    }
    // A bare `ignore` must end there (or be followed by a reason after whitespace).
    if tail.is_empty() || tail.starts_with(char::is_whitespace) {
        return Some((file_level, None));
    }
    None
}

fn collect(root: &SyntaxNode, text: &str, index: &LineIndex) -> Vec<Suppression> {
    let mut out = Vec::new();
    for tok in root
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| t.kind() == SyntaxKind::COMMENT)
    {
        let Some((file_level, list)) = parse_comment(tok.text()) else {
            continue;
        };
        let range = tok.text_range();
        let line = index.line_col(text, range.start()).line;
        let line_range = index.line_range(text, line).unwrap_or(range);
        let before = &text[usize::from(line_range.start())..usize::from(range.start())];
        let alone_on_line = before.trim().is_empty();
        let lines = if alone_on_line {
            (line, line + 1)
        } else {
            (line, line)
        };
        let (items, list_range) = match list {
            None => (None, None),
            Some((inner, s, e)) => {
                let items = inner
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|raw| Item {
                        raw: raw.to_owned(),
                        selector: Selector::parse(raw).ok(),
                        used: false,
                    })
                    .collect();
                let base = range.start();
                (
                    Some(items),
                    Some(TextRange::new(
                        base + TextSize::from(s as u32),
                        base + TextSize::from(e as u32),
                    )),
                )
            }
        };
        out.push(Suppression {
            range,
            file_level,
            items,
            used_bare: false,
            lines,
            list_range,
            alone_on_line,
        });
    }
    out
}

/// Mark `s` as used by `code` when it covers it.
fn consume(s: &mut Suppression, code: &str) -> bool {
    match &mut s.items {
        None => {
            s.used_bare = true;
            true
        }
        Some(items) => {
            let mut hit = false;
            for it in items.iter_mut() {
                if it.selector.as_ref().is_some_and(|sel| sel.matches(code)) {
                    it.used = true;
                    hit = true;
                }
            }
            hit
        }
    }
}

/// The range deleting a whole comment: its line when alone, else the comment and the spaces
/// before it.
fn removal_range(text: &str, s: &Suppression) -> TextRange {
    let start = usize::from(s.range.start());
    let end = usize::from(s.range.end());
    if s.alone_on_line {
        let line_start = text[..start].rfind(['\n', '\r']).map_or(0, |i| i + 1);
        let mut e = end;
        if text[e..].starts_with("\r\n") {
            e += 2;
        } else if text[e..].starts_with('\n') || text[e..].starts_with('\r') {
            e += 1;
        }
        TextRange::new(TextSize::from(line_start as u32), TextSize::from(e as u32))
    } else {
        let trimmed = text[..start].trim_end_matches([' ', '\t']).len();
        TextRange::new(TextSize::from(trimmed as u32), TextSize::from(end as u32))
    }
}

/// Apply the file's suppressions to `diags` (removing what they cover) and append S100
/// diagnostics. `enabled(code)` says whether a rule ran on this file; `s100` whether S100 is on.
pub fn apply(
    root: &SyntaxNode,
    text: &str,
    index: &LineIndex,
    diags: &mut Vec<Diagnostic>,
    enabled: &dyn Fn(&str) -> bool,
    s100: bool,
) {
    let mut sups = collect(root, text, index);
    if sups.is_empty() {
        return;
    }
    diags.retain(|d| {
        let line = index.line_col(text, d.range.start()).line;
        let mut suppressed = false;
        for s in sups.iter_mut() {
            let covers = s.file_level || (s.lines.0..=s.lines.1).contains(&line);
            if covers && consume(s, &d.code) {
                suppressed = true;
            }
        }
        !suppressed
    });
    if !s100 {
        return;
    }
    let any_enabled = |sel: &Selector| {
        crate::registry::RULES
            .iter()
            .any(|r| sel.matches(r.code) && enabled(r.code))
    };
    let mut extra = Vec::new();
    for s in &sups {
        let kind = if s.file_level {
            "ignore-file"
        } else {
            "ignore"
        };
        match &s.items {
            None => {
                if !s.used_bare {
                    extra.push(Diagnostic::new(
                        "S100",
                        format!("unused suppression: `{kind}` suppresses nothing here"),
                        s.range,
                        Some(Fix {
                            applicability: Applicability::Safe,
                            message: "Remove the suppression comment".to_owned(),
                            edits: vec![Edit {
                                range: removal_range(text, s),
                                content: String::new(),
                            }],
                        }),
                    ));
                }
            }
            Some(items) => {
                let unused: Vec<&Item> = items
                    .iter()
                    .filter(|it| match &it.selector {
                        None => true,
                        Some(sel) => !it.used && any_enabled(sel),
                    })
                    .collect();
                if unused.is_empty() {
                    continue;
                }
                let kept: Vec<&str> = items
                    .iter()
                    .filter(|it| !unused.iter().any(|u| std::ptr::eq(*u, *it)))
                    .map(|it| it.raw.as_str())
                    .collect();
                let edit = if kept.is_empty() {
                    Edit {
                        range: removal_range(text, s),
                        content: String::new(),
                    }
                } else {
                    Edit {
                        range: s.list_range.unwrap_or(s.range),
                        content: kept.join(", "),
                    }
                };
                let names: Vec<String> = unused.iter().map(|u| format!("`{}`", u.raw)).collect();
                let unknown = unused.iter().any(|u| u.selector.is_none());
                let message = if unknown {
                    format!(
                        "suppression names unknown or unused codes: {}",
                        names.join(", ")
                    )
                } else {
                    format!(
                        "unused suppression: {} {} nothing here",
                        names.join(", "),
                        if names.len() == 1 {
                            "suppresses"
                        } else {
                            "suppress"
                        }
                    )
                };
                extra.push(Diagnostic::new(
                    "S100",
                    message,
                    s.range,
                    Some(Fix {
                        applicability: Applicability::Safe,
                        message: if kept.is_empty() {
                            "Remove the suppression comment".to_owned()
                        } else {
                            "Remove the unused codes".to_owned()
                        },
                        edits: vec![edit],
                    }),
                ));
            }
        }
    }
    // `ignore-file[S100]` silences S100 itself.
    extra.retain(|d| {
        !sups.iter().any(|s| {
            s.file_level
                && s.items.as_ref().is_some_and(|items| {
                    items
                        .iter()
                        .any(|it| it.selector.as_ref().is_some_and(|sel| sel.matches(&d.code)))
                })
        })
    });
    diags.extend(extra);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comments() {
        assert_eq!(
            parse_comment("// gsc: ignore[B001]"),
            Some((false, Some(("B001".to_owned(), 15, 19))))
        );
        assert_eq!(
            parse_comment("//gsc:ignore-file[B001, L001] reason"),
            Some((true, Some(("B001, L001".to_owned(), 18, 28))))
        );
        assert_eq!(parse_comment("// gsc: ignore"), Some((false, None)));
        assert_eq!(parse_comment("// gsc: ignored"), None);
        assert_eq!(parse_comment("// ignore[B001]"), None);
        let t = "//gsc:ignore-file[B001, L001] reason";
        assert_eq!(&t[18..28], "B001, L001");
        let t = "// gsc: ignore[B001]";
        assert_eq!(&t[15..19], "B001");
        let t = "// gruff: ignore[B001, L001]";
        assert_eq!(
            parse_comment(t),
            Some((false, Some(("B001, L001".to_owned(), 17, 27))))
        );
        assert_eq!(&t[17..27], "B001, L001");
        assert_eq!(parse_comment("//gruff:ignore-file"), Some((true, None)));
    }
}
