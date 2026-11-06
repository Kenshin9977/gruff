//! L002 unreachable-code, L003 duplicate-case-label, L004 assignment-as-value, L005
//! non-constant-case-label.
//!
//! The semantics follow gscode's `UnreachableCodeLint`, `CaseLabelLint` and the parser's
//! `AssignmentUsedAsCondition`; what the BO3 compiler refuses was measured with its linker
//! (`tools/linker/probes/lint_*.gsc`) (Blakintosh/gscode, GPL-3.0), re-expressed over this tree.

use std::collections::hash_map::Entry;
use std::collections::HashMap;

use gsc_syntax::ast::{self, AstNode};
use gsc_syntax::{NodeOrToken, SyntaxKind, SyntaxKind::*, SyntaxNode, TextRange};

use super::{in_error, Ctx};

pub fn check(ctx: &mut Ctx) {
    let (l002, l003, l004, l005) = (
        ctx.on("L002"),
        ctx.on("L003"),
        ctx.on("L004"),
        ctx.on("L005"),
    );
    let nodes: Vec<SyntaxNode> = ctx.root.descendants().collect();
    for n in nodes {
        match n.kind() {
            // Decompiler output keeps the compiler's own dead jumps (`return; break;`).
            BLOCK | SWITCH_CLAUSE if l002 && !ctx.decompiled => unreachable(ctx, &n),
            SWITCH_STMT => {
                // Decompiler output can repeat a label the compiled jump table repeats.
                if l003 && !ctx.decompiled {
                    case_labels(ctx, &n);
                }
                if l005 {
                    case_constants(ctx, &n);
                }
            }
            ASSIGN_EXPR if l004 => assignment_value(ctx, &n),
            _ => {}
        }
    }
}

// ===== L002 =====================================================================================

/// The jump a statement always takes: `return`, `break` or `continue`.
fn terminator(n: &SyntaxNode) -> Option<&'static str> {
    match n.kind() {
        RETURN_STMT => Some("return"),
        BREAK_STMT => Some("break"),
        CONTINUE_STMT => Some("continue"),
        _ => None,
    }
}

/// Statements that run nothing, so they are neither dead code nor a way back in.
fn is_inert(kind: SyntaxKind) -> bool {
    matches!(kind, EMPTY_STMT | PREPROC_DEFINE | PREPROC_UNDEF)
}

fn unreachable(ctx: &mut Ctx, block: &SyntaxNode) {
    // A dev block only runs with developer script on: code after a `return` inside one is a
    // debugging aid left there knowingly.
    if block.ancestors().any(|a| a.kind() == DEV_BLOCK_STMT) || in_error(block) {
        return;
    }
    let mut after: Option<(&'static str, Vec<SyntaxNode>)> = None;
    for el in block.children_with_tokens() {
        match el {
            NodeOrToken::Token(t) => {
                // `#if`/`#else` between the jump and what follows: another configuration may
                // compile the jump out.
                if after.is_some()
                    && matches!(
                        t.kind(),
                        PP_IF | PP_IFDEF | PP_IFNDEF | PP_ELIF | PP_ELSE | PP_ENDIF | DISABLED_TEXT
                    )
                {
                    return;
                }
            }
            NodeOrToken::Node(n) => {
                if let Some((_, dead)) = &mut after {
                    // A label is a way back in (decompiler output only, but be safe).
                    if n.kind() == LABEL_STMT {
                        return;
                    }
                    if !is_inert(n.kind()) && n.kind().name().ends_with("_STMT")
                        || n.kind() == BLOCK
                    {
                        dead.push(n);
                    }
                    continue;
                }
                if let Some(word) = terminator(&n) {
                    after = Some((word, Vec::new()));
                }
            }
        }
    }
    let Some((word, dead)) = after else { return };
    // `return x; break;`: a habit, not a mistake.
    if dead.len() == 1 && dead[0].kind() == BREAK_STMT {
        return;
    }
    let (Some(first), Some(last)) = (dead.first(), dead.last()) else {
        return;
    };
    if ctx.near_syntax_error(first) {
        return;
    }
    let range = TextRange::new(first.text_range().start(), last.text_range().end());
    ctx.report(
        "L002",
        format!("unreachable code: the `{word}` before it always leaves this block"),
        range,
        None,
    );
}

// ===== L003 =====================================================================================

/// The value of a constant label, for comparison: integers by value, strings by their exact
/// text (a switch compares strings by value, so `"A"` and `"a"` differ). `None` for anything
/// else, which is never compared.
fn label_key(e: &ast::Expr) -> Option<String> {
    match gsc_sema::facts::strip_parens(e.clone()) {
        ast::Expr::Literal(l) => {
            let t = l.token()?;
            match t.kind() {
                INT_NUMBER => parse_int(t.text()).map(|v| format!("int:{v}")),
                STRING | ISTRING | HASH_STRING | PREFIXED_STRING => {
                    Some(format!("str:{}", t.text()))
                }
                TRUE_KW => Some("int:1".to_owned()),
                FALSE_KW => Some("int:0".to_owned()),
                _ => None,
            }
        }
        ast::Expr::PrefixExpr(p) if p.minus_token().is_some() => {
            let inner = p.expr()?;
            let ast::Expr::Literal(l) = gsc_sema::facts::strip_parens(inner) else {
                return None;
            };
            let t = l.token()?;
            (t.kind() == INT_NUMBER)
                .then(|| parse_int(t.text()))
                .flatten()
                .map(|v| format!("int:{}", -v))
        }
        _ => None,
    }
}

fn parse_int(s: &str) -> Option<i128> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return i128::from_str_radix(h, 16).ok();
    }
    s.parse().ok()
}

fn case_labels(ctx: &mut Ctx, switch: &SyntaxNode) {
    let Some(sw) = ast::SwitchStmt::cast(switch.clone()) else {
        return;
    };
    if in_error(switch) {
        return;
    }
    let mut seen: HashMap<String, TextRange> = HashMap::new();
    let mut default_seen = false;
    let mut found = Vec::new();
    for clause in sw.switch_clauses() {
        if let Some(kw) = clause.default_kw_token() {
            if default_seen {
                found.push((
                    kw.text_range(),
                    "this switch already has a `default` label; only the first is ever reached"
                        .to_owned(),
                ));
            }
            default_seen = true;
            continue;
        }
        let Some(value) = clause.value() else {
            continue;
        };
        let Some(key) = label_key(&value) else {
            continue;
        };
        let range = value.syntax().text_range();
        match seen.entry(key) {
            Entry::Occupied(_) => {
                let text = value.syntax().text().to_string();
                found.push((
                    range,
                    format!(
                        "`case {}` is already a label of this switch; only the first one can match",
                        text.trim()
                    ),
                ));
            }
            Entry::Vacant(v) => {
                v.insert(range);
            }
        }
    }
    for (range, message) in found {
        let node = ctx.root.covering_element(range);
        let near = match node {
            NodeOrToken::Node(n) => ctx.near_syntax_error(&n),
            NodeOrToken::Token(t) => t.parent().is_some_and(|p| ctx.near_syntax_error(&p)),
        };
        if !near {
            ctx.report("L003", message, range, None);
        }
    }
}

// ===== L004 =====================================================================================

/// Is this assignment a statement of its own (`x = 1;`, a `for` initialiser or step), the only
/// places GSC allows one? Parentheses around it change nothing: the BO3 compiler refuses
/// `if ( ( x = 1 ) )` as it refuses `if ( x = 1 )`.
fn is_statement_assignment(a: &SyntaxNode) -> bool {
    let mut cur = a.clone();
    while let Some(p) = cur.parent() {
        match p.kind() {
            PAREN_EXPR => cur = p,
            EXPR_STMT | MACRO_STMT | FOR_INIT | FOR_STEP => return true,
            _ => return false,
        }
    }
    false
}

fn assignment_value(ctx: &mut Ctx, n: &SyntaxNode) {
    let Some(a) = ast::AssignExpr::cast(n.clone()) else {
        return;
    };
    if is_statement_assignment(n) || in_error(n) || ctx.near_syntax_error(n) {
        return;
    }
    let lhs = a
        .lhs()
        .map(|l| l.syntax().text().to_string())
        .unwrap_or_default();
    let in_condition = n.ancestors().skip(1).any(|p| {
        matches!(
            p.kind(),
            IF_STMT | WHILE_STMT | DO_WHILE_STMT | FOR_CONDITION
        )
    });
    let message = if in_condition && a.eq_token().is_some() {
        format!(
            "this assigns to `{}` inside the condition; `==` compares (an assignment is a \
             statement in GSC, not a value)",
            lhs.trim()
        )
    } else {
        format!(
            "assignment to `{}` used as a value; an assignment is a statement in GSC: move it \
             to its own line",
            lhs.trim()
        )
    };
    ctx.report("L004", message, n.text_range(), None);
}

// ===== L005 =====================================================================================

/// Does this label name something the preprocessor replaces (`case MY_CONSTANT:`)?
fn is_macro_label(ctx: &Ctx, e: &ast::Expr) -> bool {
    let ast::Expr::NameRef(n) = gsc_sema::facts::strip_parens(e.clone()) else {
        return false;
    };
    let name = n.text();
    crate::workspace::looks_like_macro(&name) || ctx.ws.macros_spelled(&name).next().is_some()
}

/// A label the compiler cannot take: `undefined`, a variable, a call, a field.
fn non_constant_label(e: &ast::Expr) -> Option<&'static str> {
    match gsc_sema::facts::strip_parens(e.clone()) {
        ast::Expr::Literal(l) => match l.token()?.kind() {
            UNDEFINED_KW => Some("undefined"),
            _ => None,
        },
        ast::Expr::NameRef(_) => Some("variable"),
        ast::Expr::CallExpr(_) | ast::Expr::PointerCallExpr(_) | ast::Expr::MethodCallExpr(_) => {
            Some("call")
        }
        ast::Expr::FieldExpr(_) | ast::Expr::IndexExpr(_) => Some("field"),
        _ => None,
    }
}

fn case_constants(ctx: &mut Ctx, switch: &SyntaxNode) {
    let Some(sw) = ast::SwitchStmt::cast(switch.clone()) else {
        return;
    };
    if in_error(switch) {
        return;
    }
    let mut found = Vec::new();
    for clause in sw.switch_clauses() {
        let Some(value) = clause.value() else {
            continue;
        };
        let Some(what) = non_constant_label(&value) else {
            continue;
        };
        if is_macro_label(ctx, &value) || ctx.near_syntax_error(value.syntax()) {
            continue;
        }
        let message = if what == "undefined" {
            "`case undefined` never matches and is not a constant: test `isdefined()` before \
             the switch"
                .to_owned()
        } else {
            format!(
                "a case label must be a constant string or integer, not a {what}: \
                 `case {}`",
                value.syntax().text().to_string().trim()
            )
        };
        found.push((value.syntax().text_range(), message));
    }
    for (range, message) in found {
        ctx.report("L005", message, range, None);
    }
}
