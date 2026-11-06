//! M001 macro-arg-count, M002 macro-redefined, M003 duplicate-macro-parameter: `#define` and its
//! uses, as the preprocessor sees them.
//!
//! After gscode's `WrongMacroArgumentCount`, `DuplicateMacroDefinition` and
//! `DuplicateMacroParameter` (Blakintosh/gscode, GPL-3.0). The BO3 preprocessor refuses all
//! three, a redefinition even with the same body (`tools/linker/probes/lint_*macro*.gsc`).

use std::collections::{HashMap, HashSet};

use gsc_sema::facts::def_node;
use gsc_sema::{DefKind, FileId};
use gsc_syntax::ast::{self, AstNode};
use gsc_syntax::{NodeOrToken, SyntaxKind::*, SyntaxNode};

use super::{in_error, Ctx};

/// The parameters of a `#define`: `None` for an object-like macro, else the names and whether
/// it ends in `...`.
fn params_of(d: &ast::PreprocDefine) -> Option<(Vec<String>, bool)> {
    let p = d.macro_params()?;
    let mut names = Vec::new();
    let mut variadic = false;
    for t in p
        .syntax()
        .children_with_tokens()
        .filter_map(|e| e.into_token())
    {
        match t.kind() {
            IDENT => names.push(t.text().to_owned()),
            DOT3 => variadic = true,
            _ => {}
        }
    }
    Some((names, variadic))
}

/// The files `f` pastes in through `#insert`/`#inline`, transitively (`f` excluded).
fn headers(ctx: &Ctx, f: FileId) -> Vec<FileId> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut stack = vec![f];
    while let Some(x) = stack.pop() {
        if !seen.insert(x) {
            continue;
        }
        if x != f {
            out.push(x);
        }
        let facts = &ctx.ws.file(x).facts;
        let res = ctx.ws.resolution(x);
        for (imp, found) in facts.imports.iter().zip(&res.imports) {
            if !imp.is_scope() {
                if let Some(y) = found {
                    stack.push(*y);
                }
            }
        }
    }
    out
}

/// The `#define`s of a tree at its top level of `#if` nesting, in order, with `#undef`s:
/// `(name, Some(node))` for a definition, `(name, None)` for an `#undef`. Definitions inside an
/// `#if` region are left out: another configuration may skip them.
fn top_level_defines(root: &SyntaxNode) -> Vec<(String, Option<ast::PreprocDefine>)> {
    let mut depth = 0i32;
    let mut out = Vec::new();
    for el in root.descendants_with_tokens() {
        match el {
            NodeOrToken::Token(t) => match t.kind() {
                PP_IF | PP_IFDEF | PP_IFNDEF => depth += 1,
                PP_ENDIF => depth -= 1,
                _ => {}
            },
            NodeOrToken::Node(n) if depth == 0 => match n.kind() {
                PREPROC_DEFINE => {
                    if let Some(d) = ast::PreprocDefine::cast(n) {
                        if let Some(name) = d.name() {
                            out.push((name.text(), Some(d)));
                        }
                    }
                }
                PREPROC_UNDEF => {
                    if let Some(u) = ast::PreprocUndef::cast(n) {
                        if let Some(name) = u.name() {
                            out.push((name.text(), None));
                        }
                    }
                }
                _ => {}
            },
            NodeOrToken::Node(_) => {}
        }
    }
    out
}

pub fn check(ctx: &mut Ctx) {
    if ctx.on("M003") {
        duplicate_params(ctx);
    }
    let hdrs = headers(ctx, ctx.file);
    if ctx.on("M002") {
        redefinitions(ctx, &hdrs);
    }
    if ctx.on("M001") {
        arg_counts(ctx, &hdrs);
    }
}

fn duplicate_params(ctx: &mut Ctx) {
    let mut found = Vec::new();
    for d in ctx.root.descendants().filter_map(ast::PreprocDefine::cast) {
        let Some(p) = d.macro_params() else { continue };
        let mut seen = HashSet::new();
        for t in p
            .syntax()
            .children_with_tokens()
            .filter_map(|e| e.into_token())
        {
            if t.kind() == IDENT && !seen.insert(t.text().to_owned()) {
                let macro_name = d.name().map(|n| n.text()).unwrap_or_default();
                found.push((
                    t.text_range(),
                    format!(
                        "macro `{macro_name}` already has a parameter `{}`: the arguments passed \
                         for this one are never substituted",
                        t.text()
                    ),
                ));
            }
        }
    }
    for (range, message) in found {
        ctx.report("M003", message, range, None);
    }
}

fn redefinitions(ctx: &mut Ctx, hdrs: &[FileId]) {
    // Defined before this file's own lines: by the headers it inserts.
    let mut defined: HashMap<String, String> = HashMap::new();
    for &h in hdrs {
        let data = ctx.ws.file(h);
        if !data.has_tree() {
            continue;
        }
        let display = data.meta.rel.clone();
        for (name, d) in top_level_defines(&data.syntax()) {
            match d {
                Some(_) => {
                    defined.entry(name).or_insert_with(|| display.clone());
                }
                None => {
                    defined.remove(&name);
                }
            }
        }
    }
    let mut found = Vec::new();
    for (name, d) in top_level_defines(ctx.root) {
        let Some(d) = d else {
            defined.remove(&name);
            continue;
        };
        let range = d
            .name()
            .map_or(d.syntax().text_range(), |n| n.syntax().text_range());
        if let Some(where_) = defined.get(&name) {
            if !in_error(d.syntax()) {
                found.push((
                    range,
                    format!(
                        "macro `{name}` is already defined ({where_}); the preprocessor refuses \
                         a redefinition, even with the same body: `#undef` it first"
                    ),
                ));
            }
        }
        defined.insert(name, "earlier in this file".to_owned());
    }
    for (range, message) in found {
        ctx.report("M002", message, range, None);
    }
}

fn arg_counts(ctx: &mut Ctx, hdrs: &[FileId]) {
    let visible: HashSet<FileId> = hdrs.iter().copied().chain([ctx.file]).collect();
    let mut arity: HashMap<String, Option<(usize, bool)>> = HashMap::new();
    let mut found = Vec::new();
    let sites = ctx.root.descendants().filter_map(|n| match n.kind() {
        CALL_EXPR => {
            let c = ast::CallExpr::cast(n)?;
            let ast::Expr::NameRef(name) = c.callee()? else {
                return None;
            };
            Some((name, c.arg_list()?))
        }
        MACRO_CALL_ITEM => {
            let m = ast::MacroCallItem::cast(n)?;
            Some((m.name_ref()?, m.arg_list()?))
        }
        _ => None,
    });
    let sites: Vec<_> = sites.collect();
    for (name, args) in sites {
        let spelled = name.text();
        let entry = arity.entry(spelled.clone()).or_insert_with(|| {
            // Every visible definition must agree; a function-like one is required.
            let mut seen: Option<(usize, bool)> = None;
            for d in ctx.ws.macros_spelled(&spelled) {
                if !visible.contains(&d.file) || ctx.ws.def(d).kind != DefKind::Define {
                    continue;
                }
                let data = ctx.ws.file(d.file);
                if !data.has_tree() {
                    return None;
                }
                let node = def_node(&data.syntax(), ctx.ws.def(d))?;
                let (params, variadic) = params_of(&ast::PreprocDefine::cast(node)?)?;
                let a = (params.len(), variadic);
                if seen.is_some_and(|s| s != a) {
                    return None;
                }
                seen = Some(a);
            }
            seen
        });
        let Some((params, variadic)) = *entry else {
            continue;
        };
        let argc = args.exprs().count();
        let fits = if variadic {
            argc >= params
        } else {
            argc == params || (argc == 0 && params == 1)
        };
        if fits || in_error(args.syntax()) || ctx.near_syntax_error(args.syntax()) {
            continue;
        }
        let want = if variadic {
            format!("at least {params}")
        } else {
            params.to_string()
        };
        found.push((
            args.syntax().text_range(),
            format!(
                "macro `{spelled}` takes {want} argument{} but {argc} {} given",
                if params == 1 && !variadic { "" } else { "s" },
                if argc == 1 { "was" } else { "were" }
            ),
        ));
    }
    for (range, message) in found {
        ctx.report("M001", message, range, None);
    }
}
