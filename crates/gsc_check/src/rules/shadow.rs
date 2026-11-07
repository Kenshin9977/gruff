//! B006 builtin-shadowed-function: in a `builtin-strict` game (T6) the linker looks a bare call
//! up in the builtins of the call's kind before any script function, so a function named like
//! a builtin is never reached by the bare calls of its own file and of the files that
//! `#include` it. Reported when such a call exists, unless the function is called in the other
//! form (`ent f()` when only a builtin function shadows it, `f()` when only a method does).

use gsc_sema::facts::{DefKind, Target};
use gsc_sema::{DefId, FileId, How, Workspace};

use super::Ctx;

fn defines(ws: &Workspace, f: FileId, n: &str) -> bool {
    ws.file(f)
        .facts
        .defs
        .iter()
        .any(|d| d.kind == DefKind::Function && d.name == n)
}

/// The bare calls of `n` that would reach the function `d` (named `n`) but link to a builtin:
/// in its file, and in the files whose first `#include`d definition of `n` is its. `(same file,
/// other files, calls of the form `other_form` that do reach `d`)`.
fn shadowed_calls(ws: &Workspace, d: DefId, n: &str, other_form: &str) -> (usize, usize, usize) {
    let f = d.file;
    let (mut here, mut there, mut reached) = (0, 0, 0);
    for u in ws.files() {
        let res = ws.resolution(u);
        let facts = &ws.file(u).facts;
        reached += facts
            .refs
            .iter()
            .zip(&res.refs)
            .filter(|(x, r)| r.def == Some(d) && x.kind == other_form)
            .count();
        let reaches_f = u == f
            || (!defines(ws, u, n) && res.scope.iter().find(|&&s| defines(ws, s, n)) == Some(&f));
        if !reaches_f {
            continue;
        }
        let k = facts
            .refs
            .iter()
            .zip(&res.refs)
            .filter(|(x, r)| {
                matches!(&x.target, Target::Bare(m) if m == n)
                    && matches!(x.kind, "call" | "method")
                    && r.how == How::Builtin
            })
            .count();
        if u == f {
            here += k;
        } else {
            there += k;
        }
    }
    (here, there, reached)
}

pub fn check(ctx: &mut Ctx) {
    let ws = ctx.ws;
    let f = ctx.file;
    if !ws.builtin_strict(f) || !ctx.catalogue.is_some_and(|c| c.is_complete_in(ctx.mode)) {
        return;
    }
    let facts = &ws.file(f).facts;
    for (i, d) in facts.defs.iter().enumerate() {
        if d.kind != DefKind::Function {
            continue;
        }
        let n = &d.name;
        let (fun, meth) = (
            ws.is_builtin_kind(f, n, false),
            ws.is_builtin_kind(f, n, true),
        );
        if !fun && !meth {
            continue;
        }
        let id = DefId {
            file: f,
            index: i as u32,
        };
        // Called in the form no builtin takes: the shadowed calls mean the builtin.
        let other_form = match (fun, meth) {
            (true, false) => "method",
            (false, true) => "call",
            _ => "",
        };
        let (here, there, reached) = shadowed_calls(ws, id, n, other_form);
        if here + there == 0 || reached > 0 {
            continue;
        }
        let kind = match (fun, meth) {
            (true, true) => "function and method",
            (true, false) => "function",
            _ => "method",
        };
        let plural = |k: usize| if k == 1 { "" } else { "s" };
        let mut sites = Vec::new();
        if here > 0 {
            sites.push(format!("{here} call{} in this file", plural(here)));
        }
        if there > 0 {
            sites.push(format!(
                "{there} call{} in files that `#include` it",
                plural(there)
            ));
        }
        ctx.report(
            "B006",
            format!(
                "`{n}` is also a builtin {kind}: the game links bare calls to the builtin, never \
                 to this function ({}); only a path-qualified call `path::{n}()` or \
                 `thread {n}()` reaches it",
                sites.join(", ")
            ),
            d.name_range,
            None,
        );
    }
}
