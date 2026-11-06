//! Inlay hints: parameter names before the arguments of script function calls and of macro
//! invocations, and the type of a local at its first assignment when the value says it
//! (literals, `new C()`, function references, builtins whose result type is known).
//!
//! As in GSCode (Blakintosh/gscode, GPL-3.0): an argument that already spells the parameter's
//! name is left unlabelled, and a type is shown only when it is certain.

use std::collections::HashSet;

use gsc_sema::{DefKind, FileId, How};
use gsc_syntax::ast::{self, AstNode};
use gsc_syntax::{SyntaxKind, SyntaxNode, TextRange};
use lsp_types::*;

use crate::handlers::Ctx;

/// The name a parameter is shown as: `&x` and `x = 1` read `x`; `...` has none.
fn param_label(p: &str) -> Option<&str> {
    let p = p.trim().trim_start_matches('&');
    let p = p.split('=').next().unwrap_or(p).trim();
    let ok =
        !p.is_empty() && p != "..." && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    ok.then_some(p)
}

/// Does the argument already spell the parameter's name (`foo( count )`, `foo( self.count )`)?
fn spells(arg: &ast::Expr, name: &str) -> bool {
    match crate_strip(arg) {
        ast::Expr::NameRef(r) => r.text().eq_ignore_ascii_case(name),
        ast::Expr::FieldExpr(f) => f
            .field()
            .is_some_and(|n| n.text().eq_ignore_ascii_case(name)),
        _ => false,
    }
}

fn crate_strip(e: &ast::Expr) -> ast::Expr {
    gsc_sema::facts::strip_parens(e.clone())
}

/// The called name of a call node and its argument list.
fn call_parts(n: &SyntaxNode) -> Option<(TextRange, ast::ArgList)> {
    if let Some(c) = ast::CallExpr::cast(n.clone()) {
        let r = match c.callee()? {
            ast::Expr::NameRef(x) => x.syntax().text_range(),
            ast::Expr::QualifiedRef(q) => q.name_ref()?.syntax().text_range(),
            _ => return None,
        };
        return Some((r, c.arg_list()?));
    }
    if let Some(m) = ast::MethodCallExpr::cast(n.clone()) {
        return Some((m.method()?.syntax().text_range(), m.arg_list()?));
    }
    None
}

impl Ctx<'_> {
    /// The type a value certainly has, as shown in a hint.
    fn value_type(&self, f: FileId, e: &ast::Expr) -> Option<String> {
        let ws = &self.a.ws;
        let e = crate_strip(e);
        let ty = match &e {
            ast::Expr::Literal(l) => {
                let t = l.token()?;
                match t.kind() {
                    SyntaxKind::INT_NUMBER => "int",
                    SyntaxKind::FLOAT_NUMBER => "float",
                    SyntaxKind::STRING => "string",
                    SyntaxKind::ISTRING => "istring",
                    SyntaxKind::HASH_STRING => "hash",
                    SyntaxKind::TRUE_KW | SyntaxKind::FALSE_KW => "bool",
                    _ => return None,
                }
            }
            ast::Expr::VectorExpr(_) => "vector",
            ast::Expr::ArrayExpr(_) => "array",
            ast::Expr::StructExpr(_) => "struct",
            ast::Expr::FunctionRef(_) => "function",
            ast::Expr::NewExpr(n) => {
                let name = n.name_ref()?.text();
                ws.class_named(&ws.profile_of(f).config.ident(&name))?;
                return Some(name);
            }
            ast::Expr::CallExpr(c) => {
                let range = match c.callee()? {
                    ast::Expr::NameRef(x) => x.syntax().text_range(),
                    _ => return None,
                };
                let facts = &ws.file(f).facts;
                let i = facts.refs.iter().position(|r| r.range == range)?;
                let r = &ws.resolution(f).refs[i];
                if !matches!(r.how, How::Builtin) {
                    return None;
                }
                let game = &ws.profile_of(f).dialect.id;
                let t = gsc_sema::types::builtin_return(
                    &r.callee.to_ascii_lowercase(),
                    gsc_sema::types::weapon_objects(game),
                )?;
                match t.label() {
                    "unknown" | "undefined" => return None,
                    l => return Some(l.to_owned()),
                }
            }
            _ => return None,
        };
        Some(ty.to_owned())
    }

    pub fn inlay_hints(&self, p: InlayHintParams) -> Option<Vec<InlayHint>> {
        let feats = self.features;
        if !(feats.param_hints || feats.type_hints || feats.macro_hints) {
            return Some(Vec::new());
        }
        let f = self.file(&p.text_document.uri)?;
        let ws = &self.a.ws;
        let data = ws.file(f);
        let text = data.text();
        let idx = self.lines(f);
        let want = idx.text_range(text, p.range, self.enc);
        let root = data.syntax();
        let facts = &data.facts;
        let res = ws.resolution(f);
        let cfg = ws.profile_of(f).config;
        let mut out = Vec::new();
        let mut seen: HashSet<(TextRange, String)> = HashSet::new();
        let mut typed: HashSet<(TextRange, String)> = HashSet::new();
        let hint = |at: TextRange, label: String, kind: InlayHintKind, out: &mut Vec<InlayHint>| {
            out.push(InlayHint {
                position: self.range(f, at).start,
                label: InlayHintLabel::String(label),
                kind: Some(kind),
                text_edits: None,
                tooltip: None,
                padding_left: Some(kind == InlayHintKind::TYPE),
                padding_right: Some(kind == InlayHintKind::PARAMETER),
                data: None,
            });
        };
        for n in root.descendants() {
            match n.kind() {
                SyntaxKind::CALL_EXPR | SyntaxKind::METHOD_CALL_EXPR
                    if n.text_range().intersect(want).is_some() =>
                {
                    let Some((name, args)) = call_parts(&n) else {
                        continue;
                    };
                    let Some(i) = facts.refs.iter().position(|r| r.range == name) else {
                        continue;
                    };
                    let Some(d) = res.refs[i].def else { continue };
                    let def = ws.def(d);
                    let on = match def.kind {
                        DefKind::Function | DefKind::Method => feats.param_hints,
                        DefKind::Define => feats.macro_hints,
                        _ => false,
                    };
                    if !on || def.params.trim().is_empty() {
                        continue;
                    }
                    let params: Vec<&str> = def.params.split(',').collect();
                    for (arg, param) in args.exprs().zip(params) {
                        let Some(label) = param_label(param) else {
                            break;
                        };
                        if spells(&arg, label) {
                            continue;
                        }
                        let at = TextRange::empty(arg.syntax().text_range().start());
                        if want.contains_inclusive(at.start())
                            && seen.insert((at, label.to_owned()))
                        {
                            hint(at, format!("{label}:"), InlayHintKind::PARAMETER, &mut out);
                        }
                    }
                }
                SyntaxKind::ASSIGN_EXPR if feats.type_hints => {
                    let Some(a) = ast::AssignExpr::cast(n.clone()) else {
                        continue;
                    };
                    let Some(ast::Expr::NameRef(lhs)) = a.lhs() else {
                        continue;
                    };
                    if a.op().is_none_or(|t| t.text() != "=") {
                        continue;
                    }
                    let Some(func) = n.ancestors().find(|x| {
                        matches!(
                            x.kind(),
                            SyntaxKind::FUNCTION_DEF
                                | SyntaxKind::CONSTRUCTOR
                                | SyntaxKind::DESTRUCTOR
                        )
                    }) else {
                        continue;
                    };
                    // The first assignment of the name in its function only.
                    if !typed.insert((func.text_range(), cfg.ident(&lhs.text()))) {
                        continue;
                    }
                    let Some(rhs) = a.rhs() else { continue };
                    if let Some(ty) = self.value_type(f, &rhs) {
                        let at = TextRange::empty(lhs.syntax().text_range().end());
                        if want.contains_inclusive(at.start()) {
                            hint(at, format!(": {ty}"), InlayHintKind::TYPE, &mut out);
                        }
                    }
                }
                _ => {}
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::param_label;

    #[test]
    fn parameter_labels() {
        assert_eq!(param_label(" &x "), Some("x"));
        assert_eq!(param_label("n = 1"), Some("n"));
        assert_eq!(param_label("..."), None);
        assert_eq!(param_label(""), None);
    }
}
