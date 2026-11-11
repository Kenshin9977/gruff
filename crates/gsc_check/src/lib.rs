//! Ruff-like lint rules over the GSC syntax tree: the engine behind `gruff check`.
//!
//! [`check`] runs over a [`gsc_sema::Workspace`], which has parsed every file and resolves names
//! across them. A first pass adds per-function "may yield" summaries
//! ([`workspace::Summaries`]); the second runs the rules on each file of the non-library roots,
//! applies suppression comments and reports unused ones. Both passes run on the workspace's
//! threads; results come back in file order.

pub mod config;
pub mod incremental;
pub mod output;
pub mod registry;
mod rules;
pub mod select;
mod suppress;
pub mod workspace;

use std::path::{Path, PathBuf};

use gsc_sema::{par_map, FileId, Workspace};
use gsc_syntax::{check_dialect, LineCol, LineIndex, SourceText, SyntaxError, TextRange, TextSize};

pub use gsc_sema::{Catalogue, Decompiled, FieldCatalogue};
pub use registry::{Rule, RULES};
pub use select::{Layer, Selection, Selector};

/// Whether a fix keeps the program's behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applicability {
    /// Applied by `--fix`.
    Safe,
    /// Applied only with `--unsafe-fixes`.
    Unsafe,
}

impl Applicability {
    pub fn label(self) -> &'static str {
        match self {
            Applicability::Safe => "safe",
            Applicability::Unsafe => "unsafe",
        }
    }
}

/// Replace `range` with `content`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub range: TextRange,
    pub content: String,
}

/// A fix: edits applied together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fix {
    pub applicability: Applicability,
    pub message: String,
    pub edits: Vec<Edit>,
}

/// One finding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    pub range: TextRange,
    pub fix: Option<Fix>,
    /// Filled in by [`check`].
    pub start: LineCol,
    pub end: LineCol,
}

impl Diagnostic {
    pub fn new(code: &str, message: String, range: TextRange, fix: Option<Fix>) -> Diagnostic {
        Diagnostic {
            code: code.to_owned(),
            message,
            range,
            fix,
            start: LineCol { line: 1, col: 1 },
            end: LineCol { line: 1, col: 1 },
        }
    }

    fn from_syntax(e: &SyntaxError) -> Diagnostic {
        Diagnostic::new(e.code, e.message.clone(), e.range, None)
    }

    pub fn name(&self) -> &'static str {
        registry::name_of(&self.code)
    }
}

/// What `check` needs besides the workspace.
pub struct Options<'a> {
    pub selection: &'a Selection,
    /// `(glob, selectors)` matched against paths relative to `root`.
    pub per_file_ignores: &'a [(String, Vec<Selector>)],
    /// Base directory for per-file-ignore globs.
    pub root: Option<&'a Path>,
}

impl Options<'_> {
    fn relative(&self, path: &Path) -> String {
        let rel = match self.root {
            Some(root) => {
                let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
                let root = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
                abs.strip_prefix(&root)
                    .map(Path::to_path_buf)
                    .unwrap_or(abs)
            }
            None => path.to_path_buf(),
        };
        rel.to_string_lossy().replace('\\', "/")
    }

    /// Is `code` on for the file at `rel` (relative to `root`)?
    pub fn enabled_for(&self, rel: &str, code: &str) -> bool {
        self.selection.enabled(code)
            && !self
                .per_file_ignores
                .iter()
                .any(|(g, sels)| select::glob_match(g, rel) && sels.iter().any(|s| s.matches(code)))
    }
}

/// The findings of one file.
#[derive(Clone, Debug)]
pub struct FileResult {
    pub file: FileId,
    pub path: PathBuf,
    pub display: String,
    pub source: SourceText,
    pub diagnostics: Vec<Diagnostic>,
}

/// The check facts of one file of the workspace.
fn file_check_facts(ws: &Workspace, f: FileId) -> workspace::CheckFacts {
    let data = ws.file(f);
    let dialect = ws.profile_of(f).dialect(data.decompiled);
    workspace::check_facts(&data.syntax(), &data.facts, dialect)
}

/// Lint the files of the workspace's non-library roots. Library roots (game scripts) only take
/// part in name resolution and yield summaries.
pub fn check(ws: &Workspace, opts: &Options) -> Vec<FileResult> {
    let all: Vec<FileId> = ws.files().collect();
    ws.resolve_all();
    // Pass 1: yield summaries and fields over every file, libraries included.
    let facts: Vec<(FileId, workspace::CheckFacts)> =
        par_map(&all, ws.jobs(), |&f| (f, file_check_facts(ws, f)));
    let sums = workspace::Summaries::build(ws, &facts, ws.jobs());

    // Pass 2: rules, then suppressions.
    let primary: Vec<FileId> = ws.primary_files().collect();
    par_map(&primary, ws.jobs(), |&f| {
        let data = ws.file(f);
        FileResult {
            file: f,
            path: data.meta.path.clone(),
            display: data.meta.display.clone(),
            source: data.source.clone(),
            diagnostics: check_file(ws, opts, &sums, f),
        }
    })
}

/// The rules and suppressions of one file, given the workspace's summaries: sorted
/// diagnostics with their line and column.
fn check_file(
    ws: &Workspace,
    opts: &Options,
    sums: &workspace::Summaries,
    f: FileId,
) -> Vec<Diagnostic> {
    let data = ws.file(f);
    let profile = ws.profile_of(f);
    let dialect = profile.dialect(data.decompiled);
    let text = data.text();
    let rel = opts.relative(&data.meta.path);
    let enabled = |code: &str| opts.enabled_for(&rel, code);
    let root = data.syntax();
    let mut diags: Vec<Diagnostic> = data
        .errors
        .iter()
        .chain(&check_dialect(&root, dialect))
        .map(Diagnostic::from_syntax)
        .collect();
    let mut ctx = rules::Ctx {
        file: f,
        root: &root,
        text,
        ws,
        sums,
        catalogue: profile.builtins.as_ref(),
        fields: profile.fields.as_ref(),
        vm: data.meta.vm,
        mode: data.mode,
        decompiled: data.decompiled,
        decompiler_names: data.errors.iter().any(SyntaxError::is_decompiler_name),
        syntax_errors: data.errors.iter().map(|e| e.range.start()).collect(),
        enabled: &enabled,
        out: Vec::new(),
    };
    // A game's compiler refuses a file with a syntax error whole: nothing in it runs, so a
    // finding about its meaning describes code the game never sees. Report the syntax errors
    // and stop there, as the file's own compiler does. E0301 is not one of these: it marks
    // decompiler output no tokenizer can read, and the decompiled corpora are still worth
    // checking around it.
    let refused = data
        .errors
        .iter()
        .any(|e| e.is_syntax() && !e.is_decompiler_name());
    if !refused {
        rules::run(&mut ctx);
        diags.extend(ctx.out);
    }
    if profile.scheme.is_some() {
        for d in &mut diags {
            if d.message.contains('_') {
                d.message = ws.display_message(f, &d.message);
            }
        }
    }
    diags.retain(|d| enabled(&d.code));
    let index = LineIndex::new(text);
    // No rule ran, so every suppression in the file suppressed nothing: S100 would turn one
    // refusal into a report per suppression comment.
    suppress::apply(
        &root,
        text,
        &index,
        &mut diags,
        &enabled,
        !refused && enabled("S100"),
    );
    for d in &mut diags {
        d.start = index.line_col(text, d.range.start());
        d.end = index.line_col(text, d.range.end());
    }
    diags.sort_by(|a, b| {
        (a.range.start(), &a.code, a.range.end()).cmp(&(b.range.start(), &b.code, b.range.end()))
    });
    diags.dedup();
    diags
}

/// Apply the fixes of `diags` to `text`: safe ones, plus unsafe ones when `unsafe_fixes`.
/// Fixes that overlap an earlier one are skipped (a later run picks them up). Returns the new
/// text and how many fixes were applied.
pub fn apply_fixes(text: &str, diags: &[Diagnostic], unsafe_fixes: bool) -> (String, usize) {
    let mut fixes: Vec<&Fix> = diags
        .iter()
        .filter_map(|d| d.fix.as_ref())
        .filter(|f| unsafe_fixes || f.applicability == Applicability::Safe)
        .filter(|f| !f.edits.is_empty())
        .collect();
    fixes.sort_by_key(|f| f.edits.iter().map(|e| e.range.start()).min());
    let mut edits: Vec<&Edit> = Vec::new();
    let mut applied = 0;
    let mut last_end: Option<TextSize> = None;
    for f in fixes {
        let mut es: Vec<&Edit> = f.edits.iter().collect();
        es.sort_by_key(|e| (e.range.start(), e.range.end()));
        let start = es[0].range.start();
        if last_end.is_some_and(|end| start < end) {
            continue;
        }
        if es.windows(2).any(|w| w[1].range.start() < w[0].range.end()) {
            continue;
        }
        last_end = es.iter().map(|e| e.range.end()).max();
        edits.extend(es);
        applied += 1;
    }
    let mut out = String::with_capacity(text.len());
    let mut pos = 0usize;
    for e in edits {
        let (s, t) = (usize::from(e.range.start()), usize::from(e.range.end()));
        out.push_str(&text[pos..s]);
        out.push_str(&e.content);
        pos = t;
    }
    out.push_str(&text[pos..]);
    (out, applied)
}
