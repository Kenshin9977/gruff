//! The event rules, over the workspace event graph ([`gsc_sema::events`]):
//! T001 unknown-event, T002 misspelled-event, T003 waittill-arg-count, T004 missing-endon.
//!
//! Every rule reads the file's event sites from its facts and asks the workspace tables; only
//! T003 (for the bound variables) and T004 (for the loops) look at the tree, and only at the
//! sites and functions they are about to report.

use std::collections::BTreeSet;

use gsc_sema::data::levenshtein;
use gsc_sema::events::{hashed_event, EngineReceiver, EventCatalogue, Payload, Side};
use gsc_sema::facts::{def_node, DefKind};
use gsc_sema::{DefId, EventClass, EventSite};
use gsc_syntax::ast::{self, AstNode};
use gsc_syntax::{NodeOrToken, SyntaxKind, SyntaxNode, TextRange, TextSize};

use super::loops::{infinite_loop, may_exit};
use super::Ctx;
use crate::workspace::looks_like_macro;

fn side(receiver: &str) -> Side {
    Side::of(receiver)
}

fn compatible(a: Side, b: Side) -> bool {
    a.compatible(b)
}

fn engine_side(r: EngineReceiver) -> Side {
    match r {
        EngineReceiver::Level => Side::Level,
        EngineReceiver::Entity | EngineReceiver::Player => Side::Entity,
        EngineReceiver::Any => Side::Unknown,
    }
}

/// The node of a site's name argument.
fn name_node(root: &SyntaxNode, site: &EventSite) -> Option<SyntaxNode> {
    match root.covering_element(site.name_range) {
        NodeOrToken::Node(n) => Some(n),
        NodeOrToken::Token(t) => t.parent(),
    }
}

/// Is the name a plain event name a rule may judge? Hashed spellings and empty or odd strings
/// are left alone.
fn judgeable(name: &str) -> bool {
    !name.is_empty()
        && hashed_event(name).is_none()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b' ' | b'.'))
}

/// Could a script or the engine notify `lower` (some script notifies it, a pattern may, the
/// string is used elsewhere, or the engine catalogue has it)?
fn known(ctx: &Ctx, lower: &str, engine: Option<&EventCatalogue>) -> bool {
    ctx.ws.event_index().emitted(lower)
        || ctx.ws.event_mentioned(lower)
        || engine.is_some_and(|c| c.contains(lower))
}

/// Files waiting for (or ending on) one name that nothing visible notifies, from which on it
/// is someone's event (the engine's, a map's, a menu's), not a typo: typos stay local.
const WIDELY_WAITED: u32 = 3;

fn waited_widely(ctx: &Ctx, lower: &str) -> bool {
    ctx.ws.event_index().waiting_files(lower) >= WIDELY_WAITED
}

/// Are `a` and `b` naming variants rather than a typo of each other? They differ only in their
/// digits (`wave1`/`wave2`), by separators (`move_done`/`movedone`), by a plural or past-tense
/// ending (`goal`/`goals`, `enable`/`enabled`), or are an on/off pair.
fn same_family(a: &str, b: &str) -> bool {
    let strip = |s: &str| -> String {
        let t: String = s.chars().filter(|c| !c.is_ascii_digit()).collect();
        t.trim_end_matches(['_', ' ']).to_owned()
    };
    let squash = |s: &str| -> String { s.chars().filter(|c| !matches!(c, '_' | ' ')).collect() };
    let digits = |s: &str| s.bytes().any(|c| c.is_ascii_digit());
    if (digits(a) || digits(b)) && strip(a) == strip(b) {
        return true;
    }
    let (x, y) = (squash(a), squash(b));
    let ending = |long: &str, short: &str| {
        ["s", "d", "ed"]
            .iter()
            .any(|e| long.strip_suffix(e) == Some(short))
    };
    let on_off = |p: &str, q: &str| {
        p.strip_suffix("on")
            .is_some_and(|stem| q.strip_suffix("off") == Some(stem))
    };
    x == y || ending(&x, &y) || ending(&y, &x) || on_off(&x, &y) || on_off(&y, &x)
}

/// Known events one or two edits away from `lower`, on a receiver compatible with `side`.
fn suggestions(ctx: &Ctx, lower: &str, side: Side, engine: Option<&EventCatalogue>) -> Vec<String> {
    if lower.len() < 5 {
        return Vec::new();
    }
    let max = if lower.len() < 12 { 1 } else { 2 };
    let mut best: Vec<(usize, String)> = Vec::new();
    let mut consider = |name: &str, ok: bool| {
        if !ok || name == lower || same_family(name, lower) {
            return;
        }
        if let Some(d) = levenshtein(lower, name, max) {
            best.push((d, name.to_owned()));
        }
    };
    let ix = ctx.ws.event_index();
    for name in ix.emitted_near_len(lower.len(), max) {
        // A notify only wakes threads of its own VM: server events do not reach client scripts.
        let ok = ix.emitted[name].reaches(side, ctx.vm);
        consider(name, ok);
    }
    if let Some(c) = engine {
        for name in c.names() {
            if name.len().abs_diff(lower.len()) > max {
                continue;
            }
            let ok = c
                .lookup(name)
                .iter()
                .any(|e| compatible(side, engine_side(e.receiver)));
            consider(name, ok);
        }
    }
    best.sort();
    best.dedup_by(|a, b| a.1 == b.1);
    let Some(top) = best.first().map(|b| b.0) else {
        return Vec::new();
    };
    let near: Vec<String> = best
        .into_iter()
        .take_while(|b| b.0 == top)
        .map(|b| b.1)
        .collect();
    // Many equally near names: a family of events, no telling which was meant.
    if near.len() > 2 {
        return Vec::new();
    }
    near
}

fn quote_list(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("\"{n}\""))
        .collect::<Vec<_>>()
        .join(" or ")
}

fn what(site: &EventSite) -> &'static str {
    match site.class {
        EventClass::Endon => "this `endon` never fires",
        _ => "this wait never ends",
    }
}

pub fn check(ctx: &mut Ctx) {
    let engine = ctx.ws.profile_of(ctx.file).events.as_ref();
    let complete = engine.is_some_and(|c| c.complete);
    let data = ctx.ws.file(ctx.file);
    let sites: &[EventSite] = &data.facts.events;
    let t001 = complete && ctx.on("T001");
    let t002 = ctx.on("T002");
    for site in sites {
        if site.class == EventClass::Emit {
            continue;
        }
        let Some(lower) = site.key() else { continue };
        if !judgeable(&lower) || known(ctx, &lower, engine) || waited_widely(ctx, &lower) {
            continue;
        }
        let Some(node) = name_node(ctx.root, site) else {
            continue;
        };
        if ctx.near_syntax_error(&node) {
            continue;
        }
        let side = side(site.receiver);
        let near = if t001 || t002 {
            suggestions(ctx, &lower, side, engine)
        } else {
            Vec::new()
        };
        if t001 {
            let mut msg = format!(
                "no script notifies \"{}\" and the engine does not: {}",
                site.name.render(),
                what(site)
            );
            if !near.is_empty() {
                msg.push_str(&format!("; did you mean {}?", quote_list(&near)));
            }
            ctx.report("T001", msg, site.name_range, None);
        } else if t002 && !near.is_empty() {
            let msg = format!(
                "no script notifies \"{}\": did you mean {}? (otherwise {})",
                site.name.render(),
                quote_list(&near),
                what(site)
            );
            ctx.report("T002", msg, site.name_range, None);
        }
    }
    if ctx.on("T003") {
        if ctx.ws.profile_of(ctx.file).config.waittill_struct {
            result_fields(ctx, sites, engine);
        } else {
            arg_count(ctx, sites, engine);
        }
    }
    if ctx.on("T004") {
        missing_endon(ctx);
    }
}

/// T003: `waittill( "e", a, b, c )` binds more variables than any notifier sends.
fn arg_count(ctx: &mut Ctx, sites: &[EventSite], engine: Option<&EventCatalogue>) {
    let dynamic = ctx.ws.dynamic_emit_payload();
    for site in sites {
        if site.op != "waittill" || site.argc == 0 {
            continue;
        }
        let Some(lower) = site.key() else { continue };
        let Payload::AtMost(mut max) = ctx.ws.event_payload(&lower) else {
            continue;
        };
        if dynamic.is_some_and(|d| d >= site.argc) {
            continue;
        }
        if let Some(c) = engine {
            for e in c.lookup(&lower) {
                if e.args_variable {
                    max = usize::MAX;
                }
                max = max.max(e.args);
            }
        }
        if site.argc <= max {
            continue;
        }
        // Without a complete engine catalogue the engine may notify the event too: scripts
        // re-notify engine events such as "damage" with part of the payload. Then only a lone
        // waiter binding more than the scripts send is reported; several are the engine's.
        let complete = engine.is_some_and(|c| c.complete);
        if !complete {
            let over = ctx
                .ws
                .event_sites(&lower)
                .iter()
                .map(|&s| ctx.ws.event_site(s))
                .filter(|s| s.op == "waittill" && s.argc > max)
                .count();
            if over > 1 {
                continue;
            }
        }
        let Some(node) = name_node(ctx.root, site) else {
            continue;
        };
        if ctx.near_syntax_error(&node) {
            continue;
        }
        let Some(call) = node.ancestors().find_map(ast::CallExpr::cast) else {
            continue;
        };
        // `r = self waittill( "a", "b", x )` (T8+): the waittill returns a struct and every
        // argument is an event name, variables included.
        if call
            .syntax()
            .parent()
            .is_some_and(|p| p.kind() == SyntaxKind::ASSIGN_EXPR)
        {
            continue;
        }
        // The bound variables: the arguments after the event names, all plain names.
        let rest: Vec<ast::Expr> = call
            .arg_list()
            .map(|a| a.exprs().skip(1).collect())
            .unwrap_or_default();
        let bound: Vec<TextRange> = rest
            .iter()
            .filter(|e| !matches!(e, ast::Expr::Literal(_)))
            .map(|e| e.syntax().text_range())
            .collect();
        if rest
            .iter()
            .any(|e| !matches!(e, ast::Expr::Literal(_) | ast::Expr::NameRef(_)))
        {
            continue;
        }
        let Some(first_extra) = bound.get(max) else {
            continue;
        };
        let range = first_extra.cover(*bound.last().unwrap_or(first_extra));
        let sent = match max {
            0 => "no value".to_owned(),
            1 => "1 value".to_owned(),
            n => format!("{n} values"),
        };
        let extra = site.argc - max;
        ctx.report(
            "T003",
            format!(
                "every notify of \"{}\" sends {sent}: the last {} of the {} variables \
                 bound here {} always undefined",
                site.name.render(),
                if extra == 1 {
                    "one".to_owned()
                } else {
                    extra.to_string()
                },
                site.argc,
                if extra == 1 { "is" } else { "are" },
            ),
            range,
            None,
        );
    }
}

/// The local a call's result is assigned to (`r = self waittill( "e" )`).
fn assigned_local(call: &ast::CallExpr) -> Option<String> {
    let assign = call.syntax().parent().and_then(ast::AssignExpr::cast)?;
    assign.eq_token()?;
    match assign.lhs()? {
        ast::Expr::NameRef(r) => Some(r.text().to_ascii_lowercase()),
        _ => None,
    }
}

/// Is `e` the local `var`?
fn is_local(e: Option<ast::Expr>, var: &str) -> bool {
    matches!(e, Some(ast::Expr::NameRef(r)) if r.text().eq_ignore_ascii_case(var))
}

/// The fields of `var` read after `call` (lower case, with their ranges): in the block holding
/// the call (a read after an `if` may see another branch's value) and before `var` is assigned
/// again. Fields the function writes into `var` are left out.
fn field_reads(func: &SyntaxNode, call: &ast::CallExpr, var: &str) -> Vec<(String, TextRange)> {
    let from = call.syntax().text_range().end();
    let Some(block) = call
        .syntax()
        .ancestors()
        .find(|a| a.kind() == SyntaxKind::BLOCK)
    else {
        return Vec::new();
    };
    let next = func
        .descendants()
        .filter_map(ast::AssignExpr::cast)
        .filter(|a| is_local(a.lhs(), var))
        .map(|a| a.syntax().text_range().start())
        .find(|&s| s > from)
        .unwrap_or(TextSize::from(u32::MAX));
    let scope = TextRange::new(from, next.min(block.text_range().end()));
    let mut written: Vec<String> = Vec::new();
    let mut reads = Vec::new();
    for n in func.descendants() {
        let Some(fe) = ast::FieldExpr::cast(n) else {
            continue;
        };
        if !is_local(fe.base(), var) {
            continue;
        }
        let Some(name) = fe.field() else { continue };
        let key = gsc_sema::events::field_key(&name.text());
        let target = fe
            .syntax()
            .parent()
            .and_then(ast::AssignExpr::cast)
            .and_then(|a| a.lhs())
            .is_some_and(|l| l.syntax() == fe.syntax());
        if target {
            written.push(key);
        } else if scope.contains_range(fe.syntax().text_range()) {
            reads.push((key, name.syntax().text_range()));
        }
    }
    reads.retain(|(k, _)| !written.contains(k));
    reads
}

/// T003 where `waittill` returns the payload struct (BO4 and later, measured in the T8 and T9
/// VMs): `r = self waittill( "e" )` then `r.f`, while no notify of `"e"` sends `f`.
fn result_fields(ctx: &mut Ctx, sites: &[EventSite], engine: Option<&EventCatalogue>) {
    let mut dynamic: Option<Option<BTreeSet<String>>> = None;
    let mut seen: Vec<TextSize> = Vec::new();
    for site in sites {
        if site.op != "waittill" || seen.contains(&site.focus) {
            continue;
        }
        seen.push(site.focus);
        // Every event of the call (`waittill( "a", "b" )` wakes on either), each literal.
        let keys: Option<Vec<String>> = sites
            .iter()
            .filter(|s| s.op == "waittill" && s.focus == site.focus)
            .map(EventSite::key)
            .collect();
        let Some(keys) = keys else { continue };
        // What the notifies send: every notifier a script `notify` with a struct literal or no
        // payload, none from the engine; a notify of a computed name counts when the name is
        // written somewhere as a string.
        let mut sent = BTreeSet::new();
        let mut known = true;
        for k in &keys {
            if engine.is_some_and(|c| c.contains(k)) {
                known = false;
                break;
            }
            match ctx.ws.event_payload_fields(k) {
                Some(f) => sent.extend(f),
                None => known = false,
            }
            if ctx.ws.event_mentioned(k) {
                match dynamic.get_or_insert_with(|| ctx.ws.dynamic_emit_fields()) {
                    Some(f) => sent.extend(f.iter().cloned()),
                    None => known = false,
                }
            }
        }
        if !known {
            continue;
        }
        let Some(node) = name_node(ctx.root, site) else {
            continue;
        };
        if ctx.near_syntax_error(&node) {
            continue;
        }
        let Some(call) = node.ancestors().find_map(ast::CallExpr::cast) else {
            continue;
        };
        let Some(var) = assigned_local(&call) else {
            continue;
        };
        let Some(func) = call.syntax().ancestors().find(|a| {
            matches!(
                a.kind(),
                SyntaxKind::FUNCTION_DEF | SyntaxKind::CONSTRUCTOR | SyntaxKind::DESTRUCTOR
            )
        }) else {
            continue;
        };
        let reads = field_reads(&func, &call, &var);
        let events = keys
            .iter()
            .map(|k| format!("\"{k}\""))
            .collect::<Vec<_>>()
            .join(" or ");
        for (field, range) in reads {
            if field == "_notify" || sent.contains(&field) {
                continue;
            }
            ctx.report(
                "T003",
                format!(
                    "no notify of {events} sends `{field}`: `{var}.{field}` is always undefined \
                     (`waittill` returns the notify's payload struct)"
                ),
                range,
                None,
            );
        }
    }
}

/// Does the code wait for time (`wait`, `waitframe`, `waitrealtime`, `waittillframeend`)?
fn timed_wait(body: &SyntaxNode) -> bool {
    body.descendants().any(|n| {
        matches!(
            n.kind(),
            SyntaxKind::WAIT_STMT
                | SyntaxKind::WAIT_REAL_TIME_STMT
                | SyntaxKind::WAIT_FRAME_STMT
                | SyntaxKind::WAITTILL_FRAME_END_STMT
        ) || ast::CallExpr::cast(n).is_some_and(|c| {
            matches!(crate::workspace::callee(&c),
                crate::workspace::Callee::Bare(name, _)
                    if ["wait", "waitframe", "waitrealtime"]
                        .iter()
                        .any(|w| w.eq_ignore_ascii_case(&name)))
        })
    })
}

/// Does the function contain a macro use, which may hide an `endon`?
fn uses_macro(body: &SyntaxNode) -> bool {
    body.descendants().any(|n| match n.kind() {
        SyntaxKind::MACRO_STMT => true,
        SyntaxKind::CALL_EXPR => ast::CallExpr::cast(n).is_some_and(|c| {
            matches!(crate::workspace::callee(&c),
                crate::workspace::Callee::Bare(name, _) if looks_like_macro(&name))
        }),
        _ => false,
    })
}

/// T004: a function threaded on players that loops forever with no `endon` on `self`.
fn missing_endon(ctx: &mut Ctx) {
    // Client scripts run per local client: a player leaving ends the client VM itself.
    if ctx.vm == Some(gsc_sema::Vm::Csc) {
        return;
    }
    let facts = &ctx.ws.file(ctx.file).facts;
    for (i, d) in facts.defs.iter().enumerate() {
        if !matches!(d.kind, DefKind::Function | DefKind::Method) {
            continue;
        }
        let id = DefId {
            file: ctx.file,
            index: i as u32,
        };
        let st = ctx.ws.thread_starts(id);
        if st.thread_on_player == 0
            || st.thread_on_level > 0
            || st.childthread > 0
            || st.sync > 0
            || st.pointer > 0
        {
            continue;
        }
        // Any `endon` counts, whatever its receiver (`player = self; player endon( .. )`, a
        // `level endon( str )` the caller chose): the thread's end was thought about.
        let ends = facts
            .events
            .iter()
            .any(|s| s.caller == Some(i as u32) && s.class == EventClass::Endon);
        if ends {
            continue;
        }
        let Some(def) = def_node(ctx.root, d).and_then(ast::FunctionDef::cast) else {
            continue;
        };
        let Some(body) = def.body() else { continue };
        let body = body.syntax().clone();
        if uses_macro(&body) {
            continue;
        }
        // An infinite loop the function cannot leave, with a timed wait: a loop that only
        // waits on the player's own notifies dies with the player (freeing an object ends the
        // threads waiting on it), a timed wait outlives it.
        let looping = body.descendants().any(|n| {
            infinite_loop(&n).is_some_and(|(b, _)| !may_exit(b.syntax()) && timed_wait(b.syntax()))
        });
        if !looping || ctx.near_syntax_error(&body) {
            continue;
        }
        ctx.report(
            "T004",
            format!(
                "`{}` is threaded on a player ({} site{}) and loops forever, but nothing ends \
                 it when the player leaves: add `self endon( \"disconnect\" )`",
                d.short_name(),
                st.thread_on_player,
                if st.thread_on_player == 1 { "" } else { "s" }
            ),
            d.name_range,
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families() {
        assert!(same_family("wave1_done", "wave2_done"));
        assert!(same_family("goals", "goal"));
        assert!(same_family("convoy_stop_1", "convoy_stop"));
        assert!(same_family("move_done", "movedone"));
        assert!(same_family("enable", "enabled"));
        assert!(same_family("night_vision_on", "night_vision_off"));
        assert!(same_family("start_mortars 0", "start_mortars"));
        assert!(!same_family("disconnect", "dissconnect"));
    }
}
