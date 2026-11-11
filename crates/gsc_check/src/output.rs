//! Output formats: `text` (with a source snippet), `concise`, `json`, `github` and `sarif`
//! (2.1.0), plus the `--statistics` table.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use gsc_syntax::LineIndex;

use crate::registry::{self, RULES};
use crate::{Applicability, Diagnostic, FileResult};

/// How diagnostics are printed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Text,
    Concise,
    Json,
    Github,
    Sarif,
}

impl Format {
    pub fn parse(s: &str) -> Option<Format> {
        Some(match s {
            "text" | "full" => Format::Text,
            "concise" => Format::Concise,
            "json" => Format::Json,
            "github" => Format::Github,
            "sarif" => Format::Sarif,
            _ => return None,
        })
    }
}

/// Every diagnostic with its file, in output order.
fn all(results: &[FileResult]) -> impl Iterator<Item = (&FileResult, &Diagnostic)> {
    results
        .iter()
        .flat_map(|r| r.diagnostics.iter().map(move |d| (r, d)))
}

fn fix_marker(d: &Diagnostic) -> &'static str {
    match &d.fix {
        Some(f) if f.applicability == Applicability::Safe => "[*] ",
        _ => "",
    }
}

/// Render diagnostics in `format`.
pub fn render(results: &[FileResult], format: Format) -> String {
    match format {
        Format::Text => text(results, true),
        Format::Concise => text(results, false),
        Format::Json => json(results),
        Format::Github => github(results),
        Format::Sarif => sarif(results),
    }
}

/// Expand tabs to 4 columns; returns the line and the display column of each char boundary.
fn expand(line: &str) -> (String, Vec<usize>) {
    let mut out = String::new();
    let mut cols = Vec::with_capacity(line.len() + 1);
    let mut col = 0;
    for c in line.chars() {
        cols.push(col);
        if c == '\t' {
            let n = 4 - col % 4;
            out.extend(std::iter::repeat_n(' ', n));
            col += n;
        } else {
            out.push(c);
            col += 1;
        }
    }
    cols.push(col);
    (out, cols)
}

fn text(results: &[FileResult], snippets: bool) -> String {
    let mut out = String::new();
    for r in results {
        if r.diagnostics.is_empty() {
            continue;
        }
        let src = &r.source.text;
        let index = snippets.then(|| LineIndex::new(src));
        for d in &r.diagnostics {
            let _ = writeln!(
                out,
                "{}:{}:{}: {} {}{}",
                r.display,
                d.start.line,
                d.start.col,
                d.code,
                fix_marker(d),
                d.message
            );
            let Some(index) = &index else { continue };
            let Some(lr) = index.line_range(src, d.start.line) else {
                continue;
            };
            let line = &src[usize::from(lr.start())..usize::from(lr.end())];
            let (shown, cols) = expand(line);
            let nchars = line.chars().count();
            let c0 = (d.start.col as usize - 1).min(nchars);
            let c1 = if d.end.line == d.start.line {
                (d.end.col as usize - 1).clamp(c0, nchars)
            } else {
                nchars
            };
            let (x0, x1) = (cols[c0], cols[c1]);
            let width = d.start.line.to_string().len();
            let carets = "^".repeat((x1 - x0).max(1));
            let _ = writeln!(out, "{:width$} |", "");
            let _ = writeln!(out, "{} | {}", d.start.line, shown.trim_end());
            let _ = writeln!(out, "{:width$} | {:x0$}{carets}", "", "");
            if let Some(f) = &d.fix {
                let label = match f.applicability {
                    Applicability::Safe => "fix",
                    Applicability::Unsafe => "unsafe fix",
                };
                let _ = writeln!(out, "{:width$} = {label}: {}", "", f.message);
            }
            out.push('\n');
        }
    }
    out
}

/// Escape a JSON string.
pub fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json(results: &[FileResult]) -> String {
    let mut items = Vec::new();
    for r in results {
        let index = (!r.diagnostics.is_empty()).then(|| LineIndex::new(&r.source.text));
        for d in &r.diagnostics {
            let fix = match (&d.fix, &index) {
                (Some(f), Some(ix)) => {
                    let edits: Vec<String> = f
                        .edits
                        .iter()
                        .map(|e| {
                            let s = ix.line_col(&r.source.text, e.range.start());
                            let t = ix.line_col(&r.source.text, e.range.end());
                            format!(
                                "{{\"content\":{},\"location\":{{\"row\":{},\"column\":{}}},\"end_location\":{{\"row\":{},\"column\":{}}}}}",
                                json_str(&e.content),
                                s.line,
                                s.col,
                                t.line,
                                t.col
                            )
                        })
                        .collect();
                    format!(
                        "{{\"applicability\":{},\"message\":{},\"edits\":[{}]}}",
                        json_str(f.applicability.label()),
                        json_str(&f.message),
                        edits.join(",")
                    )
                }
                _ => "null".to_owned(),
            };
            items.push(format!(
                "  {{\"code\":{},\"name\":{},\"message\":{},\"filename\":{},\"location\":{{\"row\":{},\"column\":{}}},\"end_location\":{{\"row\":{},\"column\":{}}},\"fix\":{}}}",
                json_str(&d.code),
                json_str(d.name()),
                json_str(&d.message),
                json_str(&r.display),
                d.start.line,
                d.start.col,
                d.end.line,
                d.end.col,
                fix
            ));
        }
    }
    if items.is_empty() {
        "[]\n".to_owned()
    } else {
        format!("[\n{}\n]\n", items.join(",\n"))
    }
}

/// Escape a GitHub workflow command property or message.
fn gh_escape(s: &str, property: bool) -> String {
    let mut out = s
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A");
    if property {
        out = out.replace(':', "%3A").replace(',', "%2C");
    }
    out
}

fn github(results: &[FileResult]) -> String {
    let mut out = String::new();
    for (r, d) in all(results) {
        let path = r.display.replace('\\', "/");
        let _ = writeln!(
            out,
            "::error title={},file={},line={},col={},endLine={},endColumn={}::{}",
            gh_escape(&format!("gruff ({} {})", d.code, d.name()), true),
            gh_escape(&path, true),
            d.start.line,
            d.start.col,
            d.end.line,
            d.end.col,
            gh_escape(&format!("{} {}", d.code, d.message), false)
        );
    }
    out
}

/// A file URI or a relative URI reference for SARIF.
fn sarif_uri(display: &str) -> String {
    let p = display.replace('\\', "/");
    let encoded: String = p
        .chars()
        .map(|c| match c {
            ' ' => "%20".to_owned(),
            '#' => "%23".to_owned(),
            '%' => "%25".to_owned(),
            c => c.to_string(),
        })
        .collect();
    if encoded.len() > 1 && encoded.as_bytes()[1] == b':' {
        format!("file:///{encoded}")
    } else if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        encoded
    }
}

fn sarif(results: &[FileResult]) -> String {
    let mut used: BTreeMap<&str, ()> = BTreeMap::new();
    for (_, d) in all(results) {
        used.insert(d.code.as_str(), ());
    }
    let rules: Vec<String> = used
        .keys()
        .map(|code| {
            let summary = registry::rule(code).map_or("", |r| r.summary);
            format!(
                "{{\"id\":{},\"name\":{},\"shortDescription\":{{\"text\":{}}},\"help\":{{\"text\":{}}}}}",
                json_str(code),
                json_str(registry::name_of(code)),
                json_str(summary),
                json_str(&format!("gruff check --explain {code}"))
            )
        })
        .collect();
    let results: Vec<String> = all(results)
        .map(|(r, d)| {
            format!(
                "{{\"ruleId\":{},\"level\":\"{}\",\"message\":{{\"text\":{}}},\"locations\":[{{\"physicalLocation\":{{\"artifactLocation\":{{\"uri\":{}}},\"region\":{{\"startLine\":{},\"startColumn\":{},\"endLine\":{},\"endColumn\":{}}}}}}}]}}",
                json_str(&d.code),
                if d.severity.is_hint() { "note" } else { "error" },
                json_str(&d.message),
                json_str(&sarif_uri(&r.display)),
                d.start.line,
                d.start.col,
                d.end.line,
                d.end.col
            )
        })
        .collect();
    format!(
        "{{\"$schema\":\"https://json.schemastore.org/sarif-2.1.0.json\",\"version\":\"2.1.0\",\"runs\":[{{\"tool\":{{\"driver\":{{\"name\":\"gruff\",\"version\":{},\"rules\":[{}]}}}},\"results\":[{}]}}]}}\n",
        json_str(env!("CARGO_PKG_VERSION")),
        rules.join(","),
        results.join(",")
    )
}

/// `--statistics`: count, code, name and fix marker per code, most frequent first.
pub fn statistics(results: &[FileResult]) -> String {
    let mut counts: BTreeMap<&str, (usize, bool)> = BTreeMap::new();
    for (_, d) in all(results) {
        let e = counts.entry(d.code.as_str()).or_default();
        e.0 += 1;
        e.1 |= d
            .fix
            .as_ref()
            .is_some_and(|f| f.applicability == Applicability::Safe);
    }
    let mut rows: Vec<_> = counts.into_iter().collect();
    rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(b.0)));
    let width = rows.first().map_or(1, |r| r.1 .0.to_string().len());
    let mut out = String::new();
    for (code, (n, fixable)) in rows {
        let _ = writeln!(
            out,
            "{n:>width$}\t{code}\t{}{}",
            if fixable { "[*] " } else { "" },
            registry::name_of(code)
        );
    }
    out
}

/// The `--explain` page of a rule (code or name).
pub fn explain(code: &str) -> Option<String> {
    let Some(r) = registry::rule(code) else {
        // A code `gsc_syntax` added after this table: describe the family.
        let up = code.to_ascii_uppercase();
        let family = match up.as_bytes().first() {
            Some(b'E') => "a syntax error",
            Some(b'D') => "a dialect diagnostic",
            _ => return None,
        };
        if up.len() < 2 || !up[1..].bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        return Some(format!(
            "# {up} {}\n\n{up} is {family} reported by the parser; this build has no page for it. \
             See `gruff check --list-rules`.\n",
            registry::name_of(&up)
        ));
    };
    let mut out = String::from(r.doc);
    if r.preview {
        out.push_str("\nThis rule is in preview: enable it with `--preview`.\n");
    }
    Some(out)
}

/// The rule list (`--list-rules`).
pub fn rule_list() -> String {
    let mut out = String::new();
    for r in RULES {
        let mut tags = Vec::new();
        if r.preview {
            tags.push("preview");
        }
        if r.pending {
            tags.push("pending data");
        }
        if r.fix != registry::FixAvailability::None {
            tags.push("fix");
        }
        let tags = if tags.is_empty() {
            String::new()
        } else {
            format!(" ({})", tags.join(", "))
        };
        let _ = writeln!(out, "{:<6}{:<30}{}{tags}", r.code, r.name, r.summary);
    }
    out
}
