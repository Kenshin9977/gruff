//! U001 unused-variable, U002 uninitialized-variable: the locals of one function body.
//!
//! Ported in substance from gscode's `LocalUses`, `UnusedLocalLint` and `UnassignedVariableLint`
//! (Blakintosh/gscode, GPL-3.0): one walk classifies every use of a local name, flow-insensitively,
//! and each rule reads what it needs. Class members are skipped (a method reaches the class's
//! `var`s by their bare name), so are names a macro may supply.

use std::collections::{HashMap, HashSet};

use gsc_sema::facts::def_node;
use gsc_sema::{EventArgs, FileId};
use gsc_syntax::ast::{self, AstNode};
use gsc_syntax::{FeatureValue, SyntaxKind::*, SyntaxNode, TextRange};

use super::{in_error, Ctx};
use crate::workspace::looks_like_macro;

/// How a site uses a local name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Use {
    Read,
    /// `x = v` or `const x = v`: written, nothing read.
    Assign,
    /// `x += v`, `x++`, `x[ i ] = v`, `x.f = v`: written, but the old value is used.
    Update,
    /// A `foreach` binding or a `waittill` output: written by the loop or the engine.
    Bind,
}

/// The uses of the locals of one function, in source order (identifier case).
struct Uses {
    sites: Vec<(String, Use, TextRange)>,
    /// Identifiers of the macros the body uses: any of them may read or write a local.
    macro_words: HashSet<String>,
    /// A macro the body uses could not be read: nothing can be said.
    opaque: bool,
}

/// Is `n` (a `NAME_REF`) a place that names something other than a local?
fn not_a_local(n: &SyntaxNode) -> bool {
    let Some(p) = n.parent() else { return true };
    let first = p.first_child().as_ref() == Some(n);
    match p.kind() {
        CALL_EXPR => ast::CallExpr::cast(p.clone())
            .and_then(|c| c.callee())
            .is_some_and(|c| c.syntax() == n),
        QUALIFIED_REF | FUNCTION_REF | NEW_EXPR | CLASS_BASE | CAST_EXPR | GOTO_STMT
        | METHOD_CALL_EXPR | MACRO_CALL_ITEM => true,
        FIELD_EXPR => !first,
        STRUCT_FIELD | ARRAY_ELEMENT => first,
        _ => false,
    }
}

/// Is `n` the variable an assignment, `++`/`--` or `foreach` binding writes through
/// (`x` of `x[ i ] = v`, `x.f = v`)? Returns the use, or `None` for a plain read.
fn write_use(n: &SyntaxNode) -> Option<Use> {
    let mut cur = n.clone();
    let mut direct = true;
    loop {
        let p = cur.parent()?;
        let first = p.first_child().as_ref() == Some(&cur);
        match p.kind() {
            ASSIGN_EXPR if first => {
                let plain = ast::AssignExpr::cast(p)?.eq_token().is_some();
                return Some(if direct && plain {
                    Use::Assign
                } else {
                    Use::Update
                });
            }
            PREFIX_EXPR | POSTFIX_EXPR => {
                let inc = p
                    .children_with_tokens()
                    .filter_map(|e| e.into_token())
                    .any(|t| matches!(t.kind(), PLUS2 | MINUS2));
                return inc.then_some(Use::Update);
            }
            FOREACH_BINDINGS => return Some(if direct { Use::Bind } else { Use::Update }),
            INDEX_EXPR | FIELD_EXPR if first => {
                direct = false;
                cur = p;
            }
            _ => return None,
        }
    }
}

/// Is `n` an output of `waittill( "event", a, b )` (an argument after the event name)?
fn waittill_output(n: &SyntaxNode, args: EventArgs) -> bool {
    if args == EventArgs::Events {
        return false; // BO4 and later: the extra arguments are more events
    }
    let Some(list) = n.parent().filter(|p| p.kind() == ARG_LIST) else {
        return false;
    };
    let Some(call) = list.parent().and_then(ast::CallExpr::cast) else {
        return false;
    };
    let skip = match crate::workspace::callee(&call) {
        crate::workspace::Callee::Keyword(WAITTILL_KW) => 1,
        crate::workspace::Callee::Keyword(WAITTILLMATCH_KW) => 2,
        crate::workspace::Callee::Bare(n, _) if n.eq_ignore_ascii_case("waittill") => 1,
        crate::workspace::Callee::Bare(n, _) if n.eq_ignore_ascii_case("waittillmatch") => 2,
        _ => return false,
    };
    list.children()
        .position(|c| &c == n)
        .is_some_and(|i| i >= skip)
}

/// The identifiers of a `#define`'s body.
fn macro_body_words(ctx: &Ctx, name: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for d in ctx.ws.macros_spelled(name) {
        let data = ctx.ws.file(d.file);
        if !data.has_tree() {
            return None;
        }
        let root = data.syntax();
        let node = def_node(&root, ctx.ws.def(d))?;
        let body = ast::PreprocDefine::cast(node)?
            .macro_body_token()
            .map(|t| t.text().to_owned())
            .unwrap_or_default();
        out.extend(
            body.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .filter(|w| !w.is_empty())
                .map(str::to_owned),
        );
    }
    Some(out)
}

fn is_macro(ctx: &Ctx, name: &str) -> bool {
    looks_like_macro(name) || ctx.ws.macros_spelled(name).next().is_some()
}

fn collect(ctx: &Ctx, body: &SyntaxNode) -> Uses {
    let cfg = ctx.ws.profile_of(ctx.file).config;
    let mut uses = Uses {
        sites: Vec::new(),
        macro_words: HashSet::new(),
        opaque: false,
    };
    let mut macros_seen = HashSet::new();
    for n in body.descendants() {
        let (name, range, kind) = match n.kind() {
            NAME_REF => {
                let Some(tok) = n.first_token() else { continue };
                if tok.kind() != IDENT {
                    continue; // self, level, vararg...: keywords, not locals
                }
                let text = tok.text().to_owned();
                if is_macro(ctx, &text) {
                    if macros_seen.insert(text.clone()) {
                        match macro_body_words(ctx, &text) {
                            Some(w) => uses.macro_words.extend(w.iter().map(|w| cfg.ident(w))),
                            None => uses.opaque = true,
                        }
                    }
                    continue;
                }
                if not_a_local(&n) {
                    continue;
                }
                let kind = match write_use(&n) {
                    Some(u) => u,
                    None if waittill_output(&n, cfg.waittill_args) => Use::Bind,
                    None => Use::Read,
                };
                (text, tok.text_range(), kind)
            }
            // `foreach ( k, v in a )` binds names, not name references.
            NAME if n.parent().is_some_and(|p| p.kind() == FOREACH_BINDINGS) => {
                let Some(tok) = n.first_token() else { continue };
                (tok.text().to_owned(), tok.text_range(), Use::Bind)
            }
            NAME if n.parent().is_some_and(|p| p.kind() == CONST_STMT) => {
                let Some(tok) = n.first_token() else { continue };
                (tok.text().to_owned(), tok.text_range(), Use::Assign)
            }
            _ => continue,
        };
        uses.sites.push((cfg.ident(&name), kind, range));
    }
    uses
}

/// The names a function may read without assigning them: its parameters, the file's constants
/// and those of the files it includes, and the game's global objects.
fn predeclared(ctx: &Ctx, params: Option<ast::ParamList>) -> HashSet<String> {
    let cfg = ctx.ws.profile_of(ctx.file).config;
    let mut out: HashSet<String> = HashSet::new();
    if let Some(list) = params {
        for p in list.any_params() {
            if let ast::AnyParam::Param(p) = p {
                if let Some(n) = p.name() {
                    out.insert(cfg.ident(&n.text()));
                }
            }
        }
    }
    let consts = |f: FileId, out: &mut HashSet<String>| {
        for d in &ctx.ws.file(f).facts.defs {
            if d.kind == gsc_sema::DefKind::Const {
                out.insert(cfg.ident(&d.name));
            }
        }
    };
    consts(ctx.file, &mut out);
    for &u in &ctx.ws.resolution(ctx.file).scope {
        consts(u, &mut out);
    }
    let dialect = ctx.ws.profile_of(ctx.file).dialect(ctx.decompiled);
    if let Some(FeatureValue::List(globals)) = dialect.feature("global_objects") {
        out.extend(globals.iter().map(|g| cfg.ident(g)));
    }
    for g in [
        "self",
        "level",
        "game",
        "anim",
        "world",
        "classes",
        "thisthread",
    ] {
        out.insert(cfg.ident(g));
    }
    out
}

pub fn check(ctx: &mut Ctx) {
    // Decompiler output keeps every dead store of the compiled code and spells locals by hash.
    if ctx.decompiled {
        return;
    }
    let (u001, u002) = (ctx.on("U001"), ctx.on("U002"));
    // A header the file `#insert`s that is not in the workspace takes its macros with it: a
    // constant it defines would read as a local nothing assigns.
    let res = ctx.ws.resolution(ctx.file);
    let lost_macros = ctx
        .ws
        .file(ctx.file)
        .facts
        .imports
        .iter()
        .zip(&res.imports)
        .any(|(i, found)| !i.is_scope() && found.is_none());
    let functions: Vec<SyntaxNode> = ctx
        .root
        .descendants()
        .filter(|n| n.kind() == FUNCTION_DEF)
        .filter(|n| !n.ancestors().any(|a| a.kind() == CLASS_DEF))
        .collect();
    for f in functions {
        let Some(def) = ast::FunctionDef::cast(f.clone()) else {
            continue;
        };
        let Some(body) = def.body() else { continue };
        let r = f.text_range();
        if in_error(&f)
            || f.descendants().any(|d| d.kind() == ERROR)
            || ctx.syntax_errors.iter().any(|&o| r.contains_inclusive(o))
        {
            continue;
        }
        let params = def.param_list();
        let has_vararg = params.as_ref().is_some_and(|l| {
            l.any_params()
                .any(|p| matches!(p, ast::AnyParam::VarargParam(_)))
        });
        let uses = collect(ctx, body.syntax());
        if uses.opaque {
            continue;
        }
        let mut known = predeclared(ctx, params);
        if has_vararg {
            known.insert("vararg".to_owned());
        }
        let mut written: HashSet<&str> = HashSet::new();
        let mut read: HashSet<&str> = HashSet::new();
        for (name, kind, _) in &uses.sites {
            match kind {
                Use::Read => {
                    read.insert(name);
                }
                Use::Assign => {
                    written.insert(name);
                }
                Use::Update | Use::Bind => {
                    written.insert(name);
                    read.insert(name);
                }
            }
        }
        let mut reported: HashMap<&str, ()> = HashMap::new();
        let mut found = Vec::new();
        for (name, kind, range) in &uses.sites {
            let n = name.as_str();
            if uses.macro_words.contains(n) || reported.contains_key(n) {
                continue;
            }
            match kind {
                Use::Assign if u001 && !read.contains(n) && !known.contains(n) => {
                    reported.insert(n, ());
                    found.push(("U001", format!("`{n}` is assigned but never read"), *range));
                }
                Use::Read if u002 && !lost_macros && !written.contains(n) && !known.contains(n) => {
                    reported.insert(n, ());
                    let message = if n.eq_ignore_ascii_case("vararg") {
                        "`vararg` is only bound in a function declaring `...`: add `...` to the \
                         parameter list"
                            .to_owned()
                    } else {
                        format!(
                            "`{n}` is read but nothing in this function assigns it (the BO3 \
                             compiler refuses it: \"Uninitialized local variable\")"
                        )
                    };
                    found.push(("U002", message, *range));
                }
                _ => {}
            }
        }
        // `vararg` in a function without `...`: the pack is bound by the declaration.
        if u002 && !has_vararg {
            for n in body.syntax().descendants() {
                if n.kind() != NAME_REF {
                    continue;
                }
                if n.first_token().is_some_and(|t| t.kind() == VARARG_KW) {
                    found.push((
                        "U002",
                        "`vararg` is only bound in a function declaring `...`: add `...` to the \
                         parameter list"
                            .to_owned(),
                        n.text_range(),
                    ));
                    break;
                }
            }
        }
        for (code, message, range) in found {
            ctx.report(code, message, range, None);
        }
    }
}
