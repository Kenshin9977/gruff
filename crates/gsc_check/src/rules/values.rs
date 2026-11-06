//! V001 hash-equality: `==` between a hash literal and anything, in a game whose VM does not
//! compare hashes by value (`hash_equality = "script-error"`: T7, measured in the emulated VM,
//! where the pair is a script error and the result false).

use gsc_syntax::ast::{self, AstNode};
use gsc_syntax::SyntaxKind;

use super::{in_error, Ctx};

/// Is `e` a `#"..."` literal?
fn hash_literal(e: Option<ast::Expr>) -> bool {
    let Some(ast::Expr::Literal(l)) = e.map(gsc_sema::facts::strip_parens) else {
        return false;
    };
    l.token()
        .is_some_and(|t| t.kind() == SyntaxKind::HASH_STRING)
}

pub fn check(ctx: &mut Ctx) {
    if !ctx.ws.profile_of(ctx.file).config.hash_equality_error {
        return;
    }
    let mut found = Vec::new();
    for n in ctx.root.descendants() {
        let Some(b) = ast::BinExpr::cast(n) else {
            continue;
        };
        // `!=` and `===` were not run in the VM: only `==` is reported.
        if b.op().is_none_or(|t| t.kind() != SyntaxKind::EQ2) {
            continue;
        }
        if !(hash_literal(b.lhs()) || hash_literal(b.rhs())) {
            continue;
        }
        if in_error(b.syntax()) || ctx.near_syntax_error(b.syntax()) {
            continue;
        }
        found.push(b.syntax().text_range());
    }
    for range in found {
        ctx.report(
            "V001",
            "`==` with a hash is a script error in this game ('unmatching types') and is always \
             false: compare the strings instead"
                .to_owned(),
            range,
            None,
        );
    }
}
