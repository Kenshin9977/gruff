//! B004 bare-call-other-namespace, B005 thread-builtin: bare names the game's linker cannot
//! link in a `bare_call_resolution = "namespace-first"` game (T7+), and (B005) the threaded
//! calls and `::f` references to a builtin that the compiler of a `file-first` game (T4, IW3)
//! refuses, or that the linker of a `builtin-strict` game (T6) refuses (threads) or links to
//! the builtin's native handler (`::f`). See `gsc_sema::Workspace::resolve_bare` for the order.

use gsc_sema::facts::Target;
use gsc_sema::How;
use gsc_syntax::{NodeOrToken, TextRange};

use super::{in_dev_block, in_error, Ctx};
use crate::workspace::looks_like_macro;
use crate::{Applicability, Edit, Fix};

pub fn check(ctx: &mut Ctx) {
    let ws = ctx.ws;
    let f = ctx.file;
    // Without the whole catalogue a name missing from it may still be a builtin. acts writes
    // a get-call import bare whatever namespace the bytecode gives it, so a bare call of
    // decompiled output does not say which namespace the game looks in.
    let file_first = ws.file_first(f);
    let strict = ws.builtin_strict(f);
    if ctx.decompiled
        || !(ws.namespace_first(f) || file_first || strict)
        || !ctx.catalogue.is_some_and(|c| c.is_complete())
    {
        return;
    }
    let (b004, b005) = (ctx.on("B004"), ctx.on("B005"));
    let facts = &ws.file(f).facts;
    let res = ws.resolution(f);
    for (x, out) in facts.refs.iter().zip(res.refs.iter()) {
        let n = match &x.target {
            Target::Bare(n) => n,
            // `::f` (colon-style pointer) never reaches a builtin in a `file-first` game.
            Target::Qualified(None, n) if file_first && x.kind == "pointer" => n,
            // `::f` takes the builtin first in a `builtin-strict` game.
            Target::Qualified(None, n) if strict && x.kind == "pointer" => n,
            _ => continue,
        };
        let strict_ref = strict && x.kind == "pointer" && out.how == How::Builtin;
        if !strict_ref && !matches!(out.how, How::Unresolved | How::BuiltinOtherVm) {
            continue;
        }
        let written = &ctx.text[x.range];
        if looks_like_macro(written) || ws.macros_spelled(written).next().is_some() {
            continue;
        }
        let node = match ctx.root.covering_element(x.range) {
            NodeOrToken::Node(n) => n,
            NodeOrToken::Token(t) => match t.parent() {
                Some(p) => p,
                None => continue,
            },
        };
        if in_error(&node) || ctx.near_syntax_error(&node) {
            continue;
        }
        let thread = matches!(x.kind, "thread" | "childthread");
        if file_first {
            if !b005 || !ws.is_builtin(f, n) {
                continue;
            }
            let message = if thread {
                format!(
                    "`{written}` is a builtin, and a threaded call never reaches a builtin: the \
                     game's compiler refuses `thread {written}()` (unknown function); call it \
                     without `thread`, or thread a script function that calls it"
                )
            } else if x.kind == "pointer" {
                format!(
                    "`{n}` is a builtin, and a function reference never reaches a builtin: the \
                     game's compiler refuses `::{n}` (unknown function); refer to a script \
                     function that calls it"
                )
            } else {
                continue;
            };
            ctx.report("B005", message, x.range, None);
            continue;
        }
        if strict {
            if !b005 {
                continue;
            }
            let message = if strict_ref {
                let shadowed = ws
                    .visible_function(f, &ws.profile_of(f).config.ident(n))
                    .map(|d| {
                        format!(
                            " (`{n}` of {} is reached only by a path-qualified reference)",
                            ws.file(d.file).meta.rel
                        )
                    })
                    .unwrap_or_default();
                format!(
                    "`{n}` is a builtin, and a function reference takes the builtin first: the \
                     game's linker stores the builtin's native handler, which does not run as \
                     script{shadowed}; refer to a script function that calls it"
                )
            } else if thread && ws.is_builtin(f, n) {
                format!(
                    "`{written}` is a builtin, and a threaded call never reaches a builtin: the \
                     game cannot link `thread {written}()` (unresolved external); call it \
                     without `thread`, or thread a script function that calls it"
                )
            } else {
                continue;
            };
            ctx.report("B005", message, x.range, None);
            continue;
        }
        let (ns, _) = facts.ref_scope(x);
        let others = ws.other_namespace_fns(f, ns, n);
        if let Some(first) = others.first() {
            // The retail catalogues lack some dev-only builtins (BO4's `destroy`), and the game
            // links a dev block's calls only when dev blocks are on.
            if !b004 || in_dev_block(&node) {
                continue;
            }
            // The first found (the `#using`'d files come first, in order) is the one named.
            let mut spaces: Vec<&str> = vec![ws.def(*first).ns.as_str()];
            for &d in &others {
                let s = ws.def(d).ns.as_str();
                if !spaces.contains(&s) {
                    spaces.push(s);
                }
            }
            let place = &ws.file(first.file).meta.rel;
            let how = if x.kind == "pointer" {
                format!("`&{written}`")
            } else {
                format!("a bare call to `{written}`")
            };
            let mut message = format!(
                "{how} resolves in namespace `{ns}`, then the builtins, never in another \
                 namespace: `{written}` is defined in namespace `{}` ({place})",
                spaces[0]
            );
            match spaces.len() {
                1 => {}
                2 => message.push_str(" and 1 other namespace"),
                n => message.push_str(&format!(" and {} other namespaces", n - 1)),
            }
            let fix = (spaces.len() == 1).then(|| {
                message.push_str(&format!("; write `{}::{written}`", spaces[0]));
                Fix {
                    applicability: Applicability::Unsafe,
                    message: format!("Qualify as `{}::{written}`", spaces[0]),
                    edits: vec![Edit {
                        range: TextRange::empty(x.range.start()),
                        content: format!("{}::", spaces[0]),
                    }],
                }
            });
            ctx.report("B004", message, x.range, fix);
        } else if thread && b005 && ws.is_builtin(f, n) {
            ctx.report(
                "B005",
                format!(
                    "`{written}` is a builtin, and a threaded call never reaches a builtin: the \
                     game cannot link `thread {written}()`; call it without `thread`, or thread \
                     a script function that calls it"
                ),
                x.range,
                None,
            );
        }
    }
}
