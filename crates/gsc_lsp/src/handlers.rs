//! Request handlers: LSP params in, core queries, LSP results out. Every handler reads the
//! workspace; none changes it (edits come back to the client as `WorkspaceEdit`s).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gsc_sema::{Callee, DefKind, FileId, NavTarget, Symbol, SymbolKind as Sk};
use gsc_syntax::{TextRange, TextSize};
use lsp_types::*;
use serde_json::Value;

use crate::analysis::Analysis;
use crate::convert::{uri_to_path, Encoding, LineIndex};
use crate::diagnostics::{text_hash, to_lsp, Shared};
use crate::project::Project;

/// An error the client shows: the request is refused.
pub struct Refused(pub String);

pub type Result<T> = std::result::Result<T, Refused>;

/// What every handler sees.
pub struct Ctx<'a> {
    pub a: &'a Analysis,
    pub project: &'a Project,
    pub enc: Encoding,
    pub lint: &'a Shared,
    /// Line indexes built during this request.
    pub lines: RefCell<HashMap<FileId, Rc<LineIndex>>>,
}

fn symbol_kind(k: Sk) -> SymbolKind {
    match k {
        Sk::Function => SymbolKind::FUNCTION,
        Sk::Method => SymbolKind::METHOD,
        Sk::Class => SymbolKind::CLASS,
        Sk::Macro => SymbolKind::CONSTANT,
        Sk::Const => SymbolKind::CONSTANT,
        Sk::Local | Sk::Param => SymbolKind::VARIABLE,
        Sk::Builtin => SymbolKind::FUNCTION,
        Sk::Field => SymbolKind::FIELD,
        Sk::File => SymbolKind::FILE,
        Sk::Namespace => SymbolKind::NAMESPACE,
        Sk::Event => SymbolKind::EVENT,
    }
}

fn completion_kind(k: Sk) -> CompletionItemKind {
    match k {
        Sk::Function | Sk::Builtin => CompletionItemKind::FUNCTION,
        Sk::Method => CompletionItemKind::METHOD,
        Sk::Class => CompletionItemKind::CLASS,
        Sk::Macro | Sk::Const => CompletionItemKind::CONSTANT,
        Sk::Local | Sk::Param => CompletionItemKind::VARIABLE,
        Sk::Field => CompletionItemKind::FIELD,
        Sk::File => CompletionItemKind::FILE,
        Sk::Namespace => CompletionItemKind::MODULE,
        Sk::Event => CompletionItemKind::EVENT,
    }
}

impl Ctx<'_> {
    pub fn file(&self, uri: &Uri) -> Option<FileId> {
        self.a.file_id(&uri_to_path(uri)?)
    }

    fn lines(&self, f: FileId) -> Rc<LineIndex> {
        self.lines
            .borrow_mut()
            .entry(f)
            .or_insert_with(|| Rc::new(LineIndex::new(self.a.ws.file(f).text())))
            .clone()
    }

    fn offset(&self, f: FileId, pos: Position) -> TextSize {
        self.lines(f)
            .offset(self.a.ws.file(f).text(), pos, self.enc)
    }

    fn at(&self, p: &TextDocumentPositionParams) -> Option<(FileId, TextSize)> {
        let f = self.file(&p.text_document.uri)?;
        Some((f, self.offset(f, p.position)))
    }

    fn range(&self, f: FileId, r: TextRange) -> Range {
        self.lines(f).range(self.a.ws.file(f).text(), r, self.enc)
    }

    fn location(&self, f: FileId, r: TextRange) -> Location {
        Location {
            uri: self.a.uri(f),
            range: self.range(f, r),
        }
    }

    fn nav_location(&self, n: &NavTarget) -> Location {
        self.location(n.file, n.focus_range)
    }

    pub fn definition(&self, p: GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
        let (f, off) = self.at(&p.text_document_position_params)?;
        let targets = self.a.ws.definition(f, off);
        if targets.is_empty() {
            return None;
        }
        let links: Vec<LocationLink> = targets
            .iter()
            .map(|n| LocationLink {
                origin_selection_range: None,
                target_uri: self.a.uri(n.file),
                target_range: self.range(n.file, n.full_range),
                target_selection_range: self.range(n.file, n.focus_range),
            })
            .collect();
        Some(GotoDefinitionResponse::Link(links))
    }

    pub fn references(&self, p: ReferenceParams) -> Option<Vec<Location>> {
        let (f, off) = self.at(&p.text_document_position)?;
        let sym = self.a.ws.symbol_at(f, off)?;
        let refs = self.a.ws.references(&sym, p.context.include_declaration);
        Some(
            refs.iter()
                .map(|r| self.location(r.file, r.range))
                .collect(),
        )
    }

    pub fn hover(&self, p: HoverParams) -> Option<lsp_types::Hover> {
        let (f, off) = self.at(&p.text_document_position_params)?;
        let h = self.a.ws.hover(f, off)?;
        Some(lsp_types::Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: h.markdown(),
            }),
            range: Some(self.range(f, h.range)),
        })
    }

    pub fn document_symbols(&self, p: DocumentSymbolParams) -> Option<DocumentSymbolResponse> {
        let f = self.file(&p.text_document.uri)?;
        fn conv(c: &Ctx, f: FileId, s: &gsc_sema::DocumentSymbol) -> lsp_types::DocumentSymbol {
            #[allow(deprecated)]
            lsp_types::DocumentSymbol {
                name: s.name.clone(),
                detail: (!s.detail.is_empty()).then(|| s.detail.clone()),
                kind: symbol_kind(s.kind),
                tags: None,
                deprecated: None,
                range: c.range(f, s.range),
                selection_range: c.range(f, s.focus_range),
                children: (!s.children.is_empty())
                    .then(|| s.children.iter().map(|x| conv(c, f, x)).collect()),
            }
        }
        let syms = self.a.ws.document_symbols(f);
        Some(DocumentSymbolResponse::Nested(
            syms.iter().map(|s| conv(self, f, s)).collect(),
        ))
    }

    pub fn workspace_symbols(&self, p: WorkspaceSymbolParams) -> WorkspaceSymbolResponse {
        let hits = self.a.ws.workspace_symbols(&p.query, 256);
        WorkspaceSymbolResponse::Nested(
            hits.iter()
                .map(|n| WorkspaceSymbol {
                    name: n.name.clone(),
                    kind: symbol_kind(n.kind),
                    tags: None,
                    container_name: n.container.clone(),
                    location: OneOf::Left(self.nav_location(n)),
                    data: None,
                })
                .collect(),
        )
    }

    pub fn completion(&self, p: CompletionParams) -> Option<CompletionResponse> {
        let (f, off) = self.at(&p.text_document_position)?;
        let text = self.a.ws.file(f).text();
        // `:` and `>` trigger only as `::` and `->`.
        if let Some(ctx) = &p.context {
            if ctx.trigger_kind == CompletionTriggerKind::TRIGGER_CHARACTER {
                let before = &text[..usize::from(off).min(text.len())];
                let ok = match ctx.trigger_character.as_deref() {
                    Some(":") => before.ends_with("::"),
                    Some(">") => before.ends_with("->"),
                    _ => true,
                };
                if !ok {
                    return None;
                }
            }
        }
        let items = self.a.ws.completions(f, off);
        let out: Vec<CompletionItem> = items
            .into_iter()
            .enumerate()
            .map(|(i, c)| CompletionItem {
                label: c.label,
                kind: Some(completion_kind(c.kind)),
                detail: c.detail,
                // Keep the core's order (locals, then scope, then builtins).
                sort_text: Some(format!("{i:05}")),
                ..Default::default()
            })
            .collect();
        Some(CompletionResponse::List(CompletionList {
            is_incomplete: false,
            items: out,
        }))
    }

    pub fn signature_help(&self, p: SignatureHelpParams) -> Option<SignatureHelp> {
        let (f, off) = self.at(&p.text_document_position_params)?;
        crate::signature::signature_help(&self.a.ws, &self.project.params, f, off)
    }

    fn hierarchy_item(&self, d: gsc_sema::DefId) -> CallHierarchyItem {
        let n = self.a.ws.call_hierarchy_item(d);
        let def = self.a.ws.def(d);
        CallHierarchyItem {
            name: def.short_name().to_owned(),
            kind: symbol_kind(n.kind),
            tags: None,
            detail: Some(format!("{}::{}( {} )", def.ns, def.name, def.params)),
            uri: self.a.uri(n.file),
            range: self.range(n.file, n.full_range),
            selection_range: self.range(n.file, n.focus_range),
            data: Some(serde_json::json!({
                "file": self.a.ws.file(d.file).meta.path,
                "index": d.index,
                "name": def.name,
            })),
        }
    }

    /// The definition a call hierarchy item names: by index when it still has that name, else
    /// by name in the same file.
    fn item_def(&self, item: &CallHierarchyItem) -> Option<gsc_sema::DefId> {
        let data = item.data.as_ref()?;
        let path: std::path::PathBuf = serde_json::from_value(data.get("file")?.clone()).ok()?;
        let index = data.get("index")?.as_u64()? as u32;
        let name = data.get("name")?.as_str()?;
        let f = self.a.file_id(&path)?;
        let defs = &self.a.ws.file(f).facts.defs;
        let i = match defs.get(index as usize) {
            Some(d) if d.name == name => index,
            _ => defs.iter().position(|d| d.name == name)? as u32,
        };
        Some(gsc_sema::DefId { file: f, index: i })
    }

    pub fn prepare_call_hierarchy(
        &self,
        p: CallHierarchyPrepareParams,
    ) -> Option<Vec<CallHierarchyItem>> {
        let (f, off) = self.at(&p.text_document_position_params)?;
        let d = self.a.ws.call_hierarchy_target(f, off)?;
        Some(vec![self.hierarchy_item(d)])
    }

    pub fn incoming_calls(
        &self,
        p: CallHierarchyIncomingCallsParams,
    ) -> Option<Vec<CallHierarchyIncomingCall>> {
        let d = self.item_def(&p.item)?;
        let calls = self.a.ws.incoming_calls(d);
        Some(
            calls
                .iter()
                .map(|c| {
                    let from = match c.from {
                        Some(x) => self.hierarchy_item(x),
                        None => {
                            // Calls at file scope: the file itself.
                            let r = TextRange::empty(TextSize::from(0));
                            CallHierarchyItem {
                                name: self.a.ws.file(c.file).meta.rel.clone(),
                                kind: SymbolKind::FILE,
                                tags: None,
                                detail: None,
                                uri: self.a.uri(c.file),
                                range: self.range(c.file, r),
                                selection_range: self.range(c.file, r),
                                data: None,
                            }
                        }
                    };
                    CallHierarchyIncomingCall {
                        from,
                        from_ranges: c.ranges.iter().map(|r| self.range(c.file, *r)).collect(),
                    }
                })
                .collect(),
        )
    }

    pub fn outgoing_calls(
        &self,
        p: CallHierarchyOutgoingCallsParams,
    ) -> Option<Vec<CallHierarchyOutgoingCall>> {
        let d = self.item_def(&p.item)?;
        let calls = self.a.ws.outgoing_calls(d);
        Some(
            calls
                .iter()
                .filter_map(|c| match &c.to {
                    Callee::Def(t) => Some(CallHierarchyOutgoingCall {
                        to: self.hierarchy_item(*t),
                        from_ranges: c.ranges.iter().map(|r| self.range(d.file, *r)).collect(),
                    }),
                    Callee::Builtin(_) | Callee::Other(_) => None,
                })
                .collect(),
        )
    }

    pub fn formatting(&self, p: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let Some(f) = self.file(&p.text_document.uri) else {
            return Ok(None);
        };
        let data = self.a.ws.file(f);
        let text = data.text();
        let dialect = self.a.ws.profile_of(f).dialect(data.decompiled);
        match gsc_fmt::format(text, dialect, &self.project.format) {
            Ok(out) if out == text => Ok(Some(Vec::new())),
            Ok(out) => Ok(Some(vec![minimal_edit(text, &out, self.enc)])),
            Err(gsc_fmt::FormatError::Syntax(errors)) => Err(Refused(format!(
                "not formatted: {} syntax error(s), first: {} {}",
                errors.len(),
                errors[0].code,
                errors[0].message
            ))),
            Err(e) => Err(Refused(format!("not formatted: {e}"))),
        }
    }

    /// Format the statements that touch the range (`gsc_fmt::format_range`): one edit.
    pub fn range_formatting(
        &self,
        p: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let Some(f) = self.file(&p.text_document.uri) else {
            return Ok(None);
        };
        let data = self.a.ws.file(f);
        let text = data.text();
        let dialect = self.a.ws.profile_of(f).dialect(data.decompiled);
        let idx = LineIndex::new(text);
        let r = idx.text_range(text, p.range, self.enc);
        let (start, end) = (usize::from(r.start()), usize::from(r.end()));
        match gsc_fmt::format_range(text, dialect, &self.project.format, start, end) {
            Ok(None) => Ok(Some(Vec::new())),
            Ok(Some(e)) => {
                let range =
                    TextRange::new(TextSize::from(e.start as u32), TextSize::from(e.end as u32));
                Ok(Some(vec![TextEdit {
                    range: idx.range(text, range, self.enc),
                    new_text: e.text,
                }]))
            }
            Err(gsc_fmt::FormatError::Syntax(errors)) => Err(Refused(format!(
                "not formatted: {} syntax error(s), first: {} {}",
                errors.len(),
                errors[0].code,
                errors[0].message
            ))),
            Err(e) => Err(Refused(format!("not formatted: {e}"))),
        }
    }

    /// The last lint results of `f`, if they were computed on its current text.
    fn fresh_diags(&self, f: FileId) -> Vec<gsc_check::Diagnostic> {
        let text = self.a.ws.file(f).text();
        let results = self.lint.results.lock().expect("results lock");
        match results.get(&f) {
            Some(r) if r.text_hash == text_hash(text) => r.diags.clone(),
            _ => Vec::new(),
        }
    }

    pub fn code_actions(&self, p: CodeActionParams) -> Option<CodeActionResponse> {
        let f = self.file(&p.text_document.uri)?;
        let text = self.a.ws.file(f).text();
        let idx = LineIndex::new(text);
        let want = idx.text_range(text, p.range, self.enc);
        let diags = self.fresh_diags(f);
        let only = p.context.only.clone();
        let wants = |k: &CodeActionKind| {
            only.as_ref()
                .is_none_or(|o| o.iter().any(|x| k.as_str().starts_with(x.as_str())))
        };
        let uri = &p.text_document.uri;
        let mut out = Vec::new();
        if wants(&CodeActionKind::QUICKFIX) {
            for d in &diags {
                let Some(fix) = &d.fix else { continue };
                let touches =
                    d.range.intersect(want).is_some() || d.range.contains_inclusive(want.start());
                if !touches || fix.edits.is_empty() {
                    continue;
                }
                let unsafe_fix = fix.applicability == gsc_check::Applicability::Unsafe;
                let edits = fix
                    .edits
                    .iter()
                    .map(|e| TextEdit {
                        range: idx.range(text, e.range, self.enc),
                        new_text: e.content.clone(),
                    })
                    .collect();
                out.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: if unsafe_fix {
                        format!("{} ({}, unsafe fix)", fix.message, d.code)
                    } else {
                        format!("{} ({})", fix.message, d.code)
                    },
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![to_lsp(d, text, &idx, self.enc)]),
                    edit: Some(WorkspaceEdit {
                        changes: Some(HashMap::from([(uri.clone(), edits)])),
                        ..Default::default()
                    }),
                    is_preferred: Some(!unsafe_fix),
                    ..Default::default()
                }));
            }
        }
        let fix_all = CodeActionKind::new("source.fixAll.gsc");
        if wants(&fix_all) || wants(&CodeActionKind::SOURCE_FIX_ALL) {
            let (fixed, n) = gsc_check::apply_fixes(text, &diags, false);
            if n > 0 {
                out.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: format!("Apply {n} safe fix(es) (gsc)"),
                    kind: Some(fix_all),
                    edit: Some(WorkspaceEdit {
                        changes: Some(HashMap::from([(
                            uri.clone(),
                            vec![minimal_edit(text, &fixed, self.enc)],
                        )])),
                        ..Default::default()
                    }),
                    ..Default::default()
                }));
            }
        }
        Some(out)
    }

    /// What may be renamed at a position: script functions and methods outside library roots,
    /// parameters and locals. Builtins, fields, macros and files are refused.
    fn rename_target(&self, f: FileId, off: TextSize) -> Result<(Symbol, TextRange)> {
        let ws = &self.a.ws;
        let sym = ws
            .symbol_at(f, off)
            .ok_or_else(|| Refused("nothing to rename here".to_owned()))?;
        let refs = ws.references(&sym, true);
        let here = refs
            .iter()
            .find(|r| r.file == f && r.range.contains_inclusive(off))
            .map(|r| r.range);
        match &sym {
            Symbol::Builtin(n) => Err(Refused(format!("`{n}` is an engine builtin"))),
            Symbol::Field(_) => Err(Refused(
                "fields are not renamed: any object may carry them".to_owned(),
            )),
            Symbol::File(_) => Err(Refused("files are renamed on disk".to_owned())),
            Symbol::Event(_) => Err(Refused("event names are not renamed".to_owned())),
            Symbol::Def(d) => {
                let def = ws.def(*d);
                if !matches!(def.kind, DefKind::Function | DefKind::Method) {
                    return Err(Refused(format!(
                        "only functions, methods and variables are renamed, not a {}",
                        def.kind.label()
                    )));
                }
                if self.a.is_library(d.file) {
                    return Err(Refused(format!(
                        "`{}` is defined in a library root (the game's scripts)",
                        def.name
                    )));
                }
                let range = here.unwrap_or(def.name_range);
                Ok((sym, range))
            }
            Symbol::Local { .. } => {
                let range = here.ok_or_else(|| Refused("nothing to rename here".to_owned()))?;
                Ok((sym, range))
            }
        }
    }

    pub fn prepare_rename(
        &self,
        p: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let Some((f, off)) = self.at(&p) else {
            return Ok(None);
        };
        let (_, range) = self.rename_target(f, off)?;
        let text = self.a.ws.file(f).text();
        Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
            range: self.range(f, range),
            placeholder: text[range].to_owned(),
        }))
    }

    pub fn rename(&self, p: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let Some((f, off)) = self.at(&p.text_document_position) else {
            return Ok(None);
        };
        let new = p.new_name.trim();
        let valid = new
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && new.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            return Err(Refused(format!("`{new}` is not an identifier")));
        }
        if self.project.dialect.keyword(new).is_some() {
            return Err(Refused(format!("`{new}` is a keyword")));
        }
        let (sym, _) = self.rename_target(f, off)?;
        let ws = &self.a.ws;
        if let Symbol::Def(d) = &sym {
            // A builtin of that name would shadow (or be shadowed by) the function.
            let lower = new.to_ascii_lowercase();
            if ws.is_builtin(d.file, &lower) {
                return Err(Refused(format!("`{new}` is an engine builtin")));
            }
        }
        let refs = ws.references(&sym, true);
        if let Some(r) = refs.iter().find(|r| self.a.is_library(r.file)) {
            return Err(Refused(format!(
                "a use is in a library root: {}",
                ws.file(r.file).meta.rel
            )));
        }
        let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
        for r in &refs {
            let text = ws.file(r.file).text();
            // A method reference may read `class::m`: replace the last segment only.
            let old = &text[r.range];
            let range = match old.rfind("::") {
                Some(i) => TextRange::new(
                    r.range.start() + TextSize::from(i as u32 + 2),
                    r.range.end(),
                ),
                None => r.range,
            };
            changes
                .entry(self.a.uri(r.file))
                .or_default()
                .push(TextEdit {
                    range: self.range(r.file, range),
                    new_text: new.to_owned(),
                });
        }
        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }))
    }

    pub fn semantic_tokens(&self, p: SemanticTokensParams) -> Option<SemanticTokensResult> {
        let f = self.file(&p.text_document.uri)?;
        Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: crate::semantic::tokens(&self.a.ws, f, self.enc),
        }))
    }

    /// `gsc/status`: what the server loaded (for the extension's status item and tests).
    pub fn status(&self) -> Value {
        let ws = &self.a.ws;
        serde_json::json!({
            "game": self.project.dialect.id,
            "gameSource": self.project.game_source,
            "mode": self.project.dialect.mode().as_str(),
            "files": ws.len(),
            "primaryFiles": ws.primary_files().count(),
            "libraryRoots": self.project.library,
            "builtins": self.project.catalogue.as_ref().map_or(0, |c| c.len()),
            "warnings": self.project.warnings,
        })
    }
}

/// One edit turning `old` into `new`: the differing middle, after the common prefix and suffix.
pub fn minimal_edit(old: &str, new: &str, enc: Encoding) -> TextEdit {
    let (a, b) = (old.as_bytes(), new.as_bytes());
    let mut pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    while !old.is_char_boundary(pre) || !new.is_char_boundary(pre) {
        pre -= 1;
    }
    let max_suf = a.len().min(b.len()) - pre;
    let mut suf = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take(max_suf)
        .take_while(|(x, y)| x == y)
        .count();
    while !old.is_char_boundary(a.len() - suf) || !new.is_char_boundary(b.len() - suf) {
        suf -= 1;
    }
    let idx = LineIndex::new(old);
    let range = TextRange::new(
        TextSize::from(pre as u32),
        TextSize::from((a.len() - suf) as u32),
    );
    TextEdit {
        range: idx.range(old, range, enc),
        new_text: new[pre..b.len() - suf].to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(old: &str, e: &TextEdit) -> String {
        let idx = LineIndex::new(old);
        let r = idx.text_range(old, e.range, Encoding::Utf16);
        format!(
            "{}{}{}",
            &old[..usize::from(r.start())],
            e.new_text,
            &old[usize::from(r.end())..]
        )
    }

    #[test]
    fn minimal_edits_apply() {
        for (a, b) in [
            ("abc", "abc"),
            ("a  b", "a b"),
            ("f(){\nx;\n}\n", "f()\n{\n\tx;\n}\n"),
            ("\u{e9}\u{e9}", "\u{e9}x\u{e9}"),
            ("", "x"),
            ("x", ""),
            ("aaa", "aa"),
        ] {
            let e = minimal_edit(a, b, Encoding::Utf16);
            assert_eq!(apply(a, &e), b, "{a:?} -> {b:?}");
        }
    }
}
