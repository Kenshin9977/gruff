//! The rules that walk the tree. Syntax (`E`) and dialect (`D`) diagnostics come from
//! `gsc_syntax`; suppression diagnostics (`S100`) from [`crate::suppress`].

mod builtin_calls;
mod calls;
mod decls;
mod events;
mod exprs;
mod fields;
mod flow;
mod imports;
mod locals;
mod loops;
mod macros;
mod namespaces;
mod values;

use gsc_sema::{Catalogue, FieldCatalogue, FileId, Vm, Workspace};
use gsc_syntax::{SyntaxKind, SyntaxNode, TextRange, TextSize};

pub use gsc_sema::facts::in_dev_block;

use crate::workspace::Summaries;
use crate::{Diagnostic, Fix};

/// What a rule sees of one file.
pub struct Ctx<'a> {
    pub file: FileId,
    pub root: &'a SyntaxNode,
    pub text: &'a str,
    pub ws: &'a Workspace,
    pub sums: &'a Summaries,
    pub catalogue: Option<&'a Catalogue>,
    pub fields: Option<&'a FieldCatalogue>,
    pub vm: Option<Vm>,
    /// Decompiler output: dev blocks are gone (their calls remain), names may be wrong guesses.
    pub decompiled: bool,
    /// The file has E0301 errors.
    pub decompiler_names: bool,
    /// Start offsets of the file's syntax errors, sorted.
    pub syntax_errors: Vec<TextSize>,
    pub enabled: &'a dyn Fn(&str) -> bool,
    pub out: Vec<Diagnostic>,
}

impl Ctx<'_> {
    /// Is there a syntax error on the lines of the expression holding `node`? Rules stay quiet
    /// there: the tree is the parser's guess.
    pub fn near_syntax_error(&self, node: &SyntaxNode) -> bool {
        if self.syntax_errors.is_empty() {
            return false;
        }
        let top = node
            .ancestors()
            .find(|a| a.parent().is_none_or(|p| is_statement_like(p.kind())))
            .unwrap_or_else(|| node.clone());
        let r = top.text_range();
        let (s, e) = (usize::from(r.start()), usize::from(r.end()));
        let line_start = self.text[..s].rfind('\n').map_or(0, |i| i + 1);
        let line_end = self.text[e..].find('\n').map_or(self.text.len(), |i| e + i);
        let lo = TextSize::from(line_start as u32);
        let hi = TextSize::from(line_end as u32);
        let i = self.syntax_errors.partition_point(|&o| o < lo);
        self.syntax_errors.get(i).is_some_and(|&o| o <= hi)
    }

    pub fn report(&mut self, code: &str, message: String, range: TextRange, fix: Option<Fix>) {
        self.out.push(Diagnostic::new(code, message, range, fix));
    }

    pub fn on(&self, code: &str) -> bool {
        (self.enabled)(code)
    }
}

fn is_statement_like(kind: SyntaxKind) -> bool {
    use SyntaxKind::*;
    kind.name().ends_with("_STMT")
        || matches!(
            kind,
            BLOCK
                | SOURCE_FILE
                | SWITCH_CLAUSE
                | ELSE_BRANCH
                | FOR_INIT
                | FOR_CONDITION
                | FOR_STEP
                | FUNCTION_DEF
                | DEV_BLOCK_ITEM
                | ERROR
        )
}

/// Is `node` inside text the parser could not make sense of?
pub fn in_error(node: &SyntaxNode) -> bool {
    node.ancestors().any(|a| a.kind() == SyntaxKind::ERROR)
}

/// Run every enabled tree rule. Files with an E0301 (a decompiler name the parser cannot read)
/// get no builtin or loop rule: their tree is a guess.
pub fn run(ctx: &mut Ctx) {
    if ctx.decompiler_names {
        if ctx.on("F001") {
            fields::check(ctx);
        }
        return;
    }
    if ctx.on("B001") || ctx.on("B002") || ctx.on("B003") {
        builtin_calls::check(ctx);
    }
    if ctx.on("B004") || ctx.on("B005") {
        namespaces::check(ctx);
    }
    if ctx.on("L001") {
        loops::check(ctx);
    }
    if ctx.on("F001") {
        fields::check(ctx);
    }
    if ["T001", "T002", "T003", "T004"].iter().any(|c| ctx.on(c)) {
        events::check(ctx);
    }
    if ctx.on("V001") {
        values::check(ctx);
    }
    run_parity(ctx);
}

/// The rules that follow gscode's checks (I, M, N, U, L002-L005, V002-V005).
fn run_parity(ctx: &mut Ctx) {
    fn any(ctx: &Ctx, codes: &[&str]) -> bool {
        codes.iter().any(|c| ctx.on(c))
    }
    if any(ctx, &["I001", "I002", "I003"]) {
        imports::check(ctx);
    }
    if any(ctx, &["L002", "L003", "L004", "L005"]) {
        flow::check(ctx);
    }
    if any(ctx, &["M001", "M002", "M003"]) {
        macros::check(ctx);
    }
    if any(ctx, &["N001", "N002", "N003", "N004"]) {
        calls::check(ctx);
    }
    if any(ctx, &["N005", "N006", "N007", "N008", "N009"]) {
        decls::check(ctx);
    }
    if any(ctx, &["U001", "U002"]) {
        locals::check(ctx);
    }
    if any(ctx, &["V002", "V003", "V004", "V005"]) {
        exprs::check(ctx);
    }
}
