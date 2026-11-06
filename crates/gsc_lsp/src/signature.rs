//! Signature help: the parameters of the call around the cursor. Script functions show their
//! parameters as written; builtins show their parameter types when the catalogue records them,
//! and their arity always.

use gsc_sema::{FileId, How, Kind, Vm, Workspace};
use gsc_syntax::ast::{self, AstNode};
use gsc_syntax::{SyntaxKind, SyntaxNode, TextRange, TextSize};
use lsp_types::{
    Documentation, MarkupContent, MarkupKind, ParameterInformation, ParameterLabel, SignatureHelp,
    SignatureInformation,
};

use crate::project::ParamTypes;

/// The innermost argument list whose parentheses enclose `offset`, with its call node.
fn call_at(root: &SyntaxNode, offset: TextSize) -> Option<(SyntaxNode, SyntaxNode)> {
    let tok = root.token_at_offset(offset).left_biased()?;
    for n in tok.parent_ancestors() {
        if n.kind() != SyntaxKind::ARG_LIST {
            continue;
        }
        let open = n
            .children_with_tokens()
            .find(|t| t.kind() == SyntaxKind::L_PAREN)?
            .text_range();
        let close = n
            .children_with_tokens()
            .find(|t| t.kind() == SyntaxKind::R_PAREN)
            .map(|t| t.text_range());
        let inside = open.end() <= offset && close.is_none_or(|c| offset <= c.start());
        if inside {
            return Some((n.parent()?, n));
        }
    }
    None
}

/// The range of the called name.
fn callee_range(call: &SyntaxNode) -> Option<(TextRange, bool)> {
    if let Some(c) = ast::CallExpr::cast(call.clone()) {
        let has_receiver = c.receiver().is_some();
        let r = match c.callee()? {
            ast::Expr::NameRef(n) => n.syntax().text_range(),
            ast::Expr::QualifiedRef(q) => q.name_ref()?.syntax().text_range(),
            _ => return None,
        };
        return Some((r, has_receiver));
    }
    if let Some(m) = ast::MethodCallExpr::cast(call.clone()) {
        return Some((m.method()?.syntax().text_range(), true));
    }
    None
}

fn split_params(p: &str) -> Vec<String> {
    if p.trim().is_empty() {
        return Vec::new();
    }
    p.split(',').map(|s| s.trim().to_owned()).collect()
}

fn build(name: &str, params: &[String], doc: Option<String>) -> SignatureInformation {
    let mut label = format!("{name}(");
    let mut infos = Vec::new();
    if !params.is_empty() {
        label.push(' ');
    }
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            label.push_str(", ");
        }
        let start = label.encode_utf16().count() as u32;
        label.push_str(p);
        let end = label.encode_utf16().count() as u32;
        infos.push(ParameterInformation {
            label: ParameterLabel::LabelOffsets([start, end]),
            documentation: None,
        });
    }
    if !params.is_empty() {
        label.push(' ');
    }
    label.push(')');
    SignatureInformation {
        label,
        documentation: doc.map(|d| {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: d,
            })
        }),
        parameters: Some(infos),
        active_parameter: None,
    }
}

pub fn signature_help(
    ws: &Workspace,
    params: &ParamTypes,
    file: FileId,
    offset: TextSize,
) -> Option<SignatureHelp> {
    let data = ws.file(file);
    let root = data.syntax();
    let (call, args) = call_at(&root, offset)?;
    let (name_range, has_receiver) = callee_range(&call)?;
    let active = args
        .children_with_tokens()
        .filter(|t| t.kind() == SyntaxKind::COMMA && t.text_range().end() <= offset)
        .count() as u32;
    let i = data.facts.refs.iter().position(|r| r.range == name_range)?;
    let r = &ws.resolution(file).refs[i];
    let mut sigs = Vec::new();
    let mut active_sig = 0;
    match (r.def, r.how) {
        (Some(d), _) => {
            let def = ws.def(d);
            let doc = (!def.doc.is_empty() && def.kind.is_callable()).then(|| def.doc.clone());
            sigs.push(build(
                &format!("{}::{}", def.ns, def.short_name()),
                &split_params(&def.params),
                doc,
            ));
        }
        (None, How::Builtin | How::BuiltinOtherVm) => {
            let cat = ws.profile_of(file).builtins.as_ref()?;
            let name = r.callee.to_ascii_lowercase();
            let vm = data.meta.vm.unwrap_or(Vm::Gsc);
            let mut entries: Vec<_> = cat.lookup(&name).iter().collect();
            entries.sort_by_key(|b| (b.vm != vm, (b.kind == Kind::Method) != has_receiver));
            for b in entries {
                let vm_s = if b.vm == Vm::Csc { "csc" } else { "gsc" };
                let types = params.get(&name, vm_s, b.kind.label());
                let list: Vec<String> = match (types, b.arity) {
                    (Some(t), _) if !t.is_empty() => t
                        .iter()
                        .enumerate()
                        .map(|(i, ty)| {
                            let optional = b.arity.is_some_and(|(lo, _)| i as u32 >= lo);
                            if optional {
                                format!("[{ty}]")
                            } else {
                                ty.clone()
                            }
                        })
                        .collect(),
                    (_, Some((lo, hi))) => {
                        let shown = hi.min(lo.max(1) + 3).min(8);
                        let mut v: Vec<String> = (0..shown)
                            .map(|i| {
                                if i < lo {
                                    format!("arg{}", i + 1)
                                } else {
                                    format!("[arg{}]", i + 1)
                                }
                            })
                            .collect();
                        if hi > shown {
                            v.push("...".to_owned());
                        }
                        v
                    }
                    _ => Vec::new(),
                };
                let mut doc = format!("builtin {} {} ({})", b.vm.label(), b.kind.label(), cat.game);
                if let Some(a) = b.describe_arity() {
                    doc.push_str(&format!(": takes {a}"));
                }
                if b.dev_only {
                    doc.push_str(" (dev only)");
                }
                if list
                    .iter()
                    .any(|t| t.trim_start_matches('[').starts_with('~'))
                {
                    doc.push_str("; ~type: measured on the game's own scripts");
                }
                sigs.push(build(&name, &list, Some(doc)));
            }
            if sigs.is_empty() {
                return None;
            }
            active_sig = 0;
        }
        _ => return None,
    }
    for s in &mut sigs {
        let n = s.parameters.as_ref().map_or(0, Vec::len) as u32;
        s.active_parameter = Some(active.min(n.saturating_sub(1)));
    }
    Some(SignatureHelp {
        signatures: sigs,
        active_signature: Some(active_sig),
        active_parameter: Some(active),
    })
}
