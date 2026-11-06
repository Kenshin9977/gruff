//! L001 infinite-loop-without-wait.

use gsc_syntax::ast::{self, AstNode};
use gsc_syntax::{SyntaxKind::*, SyntaxNode, TextRange, TextSize};

use super::{in_error, Ctx};
use crate::workspace::summarize;
use crate::{Applicability, Edit, Fix};

/// `1`, `true`, `(1)`: a condition that is always true.
fn always_true(e: Option<ast::Expr>) -> bool {
    match e {
        Some(ast::Expr::ParenExpr(p)) => always_true(p.expr()),
        Some(ast::Expr::Literal(l)) => match l.token() {
            Some(t) if t.kind() == TRUE_KW => true,
            Some(t) if t.kind() == INT_NUMBER => {
                let s = t.text();
                s.parse::<i64>().is_ok_and(|v| v != 0)
            }
            _ => false,
        },
        _ => false,
    }
}

/// The body of an infinite loop, and the range to report (the header).
pub(super) fn infinite_loop(n: &SyntaxNode) -> Option<(ast::Stmt, TextRange)> {
    let header = |close: Option<gsc_syntax::SyntaxToken>| {
        let start = n.text_range().start();
        let end = close.map_or(start, |t| t.text_range().end());
        TextRange::new(start, end.max(start))
    };
    match n.kind() {
        WHILE_STMT => {
            let w = ast::WhileStmt::cast(n.clone())?;
            always_true(w.condition()).then_some(())?;
            Some((w.body()?, header(w.r_paren_token())))
        }
        FOR_STMT => {
            let f = ast::ForStmt::cast(n.clone())?;
            match f.for_condition() {
                None => {}
                Some(c) if always_true(c.expr()) => {}
                Some(_) => return None,
            }
            Some((f.body()?, header(f.r_paren_token())))
        }
        DO_WHILE_STMT => {
            let d = ast::DoWhileStmt::cast(n.clone())?;
            always_true(d.condition()).then_some(())?;
            let kw = d.do_kw_token()?;
            Some((d.body()?, kw.text_range()))
        }
        _ => None,
    }
}

/// Can the body leave the loop (`break`, `return`, `goto`)? Counted anywhere in the body, even
/// a `break` that only leaves a nested `switch`: missing a report beats a false one.
pub(super) fn may_exit(body: &SyntaxNode) -> bool {
    body.descendants()
        .any(|n| matches!(n.kind(), BREAK_STMT | RETURN_STMT | GOTO_STMT))
}

/// Indentation of the line holding `offset`.
fn indent_at(text: &str, offset: TextSize) -> &str {
    let o = usize::from(offset).min(text.len());
    let line_start = text[..o].rfind(['\n', '\r']).map_or(0, |i| i + 1);
    let rest = &text[line_start..];
    let end = rest
        .find(|c: char| c != ' ' && c != '\t')
        .unwrap_or(rest.len());
    &rest[..end]
}

/// Insert `wait 0.05;` as the last statement of a block body.
fn wait_fix(text: &str, body: &ast::Stmt) -> Option<Fix> {
    let ast::Stmt::Block(block) = body else {
        return None;
    };
    let at = block.r_curly_token()?.text_range().start();
    let o = usize::from(at);
    let line_start = text[..o].rfind(['\n', '\r']).map_or(0, |i| i + 1);
    let edit = if text[line_start..o].trim().is_empty() {
        // `}` on its own line: a new line before it, indented like the body.
        let brace_indent = indent_at(text, at);
        let inner = block
            .stmts()
            .next()
            .map(|s| indent_at(text, s.syntax().text_range().start()).to_owned())
            .filter(|i| i.len() > brace_indent.len())
            .unwrap_or_else(|| format!("{brace_indent}\t"));
        let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
        Edit {
            range: TextRange::empty(TextSize::from(line_start as u32)),
            content: format!("{inner}wait 0.05;{nl}"),
        }
    } else {
        Edit {
            range: TextRange::empty(at),
            content: "wait 0.05; ".to_owned(),
        }
    };
    Some(Fix {
        applicability: Applicability::Unsafe,
        message: "Add `wait 0.05;` at the end of the loop body".to_owned(),
        edits: vec![edit],
    })
}

pub fn check(ctx: &mut Ctx) {
    let loops: Vec<_> = ctx
        .root
        .descendants()
        .filter(|n| matches!(n.kind(), WHILE_STMT | FOR_STMT | DO_WHILE_STMT))
        .collect();
    for n in loops {
        let Some((body, range)) = infinite_loop(&n) else {
            continue;
        };
        if in_error(&n) || may_exit(body.syntax()) || ctx.near_syntax_error(&n) {
            continue;
        }
        let y = summarize(body.syntax());
        if ctx.sums.yields(ctx.ws, ctx.file, &y) {
            continue;
        }
        let fix = wait_fix(ctx.text, &body);
        ctx.report(
            "L001",
            "infinite loop never waits: the VM stops the script with \"potential infinite loop\""
                .to_owned(),
            range,
            fix,
        );
    }
}
