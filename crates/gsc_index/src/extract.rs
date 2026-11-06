//! What only the index needs from a file: field accesses, read from its syntax tree alone, and
//! the rows of its event sites. Definitions, imports, calls and event sites come from the
//! workspace ([`gsc_sema::facts`], [`gsc_sema::events`]).

use gsc_sema::facts::{focus_of, strip_parens, FileFacts, LineStarts, Scope};
use gsc_sema::Config;
use gsc_syntax::ast::{AstNode, Expr, FieldExpr};
use gsc_syntax::{SyntaxKind, SyntaxNode, TextSize, WalkEvent};

pub use gsc_sema::events::{event_name, EventClass, EventName, WRAPPERS};

#[derive(Clone, Debug)]
pub struct Event {
    pub name: EventName,
    /// `notify`, `waittill`, `waittillmatch`, `endon` or a wrapper name (`waittill_any`...).
    pub op: String,
    pub class: EventClass,
    pub receiver: &'static str,
    pub receiver_text: String,
    pub argc: usize,
    pub func: String,
    pub line: u32,
    /// The source text of the name argument when it is not a plain literal.
    pub expr: String,
    /// Where the site is (the first significant token of the call; the macro use for
    /// `via_macro` sites).
    pub focus: TextSize,
    /// The macro whose expansion holds the site, if any.
    pub via_macro: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Field {
    /// `level`, `self`, `anim`, `game` or `other`.
    pub obj: &'static str,
    pub name: String,
    /// `read`, `write` or `write_index` (`level.a[ i ] = x`).
    pub op: &'static str,
    pub func: String,
    pub line: u32,
    /// Where the access is (its first significant token; the macro use for `via_macro` ones).
    pub focus: TextSize,
    /// The macro whose expansion holds the access, if any.
    pub via_macro: Option<String>,
}

struct Ctx<'a> {
    lines: &'a LineStarts,
    scope: Scope<'a>,
    fields: Vec<Field>,
}

impl Ctx<'_> {
    fn ident(&self, s: &str) -> String {
        self.scope.ident(s)
    }

    fn func_name(&self) -> String {
        self.scope.func_name()
    }

    fn field(&mut self, f: &FieldExpr) {
        let name = match f.field() {
            Some(n) => self.ident(&n.text()),
            None if f.field_expr().is_some() => "<expr>".to_owned(),
            None => return,
        };
        if name.eq_ignore_ascii_case("size") {
            return;
        }
        let obj = match f.base().map(strip_parens) {
            Some(Expr::NameRef(n)) => match n.text().to_ascii_lowercase().as_str() {
                "level" => "level",
                "self" => "self",
                "anim" => "anim",
                "game" => "game",
                _ => "other",
            },
            _ => "other",
        };
        let op = write_kind(f.syntax());
        let focus = focus_of(f.syntax());
        self.fields.push(Field {
            obj,
            name,
            op,
            func: self.func_name(),
            line: self.lines.line(focus),
            focus,
            via_macro: None,
        });
    }
}

/// `write` when the node is assigned or incremented, `write_index` when an index into it is.
fn write_kind(node: &SyntaxNode) -> &'static str {
    let mut cur = node.clone();
    let mut indexed = false;
    loop {
        let Some(p) = cur.parent() else { return "read" };
        match p.kind() {
            SyntaxKind::PAREN_EXPR => {}
            SyntaxKind::INDEX_EXPR => {
                // Only the base of an index, not the index expression itself.
                let base = p.children().next();
                if base.as_ref() != Some(&cur) {
                    return "read";
                }
                indexed = true;
            }
            SyntaxKind::ASSIGN_EXPR => {
                let lhs = p.children().next();
                if lhs.as_ref() == Some(&cur) {
                    return if indexed { "write_index" } else { "write" };
                }
                return "read";
            }
            SyntaxKind::POSTFIX_EXPR | SyntaxKind::PREFIX_EXPR => {
                let incr = p
                    .children_with_tokens()
                    .filter_map(|e| e.into_token())
                    .any(|t| matches!(t.kind(), SyntaxKind::PLUS2 | SyntaxKind::MINUS2));
                return match (incr, indexed) {
                    (true, false) => "write",
                    (true, true) => "write_index",
                    _ => "read",
                };
            }
            _ => return "read",
        }
        cur = p;
    }
}

/// The index rows of the event sites of `facts` (a file's, or a macro expansion's).
pub fn events(facts: &FileFacts, lines: &LineStarts) -> Vec<Event> {
    facts
        .events
        .iter()
        .map(|s| Event {
            name: s.name.clone(),
            op: s.op.clone(),
            class: s.class,
            receiver: s.receiver,
            receiver_text: s.receiver_text.clone(),
            argc: s.argc,
            func: facts.caller_name(s.caller),
            line: lines.line(s.focus),
            expr: s.expr.clone(),
            focus: s.focus,
            via_macro: None,
        })
        .collect()
}

/// The event sites (from `facts`, the file's own) and field accesses of a parsed file whose
/// key is `key`.
pub fn extract(
    root: &SyntaxNode,
    key: &str,
    cfg: &Config,
    lines: &LineStarts,
    facts: &FileFacts,
) -> (Vec<Event>, Vec<Field>) {
    let mut ctx = Ctx {
        lines,
        scope: Scope::new(cfg, key),
        fields: Vec::new(),
    };
    for ev in root.preorder() {
        match ev {
            WalkEvent::Enter(n) => {
                ctx.scope.enter(&n);
                if n.kind() == SyntaxKind::FIELD_EXPR {
                    if let Some(f) = FieldExpr::cast(n) {
                        ctx.field(&f);
                    }
                }
            }
            WalkEvent::Leave(n) => ctx.scope.leave(&n),
        }
    }
    (events(facts, lines), ctx.fields)
}
