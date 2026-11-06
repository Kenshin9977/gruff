//! V002 thread-call-value, V003 read-only-assignment, V004 division-by-zero, V005
//! useless-expression.
//!
//! After gscode's `ConsumedThreadedCallResult`, `SizeIsReadOnly`, `CannotAssignToGlobalObject`,
//! `CannotAssignToConstant`, `DivisionByZero` and `InvalidExpressionStatement`
//! (Blakintosh/gscode, GPL-3.0). V002 follows the profile's measured `thread_call_value`; what
//! the BO3 compiler refuses was measured with its linker (`tools/linker/probes/lint_*.gsc`).

use std::collections::HashSet;

use gsc_sema::facts::strip_parens;
use gsc_syntax::ast::{self, AstNode};
use gsc_syntax::{FeatureValue, SyntaxKind::*, SyntaxNode};

use super::{in_error, Ctx};
use crate::workspace::is_threaded;

pub fn check(ctx: &mut Ctx) {
    let cfg = ctx.ws.profile_of(ctx.file).config;
    let v002 = ctx.on("V002") && cfg.thread_call_result == Some(true);
    let (v003, v004, v005) = (ctx.on("V003"), ctx.on("V004"), ctx.on("V005"));
    let globals = global_objects(ctx);
    let nodes: Vec<SyntaxNode> = ctx.root.descendants().collect();
    let mut found = Vec::new();
    for n in &nodes {
        if in_error(n) {
            continue;
        }
        match n.kind() {
            CALL_EXPR | POINTER_CALL_EXPR | METHOD_CALL_EXPR if v002 => {
                if threaded(n) && value_used(n) && callee_may_wait(ctx, n) {
                    found.push((
                        "V002",
                        "the value of a `thread` call is the function's return value only if it \
                         ends before its first wait, and this one may wait: the value is then \
                         undefined (call it without `thread` to use the result)"
                            .to_owned(),
                        n.text_range(),
                    ));
                }
            }
            ASSIGN_EXPR if v003 => {
                if let Some(lhs) = ast::AssignExpr::cast(n.clone()).and_then(|a| a.lhs()) {
                    if let Some(m) = read_only(ctx, &globals, lhs.clone()) {
                        found.push(("V003", m, lhs.syntax().text_range()));
                    }
                }
            }
            PREFIX_EXPR | POSTFIX_EXPR if v003 => {
                let inc = n
                    .children_with_tokens()
                    .filter_map(|e| e.into_token())
                    .any(|t| matches!(t.kind(), PLUS2 | MINUS2));
                if let Some(e) = n.children().find_map(ast::Expr::cast).filter(|_| inc) {
                    if let Some(m) = read_only(ctx, &globals, e.clone()) {
                        found.push(("V003", m, e.syntax().text_range()));
                    }
                }
            }
            BIN_EXPR if v004 => {
                let Some(b) = ast::BinExpr::cast(n.clone()) else {
                    continue;
                };
                if (b.slash_token().is_some() || b.percent_token().is_some()) && is_zero(b.rhs()) {
                    found.push((
                        "V004",
                        "division by zero: a script error when this runs".to_owned(),
                        n.text_range(),
                    ));
                }
            }
            EXPR_STMT if v005 => {
                let Some(e) = ast::ExprStmt::cast(n.clone()).and_then(|s| s.expr()) else {
                    continue;
                };
                if let Some(what) = no_effect(ctx, strip_parens(e)) {
                    found.push((
                        "V005",
                        format!(
                            "this statement is {what} whose value is discarded, so it does \
                             nothing (the BO3 compiler refuses it): a missing `=` or `()`?"
                        ),
                        n.text_range(),
                    ));
                }
            }
            _ => {}
        }
        if v004 && n.kind() == ASSIGN_EXPR {
            if let Some(a) = ast::AssignExpr::cast(n.clone()) {
                if (a.slasheq_token().is_some() || a.percenteq_token().is_some())
                    && is_zero(a.rhs())
                {
                    found.push((
                        "V004",
                        "division by zero: a script error when this runs".to_owned(),
                        n.text_range(),
                    ));
                }
            }
        }
    }
    for (code, message, range) in found {
        let node = match ctx.root.covering_element(range) {
            gsc_syntax::NodeOrToken::Node(n) => n,
            gsc_syntax::NodeOrToken::Token(t) => match t.parent() {
                Some(p) => p,
                None => continue,
            },
        };
        if !ctx.near_syntax_error(&node) {
            ctx.report(code, message, range, None);
        }
    }
}

fn threaded(n: &SyntaxNode) -> bool {
    let m = match n.kind() {
        CALL_EXPR => ast::CallExpr::cast(n.clone()).and_then(|c| c.modifier()),
        POINTER_CALL_EXPR => ast::PointerCallExpr::cast(n.clone()).and_then(|c| c.modifier()),
        METHOD_CALL_EXPR => ast::MethodCallExpr::cast(n.clone()).and_then(|c| c.modifier()),
        _ => None,
    };
    is_threaded(m)
}

/// May the function a threaded call starts wait (a pointer or an unknown function may)?
fn callee_may_wait(ctx: &Ctx, n: &SyntaxNode) -> bool {
    use crate::workspace::{callee, Callee, Target, Yields};
    let Some(call) = ast::CallExpr::cast(n.clone()) else {
        return true;
    };
    let target = match callee(&call) {
        Callee::Bare(name, _) => Target::Bare(name),
        Callee::Qualified(ns, name, _) => Target::Qualified(ns, name),
        Callee::Local(name, _) => Target::Local(name),
        _ => return true,
    };
    let y = Yields {
        direct: false,
        calls: vec![target],
    };
    ctx.sums.yields(ctx.ws, ctx.file, &y)
}

/// Is the value of this expression used: anything but the expression of a statement?
fn value_used(n: &SyntaxNode) -> bool {
    let mut cur = n.clone();
    while let Some(p) = cur.parent() {
        match p.kind() {
            PAREN_EXPR => cur = p,
            EXPR_STMT | MACRO_STMT | FOR_INIT | FOR_STEP => return false,
            _ => return true,
        }
    }
    false
}

fn global_objects(ctx: &Ctx) -> HashSet<String> {
    let dialect = ctx.ws.profile_of(ctx.file).dialect(ctx.decompiled);
    match dialect.feature("global_objects") {
        Some(FeatureValue::List(v)) => v.iter().map(|s| s.to_ascii_lowercase()).collect(),
        _ => ["self", "level", "game", "anim"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    }
}

/// Why `lhs` cannot be written, if it cannot.
fn read_only(ctx: &Ctx, globals: &HashSet<String>, lhs: ast::Expr) -> Option<String> {
    match strip_parens(lhs) {
        ast::Expr::FieldExpr(f) => {
            let tok = f.field()?.syntax().first_token()?;
            (tok.text().eq_ignore_ascii_case("size")).then(|| {
                "`.size` is read-only: it counts an array's elements (or a string's characters) \
                 and cannot be assigned"
                    .to_owned()
            })
        }
        ast::Expr::NameRef(n) => {
            let tok = n.syntax().first_token()?;
            let lower = tok.text().to_ascii_lowercase();
            // Only where the profile makes the word a keyword: decompiler output (and games
            // whose profile leaves it out) may have a local of that name.
            if tok.kind().is_name_like_keyword() && globals.contains(&lower) && !ctx.decompiled {
                return Some(format!(
                    "`{lower}` is an engine global and cannot be assigned: write a field of it \
                     instead (`{lower}.name = value`)"
                ));
            }
            if tok.kind() == IDENT && is_const_local(ctx, n.syntax(), tok.text()) {
                return Some(format!(
                    "`{}` is declared `const` and cannot be assigned",
                    tok.text()
                ));
            }
            None
        }
        _ => None,
    }
}

/// Does the function holding `n` declare `const name`?
fn is_const_local(ctx: &Ctx, n: &SyntaxNode, name: &str) -> bool {
    let cfg = ctx.ws.profile_of(ctx.file).config;
    let Some(body) = n
        .ancestors()
        .find(|a| matches!(a.kind(), FUNCTION_DEF | CONSTRUCTOR | DESTRUCTOR))
    else {
        return false;
    };
    let want = cfg.ident(name);
    body.descendants()
        .filter_map(ast::ConstStmt::cast)
        .filter_map(|c| c.name())
        .any(|c| cfg.ident(&c.text()) == want)
}

fn is_zero(e: Option<ast::Expr>) -> bool {
    let Some(ast::Expr::Literal(l)) = e.map(strip_parens) else {
        return false;
    };
    l.token().is_some_and(|t| match t.kind() {
        INT_NUMBER => t.text().trim().parse::<i64>() == Ok(0),
        FLOAT_NUMBER => t
            .text()
            .trim()
            .trim_end_matches(['f', 'F'])
            .parse::<f64>()
            .is_ok_and(|v| v == 0.0),
        _ => false,
    })
}

/// What an expression statement is when it can have no effect.
fn no_effect(ctx: &Ctx, e: ast::Expr) -> Option<&'static str> {
    match e {
        ast::Expr::BinExpr(b) => Some(
            if b.eq2_token().is_some()
                || b.neq_token().is_some()
                || b.eq3_token().is_some()
                || b.neq2_token().is_some()
            {
                "a comparison"
            } else {
                "an operation"
            },
        ),
        ast::Expr::Literal(_) => Some("a literal"),
        ast::Expr::NameRef(n) => {
            // `WAIT_SERVER_FRAME;`: a macro supplies the statement.
            let t = n.syntax().first_token()?;
            let text = t.text();
            // A macro supplies the statement; a keyword of another game read as a name
            // (`do` where the profile has no `do`) is the parser's guess, not the author's.
            if crate::workspace::looks_like_macro(text)
                || ctx.ws.macros_spelled(text).next().is_some()
                || gsc_syntax::SyntaxKind::KEYWORDS
                    .iter()
                    .any(|(k, _)| k.eq_ignore_ascii_case(text))
            {
                return None;
            }
            Some("a variable")
        }
        ast::Expr::FieldExpr(_) | ast::Expr::IndexExpr(_) => Some("a read"),
        ast::Expr::PrefixExpr(p) if p.plus2_token().is_none() && p.minus2_token().is_none() => {
            Some("an operation")
        }
        ast::Expr::VectorExpr(_) | ast::Expr::ArrayExpr(_) | ast::Expr::IsExpr(_) => {
            Some("a value")
        }
        ast::Expr::FunctionRef(_) => Some("a function reference"),
        _ => None,
    }
}
