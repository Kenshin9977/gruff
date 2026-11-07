//! B001 unknown-builtin, B002 builtin-arg-count, B003 dev-only-builtin.

use gsc_syntax::ast::{self, AstNode};
use gsc_syntax::SyntaxKind::CALL_EXPR;

use gsc_sema::{Builtin, Kind, Mode};

use super::{in_dev_block, in_error, Ctx};
use crate::workspace::{callee, is_hashed_name, looks_like_macro, Callee};
use crate::{Applicability, Edit, Fix};

fn describe_arity(entries: &[&Builtin], mode: Option<Mode>) -> String {
    let mut parts: Vec<String> = entries
        .iter()
        .filter_map(|b| b.describe_arity_in(mode))
        .collect();
    parts.dedup();
    parts.join(" or ")
}

/// Does `f` or a file it imports, transitively, have an import the workspace cannot find? A name
/// it calls may then be a function of the missing file.
fn imports_missing(ctx: &Ctx) -> bool {
    super::calls::closure(ctx, ctx.file)
        .into_iter()
        .any(|u| ctx.ws.resolution(u).imports.iter().any(Option::is_none))
}

pub fn check(ctx: &mut Ctx) {
    let Some(cat) = ctx.catalogue else { return };
    let mut missing: Option<bool> = None;
    let ws = ctx.ws;
    let cfg = ws.profile_of(ctx.file).config;
    let (b001, b002, b003) = (ctx.on("B001"), ctx.on("B002"), ctx.on("B003"));
    let calls: Vec<_> = ctx
        .root
        .descendants()
        .filter(|n| n.kind() == CALL_EXPR)
        .filter_map(ast::CallExpr::cast)
        .collect();
    for call in calls {
        let Callee::Bare(name, range) = callee(&call) else {
            continue;
        };
        if is_hashed_name(&name)
            || looks_like_macro(&name)
            || ws.macros_spelled(&name).next().is_some()
        {
            continue;
        }
        // The decompiler's placeholder for bytes it could not decode, not a call.
        if ctx.decompiled && name == "InvalidOpCode" {
            continue;
        }
        if in_error(call.syntax()) || ctx.near_syntax_error(call.syntax()) {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        let ident = cfg.ident(&name);
        let argc = || call.arg_list().map_or(0, |a| a.exprs().count() as u32);
        // An intrinsic compiles to its own opcode: known whatever the tables hold, and checked
        // against the arity its `[[intrinsic]]` entry gives, if any.
        if let Some(intr) = cat.intrinsic(&name) {
            let n = argc();
            if b002 && !intr.accepts(n) && ws.builtin_shadow(ctx.file, &ident).is_none() {
                let range = call.arg_list().map_or(range, |a| a.syntax().text_range());
                ctx.report(
                    "B002",
                    format!(
                        "intrinsic `{lower}` takes {}, but {n} {} given",
                        intr.describe_arity().unwrap_or_default(),
                        if n == 1 { "was" } else { "were" }
                    ),
                    range,
                    None,
                );
            }
            continue;
        }
        let kind = if call.receiver().is_some() {
            Kind::Method
        } else {
            Kind::Function
        };
        // Only the builtins of the file's game mode (its executable's tables), when known.
        let entries = cat.lookup_in(&name, ctx.mode);
        let vm_ok = |b: &&Builtin| ctx.vm.is_none_or(|vm| vm == b.vm);
        let matching: Vec<&Builtin> = entries
            .iter()
            .copied()
            .filter(vm_ok)
            .filter(|b| b.kind == kind)
            .collect();
        if matching.is_empty() {
            // A partial catalogue (an executable not extracted, entries without a name) cannot
            // tell an unknown name from one it does not name.
            if !b001
                || !cat.is_complete_in(ctx.mode)
                || !ws.callables_matching(ctx.file, &ident).is_empty()
            {
                continue;
            }
            // A name no builtin of any mode or VM has, with an import missing from the
            // workspace: most likely a function of that file (I003 reports the import).
            if cat.lookup(&name).is_empty() && *missing.get_or_insert_with(|| imports_missing(ctx))
            {
                continue;
            }
            let other_kind = entries
                .iter()
                .copied()
                .filter(vm_ok)
                .find(|b| b.kind != kind);
            let other_vm = entries.iter().copied().find(|b| !vm_ok(b));
            let other_mode = cat
                .lookup(&name)
                .iter()
                .filter(vm_ok)
                .find(|b| b.kind == kind);
            let mut fix = None;
            let message = if let Some(b) = other_kind {
                match kind {
                    Kind::Function => format!(
                        "`{lower}` is a builtin method, not a function: call it on an entity (`ent {lower}()`)"
                    ),
                    Kind::Method => format!(
                        "`{lower}` is a builtin {}, not a method: call it without a receiver",
                        b.kind.label()
                    ),
                }
            } else if let Some(b) = other_vm {
                let here = ctx.vm.map_or("this", |v| v.label());
                format!(
                    "`{lower}` is a {} builtin {}, not available in {here} scripts",
                    b.vm.label(),
                    b.kind.label()
                )
            } else if let (Some(b), Some(here)) = (other_mode, ctx.mode) {
                format!(
                    "`{lower}` is a builtin {} of {} scripts only, not available in {here} scripts",
                    b.kind.label(),
                    b.modes.map(|m| m.labels()).unwrap_or_default()
                )
            } else {
                let sugg = cat.suggest_in(&name, ctx.vm, kind, ctx.mode);
                let mut m = format!("unknown builtin {} `{name}`", kind.label());
                if !sugg.is_empty() {
                    let list: Vec<String> = sugg.iter().map(|s| format!("`{s}`")).collect();
                    m.push_str(&format!("; did you mean {}?", list.join(" or ")));
                }
                if let [only] = sugg.as_slice() {
                    fix = Some(Fix {
                        applicability: Applicability::Unsafe,
                        message: format!("Replace with `{only}`"),
                        edits: vec![Edit {
                            range,
                            content: (*only).to_owned(),
                        }],
                    });
                }
                m
            };
            ctx.report("B001", message, range, fix);
            continue;
        }
        if ws.builtin_shadow(ctx.file, &ident).is_some() {
            continue; // a script function shadows the builtin
        }
        // `file-first` and `builtin-strict` games never link a threaded call to a builtin
        // (B005's case).
        let threaded = matches!(
            gsc_sema::facts::call_kind(call.modifier(), false),
            "thread" | "childthread"
        );
        if threaded && (ws.file_first(ctx.file) || ws.builtin_strict(ctx.file)) {
            continue;
        }
        if b002 && matching.iter().all(|b| b.arity_in(ctx.mode).is_some()) {
            let argc = argc();
            // `arity_unknown` entries have no arity and never get here; a maximum the engine
            // does not check (`max_checked = false`) only bounds from below. A file of a known
            // mode is held to its executable's arity (`mode_args`).
            let fits = matching.iter().any(|b| b.accepts_in(argc, ctx.mode));
            if !fits {
                let range = call.arg_list().map_or(range, |a| a.syntax().text_range());
                ctx.report(
                    "B002",
                    format!(
                        "builtin {} `{lower}` takes {}, but {argc} {} given",
                        kind.label(),
                        describe_arity(&matching, ctx.mode),
                        if argc == 1 { "was" } else { "were" }
                    ),
                    range,
                    None,
                );
            }
        }
        // Decompilers drop the `/# #/` markers and keep the calls: B003 cannot tell there.
        if b003
            && !ctx.decompiled
            && matching.iter().all(|b| b.dev_only)
            && !in_dev_block(call.syntax())
        {
            ctx.report(
                "B003",
                format!("`{lower}` is a dev-only builtin; wrap the call in a `/# #/` dev block"),
                range,
                None,
            );
        }
    }
}
