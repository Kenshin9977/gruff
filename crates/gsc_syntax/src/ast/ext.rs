//! Hand-written accessors: the nodes the generator leaves alone (see `MANUAL` in
//! tests/sourcegen.rs) and conveniences over generated ones.

use rowan::TextSize;

use super::*;
use crate::ast::{first_token, support};
use crate::{SyntaxKind, SyntaxKind::*, SyntaxNode, SyntaxToken};

const MODIFIERS: &[SyntaxKind] = &[
    THREAD_KW,
    CHILDTHREAD_KW,
    CALL_KW,
    THREADENDON_KW,
    BUILTIN_KW,
];

fn exprs(node: &SyntaxNode) -> impl Iterator<Item = Expr> {
    node.children().filter_map(Expr::cast)
}

fn token_start(node: &SyntaxNode, kinds: &[SyntaxKind]) -> Option<TextSize> {
    node.children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| kinds.contains(&t.kind()))
        .map(|t| t.text_range().start())
}

fn modifier(node: &SyntaxNode) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| MODIFIERS.contains(&t.kind()))
}

/// The expression ending before `mark`, if any (the receiver of a call).
fn before(node: &SyntaxNode, mark: Option<TextSize>) -> Option<Expr> {
    let mark = mark?;
    exprs(node).find(|e| e.syntax().text_range().end() <= mark)
}

/// The receiver of a `[[ ]]` call: the expression before the modifier, or before `[[`.
fn bracket_receiver(node: &SyntaxNode) -> Option<Expr> {
    let mark = modifier(node)
        .map(|m| m.text_range().start())
        .or_else(|| token_start(node, &[L_BRACK]));
    before(node, mark)
}

/// The first child of type `N` after the `.` or `?.` of a field access.
fn child_after_dot<N: AstNode>(node: &SyntaxNode) -> Option<N> {
    let dot = token_start(node, &[DOT, QUESTION_DOT])?;
    node.children()
        .filter_map(N::cast)
        .find(|n| n.syntax().text_range().start() > dot)
}

/// The first non-trivia token starting at or after the end of the left operand.
fn op_after_lhs(node: &SyntaxNode, lhs: Option<Expr>) -> Option<SyntaxToken> {
    let lhs_end = lhs?.syntax().text_range().end();
    node.children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| !t.kind().is_trivia() && t.text_range().start() >= lhs_end)
}

/// The text of a name node, without surrounding trivia.
fn name_text(node: &SyntaxNode) -> String {
    node.text().to_string().trim().to_owned()
}

impl CallExpr {
    /// `self` in `self thread foo()`.
    pub fn receiver(&self) -> Option<Expr> {
        if let Some(m) = self.modifier() {
            return before(&self.syntax, Some(m.text_range().start()));
        }
        let all: Vec<Expr> = exprs(&self.syntax).collect();
        (all.len() == 2).then(|| all[0].clone())
    }

    /// `thread`, `childthread`, `call`, `threadendon` or `builtin`.
    pub fn modifier(&self) -> Option<SyntaxToken> {
        modifier(&self.syntax)
    }

    /// The called name: a `NameRef` or a `QualifiedRef`.
    pub fn callee(&self) -> Option<Expr> {
        let after = self.modifier().map(|m| m.text_range().end());
        exprs(&self.syntax)
            .filter(|e| after.is_none_or(|a| e.syntax().text_range().start() >= a))
            .last()
    }

    pub fn arg_list(&self) -> Option<ArgList> {
        support::child(&self.syntax, 0)
    }
}

impl PointerCallExpr {
    pub fn receiver(&self) -> Option<Expr> {
        bracket_receiver(&self.syntax)
    }

    pub fn modifier(&self) -> Option<SyntaxToken> {
        modifier(&self.syntax)
    }

    /// The expression between `[[` and `]]`.
    pub fn target(&self) -> Option<Expr> {
        let open = token_start(&self.syntax, &[L_BRACK])?;
        exprs(&self.syntax).find(|e| e.syntax().text_range().start() > open)
    }

    pub fn arg_list(&self) -> Option<ArgList> {
        support::child(&self.syntax, 0)
    }
}

impl MethodCallExpr {
    pub fn receiver(&self) -> Option<Expr> {
        bracket_receiver(&self.syntax)
    }

    pub fn modifier(&self) -> Option<SyntaxToken> {
        modifier(&self.syntax)
    }

    /// The object between `[[` and `]]`.
    pub fn object(&self) -> Option<Expr> {
        let open = token_start(&self.syntax, &[L_BRACK])?;
        let arrow = token_start(&self.syntax, &[ARROW])?;
        exprs(&self.syntax).find(|e| {
            let r = e.syntax().text_range();
            r.start() > open && r.end() <= arrow
        })
    }

    /// The method name after `->`.
    pub fn method(&self) -> Option<NameRef> {
        let arrow = token_start(&self.syntax, &[ARROW])?;
        self.syntax
            .children()
            .filter_map(NameRef::cast)
            .find(|n| n.syntax().text_range().start() > arrow)
    }

    pub fn arg_list(&self) -> Option<ArgList> {
        support::child(&self.syntax, 0)
    }
}

impl FieldExpr {
    pub fn base(&self) -> Option<Expr> {
        support::child(&self.syntax, 0)
    }

    /// The field name, as in `self.origin`.
    pub fn field(&self) -> Option<NameRef> {
        child_after_dot(&self.syntax)
    }

    /// The expression naming the field in acts' `obj.( expr )`.
    pub fn field_expr(&self) -> Option<ParenExpr> {
        child_after_dot(&self.syntax)
    }

    /// Is this `?.`?
    pub fn is_optional(&self) -> bool {
        support::token(&self.syntax, QUESTION_DOT).is_some()
    }
}

impl AnimRef {
    /// The tree in `%tree::anim`.
    pub fn tree(&self) -> Option<Name> {
        support::token(&self.syntax, COLON2)?;
        support::child(&self.syntax, 0)
    }

    pub fn anim(&self) -> Option<Name> {
        let nth = usize::from(support::token(&self.syntax, COLON2).is_some());
        support::child(&self.syntax, nth)
    }
}

impl CastExpr {
    /// `int` or `float`.
    pub fn ty(&self) -> Option<NameRef> {
        support::child(&self.syntax, 0)
    }

    pub fn expr(&self) -> Option<Expr> {
        exprs(&self.syntax).nth(1)
    }
}

impl GotoStmt {
    /// The condition of acts' `jumpiffalse( c ) LOC;`.
    pub fn condition(&self) -> Option<Expr> {
        let all: Vec<Expr> = exprs(&self.syntax).collect();
        (all.len() == 2).then(|| all[0].clone())
    }

    /// The label jumped to.
    pub fn target(&self) -> Option<NameRef> {
        self.syntax.children().filter_map(NameRef::cast).last()
    }
}

impl Name {
    pub fn text(&self) -> String {
        name_text(&self.syntax)
    }
}

impl NameRef {
    pub fn text(&self) -> String {
        name_text(&self.syntax)
    }
}

impl Literal {
    /// The literal's token.
    pub fn token(&self) -> Option<SyntaxToken> {
        first_token(&self.syntax)
    }
}

impl BinExpr {
    /// The operator token.
    pub fn op(&self) -> Option<SyntaxToken> {
        op_after_lhs(&self.syntax, self.lhs())
    }
}

impl AssignExpr {
    /// The operator token (`=`, `+=`...).
    pub fn op(&self) -> Option<SyntaxToken> {
        op_after_lhs(&self.syntax, self.lhs())
    }
}

impl FunctionDef {
    /// The `/@ ... @/` doc comment right before the function, if any.
    pub fn doc_comment(&self) -> Option<SyntaxToken> {
        let mut prev = self.syntax.prev_sibling_or_token();
        while let Some(e) = prev {
            match e {
                rowan::NodeOrToken::Token(t) if t.kind() == DOC_COMMENT => return Some(t),
                rowan::NodeOrToken::Token(t) if t.kind().is_trivia() => {
                    prev = t.prev_sibling_or_token()
                }
                _ => return None,
            }
        }
        None
    }

    pub fn is_autoexec(&self) -> bool {
        self.autoexec_kw_token().is_some()
    }

    pub fn is_private(&self) -> bool {
        self.private_kw_token().is_some()
    }
}

impl SourceFile {
    /// The namespace set by the first `#namespace` directive, if any.
    pub fn namespace(&self) -> Option<Name> {
        self.items().find_map(|i| match i {
            Item::NamespaceDirective(d) => d.name(),
            _ => None,
        })
    }

    /// Every function declared at file scope (dev blocks included).
    pub fn functions(&self) -> impl Iterator<Item = FunctionDef> {
        self.syntax
            .descendants()
            .filter(|n| {
                n.kind() == FUNCTION_DEF
                    && n.parent()
                        .is_some_and(|p| matches!(p.kind(), SOURCE_FILE | DEV_BLOCK_ITEM))
            })
            .filter_map(FunctionDef::cast)
    }
}
