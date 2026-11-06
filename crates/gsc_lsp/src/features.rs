//! Editor features over the syntax tree and the navigation queries: folding, selection ranges,
//! document links, highlights, code lenses, implementations, type definitions and the class
//! type hierarchy.
//!
//! Behaviour follows GSCode's handlers (Blakintosh/gscode, GPL-3.0) where it is specific: folds
//! for declarations, blocks, comment runs, dev blocks and `region` / `endregion` comment pairs;
//! "N references" lenses on functions, methods, classes and macros, an autoexec function saying
//! so.

use std::collections::HashMap;

use gsc_sema::{DefId, DefKind, FileId, NavTarget};
use gsc_syntax::{NodeOrToken, SyntaxKind, TextRange};
use lsp_types::request::{GotoImplementationResponse, GotoTypeDefinitionResponse};
use lsp_types::*;
use serde_json::{json, Value};

use crate::handlers::Ctx;

/// The client command a reference-count lens runs: `(uri, line, character)`. The VS Code
/// extension turns it into `editor.action.showReferences`.
pub const SHOW_REFERENCES: &str = "gruff.showReferences";

/// The text of a comment after its opening characters, lower case.
fn comment_body(text: &str) -> String {
    text.trim_start_matches(['/', '*', '@'])
        .trim()
        .to_ascii_lowercase()
}

impl Ctx<'_> {
    fn links(&self, targets: Vec<NavTarget>) -> Vec<LocationLink> {
        targets
            .iter()
            .map(|n| LocationLink {
                origin_selection_range: None,
                target_uri: self.a.uri(n.file),
                target_range: self.range(n.file, n.full_range),
                target_selection_range: self.range(n.file, n.focus_range),
            })
            .collect()
    }

    pub fn folding_ranges(&self, p: FoldingRangeParams) -> Option<Vec<FoldingRange>> {
        let f = self.file(&p.text_document.uri)?;
        let data = self.a.ws.file(f);
        let text = data.text();
        let idx = self.lines(f);
        let root = data.syntax();
        let mut out: HashMap<u32, FoldingRange> = HashMap::new();
        let mut add = |start: u32, end: u32, kind: Option<FoldingRangeKind>| {
            if end <= start {
                return;
            }
            let e = out.entry(start).or_insert(FoldingRange {
                start_line: start,
                start_character: None,
                end_line: end,
                end_character: None,
                kind: kind.clone(),
                collapsed_text: None,
            });
            if end > e.end_line {
                e.end_line = end;
                e.kind = kind;
            }
        };
        // The last line a fold hides: a closing `}` (or `#/`) alone on its line stays visible.
        let end_line = |r: TextRange| {
            let last = idx.line_of(r.end());
            let line_start = usize::from(idx.line_start(last));
            let before = text[line_start.min(usize::from(r.end()))..usize::from(r.end())].trim();
            if before == "}" || before == "#/" {
                last.saturating_sub(1)
            } else {
                last
            }
        };
        let start_of = |n: &gsc_syntax::SyntaxNode| {
            gsc_syntax::first_significant_token(n)
                .map_or(n.text_range().start(), |t| t.text_range().start())
        };
        for n in root.descendants() {
            let r = n.text_range();
            match n.kind() {
                SyntaxKind::FUNCTION_DEF
                | SyntaxKind::CONSTRUCTOR
                | SyntaxKind::DESTRUCTOR
                | SyntaxKind::CLASS_DEF
                | SyntaxKind::SWITCH_STMT
                | SyntaxKind::DEV_BLOCK_STMT
                | SyntaxKind::DEV_BLOCK_ITEM
                | SyntaxKind::ARRAY_EXPR
                | SyntaxKind::STRUCT_EXPR => add(idx.line_of(start_of(&n)), end_line(r), None),
                SyntaxKind::BLOCK => {
                    // A body block folds with its statement (`if`, `while`...), from its line.
                    let from = match n.parent() {
                        Some(p)
                            if !matches!(p.kind(), SyntaxKind::BLOCK | SyntaxKind::SOURCE_FILE) =>
                        {
                            p
                        }
                        _ => n.clone(),
                    };
                    add(idx.line_of(start_of(&from)), end_line(r), None);
                }
                _ => {}
            }
        }
        // Comments: multi-line block comments, runs of line comments, region pairs; runs of
        // import directives.
        let mut run: Option<(u32, u32)> = None;
        let mut regions: Vec<u32> = Vec::new();
        let mut imports: Option<(u32, u32)> = None;
        let mut region =
            |body: &str, line: u32, add: &mut dyn FnMut(u32, u32, Option<FoldingRangeKind>)| {
                if body.starts_with("endregion") {
                    if let Some(s) = regions.pop() {
                        add(s, line, Some(FoldingRangeKind::Region));
                    }
                    true
                } else if body.starts_with("region") {
                    regions.push(line);
                    true
                } else {
                    false
                }
            };
        for el in root.descendants_with_tokens() {
            let t = match el {
                NodeOrToken::Node(n) => {
                    if matches!(
                        n.kind(),
                        SyntaxKind::USING_DIRECTIVE | SyntaxKind::INCLUDE_DIRECTIVE
                    ) {
                        let a = idx.line_of(n.text_range().start());
                        let b = idx.line_of(n.text_range().end());
                        imports = match imports {
                            Some((s, e)) if a <= e + 1 => Some((s, b)),
                            Some((s, e)) => {
                                add(s, e, Some(FoldingRangeKind::Imports));
                                Some((a, b))
                            }
                            None => Some((a, b)),
                        };
                    }
                    continue;
                }
                NodeOrToken::Token(t) => t,
            };
            let tr = t.text_range();
            let (a, b) = (idx.line_of(tr.start()), idx.line_of(tr.end()));
            match t.kind() {
                SyntaxKind::COMMENT => {
                    run = match run {
                        Some((s, e)) if a == e + 1 => Some((s, a)),
                        Some((s, e)) => {
                            add(s, e, Some(FoldingRangeKind::Comment));
                            Some((a, a))
                        }
                        None => Some((a, a)),
                    };
                    region(&comment_body(t.text()), a, &mut add);
                }
                SyntaxKind::BLOCK_COMMENT | SyntaxKind::DOC_COMMENT => {
                    if !region(&comment_body(t.text()), a, &mut add) {
                        add(a, b, Some(FoldingRangeKind::Comment));
                    }
                }
                k if k.is_trivia() => {}
                _ => {
                    if let Some((s, e)) = run.take() {
                        add(s, e, Some(FoldingRangeKind::Comment));
                    }
                }
            }
        }
        if let Some((s, e)) = run {
            add(s, e, Some(FoldingRangeKind::Comment));
        }
        if let Some((s, e)) = imports {
            add(s, e, Some(FoldingRangeKind::Imports));
        }
        let mut v: Vec<FoldingRange> = out.into_values().collect();
        v.sort_by_key(|r| (r.start_line, r.end_line));
        Some(v)
    }

    pub fn selection_ranges(&self, p: SelectionRangeParams) -> Option<Vec<SelectionRange>> {
        let f = self.file(&p.text_document.uri)?;
        let root = self.a.ws.file(f).syntax();
        let mut out = Vec::new();
        for pos in p.positions {
            let off = self.offset(f, pos);
            let at = root.token_at_offset(off);
            let tok = at
                .clone()
                .right_biased()
                .filter(|t| !t.kind().is_trivia())
                .or(at.left_biased());
            let mut ranges: Vec<TextRange> = Vec::new();
            if let Some(t) = &tok {
                if !t.kind().is_trivia() {
                    ranges.push(t.text_range());
                }
                for n in t.parent_ancestors() {
                    let r = n.text_range();
                    if ranges.last() != Some(&r) && !r.is_empty() {
                        ranges.push(r);
                    }
                }
            }
            let mut sel: Option<SelectionRange> = None;
            for r in ranges.into_iter().rev() {
                sel = Some(SelectionRange {
                    range: self.range(f, r),
                    parent: sel.map(Box::new),
                });
            }
            out.push(sel.unwrap_or(SelectionRange {
                range: Range::new(pos, pos),
                parent: None,
            }));
        }
        Some(out)
    }

    pub fn document_links(&self, p: DocumentLinkParams) -> Option<Vec<DocumentLink>> {
        let f = self.file(&p.text_document.uri)?;
        let facts = &self.a.ws.file(f).facts;
        let res = self.a.ws.resolution(f);
        let links = facts
            .imports
            .iter()
            .zip(res.imports.iter())
            .filter_map(|(imp, target)| {
                let t = (*target)?;
                Some(DocumentLink {
                    range: self.range(f, imp.range),
                    target: Some(self.a.uri(t)),
                    tooltip: Some(self.a.ws.file(t).meta.rel.clone()),
                    data: None,
                })
            })
            .collect();
        Some(links)
    }

    pub fn document_highlight(&self, p: DocumentHighlightParams) -> Option<Vec<DocumentHighlight>> {
        let (f, off) = self.at(&p.text_document_position_params)?;
        let hs = self.a.ws.highlights(f, off);
        if hs.is_empty() {
            return None;
        }
        Some(
            hs.iter()
                .map(|h| DocumentHighlight {
                    range: self.range(f, h.range),
                    kind: Some(if h.write {
                        DocumentHighlightKind::WRITE
                    } else {
                        DocumentHighlightKind::READ
                    }),
                })
                .collect(),
        )
    }

    pub fn code_lens(&self, p: CodeLensParams) -> Option<Vec<CodeLens>> {
        if !self.features.code_lens {
            return Some(Vec::new());
        }
        let f = self.file(&p.text_document.uri)?;
        let ws = &self.a.ws;
        let uri = self.a.uri(f);
        let lenses = ws
            .reference_counts(f)
            .into_iter()
            .map(|(d, n)| {
                let def = ws.def(d);
                let mut title = if n == 1 {
                    "1 reference".to_owned()
                } else {
                    format!("{n} references")
                };
                if def.flags.split(',').any(|x| x == "autoexec") {
                    title = if n == 0 {
                        "autoexec entry point".to_owned()
                    } else {
                        format!("{title} \u{b7} autoexec")
                    };
                }
                let range = self.range(f, def.name_range);
                CodeLens {
                    range,
                    command: Some(Command {
                        title,
                        command: SHOW_REFERENCES.to_owned(),
                        arguments: Some(vec![
                            json!(uri.as_str()),
                            json!(range.start.line),
                            json!(range.start.character),
                        ]),
                    }),
                    data: None,
                }
            })
            .collect();
        Some(lenses)
    }

    pub fn implementation(
        &self,
        p: request::GotoImplementationParams,
    ) -> Option<GotoImplementationResponse> {
        let (f, off) = self.at(&p.text_document_position_params)?;
        let t = self.a.ws.implementations(f, off);
        (!t.is_empty()).then(|| GotoDefinitionResponse::Link(self.links(t)))
    }

    pub fn type_definition(
        &self,
        p: request::GotoTypeDefinitionParams,
    ) -> Option<GotoTypeDefinitionResponse> {
        let (f, off) = self.at(&p.text_document_position_params)?;
        let t = self.a.ws.type_definitions(f, off);
        (!t.is_empty()).then(|| GotoDefinitionResponse::Link(self.links(t)))
    }

    fn type_item(&self, d: DefId) -> TypeHierarchyItem {
        let def = self.a.ws.def(d);
        TypeHierarchyItem {
            name: def.name.clone(),
            kind: SymbolKind::CLASS,
            tags: None,
            detail: (!def.params.is_empty()).then(|| format!(": {}", def.params)),
            uri: self.a.uri(d.file),
            range: self.range(d.file, def.range),
            selection_range: self.range(d.file, def.name_range),
            data: Some(json!({
                "file": self.a.ws.file(d.file).meta.path,
                "index": d.index,
                "name": def.name,
            })),
        }
    }

    /// The class a type hierarchy item names: by index when it still has that name, else by
    /// name.
    fn type_item_def(&self, item: &TypeHierarchyItem) -> Option<DefId> {
        let data: &Value = item.data.as_ref()?;
        let name = data.get("name")?.as_str()?;
        let path: Option<std::path::PathBuf> =
            serde_json::from_value(data.get("file")?.clone()).ok();
        let index = data.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;
        let f: Option<FileId> = path.and_then(|p| self.a.file_id(&p));
        if let Some(f) = f {
            let defs = &self.a.ws.file(f).facts.defs;
            if defs
                .get(index as usize)
                .is_some_and(|d| d.name == name && d.kind == DefKind::Class)
            {
                return Some(DefId { file: f, index });
            }
        }
        self.a.ws.class_named(name)
    }

    pub fn prepare_type_hierarchy(
        &self,
        p: TypeHierarchyPrepareParams,
    ) -> Option<Vec<TypeHierarchyItem>> {
        let (f, off) = self.at(&p.text_document_position_params)?;
        let d = self.a.ws.class_at(f, off)?;
        Some(vec![self.type_item(d)])
    }

    pub fn supertypes(&self, p: TypeHierarchySupertypesParams) -> Option<Vec<TypeHierarchyItem>> {
        let d = self.type_item_def(&p.item)?;
        let items = self.a.ws.superclasses(d);
        Some(items.into_iter().map(|x| self.type_item(x)).collect())
    }

    pub fn subtypes(&self, p: TypeHierarchySubtypesParams) -> Option<Vec<TypeHierarchyItem>> {
        let d = self.type_item_def(&p.item)?;
        let name = self.a.ws.def(d).name.clone();
        let items = self.a.ws.subclasses(&name);
        Some(items.into_iter().map(|x| self.type_item(x)).collect())
    }
}
