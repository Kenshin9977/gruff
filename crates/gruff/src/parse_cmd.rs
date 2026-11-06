//! `gruff parse`: parse files or directories, print diagnostics and a corpus summary.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gsc_sema::par_map;
use gsc_sema::vfs::collect_files;
use gsc_syntax::{
    check_dialect, header_paths, is_binary, looks_decompiled, macro_expansions_with, parse,
    Dialect, HeaderSet, LineIndex, MacroTable, Mode, SourceText,
};

/// Options of `gruff parse`.
pub struct Options {
    pub paths: Vec<PathBuf>,
    pub game: Option<String>,
    pub profile: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
    pub tree: bool,
    pub stats: bool,
    pub quiet: bool,
    pub max_per_file: usize,
    pub decompiled: Decompiled,
    pub jobs: usize,
    /// Retail (the default) or community compiler semantics.
    pub mode: Mode,
}

/// Whether inputs are decompiler output (names may then hold `/`, `\`, `.` or a leading digit).
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Decompiled {
    /// Decide per file from the decompiler's header comments.
    Auto,
    On,
    Off,
}

/// Find a gruff-data `profiles` directory: `--data-dir`, then `$GRUFF_DATA` or `$GSC_DATA`, then a
/// `gruff-data` or `gsc-data` folder next to the working directory or the executable, or one of
/// their ancestors.
pub fn profiles_dir(data_dir: Option<&Path>) -> Option<PathBuf> {
    gsc_sema::data::find_profiles_dir(data_dir, &[])
}

/// The dialect named by the options (default `t7`).
pub fn load_dialect(
    game: Option<&str>,
    profile: Option<&Path>,
    data_dir: Option<&Path>,
) -> Result<Dialect, String> {
    if let Some(p) = profile {
        return Dialect::load(p).map_err(|e| e.to_string());
    }
    let game = game.unwrap_or("t7");
    if game == "superset" {
        return Ok(Dialect::superset());
    }
    let dir = profiles_dir(data_dir).ok_or_else(|| {
        "cannot find gruff-data/profiles; pass --data-dir, --profile or set GRUFF_DATA".to_owned()
    })?;
    Dialect::find(&dir, game).map_err(|e| e.to_string())
}

#[derive(Default)]
struct Totals {
    files: usize,
    bytes: u64,
    files_with_errors: usize,
    files_with_dialect: usize,
    syntax_errors: usize,
    dialect_diags: usize,
    round_trip_failures: usize,
    unreadable: usize,
    binary: usize,
    decompiled: usize,
    /// Files whose syntax errors are all E0301 (decompiler names no tokenizer can read).
    decompiler_name_files: usize,
    macro_defs: usize,
    macro_uses: usize,
    macro_files: usize,
    macro_incomplete: usize,
    headers_wanted: usize,
    headers_found: usize,
    by_code: BTreeMap<String, usize>,
    by_message: BTreeMap<String, usize>,
    parse_time: Duration,
}

/// What one file contributes: its printed lines and its counts.
#[derive(Default)]
struct Outcome {
    out: String,
    totals: Totals,
}

impl Totals {
    fn add(&mut self, o: Totals) {
        self.files += o.files;
        self.bytes += o.bytes;
        self.files_with_errors += o.files_with_errors;
        self.files_with_dialect += o.files_with_dialect;
        self.syntax_errors += o.syntax_errors;
        self.dialect_diags += o.dialect_diags;
        self.round_trip_failures += o.round_trip_failures;
        self.unreadable += o.unreadable;
        self.binary += o.binary;
        self.decompiled += o.decompiled;
        self.decompiler_name_files += o.decompiler_name_files;
        self.macro_defs += o.macro_defs;
        self.macro_uses += o.macro_uses;
        self.macro_files += o.macro_files;
        self.macro_incomplete += o.macro_incomplete;
        self.headers_wanted += o.headers_wanted;
        self.headers_found += o.headers_found;
        for (k, v) in o.by_code {
            *self.by_code.entry(k).or_default() += v;
        }
        for (k, v) in o.by_message {
            *self.by_message.entry(k).or_default() += v;
        }
        self.parse_time += o.parse_time;
    }
}

struct Ctx<'a> {
    opts: &'a Options,
    dialect: &'a Dialect,
    dialect_dec: &'a Dialect,
    headers: &'a HeaderSet,
}

/// The `.gsh` headers among the inputs, for the macro statistics.
fn load_headers(files: &[PathBuf], dialect: &Dialect) -> HeaderSet {
    let mut set = HeaderSet::new();
    for f in files {
        if !f.extension().is_some_and(|x| x.eq_ignore_ascii_case("gsh")) {
            continue;
        }
        let Ok(bytes) = std::fs::read(f) else {
            continue;
        };
        let src = SourceText::decode(&bytes);
        let p = parse(&src.text, dialect);
        set.add(&f.to_string_lossy(), &p.syntax(), dialect);
    }
    set
}

fn process(ctx: &Ctx, path: &Path) -> Outcome {
    use std::fmt::Write as _;
    let opts = ctx.opts;
    let mut o = Outcome::default();
    let t = &mut o.totals;
    let out = &mut o.out;
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            let _ = writeln!(out, "{}: error: {e}", path.display());
            t.unreadable += 1;
            return o;
        }
    };
    if is_binary(&bytes) {
        t.binary += 1;
        if !opts.quiet {
            let _ = writeln!(out, "{}: skipped: compiled (binary) script", path.display());
        }
        return o;
    }
    t.files += 1;
    t.bytes += bytes.len() as u64;
    let start = Instant::now();
    let src = SourceText::decode(&bytes);
    let decompiled = match opts.decompiled {
        Decompiled::On => true,
        Decompiled::Off => false,
        Decompiled::Auto => looks_decompiled(&src.text),
    };
    t.decompiled += usize::from(decompiled);
    let d = if decompiled {
        ctx.dialect_dec
    } else {
        ctx.dialect
    };
    let parsed = parse(&src.text, d);
    let root = parsed.syntax();
    let dialect_diags = check_dialect(&root, d);
    t.parse_time += start.elapsed();
    if opts.stats {
        let wanted = header_paths(&root);
        t.headers_wanted += wanted.len();
        t.headers_found += wanted.iter().filter(|w| ctx.headers.contains(w)).count();
        let table = ctx.headers.macros_for(&wanted);
        let own = MacroTable::from_tree(&root, d);
        t.macro_defs += own.len();
        if !table.is_empty() || !own.is_empty() {
            let ex = macro_expansions_with(&root, &table, d);
            t.macro_files += usize::from(!ex.is_empty());
            t.macro_uses += ex.len();
            t.macro_incomplete += ex.iter().filter(|e| !e.complete).count();
        }
    }

    if src.encode(&parsed.text()) != bytes {
        t.round_trip_failures += 1;
        let _ = writeln!(
            out,
            "{}:1:1: E0900 the tree does not round-trip",
            path.display()
        );
    }
    if !parsed.errors().is_empty() {
        t.files_with_errors += 1;
        if parsed.errors().iter().all(|e| e.is_decompiler_name()) {
            t.decompiler_name_files += 1;
        }
    }
    if !dialect_diags.is_empty() {
        t.files_with_dialect += 1;
    }
    t.syntax_errors += parsed.errors().len();
    t.dialect_diags += dialect_diags.len();

    let mut diags: Vec<_> = parsed.errors().iter().chain(&dialect_diags).collect();
    diags.sort_by_key(|d| d.range.start());
    let index = (!diags.is_empty()).then(|| LineIndex::new(&src.text));
    for (i, d) in diags.iter().enumerate() {
        *t.by_code.entry(d.code.to_owned()).or_default() += 1;
        if opts.stats {
            *t.by_message
                .entry(format!("{} {}", d.code, normalize(&d.message)))
                .or_default() += 1;
        }
        if opts.quiet || i >= opts.max_per_file {
            continue;
        }
        let lc = index
            .as_ref()
            .map(|ix| ix.line_col(&src.text, d.range.start()));
        let (line, col) = lc.map_or((1, 1), |lc| (lc.line, lc.col));
        let _ = writeln!(
            out,
            "{}:{line}:{col}: {} {}",
            path.display(),
            d.code,
            d.message
        );
    }
    if !opts.quiet && diags.len() > opts.max_per_file {
        let more = diags.len() - opts.max_per_file;
        let _ = writeln!(out, "{}: ... {more} more diagnostics", path.display());
    }
    if opts.tree {
        out.push_str(&parsed.debug_tree());
    }
    o
}

pub fn run(opts: &Options) -> Result<bool, String> {
    let dialect = load_dialect(
        opts.game.as_deref(),
        opts.profile.as_deref(),
        opts.data_dir.as_deref(),
    )?
    .with_mode(opts.mode);
    let dialect_dec = dialect.clone().with_decompiled(true);
    let mut files = Vec::new();
    for p in &opts.paths {
        collect_files(p, &mut files).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    let headers = if opts.stats {
        load_headers(&files, &dialect)
    } else {
        HeaderSet::new()
    };
    let ctx = Ctx {
        opts,
        dialect: &dialect,
        dialect_dec: &dialect_dec,
        headers: &headers,
    };
    let jobs = opts.jobs.max(1).min(files.len().max(1));
    let wall = Instant::now();
    let outcomes = parallel(&ctx, &files, jobs);
    let wall = wall.elapsed();

    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    let mut t = Totals::default();
    for o in outcomes {
        let _ = out.write_all(o.out.as_bytes());
        t.add(o.totals);
    }
    summary(&mut out, &t, &dialect, opts.stats, wall, jobs);
    Ok(t.syntax_errors == 0 && t.round_trip_failures == 0 && t.unreadable == 0)
}

/// Parse on `jobs` threads; the outcomes come back in file order.
fn parallel(ctx: &Ctx, files: &[PathBuf], jobs: usize) -> Vec<Outcome> {
    par_map(files, jobs, |f| process(ctx, f))
}

/// Strip the token text out of a message so that the stats group by error shape.
fn normalize(message: &str) -> String {
    match message.find(", found") {
        Some(i) => message[..i].to_owned(),
        None => message.to_owned(),
    }
}

fn pct(n: usize, d: usize) -> f64 {
    if d == 0 {
        0.0
    } else {
        100.0 * n as f64 / d as f64
    }
}

fn summary(
    out: &mut impl Write,
    t: &Totals,
    dialect: &Dialect,
    stats: bool,
    wall: Duration,
    jobs: usize,
) {
    let mode = match dialect.mode() {
        Mode::Retail => "",
        Mode::Community => ", community mode",
    };
    let _ = writeln!(
        out,
        "\n{} files parsed as {} ({}{mode}): {} with syntax errors ({:.2}% error rate), {} syntax \
         errors",
        t.files,
        dialect.id,
        dialect.name,
        t.files_with_errors,
        pct(t.files_with_errors, t.files),
        t.syntax_errors
    );
    if t.decompiler_name_files > 0 {
        let _ = writeln!(
            out,
            "{} of them only E0301 (decompiler names no tokenizer can read): {:.2}% clean without \
             them",
            t.decompiler_name_files,
            100.0 - pct(t.files_with_errors - t.decompiler_name_files, t.files)
        );
    }
    let _ = writeln!(
        out,
        "dialect: {} diagnostics in {} files ({:.2}%); round-trip: {}/{} files exact",
        t.dialect_diags,
        t.files_with_dialect,
        pct(t.files_with_dialect, t.files),
        t.files - t.round_trip_failures,
        t.files
    );
    if t.decompiled > 0 {
        let _ = writeln!(out, "{} files read as decompiler output", t.decompiled);
    }
    if t.binary > 0 {
        let _ = writeln!(out, "{} compiled (binary) files skipped", t.binary);
    }
    if t.unreadable > 0 {
        let _ = writeln!(out, "{} files could not be read", t.unreadable);
    }
    if stats {
        let secs = t.parse_time.as_secs_f64().max(1e-9);
        let wall_secs = wall.as_secs_f64().max(1e-9);
        let _ = writeln!(
            out,
            "time: {:.3} s decode + lex + parse + check per thread: {:.0} files/s, {:.1} MB/s",
            t.parse_time.as_secs_f64(),
            t.files as f64 / secs,
            t.bytes as f64 / 1e6 / secs
        );
        let _ = writeln!(
            out,
            "wall: {:.3} s on {jobs} thread(s) with file reads and round-trip: {:.0} files/s",
            wall_secs,
            t.files as f64 / wall_secs
        );
        let _ = writeln!(
            out,
            "macros: {} #define in the inputs, {} uses expanded in {} files ({} incomplete); \
             #insert/#inline headers found: {}/{}",
            t.macro_defs,
            t.macro_uses,
            t.macro_files,
            t.macro_incomplete,
            t.headers_found,
            t.headers_wanted
        );
        for (code, n) in &t.by_code {
            let _ = writeln!(out, "  {code}: {n}");
        }
        let mut by_message: Vec<_> = t.by_message.iter().collect();
        by_message.sort_by(|a, b| b.1.cmp(a.1));
        for (m, n) in by_message.into_iter().take(30) {
            let _ = writeln!(out, "  {n:>7}  {m}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../gsc_syntax/tests/data")
            .join(rel)
    }

    fn options() -> Options {
        Options {
            paths: Vec::new(),
            game: Some("superset".to_owned()),
            profile: None,
            data_dir: None,
            tree: false,
            stats: true,
            quiet: false,
            max_per_file: 50,
            decompiled: Decompiled::Auto,
            jobs: 1,
            mode: Mode::Retail,
        }
    }

    #[test]
    fn reports_in_ruff_format() {
        let opts = options();
        let d = Dialect::superset();
        let dd = d.clone().with_decompiled(true);
        let headers = HeaderSet::new();
        let ctx = Ctx {
            opts: &opts,
            dialect: &d,
            dialect_dec: &dd,
            headers: &headers,
        };
        let ok = process(&ctx, &fixture("ok/functions.gsc"));
        assert_eq!(ok.out, "");
        assert_eq!(ok.totals.files, 1);
        assert_eq!(ok.totals.round_trip_failures, 0);

        let err = process(&ctx, &fixture("err/missing_semicolon.gsc"));
        let first = err.out.lines().next().unwrap();
        assert!(
            first.ends_with("missing_semicolon.gsc:4:2: E0201 expected `;`, found `b`"),
            "{first}"
        );
        assert_eq!(err.totals.files_with_errors, 1);
    }

    #[test]
    fn parallel_keeps_file_order() {
        let opts = options();
        let d = Dialect::superset();
        let dd = d.clone().with_decompiled(true);
        let headers = HeaderSet::new();
        let ctx = Ctx {
            opts: &opts,
            dialect: &d,
            dialect_dec: &dd,
            headers: &headers,
        };
        let mut files = Vec::new();
        collect_files(&fixture("err"), &mut files).unwrap();
        let seq: Vec<String> = files.iter().map(|f| process(&ctx, f).out).collect();
        let par: Vec<String> = parallel(&ctx, &files, 3)
            .into_iter()
            .map(|o| o.out)
            .collect();
        assert_eq!(seq, par);
    }
}
