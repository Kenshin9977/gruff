//! Semantic tokens: what the TextMate grammar cannot know. Script functions versus builtins
//! (resolved), namespaces, parameters, locals, fields, macros and hash literals.

use std::collections::{HashMap, HashSet};

use gsc_sema::{DefKind, FileId, How, Target, Workspace};
use gsc_syntax::ast::{self, AstNode};
use gsc_syntax::{SyntaxKind, TextRange};
use lsp_types::{SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokensLegend};

use crate::convert::{Encoding, LineIndex};

/// Token types, in legend order.
pub const TYPES: &[&str] = &[
    "namespace",
    "function",
    "method",
    "parameter",
    "variable",
    "property",
    "macro",
    "class",
    "hashLiteral",
];
/// Token modifiers, in legend order (bit i).
pub const MODIFIERS: &[&str] = &["declaration", "readonly", "defaultLibrary"];

const NAMESPACE: u32 = 0;
const FUNCTION: u32 = 1;
const METHOD: u32 = 2;
const PARAMETER: u32 = 3;
const VARIABLE: u32 = 4;
const PROPERTY: u32 = 5;
const MACRO: u32 = 6;
const CLASS: u32 = 7;
const HASH: u32 = 8;

const DECLARATION: u32 = 1;
const READONLY: u32 = 2;
const DEFAULT_LIBRARY: u32 = 4;

pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TYPES.iter().map(|t| SemanticTokenType::new(t)).collect(),
        token_modifiers: MODIFIERS
            .iter()
            .map(|m| SemanticTokenModifier::new(m))
            .collect(),
    }
}

/// Parameter names of every function, method, constructor and destructor, by range.
fn functions(
    root: &gsc_syntax::SyntaxNode,
    ident: impl Fn(&str) -> String,
) -> Vec<(TextRange, HashSet<String>)> {
    let mut out = Vec::new();
    for n in root.descendants() {
        if !matches!(
            n.kind(),
            SyntaxKind::FUNCTION_DEF | SyntaxKind::CONSTRUCTOR | SyntaxKind::DESTRUCTOR
        ) {
            continue;
        }
        let params = n
            .descendants()
            .filter(|c| c.kind() == SyntaxKind::PARAM)
            .filter_map(ast::Param::cast)
            .filter_map(|p| p.name())
            .map(|name| ident(&name.text()))
            .collect();
        out.push((n.text_range(), params));
    }
    out
}

/// The semantic tokens of a file, encoded relative to each other.
pub fn tokens(ws: &Workspace, file: FileId, enc: Encoding) -> Vec<SemanticToken> {
    let data = ws.file(file);
    let text = data.text();
    let cfg = ws.profile_of(file).config;
    let res = ws.resolution(file);
    let root = data.syntax();
    let idx = LineIndex::new(text);

    // Classified ranges: references, then definitions.
    let mut known: HashMap<TextRange, (u32, u32)> = HashMap::new();
    for (r, x) in res.refs.iter().zip(&data.facts.refs) {
        if matches!(x.target, Target::Dynamic(_)) {
            continue;
        }
        let class = match r.how {
            How::Builtin | How::BuiltinOtherVm => (FUNCTION, DEFAULT_LIBRARY),
            How::Macro => (MACRO, 0),
            How::Method => (METHOD, 0),
            _ => match r.def.map(|d| ws.def(d).kind) {
                Some(DefKind::Method) => (METHOD, 0),
                Some(DefKind::Define) => (MACRO, 0),
                _ if matches!(x.target, Target::Method(_)) => (METHOD, 0),
                _ if matches!(x.target, Target::Keyword(_)) => (FUNCTION, DEFAULT_LIBRARY),
                _ => (FUNCTION, 0),
            },
        };
        known.insert(x.range, class);
    }
    for d in &data.facts.defs {
        if d.name_range == d.range {
            continue;
        }
        let class = match d.kind {
            DefKind::Function => (FUNCTION, DECLARATION),
            DefKind::Method => (METHOD, DECLARATION),
            DefKind::Class => (CLASS, DECLARATION),
            DefKind::Define => (MACRO, DECLARATION),
            DefKind::Const => (VARIABLE, DECLARATION | READONLY),
        };
        known.insert(d.name_range, class);
    }

    let funcs = functions(&root, |s| cfg.ident(s));
    let mut fi = 0;
    let mut out: Vec<(TextRange, u32, u32)> = Vec::new();
    for tok in root
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
    {
        let tr = tok.text_range();
        let class = match tok.kind() {
            SyntaxKind::HASH_STRING | SyntaxKind::HASH_IDENT => Some((HASH, 0)),
            SyntaxKind::IDENT => {
                let parent = tok.parent();
                let pk = parent.as_ref().map(|p| p.kind());
                let grand = parent.as_ref().and_then(|p| p.parent());
                if let Some(&c) = known.get(&tr) {
                    Some(c)
                } else if pk == Some(SyntaxKind::QUALIFIER)
                    || grand
                        .as_ref()
                        .is_some_and(|g| g.kind() == SyntaxKind::NAMESPACE_DIRECTIVE)
                {
                    Some((NAMESPACE, 0))
                } else if grand
                    .as_ref()
                    .and_then(|g| ast::FieldExpr::cast(g.clone()))
                    .and_then(|fe| fe.field())
                    .is_some_and(|f| Some(f.syntax()) == parent.as_ref())
                {
                    Some((PROPERTY, 0))
                } else {
                    while fi < funcs.len() && funcs[fi].0.end() <= tr.start() {
                        fi += 1;
                    }
                    let in_fn = funcs.get(fi).filter(|(r, _)| r.contains_range(tr));
                    let name = cfg.ident(tok.text());
                    let is_macro = ws.macros_spelled(tok.text()).next().is_some();
                    match in_fn {
                        Some((_, params)) if params.contains(&name) => {
                            let decl = pk == Some(SyntaxKind::NAME);
                            Some((PARAMETER, if decl { DECLARATION } else { 0 }))
                        }
                        _ if is_macro => Some((MACRO, 0)),
                        Some(_) if matches!(pk, Some(SyntaxKind::NAME_REF | SyntaxKind::NAME)) => {
                            Some((VARIABLE, 0))
                        }
                        _ => None,
                    }
                }
            }
            _ => None,
        };
        if let Some((t, m)) = class {
            out.push((tr, t, m));
        }
    }

    // Encode: one token per line segment, deltas against the previous token.
    let mut tokens = Vec::with_capacity(out.len());
    let (mut pl, mut pc) = (0u32, 0u32);
    for (r, t, m) in out {
        let s = idx.position(text, r.start(), enc);
        let e = idx.position(text, r.end(), enc);
        if e.line != s.line || e.character <= s.character {
            continue;
        }
        let delta_line = s.line - pl;
        let delta_start = if delta_line == 0 {
            s.character - pc
        } else {
            s.character
        };
        tokens.push(SemanticToken {
            delta_line,
            delta_start,
            length: e.character - s.character,
            token_type: t,
            token_modifiers_bitset: m,
        });
        pl = s.line;
        pc = s.character;
    }
    tokens
}
