//! N005 duplicate-function, N006 duplicate-parameter, N007 misplaced-vararg, N008
//! inheritance-cycle, N009 constructor-parameters: declarations the compiler refuses.
//!
//! After gscode's `DuplicateFunction`, `DuplicateParameter`, `VarargNotLastParameter`,
//! `ClassInheritanceCycle` and `ConstructorHasParameters` (Blakintosh/gscode, GPL-3.0); each was
//! confirmed with the BO3 linker (`tools/linker/probes/lint_*.gsc`).

use std::collections::HashMap;

use gsc_sema::DefKind;
use gsc_syntax::ast::{self, AstNode};
use gsc_syntax::{SyntaxKind::*, SyntaxNode, TextRange};

use super::{in_error, Ctx};

pub fn check(ctx: &mut Ctx) {
    if ctx.on("N005") {
        duplicate_functions(ctx);
    }
    if ctx.on("N006") || ctx.on("N007") || ctx.on("N009") {
        let lists: Vec<SyntaxNode> = ctx
            .root
            .descendants()
            .filter(|n| n.kind() == PARAM_LIST)
            .collect();
        for l in lists {
            params(ctx, &l);
        }
    }
    if ctx.on("N008") {
        cycles(ctx);
    }
}

fn duplicate_functions(ctx: &mut Ctx) {
    // Decompiler output names functions by hash; two spellings of one hash are not two functions.
    if ctx.decompiled {
        return;
    }
    let facts = &ctx.ws.file(ctx.file).facts;
    let mut first: HashMap<(DefKind, &str, &str), TextRange> = HashMap::new();
    let mut found = Vec::new();
    for d in &facts.defs {
        if !d.kind.is_callable() || d.name.is_empty() {
            continue;
        }
        let key = (d.kind, d.ns.as_str(), d.name.as_str());
        match first.get(&key) {
            Some(_) => found.push((d.name_range, d.short_name().to_owned(), d.ns.clone())),
            None => {
                first.insert(key, d.name_range);
            }
        }
    }
    for (range, name, ns) in found {
        let node = match ctx.root.covering_element(range) {
            gsc_syntax::NodeOrToken::Node(n) => n,
            gsc_syntax::NodeOrToken::Token(t) => match t.parent() {
                Some(p) => p,
                None => continue,
            },
        };
        if in_error(&node) || ctx.near_syntax_error(&node) {
            continue;
        }
        let namespaces =
            ctx.ws.profile_of(ctx.file).config.resolution == gsc_sema::Resolution::Namespace;
        let where_ = if ns.is_empty() || !namespaces {
            String::new()
        } else {
            format!(" in namespace `{ns}`")
        };
        ctx.report(
            "N005",
            format!("function `{name}` is already defined{where_} in this file (BO3's compiler refuses it: \"Duplicated function name detected\")"),
            range,
            None,
        );
    }
}

fn params(ctx: &mut Ctx, list: &SyntaxNode) {
    let Some(pl) = ast::ParamList::cast(list.clone()) else {
        return;
    };
    if in_error(list) || ctx.near_syntax_error(list) {
        return;
    }
    let cfg = ctx.ws.profile_of(ctx.file).config;
    let owner = list.parent().map(|p| p.kind());
    let params: Vec<ast::AnyParam> = pl.any_params().collect();
    if matches!(owner, Some(CONSTRUCTOR | DESTRUCTOR)) && !params.is_empty() && ctx.on("N009") {
        let what = if owner == Some(CONSTRUCTOR) {
            "a constructor"
        } else {
            "a destructor"
        };
        let first = params[0].syntax().text_range().start();
        let last = params[params.len() - 1].syntax().text_range().end();
        ctx.report(
            "N009",
            format!("{what} takes no parameters (BO3's compiler refuses one)"),
            TextRange::new(first, last),
            None,
        );
    }
    let mut seen: HashMap<String, ()> = HashMap::new();
    let mut vararg_seen = false;
    let mut found = Vec::new();
    for p in &params {
        match p {
            ast::AnyParam::VarargParam(v) => {
                if vararg_seen {
                    found.push((
                        "N007",
                        "`...` appears twice: it must be the last parameter, once".to_owned(),
                        v.syntax().text_range(),
                    ));
                }
                vararg_seen = true;
            }
            ast::AnyParam::Param(p) => {
                let Some(name) = p.name() else { continue };
                if vararg_seen {
                    found.push((
                        "N007",
                        format!(
                            "parameter `{}` after `...`: `...` must be the last parameter",
                            name.text()
                        ),
                        p.syntax().text_range(),
                    ));
                }
                if seen.insert(cfg.ident(&name.text()), ()).is_some() {
                    found.push((
                        "N006",
                        format!(
                            "parameter `{}` is declared twice: the first one can never be read \
                             (BO3's compiler refuses it)",
                            name.text()
                        ),
                        name.syntax().text_range(),
                    ));
                }
            }
        }
    }
    for (code, message, range) in found {
        if ctx.on(code) {
            ctx.report(code, message, range, None);
        }
    }
}

fn cycles(ctx: &mut Ctx) {
    let classes: Vec<ast::ClassDef> = ctx
        .root
        .descendants()
        .filter_map(ast::ClassDef::cast)
        .collect();
    let cfg = ctx.ws.profile_of(ctx.file).config;
    for c in classes {
        let Some(name) = c.name() else { continue };
        if c.class_base().is_none() || in_error(c.syntax()) {
            continue;
        }
        let me = cfg.ident(&name.text());
        // class_and_bases walks the bases depth first, each once: the class itself coming back
        // through a base is a cycle.
        let chain = ctx.ws.class_and_bases(&me);
        let cyclic = chain.iter().skip(1).any(|(c, decl)| {
            decl.is_some()
                && ctx
                    .ws
                    .class_and_bases(c)
                    .iter()
                    .skip(1)
                    .any(|(b, _)| *b == me)
        });
        if cyclic {
            let path: Vec<&str> = chain.iter().map(|(c, _)| c.as_str()).collect();
            ctx.report(
                "N008",
                format!(
                    "class `{me}` inherits from itself (through {})",
                    path[1..].join(", ")
                ),
                name.syntax().text_range(),
                None,
            );
        }
    }
}
