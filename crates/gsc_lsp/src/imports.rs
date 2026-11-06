//! Import directives, written the way the game reads them: `#using` in the namespace games,
//! `#include` in the merge games (the profile's `import_directive`). Completion of functions the
//! file has not imported (with the directive added), and the import code actions: add a missing
//! import, remove a duplicate, organize imports (drop the unused, sort).
//!
//! "Unused" is judged conservatively, as GSCode (Blakintosh/gscode, GPL-3.0) does: an import is
//! kept when the file calls into it, uses one of its classes, names a namespace it declares, or
//! when it declares an autoexec function or runs code outside functions (`REGISTER_SYSTEM`:
//! imported for its side effects); nothing is judged when
//! a header did not resolve (its macros could hide uses) or when the target is not found.

use std::collections::{HashMap, HashSet};

use gsc_sema::{DefId, DefKind, FileId, How, Resolution, Target, Workspace};
use gsc_syntax::{SyntaxKind, TextRange, TextSize};
use lsp_types::*;

use crate::handlers::Ctx;

/// `using` or `include`: what this file's game imports functions with.
pub fn directive(ws: &Workspace, f: FileId) -> &'static str {
    let p = ws.profile_of(f);
    match p.dialect.text("import_directive") {
        Some("include") => "include",
        Some("using") => "using",
        _ if p.config.resolution == Resolution::Include => "include",
        _ => "using",
    }
}

/// The path an import directive in `from` names `target` by: its path in its root, without the
/// script extension, with the separator the file's imports already use (`\` by default).
pub fn import_path(ws: &Workspace, from: FileId, target: FileId) -> String {
    let rel = &ws.file(target).meta.rel;
    let rel = rel.replace('\\', "/");
    let stem = gsc_sema::vfs::strip_script_ext(&rel).to_owned();
    let text = ws.file(from).text();
    let slash = ws
        .file(from)
        .facts
        .imports
        .iter()
        .any(|i| text[i.range].contains('/'));
    if slash {
        stem
    } else {
        stem.replace('/', "\\")
    }
}

fn line_end(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// The whole lines a range is on, line break included.
fn whole_lines(text: &str, r: TextRange) -> TextRange {
    let s = text[..usize::from(r.start())]
        .rfind('\n')
        .map_or(0, |i| i + 1);
    let e = text[usize::from(r.end())..]
        .find('\n')
        .map_or(text.len(), |i| usize::from(r.end()) + i + 1);
    TextRange::new(TextSize::from(s as u32), TextSize::from(e as u32))
}

/// The directive node around an import's path.
fn directive_range(ws: &Workspace, f: FileId, path: TextRange) -> TextRange {
    let root = ws.file(f).syntax();
    let el = root.covering_element(path);
    let node = match el {
        gsc_syntax::NodeOrToken::Node(n) => Some(n),
        gsc_syntax::NodeOrToken::Token(t) => t.parent(),
    };
    node.and_then(|n| {
        n.ancestors().find(|a| {
            matches!(
                a.kind(),
                SyntaxKind::USING_DIRECTIVE
                    | SyntaxKind::INCLUDE_DIRECTIVE
                    | SyntaxKind::INSERT_DIRECTIVE
                    | SyntaxKind::INLINE_DIRECTIVE
            )
        })
    })
    .map_or(path, |n| n.text_range())
}

/// The edit adding `#<directive> <path>;` to `f`: after the last directive of that kind, else
/// before the first import, else at the top.
pub fn add_import_edit(ws: &Workspace, f: FileId, target: FileId) -> (TextSize, String) {
    let data = ws.file(f);
    let text = data.text();
    let kind = directive(ws, f);
    let path = import_path(ws, f, target);
    let nl = line_end(text);
    let new = format!("#{kind} {path};{nl}");
    let imports = &data.facts.imports;
    let at = match imports.iter().rev().find(|i| i.directive == kind) {
        Some(last) => whole_lines(text, directive_range(ws, f, last.range)).end(),
        None => match imports.first() {
            Some(first) => whole_lines(text, directive_range(ws, f, first.range)).start(),
            None => TextSize::from(0),
        },
    };
    // A last line without its line break.
    if usize::from(at) == text.len() && !text.is_empty() && !text.ends_with('\n') {
        return (at, format!("{nl}#{kind} {path};"));
    }
    (at, new)
}

/// Does `f` import `target` already?
fn imports(ws: &Workspace, f: FileId, target: FileId) -> bool {
    let res = ws.resolution(f);
    res.scope.contains(&target)
}

/// The namespaces a target declares (identifier case).
fn namespaces_of(ws: &Workspace, t: FileId) -> HashSet<String> {
    let facts = &ws.file(t).facts;
    let mut set: HashSet<String> = facts.ns_switches.iter().map(|(_, n)| n.clone()).collect();
    set.insert(facts.namespace.clone());
    set.extend(facts.defs.iter().map(|d| d.ns.clone()));
    set
}

impl Ctx<'_> {
    /// Completion items for functions of files `f` does not import, adding the import.
    pub fn auto_import_items(
        &self,
        f: FileId,
        prefix: &str,
        taken: &HashSet<String>,
    ) -> Vec<CompletionItem> {
        let ws = &self.a.ws;
        let data = ws.file(f);
        let cfg = ws.profile_of(f).config;
        let namespaced = cfg.resolution == Resolution::Namespace;
        let vm = data.meta.vm;
        let own_ns = &data.facts.namespace;
        // Per label, the best candidate: same root, then a primary root.
        let mut best: HashMap<String, (u8, DefId, bool)> = HashMap::new();
        for d in ws.functions_with_prefix(prefix) {
            let def = ws.def(d);
            let g = ws.file(d.file);
            if d.file == f
                || g.meta.vm.is_none()
                || !gsc_sema::vfs::vm_compatible(g.meta.vm, vm)
                || def.flags.split(',').any(|x| x == "private")
            {
                continue;
            }
            let imported = imports(ws, f, d.file);
            let label = if namespaced {
                if def.ns == *own_ns {
                    continue;
                }
                format!("{}::{}", def.ns, def.short_name())
            } else {
                if imported {
                    continue;
                }
                def.short_name().to_owned()
            };
            if taken.contains(&cfg.ident(&label)) {
                continue;
            }
            let rank = u8::from(g.meta.root != data.meta.root) + u8::from(ws.is_library(d.file));
            match best.get(&label) {
                Some((r, _, _)) if *r <= rank => {}
                _ => {
                    best.insert(label, (rank, d, imported));
                }
            }
        }
        let mut labels: Vec<_> = best.into_iter().collect();
        labels.sort_by(|a, b| a.0.cmp(&b.0));
        labels.truncate(100);
        labels
            .into_iter()
            .map(|(label, (_, d, imported))| {
                let def = ws.def(d);
                let path = import_path(ws, f, d.file);
                let edits = (!imported).then(|| {
                    let (at, new) = add_import_edit(ws, f, d.file);
                    vec![TextEdit {
                        range: self.range(f, TextRange::empty(at)),
                        new_text: new,
                    }]
                });
                CompletionItem {
                    label: label.clone(),
                    label_details: Some(CompletionItemLabelDetails {
                        detail: None,
                        description: Some(path.clone()),
                    }),
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some(if imported {
                        format!("{}::{}( {} )", def.ns, def.short_name(), def.params)
                    } else {
                        format!("#{} {path};", directive(ws, f))
                    }),
                    sort_text: Some(format!("~{label}")),
                    filter_text: Some(def.short_name().to_owned()),
                    insert_text: Some(label),
                    additional_text_edits: edits,
                    ..Default::default()
                }
            })
            .collect()
    }

    /// Is import `i` of `f` used? `None` when it cannot be judged.
    fn import_used(&self, f: FileId, i: usize, uses: &Uses) -> Option<bool> {
        let ws = &self.a.ws;
        let res = ws.resolution(f);
        let t = res.imports.get(i).copied().flatten()?;
        if res.refs.iter().any(|r| r.def.is_some_and(|d| d.file == t)) {
            return Some(true);
        }
        let tf = &ws.file(t).facts;
        // Code outside any function (`REGISTER_SYSTEM( "x", &__init__, undefined )`) runs when
        // the file loads: the import may be there for it.
        if tf.refs.iter().any(|r| r.caller.is_none()) {
            return Some(true);
        }
        if tf.defs.iter().any(|d| {
            (d.kind == DefKind::Class && uses.classes.contains(&d.name))
                || (d.kind == DefKind::Function && uses.bare.contains(&d.name))
                || d.flags.split(',').any(|x| x == "autoexec")
        }) {
            return Some(true);
        }
        if ws.profile_of(f).config.resolution == Resolution::Namespace
            && namespaces_of(ws, t)
                .iter()
                .any(|n| uses.namespaces.contains(n))
        {
            return Some(true);
        }
        Some(false)
    }

    /// What `f` names besides its resolved references: classes, qualified namespaces, and the
    /// names its macro expansions call. `None` when a header did not resolve.
    fn uses(&self, f: FileId) -> Option<Uses> {
        let ws = &self.a.ws;
        let data = ws.file(f);
        let res = ws.resolution(f);
        let unresolved_header = data
            .facts
            .imports
            .iter()
            .zip(res.imports.iter())
            .any(|(i, t)| !i.is_scope() && t.is_none());
        if unresolved_header {
            return None;
        }
        let cfg = ws.profile_of(f).config;
        let mut u = Uses::default();
        for x in &data.facts.refs {
            if let Target::Qualified(Some(q), _) = &x.target {
                u.namespaces.insert(q.clone());
            }
        }
        let root = data.syntax();
        for n in root.descendants() {
            if matches!(n.kind(), SyntaxKind::NEW_EXPR | SyntaxKind::CLASS_BASE) {
                for r in n.children().filter(|c| c.kind() == SyntaxKind::NAME_REF) {
                    u.classes.insert(cfg.ident(&r.text().to_string()));
                }
            }
        }
        for x in ws.macro_expansions(f) {
            let toks = &x.tokens;
            for (k, t) in toks.iter().enumerate() {
                if t.kind != SyntaxKind::IDENT {
                    continue;
                }
                match toks.get(k + 1).map(|n| n.text.as_str()) {
                    Some("::") => {
                        u.namespaces.insert(cfg.ident(&t.text));
                    }
                    Some("(") => {
                        u.bare.insert(cfg.ident(&t.text));
                    }
                    _ => {
                        if toks.get(k.wrapping_sub(1)).is_some_and(|p| p.text == "new") {
                            u.classes.insert(cfg.ident(&t.text));
                        }
                    }
                }
            }
        }
        Some(u)
    }

    /// The import code actions at `want`: add a missing import, remove a duplicate one, and
    /// organize imports.
    pub fn import_actions(
        &self,
        f: FileId,
        uri: &Uri,
        want: TextRange,
        wants: &dyn Fn(&CodeActionKind) -> bool,
    ) -> Vec<CodeActionOrCommand> {
        let ws = &self.a.ws;
        let data = ws.file(f);
        let text = data.text();
        let facts = &data.facts;
        let mut out = Vec::new();
        let edit = |edits: Vec<(TextRange, String)>| WorkspaceEdit {
            changes: Some(HashMap::from([(
                uri.clone(),
                edits
                    .into_iter()
                    .map(|(r, t)| TextEdit {
                        range: self.range(f, r),
                        new_text: t,
                    })
                    .collect(),
            )])),
            ..Default::default()
        };
        // Duplicates: a later directive naming what an earlier one does.
        let mut first: HashMap<(&str, &str), usize> = HashMap::new();
        let mut dups: Vec<usize> = Vec::new();
        for (i, imp) in facts.imports.iter().enumerate() {
            if first
                .insert((imp.directive, imp.target.as_str()), i)
                .is_some()
            {
                dups.push(i);
            }
        }
        if wants(&CodeActionKind::QUICKFIX) {
            for &i in &dups {
                let imp = &facts.imports[i];
                let line = whole_lines(text, directive_range(ws, f, imp.range));
                if line.intersect(want).is_none() && !line.contains_inclusive(want.start()) {
                    continue;
                }
                out.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: format!("Remove duplicate `#{} {}`", imp.directive, &text[imp.range]),
                    kind: Some(CodeActionKind::QUICKFIX),
                    edit: Some(edit(vec![(line, String::new())])),
                    is_preferred: Some(true),
                    ..Default::default()
                }));
            }
            out.extend(self.missing_import_actions(f, want, &edit));
        }
        if wants(&CodeActionKind::SOURCE_ORGANIZE_IMPORTS) {
            if let Some(edits) = self.organize(f, &dups) {
                out.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: "Organize imports".to_owned(),
                    kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
                    edit: Some(edit(edits)),
                    ..Default::default()
                }));
            }
        }
        out
    }

    fn missing_import_actions(
        &self,
        f: FileId,
        want: TextRange,
        edit: &dyn Fn(Vec<(TextRange, String)>) -> WorkspaceEdit,
    ) -> Vec<CodeActionOrCommand> {
        let ws = &self.a.ws;
        let data = ws.file(f);
        let res = ws.resolution(f);
        let kind = directive(ws, f);
        let mut targets: Vec<FileId> = Vec::new();
        for (x, r) in data.facts.refs.iter().zip(res.refs.iter()) {
            if x.range.intersect(want).is_none() && !x.range.contains_inclusive(want.start()) {
                continue;
            }
            match (&x.target, kind) {
                (Target::Qualified(Some(q), name), "using") if !q.contains('/') => {
                    let d = r.def.or_else(|| ws.namespace_fn(f, q, name));
                    if let Some(d) = d {
                        if d.file != f && !imports(ws, f, d.file) {
                            targets.push(d.file);
                        }
                    }
                }
                (Target::Bare(name), "include") if r.how == How::Unresolved => {
                    for &d in ws.callables_named(name).iter().take(5) {
                        let def = ws.def(d);
                        let g = ws.file(d.file);
                        if def.kind == DefKind::Function
                            && d.file != f
                            && g.meta.vm.is_some()
                            && gsc_sema::vfs::vm_compatible(g.meta.vm, data.meta.vm)
                            && !def.flags.split(',').any(|x| x == "private")
                        {
                            targets.push(d.file);
                        }
                    }
                }
                _ => {}
            }
        }
        targets.sort();
        targets.dedup();
        targets
            .into_iter()
            .map(|t| {
                let (at, new) = add_import_edit(ws, f, t);
                CodeActionOrCommand::CodeAction(CodeAction {
                    title: format!("Add `#{kind} {};`", import_path(ws, f, t)),
                    kind: Some(CodeActionKind::QUICKFIX),
                    edit: Some(edit(vec![(TextRange::empty(at), new)])),
                    is_preferred: Some(true),
                    ..Default::default()
                })
            })
            .collect()
    }

    /// Organize imports: drop duplicates and unused `#using`/`#include`s, and sort the block of
    /// them when it is one run of lines. `None` when nothing changes.
    fn organize(&self, f: FileId, dups: &[usize]) -> Option<Vec<(TextRange, String)>> {
        let ws = &self.a.ws;
        let data = ws.file(f);
        let text = data.text();
        let facts = &data.facts;
        let header = data.meta.vm.is_none();
        let uses = if header { None } else { self.uses(f) };
        let mut drop: HashSet<usize> = dups.iter().copied().collect();
        if let Some(u) = &uses {
            for (i, imp) in facts.imports.iter().enumerate() {
                if imp.is_scope() && self.import_used(f, i, u) == Some(false) {
                    drop.insert(i);
                }
            }
        }
        let scope: Vec<usize> = (0..facts.imports.len())
            .filter(|&i| facts.imports[i].is_scope())
            .collect();
        let lines: Vec<TextRange> = scope
            .iter()
            .map(|&i| whole_lines(text, directive_range(ws, f, facts.imports[i].range)))
            .collect();
        // One run: each directive's lines start where the previous one's end.
        let contiguous = !lines.is_empty() && lines.windows(2).all(|w| w[0].end() == w[1].start());
        if contiguous {
            let block = TextRange::new(lines[0].start(), lines[lines.len() - 1].end());
            let mut kept: Vec<(String, String)> = scope
                .iter()
                .zip(&lines)
                .filter(|(i, _)| !drop.contains(i))
                .map(|(&i, l)| {
                    let line = text[*l].trim_end_matches(['\n', '\r']).to_owned();
                    let imp = &facts.imports[i];
                    (
                        format!("{}\u{0}{}", imp.directive, imp.target.to_ascii_lowercase()),
                        line,
                    )
                })
                .collect();
            kept.sort();
            let nl = line_end(text);
            let mut new: String = kept.into_iter().map(|(_, l)| format!("{l}{nl}")).collect();
            if !text[block].ends_with('\n') {
                new.truncate(new.len() - nl.len());
            }
            if new == text[block] {
                return None;
            }
            return Some(vec![(block, new)]);
        }
        let mut edits: Vec<(TextRange, String)> = drop
            .iter()
            .map(|&i| {
                (
                    whole_lines(text, directive_range(ws, f, facts.imports[i].range)),
                    String::new(),
                )
            })
            .collect();
        edits.sort_by_key(|e| e.0.start());
        edits.dedup_by_key(|e| e.0);
        (!edits.is_empty()).then_some(edits)
    }
}

/// Names a file uses that its references do not record.
#[derive(Default)]
struct Uses {
    classes: HashSet<String>,
    namespaces: HashSet<String>,
    bare: HashSet<String>,
}
