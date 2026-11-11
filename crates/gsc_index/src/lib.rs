//! Workspace index: symbols, calls, events, fields and includes as grep-able TSV.
//!
//! `gruff index` is a view of a [`gsc_sema::Workspace`], which parses every file and resolves calls
//! and imports across files; [`extract`] adds what only the index needs (event sites, field
//! accesses) and [`write`] renders one TSV per fact kind. The output is a pure function of the
//! inputs, whatever the thread count.
//!
//! The index is meant for agents with a small context: one fact per line, a header line, paths
//! relative to the indexed root. See the generated `README.md` for the recipes. [`unknowns`] adds
//! the two optional tables of `docs/hash-recovery.md`.

pub mod extract;
pub mod macros;
pub mod report;
pub mod unknowns;
pub mod write;

use std::sync::Arc;
use std::time::{Duration, Instant};

use gsc_sema::facts::LineStarts;
use gsc_sema::{par_map, FileId, FileResolution, Profile, Root, Workspace};

pub use extract::{Event, EventClass, EventName, Field};
pub use gsc_sema::{BareCall, Config, Decompiled, Resolution};
pub use macros::MacroCall;

/// What the index keeps of one file, beside the workspace's facts.
pub struct FileIndex {
    pub id: FileId,
    /// The path written in the index (`/` separators, root label first when there are several
    /// roots).
    pub rel: String,
    pub lines: LineStarts,
    /// Event sites: the tree's, then those of macro expansions (`via_macro`).
    pub events: Vec<Event>,
    /// Field accesses: the tree's, then those of macro expansions (`via_macro`).
    pub fields: Vec<Field>,
    /// Calls inside macro expansions, at the use (the tree's are the workspace's facts).
    pub macro_calls: Vec<MacroCall>,
    pub resolution: Arc<FileResolution>,
}

impl FileIndex {
    /// `path:line` of an offset.
    pub fn at(&self, offset: gsc_syntax::TextSize) -> String {
        format!("{}:{}", self.rel, self.lines.line(offset))
    }
}

/// The whole index: a workspace plus the index's own per-file facts, in file order.
pub struct Index<'a> {
    pub ws: &'a Workspace,
    pub files: Vec<FileIndex>,
    /// Position in `files` of each file id.
    pos: Vec<Option<usize>>,
}

impl Index<'_> {
    /// The indexed path of a workspace file.
    pub fn rel(&self, id: FileId) -> &str {
        self.pos
            .get(id.index())
            .copied()
            .flatten()
            .map_or("?", |i| self.files[i].rel.as_str())
    }
}

/// What a run did, for the command line summary.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub files: usize,
    pub bytes: usize,
    pub skipped_binary: usize,
    pub unreadable: Vec<String>,
    pub syntax_errors: usize,
    pub files_with_errors: usize,
    pub extract: Duration,
    pub resolve: Duration,
    pub jobs: usize,
}

/// A workspace over `roots`, each labelled with its last path component (`root<i>` without
/// one), read as `profile`.
pub fn workspace(
    roots: &[std::path::PathBuf],
    profile: Profile,
    decompiled: Decompiled,
    jobs: usize,
) -> Workspace {
    let mut ws = Workspace::new(profile)
        .with_decompiled(decompiled)
        .with_jobs(jobs);
    for (i, r) in roots.iter().enumerate() {
        let mut root = Root::new(r.clone());
        if r.file_name().is_none() {
            root.label = format!("root{i}");
        }
        ws.add_root(root);
    }
    ws
}

/// Load `ws` (read, parse, extract) and resolve it. The result does not depend on the thread
/// count.
pub fn build(ws: &mut Workspace) -> Stats {
    let start = Instant::now();
    let report = ws.load();
    let mut stats = Stats {
        jobs: ws.jobs(),
        skipped_binary: report.binary,
        unreadable: report.errors,
        extract: start.elapsed(),
        ..Stats::default()
    };
    for f in ws.files() {
        let data = ws.file(f);
        stats.files += 1;
        stats.bytes += data.text().len();
        stats.syntax_errors += data.errors.len();
        stats.files_with_errors += usize::from(!data.errors.is_empty());
    }
    let t = Instant::now();
    ws.resolve_all();
    stats.resolve = t.elapsed();
    stats
}

/// Index a loaded workspace: events and fields of every file, and what its macro uses expand
/// to, on its threads.
pub fn index(ws: &Workspace) -> Index<'_> {
    let ids: Vec<FileId> = ws.files().collect();
    let headers = macros::headers(ws);
    let several = ws.roots().len() > 1;
    let files = par_map(&ids, ws.jobs(), |&id| {
        let data = ws.file(id);
        let rel = if several {
            format!("{}/{}", ws.root(data.meta.root).label, data.meta.rel)
        } else {
            data.meta.rel.clone()
        };
        let lines = LineStarts::new(data.text());
        let cfg = ws.profile_of(id).config;
        let (mut events, mut fields) =
            extract::extract(&data.syntax(), &data.meta.key, &cfg, &lines, &data.facts);
        let ex = macros::expand(ws, id, &headers, &lines, &events, &fields);
        events.extend(ex.events);
        fields.extend(ex.fields);
        FileIndex {
            id,
            rel,
            lines,
            events,
            fields,
            macro_calls: ex.calls,
            resolution: ws.resolution(id),
        }
    });
    let mut pos = vec![None; ids.last().map_or(0, |l| l.index() + 1)];
    for (i, f) in files.iter().enumerate() {
        pos[f.id.index()] = Some(i);
    }
    Index { ws, files, pos }
}

/// A workspace of in-memory sources (`(path, text)`), one root, for tests and embedders.
pub fn workspace_from_sources(sources: &[(&str, &str)], profile: Profile) -> Workspace {
    let mut ws = Workspace::new(profile)
        .with_decompiled(Decompiled::Off)
        .with_jobs(1);
    let root = ws.add_root(Root::new("."));
    for (rel, text) in sources {
        ws.add_file(root, rel.into(), rel, text);
    }
    ws
}
