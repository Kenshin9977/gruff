//! N001 unresolved-script-function, N002 namespace-not-imported, N003 private-function-call,
//! N004 script-arg-count: calls to script functions, checked against the workspace's name
//! resolution.
//!
//! After gscode's `FunctionResolutionLint` (`ScriptFunctionNotFound`), `NamespaceUsageLint`,
//! `PrivateAccessLint` and `ArgumentCountLint` (Blakintosh/gscode, GPL-3.0).
//! Name lookup is this project's own (`gsc_sema::resolve`, per profile).

use std::collections::{HashMap, HashSet};

use gsc_sema::{DefKind, FileId, How, Resolution, Target};
use gsc_syntax::ast::{self, AstNode};
use gsc_syntax::{SyntaxKind::*, TextRange, TextSize};

use super::{in_error, Ctx};
use crate::workspace::{is_hashed_name, looks_like_macro};
use crate::{Applicability, Edit, Fix};

pub fn check(ctx: &mut Ctx) {
    // Decompiler output names functions and namespaces by hash and drops the dev blocks that
    // hold some of the calls' targets.
    if ctx.decompiled {
        return;
    }
    let ws = ctx.ws;
    let f = ctx.file;
    let facts = &ws.file(f).facts;
    let res = ws.resolution(f);
    let cfg = ws.profile_of(f).config;
    let args = arg_counts(ctx);
    let mut ns_known: HashMap<String, bool> = HashMap::new();
    let mut missing_ns: HashMap<String, (FileId, Vec<TextRange>)> = HashMap::new();
    let mut found: Vec<(&str, String, TextRange)> = Vec::new();
    for (x, r) in facts.refs.iter().zip(&res.refs) {
        if !matches!(
            x.kind,
            "call" | "method" | "thread" | "childthread" | "pointer"
        ) {
            continue;
        }
        let name = match &x.target {
            Target::Bare(n) | Target::Qualified(_, n) => n,
            _ => continue,
        };
        if is_hashed_name(name) || looks_like_macro(name) {
            continue;
        }
        if let Target::Qualified(Some(q), _) = &x.target {
            if is_hashed_name(q) {
                continue;
            }
        }
        match (&x.target, r.how) {
            // N001: a qualified name names a script location; nothing else could have matched.
            (Target::Qualified(q, n), How::Unresolved) if ctx.on("N001") => {
                let report = match q {
                    // `::f` may point at a builtin: only with a complete catalogue that has no
                    // such name in either VM.
                    None => {
                        ws.callables_matching(f, n).is_empty()
                            && ws.profile_of(f).builtins.as_ref().is_some_and(|c| {
                                c.is_complete() && c.lookup(&n.to_ascii_lowercase()).is_empty()
                            })
                    }
                    // `path::f` reaches what the file `#include`s too: report only when that
                    // whole closure is in the workspace and none of it defines `f`.
                    Some(q) if q.contains('/') => match ws.find_file(f, q) {
                        Some(other) => {
                            let all = closure(ctx, other);
                            all.iter()
                                .all(|&u| ws.resolution(u).imports.iter().all(Option::is_some))
                                && !all.iter().any(|&u| {
                                    ws.file(u)
                                        .facts
                                        .defs
                                        .iter()
                                        .any(|d| d.kind.is_callable() && d.name == *n)
                                })
                        }
                        None => false,
                    },
                    Some(q) if q.eq_ignore_ascii_case("sys") => false,
                    Some(q) => {
                        // Only a namespace the workspace knows: else the scripts that declare it
                        // are simply not loaded.
                        *ns_known
                            .entry(q.clone())
                            .or_insert_with(|| namespace_exists(ctx, q))
                            && ws.class_and_bases(q).iter().all(|(_, d)| d.is_none())
                    }
                };
                if report {
                    let shown = match q {
                        Some(q) => format!("{q}::{n}"),
                        None => format!("::{n}"),
                    };
                    found.push((
                        "N001",
                        format!("no script function `{shown}`: the call names a script, so no builtin can match"),
                        x.range,
                    ));
                }
            }
            // N002: `ns::f` where no `#using`'d file declares `ns`.
            (Target::Qualified(Some(q), _), How::Qualified)
                if cfg.resolution == Resolution::Namespace && !q.contains('/') =>
            {
                if let Some(d) = r.def {
                    if d.file != f && !imports_namespace(ctx, &res.scope, q) {
                        let e = missing_ns.entry(q.clone()).or_insert((d.file, Vec::new()));
                        e.1.push(x.range);
                    }
                }
            }
            _ => {}
        }
        // N003 and N004 need the definition reached.
        let Some(d) = r.def.filter(|_| r.how.is_script() && r.how != How::Macro) else {
            continue;
        };
        let def = ws.def(d);
        if ctx.on("N003") && d.file != f && def.flags.split(',').any(|s| s == "private") {
            let (ns, _) = facts.ref_scope(x);
            if def.ns != ns {
                found.push((
                    "N003",
                    format!(
                        "`{}` is private to namespace `{}`: only files of that namespace can call it",
                        def.short_name(),
                        def.ns
                    ),
                    x.range,
                ));
            }
        }
        if ctx.on("N004") && def.kind == DefKind::Function && x.kind != "pointer" {
            let Some(&argc) = args.get(&x.range.start()) else {
                continue;
            };
            let (params, vararg) = count_params(&def.params);
            // Every function of that name must agree: a csc and a gsc twin may differ.
            let agree = ws
                .callables_matching(f, &def.name)
                .iter()
                .filter(|o| ws.def(**o).ns == def.ns)
                .all(|o| count_params(&ws.def(*o).params) == (params, vararg));
            if !vararg && argc > params && agree {
                found.push((
                    "N004",
                    format!(
                        "`{}` declares {params} parameter{} but {argc} argument{} passed; the \
                         extra ones are never read",
                        def.short_name(),
                        if params == 1 { "" } else { "s" },
                        if argc == 1 { " is" } else { "s are" },
                    ),
                    x.range,
                ));
            }
        }
    }
    if ctx.on("N002") {
        let mut missing: Vec<_> = missing_ns.into_iter().collect();
        missing.sort_by_key(|(_, (_, r))| r[0].start());
        for (q, (file, ranges)) in missing {
            let key = ws.file(file).meta.key.replace('/', "\\");
            let fix = using_fix(ctx, &key);
            for range in ranges {
                if !near(ctx, range) {
                    ctx.report(
                        "N002",
                        format!("namespace `{q}` is called, but no `#using` imports a file that declares it (`{key}` does)"),
                        range,
                        Some(fix.clone()),
                    );
                }
            }
        }
    }
    for (code, message, range) in found {
        if !near(ctx, range) {
            ctx.report(code, message, range, None);
        }
    }
}

fn near(ctx: &Ctx, range: TextRange) -> bool {
    let node = match ctx.root.covering_element(range) {
        gsc_syntax::NodeOrToken::Node(n) => n,
        gsc_syntax::NodeOrToken::Token(t) => match t.parent() {
            Some(p) => p,
            None => return true,
        },
    };
    in_error(&node) || ctx.near_syntax_error(&node)
}

/// Argument counts of the calls of the file, by the start of the callee's name.
fn arg_counts(ctx: &Ctx) -> HashMap<TextSize, usize> {
    let mut out = HashMap::new();
    for n in ctx.root.descendants().filter(|n| n.kind() == CALL_EXPR) {
        let Some(c) = ast::CallExpr::cast(n) else {
            continue;
        };
        let name = match c.callee() {
            Some(ast::Expr::NameRef(n)) => n.syntax().text_range(),
            Some(ast::Expr::QualifiedRef(q)) => match q.name_ref() {
                Some(n) => n.syntax().text_range(),
                None => continue,
            },
            _ => continue,
        };
        let argc = c.arg_list().map_or(0, |a| a.exprs().count());
        // The range facts record for a bare call is the whole `NameRef`.
        out.insert(name.start(), argc);
        if let Some(ast::Expr::NameRef(n)) = c.callee() {
            out.insert(n.syntax().text_range().start(), argc);
        }
    }
    out
}

/// Parameters as `Def::params` writes them (`a, &b, c = 1, ...`): how many, and a `...`.
fn count_params(params: &str) -> (usize, bool) {
    let mut depth = 0i32;
    let mut parts = Vec::new();
    let mut cur = String::new();
    for c in params.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    parts.push(cur);
    let parts: Vec<&str> = parts
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    let vararg = parts.contains(&"...");
    (parts.iter().filter(|p| **p != "...").count(), vararg)
}

/// Does any function of the workspace live in namespace `q`?
fn namespace_exists(ctx: &Ctx, q: &str) -> bool {
    ctx.ws.files().any(|f| {
        let facts = &ctx.ws.file(f).facts;
        facts.namespace == q || facts.defs.iter().any(|d| d.ns == q)
    })
}

/// Does this file, or one it `#using`s, declare namespace `q`?
fn imports_namespace(ctx: &Ctx, scope: &[FileId], q: &str) -> bool {
    std::iter::once(ctx.file)
        .chain(scope.iter().copied())
        .any(|u| {
            let facts = &ctx.ws.file(u).facts;
            facts.namespace == q || facts.ns_switches.iter().any(|(_, ns)| ns == q)
        })
}

/// The files `f` reaches through `#include`, transitively (`f` included).
pub(super) fn closure(ctx: &Ctx, f: FileId) -> HashSet<FileId> {
    let mut seen = HashSet::new();
    let mut stack = vec![f];
    while let Some(x) = stack.pop() {
        if !seen.insert(x) {
            continue;
        }
        stack.extend(ctx.ws.resolution(x).scope.iter().copied());
    }
    seen
}

/// Insert `#using <key>;` after the file's last `#using` (or at its top).
fn using_fix(ctx: &Ctx, key: &str) -> Fix {
    let nl = if ctx.text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let last = ctx
        .root
        .children()
        .filter(|n| n.kind() == USING_DIRECTIVE)
        .last();
    let (at, content) = match last {
        Some(u) => {
            let end = u.text_range().end();
            (end, format!("{nl}#using {key};"))
        }
        None => (TextSize::from(0), format!("#using {key};{nl}")),
    };
    Fix {
        applicability: Applicability::Unsafe,
        message: format!("Add `#using {key};`"),
        edits: vec![Edit {
            range: TextRange::empty(at),
            content,
        }],
    }
}
