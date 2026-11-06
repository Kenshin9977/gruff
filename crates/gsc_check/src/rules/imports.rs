//! I001 duplicate-import, I002 unused-import, I003 unresolved-import.
//!
//! After gscode's `DuplicateImportLint`, `UnusedUsingLint`, `UnusedIncludeLint` and
//! `UsingNotFoundLint`/`InsertNotFound` (Blakintosh/gscode, GPL-3.0). An import is kept as used
//! whenever it might be: it supplies a name the file refers to (by any spelling a macro body or a
//! qualifier holds), it declares a namespace the file qualifies a call with, or loading it has an
//! effect of its own (an `autoexec`, `codecall` or `event_handler` function, a file-scope macro
//! such as `REGISTER_SYSTEM`) anywhere in what it imports in turn.

use std::collections::{HashMap, HashSet};

use gsc_sema::{FileId, Resolution, Target};
use gsc_syntax::{NodeOrToken, SyntaxKind::*, SyntaxNode, TextRange, TextSize};

use super::Ctx;
use crate::{Applicability, Edit, Fix};

pub fn check(ctx: &mut Ctx) {
    let ws = ctx.ws;
    let facts = &ws.file(ctx.file).facts;
    let res = ws.resolution(ctx.file);
    let mut found: Vec<(&str, String, TextRange, Option<Fix>)> = Vec::new();

    // I001 and I003. Without the game's scripts as a library root every stock import would be
    // missing: I003 needs one.
    let with_library = ws.roots().iter().any(|r| r.library);
    let mut seen: HashMap<(&str, String), ()> = HashMap::new();
    for (imp, target) in facts.imports.iter().zip(&res.imports) {
        let key = (imp.directive, imp.target.to_ascii_lowercase());
        if seen.insert(key, ()).is_some() {
            if ctx.on("I001") {
                let fix = directive_node(ctx.root, imp.range).map(|n| Fix {
                    applicability: Applicability::Safe,
                    message: format!("Remove the second `#{}`", imp.directive),
                    edits: vec![delete_line(ctx.text, n.text_range())],
                });
                found.push((
                    "I001",
                    format!(
                        "`{}` is already imported by an earlier `#{}`",
                        spelled(ctx.text, imp.range),
                        imp.directive
                    ),
                    imp.range,
                    fix,
                ));
            }
            continue;
        }
        if target.is_none() && ctx.on("I003") && with_library && directory_known(ctx, &imp.target) {
            found.push((
                "I003",
                format!(
                    "`#{} {}`: no such file in the workspace (the compiler refuses a missing \
                     import)",
                    imp.directive,
                    spelled(ctx.text, imp.range)
                ),
                imp.range,
                None,
            ));
        }
    }

    // I002.
    if ctx.on("I002") && !ctx.decompiled {
        unused(ctx, &mut found);
    }

    for (code, message, range, fix) in found {
        ctx.report(code, message, range, fix);
    }
}

fn spelled(text: &str, range: TextRange) -> &str {
    &text[usize::from(range.start())..usize::from(range.end())]
}

/// The directive whose path token is at `range`.
fn directive_node(root: &SyntaxNode, range: TextRange) -> Option<SyntaxNode> {
    let start = match root.covering_element(range) {
        NodeOrToken::Node(n) => n,
        NodeOrToken::Token(t) => t.parent()?,
    };
    start.ancestors().find(|n| {
        matches!(
            n.kind(),
            USING_DIRECTIVE | INCLUDE_DIRECTIVE | INSERT_DIRECTIVE | INLINE_DIRECTIVE
        )
    })
}

/// Delete `range` with its line when nothing else is on it.
fn delete_line(text: &str, range: TextRange) -> Edit {
    let (s, e) = (usize::from(range.start()), usize::from(range.end()));
    let line_start = text[..s].rfind('\n').map_or(0, |i| i + 1);
    let rest = &text[e..];
    let line_end = rest.find('\n').map_or(text.len(), |i| e + i + 1);
    let alone = text[line_start..s].trim().is_empty() && text[e..line_end].trim().is_empty();
    let range = if alone {
        TextRange::new(
            TextSize::from(line_start as u32),
            TextSize::from(line_end as u32),
        )
    } else {
        range
    };
    Edit {
        range,
        content: String::new(),
    }
}

/// Is the folder of an import target known to the workspace (another file lives in it)? Only
/// then can a missing file be told from a game script tree that is simply not loaded.
fn directory_known(ctx: &Ctx, target: &str) -> bool {
    let t = gsc_sema::vfs::strip_script_ext(target).to_ascii_lowercase();
    let Some((dir, _)) = t.rsplit_once('/') else {
        return false;
    };
    let prefix = format!("{dir}/");
    ctx.ws.files().any(|f| {
        ctx.ws
            .file(f)
            .meta
            .key
            .to_ascii_lowercase()
            .starts_with(&prefix)
    })
}

/// Everything the file names that an import could supply: called names, qualifiers, class
/// names, and the identifiers of the macro bodies it can see (identifier case).
fn referenced_words(ctx: &Ctx) -> HashSet<String> {
    let ws = ctx.ws;
    let cfg = ws.profile_of(ctx.file).config;
    let facts = &ws.file(ctx.file).facts;
    let mut out = HashSet::new();
    for r in &facts.refs {
        match &r.target {
            Target::Bare(n) | Target::Keyword(n) | Target::Method(n) => {
                out.insert(n.clone());
            }
            Target::Qualified(q, n) => {
                out.insert(n.clone());
                if let Some(q) = q {
                    out.insert(q.clone());
                }
            }
            Target::Dynamic(_) => {}
        }
    }
    let mut words = |text: &str| {
        for w in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            if !w.is_empty() {
                out.insert(cfg.ident(w));
            }
        }
    };
    // Class names (`new c()`, `class d : c`), file-scope macro calls, and macro bodies: the
    // tree's identifiers outside function bodies and every `#define` the file sees.
    for n in ctx.root.descendants() {
        match n.kind() {
            NEW_EXPR | CLASS_BASE | MACRO_CALL_ITEM | PREPROC_DEFINE => {
                words(&n.text().to_string());
            }
            _ => {}
        }
    }
    let res = ws.resolution(ctx.file);
    for (imp, found) in facts.imports.iter().zip(&res.imports) {
        if imp.is_scope() {
            continue;
        }
        // A header's macros reach the file: what they name counts.
        let Some(h) = found else { continue };
        let data = ws.file(*h);
        if data.has_tree() {
            for n in data.syntax().descendants() {
                if n.kind() == PREPROC_DEFINE {
                    words(&n.text().to_string());
                }
            }
        }
    }
    out
}

/// What loading `f` does on its own: an engine-called function, a file-scope macro call.
fn has_load_effect(ctx: &Ctx, f: FileId) -> bool {
    let data = ctx.ws.file(f);
    let flagged = data.facts.defs.iter().any(|d| {
        d.flags
            .split(',')
            .any(|s| s == "autoexec" || s == "codecall" || s.starts_with("event_handler"))
    });
    flagged
        || !data.has_tree()
        || data
            .syntax()
            .children()
            .any(|n| n.kind() == MACRO_CALL_ITEM)
}

/// `f` and the files it imports (`#using`/`#include`), transitively.
fn import_closure(ctx: &Ctx, f: FileId) -> Vec<FileId> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut stack = vec![f];
    while let Some(x) = stack.pop() {
        if !seen.insert(x) {
            continue;
        }
        out.push(x);
        stack.extend(ctx.ws.resolution(x).scope.iter().copied());
    }
    out
}

fn unused(ctx: &Ctx, found: &mut Vec<(&str, String, TextRange, Option<Fix>)>) {
    let ws = ctx.ws;
    let cfg = ws.profile_of(ctx.file).config;
    let facts = &ws.file(ctx.file).facts;
    let res = ws.resolution(ctx.file);
    // A header that did not resolve takes its macros, and what they call, with it.
    if facts
        .imports
        .iter()
        .zip(&res.imports)
        .any(|(i, t)| !i.is_scope() && t.is_none())
    {
        return;
    }
    // Calls resolution could not place may be meant for any import. Under `#include`, which
    // merges what the included files include, a name one of those defines is placed.
    let merged: HashSet<String> = match cfg.resolution {
        Resolution::Include => import_closure(ctx, ctx.file)
            .into_iter()
            .flat_map(|u| ws.file(u).facts.defs.iter().map(|d| d.name.clone()))
            .collect(),
        Resolution::Namespace => HashSet::new(),
    };
    let unplaced = res.refs.iter().zip(&facts.refs).any(|(r, x)| {
        r.how == gsc_sema::How::Unresolved
            && match &x.target {
                Target::Dynamic(_) => false,
                Target::Bare(n) => !merged.contains(n),
                _ => true,
            }
    });
    if unplaced {
        return;
    }
    let words = referenced_words(ctx);
    let reached: HashSet<FileId> = res
        .refs
        .iter()
        .filter_map(|r| r.def)
        .map(|d| d.file)
        .collect();
    let mut done = HashSet::new();
    for (imp, target) in facts.imports.iter().zip(&res.imports) {
        if !imp.is_scope() {
            continue;
        }
        let Some(t) = *target else { continue };
        if !done.insert(t) {
            continue; // I001's business
        }
        let closure = import_closure(ctx, t);
        if closure.iter().any(|&u| has_load_effect(ctx, u)) {
            continue;
        }
        // `#include` merges its own includes into the file; `#using` brings in one file.
        let suppliers: &[FileId] = match cfg.resolution {
            Resolution::Include => &closure,
            Resolution::Namespace => std::slice::from_ref(&t),
        };
        let used = suppliers.iter().any(|&u| {
            let data = ws.file(u);
            reached.contains(&u)
                || words.contains(&data.facts.namespace)
                || data
                    .facts
                    .ns_switches
                    .iter()
                    .any(|(_, ns)| words.contains(ns))
                || data.facts.defs.iter().any(|d| {
                    words.contains(d.short_name()) || d.class().is_some_and(|c| words.contains(c))
                })
        });
        if used {
            continue;
        }
        let fix = directive_node(ctx.root, imp.range).map(|n| Fix {
            applicability: Applicability::Safe,
            message: format!("Remove the unused `#{}`", imp.directive),
            edits: vec![delete_line(ctx.text, n.text_range())],
        });
        found.push((
            "I002",
            format!(
                "`{}` is imported but nothing from it is used",
                spelled(ctx.text, imp.range)
            ),
            imp.range,
            fix,
        ));
    }
}
