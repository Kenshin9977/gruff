//! Corpus gates, run by hand: `cargo test -p gsc_fmt --release --test corpus -- --ignored
//! --nocapture`. Everything happens in memory; the corpus is only read.
//!
//! `GSC_CORPUS` and `GRUFF_DATA` override the default locations (`../corpus` and `../gruff-data`
//! next to the gruff checkout). `GSC_FMT_STYLE=minimal` runs the minimal style.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use gsc_fmt::{format, lexemes, FormatError, Options, ParenSpacing, Style};
use gsc_sema::vfs::collect_files;
use gsc_syntax::{is_binary, looks_decompiled, Dialect, SourceText};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The style of the gates: `GSC_FMT_STYLE`, default full.
fn style() -> Style {
    std::env::var("GSC_FMT_STYLE")
        .ok()
        .and_then(|s| Style::from_name(&s))
        .unwrap_or(Style::Full)
}

/// Input lines that a line diff does not keep (deleted or rewritten).
fn diff_lines(a: &str, b: &str) -> usize {
    similar::TextDiff::from_lines(a, b)
        .iter_all_changes()
        .filter(|c| c.tag() == similar::ChangeTag::Delete)
        .count()
}

fn corpus_dir() -> PathBuf {
    std::env::var_os("GSC_CORPUS").map_or_else(|| root().join("corpus"), PathBuf::from)
}

/// `$GRUFF_DATA` or `$GSC_DATA`, else a `gruff-data` (or the older `gsc-data`) folder next to the
/// repository.
fn data_root() -> PathBuf {
    if let Some(d) = ["GRUFF_DATA", "GSC_DATA"]
        .iter()
        .find_map(|v| std::env::var_os(v).filter(|d| !d.is_empty()))
    {
        return PathBuf::from(d);
    }
    let up = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    ["gruff-data", "gsc-data"]
        .iter()
        .map(|n| up.join(n))
        .find(|d| d.is_dir())
        .unwrap_or_else(|| up.join("gruff-data"))
}

fn profiles_dir() -> PathBuf {
    data_root().join("profiles")
}

/// The profile of a corpus game folder.
fn profile_id(game: &str) -> &str {
    match game {
        "jup_sp" => "jup",
        // Call of Duty Online runs on the MW3 engine.
        "codol" => "iw5",
        g => g,
    }
}

/// The subfolders of `dir`, sorted, without the `_`-prefixed ones (notes, scratch).
fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir() && !p.file_name().unwrap().to_string_lossy().starts_with('_'))
        .collect();
    out.sort();
    out
}

#[derive(Default, Debug)]
struct Tally {
    files: usize,
    bytes: u64,
    lines: usize,
    changed_files: usize,
    /// Files with a change other than trailing blanks and the final newline.
    changed_files_beyond_blanks: usize,
    changed_lines: usize,
    /// Input lines a line diff rewrites (meaningful when line breaks move).
    diff_lines: usize,
    /// Changed lines by kind: indentation only, trailing blanks only, inside the line.
    by_kind: [usize; 3],
    samples: Vec<String>,
    skipped_syntax: usize,
    binary: usize,
    unsafe_: Vec<String>,
    not_idempotent: Vec<String>,
    token_mismatch: Vec<String>,
    secs: f64,
}

/// Lines of `a` that differ from the line at the same index of `b` (line breaks are kept, so
/// the lines align).
fn changed_lines(a: &str, b: &str, t: &mut Tally) -> usize {
    let mut lb = b.lines();
    let mut n = 0;
    for l in a.lines() {
        let m = lb.next().unwrap_or("");
        if l == m {
            continue;
        }
        n += 1;
        let k = if l.trim_end() == m {
            1
        } else if l.trim_start() == m.trim_start() {
            0
        } else {
            2
        };
        t.by_kind[k] += 1;
        let wanted = std::env::var("GSC_FMT_SAMPLES").is_ok_and(|v| v == k.to_string());
        if t.samples.len() < 40 && wanted {
            t.samples.push(format!("{l:?}\n      {m:?}"));
        }
    }
    n
}

/// Format every file of `dir` as `game` in memory. With `check`, also format the output again
/// and compare the token streams.
fn run_dir(dir: &Path, game: &str, opts: Option<Options>, check: bool) -> Tally {
    let dialect = Dialect::find(&profiles_dir(), profile_id(game))
        .unwrap_or_else(|e| panic!("profile for {game}: {e}"));
    let dialect_dec = dialect.clone().with_decompiled(true);
    let opts = opts.unwrap_or_else(|| Options::for_game(profile_id(game)).with_style(style()));
    let mut files = Vec::new();
    // A missing folder formats nothing; the tally then shows zero files.
    let _ = collect_files(dir, &mut files);
    let next = AtomicUsize::new(0);
    let total = Mutex::new(Tally::default());
    let start = Instant::now();
    let jobs = std::thread::available_parallelism().map_or(1, |n| n.get());
    std::thread::scope(|s| {
        for _ in 0..jobs {
            std::thread::Builder::new()
                .stack_size(64 << 20)
                .spawn_scoped(s, || {
                    let mut t = Tally::default();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(path) = files.get(i) else { break };
                        let bytes = std::fs::read(path).expect("read");
                        if is_binary(&bytes) {
                            t.binary += 1;
                            continue;
                        }
                        let src = SourceText::decode(&bytes);
                        let d = if looks_decompiled(&src.text) {
                            &dialect_dec
                        } else {
                            &dialect
                        };
                        t.files += 1;
                        t.bytes += bytes.len() as u64;
                        t.lines += src.text.lines().count();
                        let name = path.display().to_string();
                        let out = match format(&src.text, d, &opts) {
                            Ok(out) => out,
                            Err(FormatError::Syntax(_)) => {
                                t.skipped_syntax += 1;
                                continue;
                            }
                            Err(FormatError::Unsafe(m)) => {
                                t.unsafe_.push(format!("{name}: {m}"));
                                continue;
                            }
                        };
                        if out != src.text {
                            t.changed_files += 1;
                            let before = t.by_kind[0] + t.by_kind[2];
                            t.changed_lines += changed_lines(&src.text, &out, &mut t);
                            t.diff_lines += diff_lines(&src.text, &out);
                            if t.by_kind[0] + t.by_kind[2] > before {
                                t.changed_files_beyond_blanks += 1;
                            }
                        }
                        if check {
                            if lexemes(&src.text, d) != lexemes(&out, d) {
                                t.token_mismatch.push(name.clone());
                            }
                            match format(&out, d, &opts) {
                                Ok(again) if again == out => {}
                                _ => t.not_idempotent.push(name),
                            }
                        }
                    }
                    let mut total = total.lock().unwrap();
                    total.files += t.files;
                    total.bytes += t.bytes;
                    total.lines += t.lines;
                    total.changed_files += t.changed_files;
                    total.changed_files_beyond_blanks += t.changed_files_beyond_blanks;
                    total.changed_lines += t.changed_lines;
                    total.diff_lines += t.diff_lines;
                    total.skipped_syntax += t.skipped_syntax;
                    for k in 0..3 {
                        total.by_kind[k] += t.by_kind[k];
                    }
                    total.samples.extend(t.samples);
                    total.binary += t.binary;
                    total.unsafe_.extend(t.unsafe_);
                    total.not_idempotent.extend(t.not_idempotent);
                    total.token_mismatch.extend(t.token_mismatch);
                })
                .expect("spawn");
        }
    });
    let mut t = total.into_inner().unwrap();
    t.secs = start.elapsed().as_secs_f64();
    t
}

fn pct(a: usize, b: usize) -> f64 {
    if b == 0 {
        0.0
    } else {
        100.0 * a as f64 / b as f64
    }
}

fn print(label: &str, t: &Tally) {
    println!(
        "{label:<24} files {:>5}  skipped(syntax) {:>3}  changed files {:>5} ({:>5.1}%)  changed lines {:>7}/{:>8} ({:>5.2}%)  unsafe {}  non-idempotent {}  token-mismatch {}  {:.1} MB/s",
        t.files,
        t.skipped_syntax,
        t.changed_files,
        pct(t.changed_files, t.files - t.skipped_syntax),
        t.changed_lines,
        t.lines,
        pct(t.changed_lines, t.lines),
        t.unsafe_.len(),
        t.not_idempotent.len(),
        t.token_mismatch.len(),
        t.bytes as f64 / 1e6 / t.secs,
    );
    println!(
        "    lines rewritten (line diff): {} of {} ({:.1}%)",
        t.diff_lines,
        t.lines,
        pct(t.diff_lines, t.lines)
    );
    println!(
        "    changed lines: indentation {}, trailing blanks {}, inside the line {}; files changed beyond trailing blanks {} ({:.1}%)",
        t.by_kind[0],
        t.by_kind[1],
        t.by_kind[2],
        t.changed_files_beyond_blanks,
        pct(t.changed_files_beyond_blanks, t.files - t.skipped_syntax),
    );
    if std::env::var_os("GSC_FMT_SAMPLES").is_some() {
        for s in &t.samples {
            println!("    {s}");
        }
    }
    for m in t
        .unsafe_
        .iter()
        .chain(&t.not_idempotent)
        .chain(&t.token_mismatch)
        .take(5)
    {
        println!("    {m}");
    }
}

/// Release gate: every file of every game formats to a token-equivalent, idempotent result.
#[test]
#[ignore = "needs the local corpus; run with --ignored"]
fn corpus_is_idempotent_and_token_equivalent() {
    let corpus = corpus_dir();
    let mut failures = 0;
    for game_dir in subdirs(&corpus) {
        let game = game_dir.file_name().unwrap().to_string_lossy().into_owned();
        for sub in subdirs(&game_dir) {
            let label = format!("{game}/{}", sub.file_name().unwrap().to_string_lossy());
            let t = run_dir(&sub, &game, None, true);
            print(&label, &t);
            failures += t.unsafe_.len() + t.not_idempotent.len() + t.token_mismatch.len();
        }
    }
    assert_eq!(failures, 0);
}

/// Debug one file: `GSC_FMT_FILE=path GSC_FMT_GAME=t4`. Prints the diff of the first pass and,
/// if any, the diff of the second.
#[test]
#[ignore = "debugging aid"]
fn one_file() {
    let Ok(path) = std::env::var("GSC_FMT_FILE") else {
        return;
    };
    let game = std::env::var("GSC_FMT_GAME").unwrap_or_else(|_| "t7".to_owned());
    let d = Dialect::find(&profiles_dir(), profile_id(&game)).unwrap();
    let src = SourceText::decode(&std::fs::read(&path).unwrap());
    let d = d.with_decompiled(looks_decompiled(&src.text));
    let opts = Options::for_game(profile_id(&game)).with_style(style());
    let once = format(&src.text, &d, &opts).unwrap();
    println!("{}", gsc_fmt::unified_diff(&path, &src.text, &once));
    let twice = format(&once, &d, &opts).unwrap();
    println!(
        "second pass:\n{}",
        gsc_fmt::unified_diff(&path, &once, &twice)
    );
}

/// How much the default style changes Treyarch's own BO3 sources (lower is better).
/// `GSC_FMT_PAREN=compact|padded|preserve` measures another parenthesis style.
#[test]
#[ignore = "needs the local corpus; run with --ignored"]
fn measure_bo3_modtools() {
    let mut opts = Options::for_game("t7");
    if let Some(p) = std::env::var("GSC_FMT_PAREN")
        .ok()
        .and_then(|p| ParenSpacing::from_name(&p))
    {
        opts.paren_spacing = p;
    }
    let dirs = std::env::var("GSC_FMT_DIRS").unwrap_or_else(|_| "t7/source-modtools".to_owned());
    for d in dirs.split(',') {
        let game = d.split('/').next().unwrap();
        let mut o = opts;
        if std::env::var("GSC_FMT_PAREN").is_err() {
            o = Options::for_game(profile_id(game));
        }
        let t = run_dir(&corpus_dir().join(d), game, Some(o), false);
        print(d, &t);
    }
}

/// Style counters over the CST of one file (see `measure_style`).
#[derive(Default, Clone)]
struct StyleCounts {
    c: std::collections::BTreeMap<&'static str, [usize; 2]>,
}

impl StyleCounts {
    fn add(&mut self, key: &'static str, yes: bool) {
        self.c.entry(key).or_default()[usize::from(!yes)] += 1;
    }
}

fn measure_file(text: &str, d: &Dialect, s: &mut StyleCounts) {
    use gsc_syntax::{SyntaxKind::*, SyntaxToken};
    let parse = gsc_syntax::parse(text, d);
    if parse.errors().iter().any(|e| e.is_syntax()) {
        return;
    }
    let ws = |t: Option<SyntaxToken>| t.filter(|t| t.kind() == WHITESPACE);
    let nl_before = |t: &SyntaxToken| ws(t.prev_token()).is_some_and(|w| w.text().contains('\n'));
    let sp_before = |t: &SyntaxToken| ws(t.prev_token()).is_some_and(|w| !w.text().contains('\n'));
    let sp_after = |t: &SyntaxToken| ws(t.next_token()).is_some_and(|w| !w.text().contains('\n'));
    let nl_after = |t: &SyntaxToken| ws(t.next_token()).is_some_and(|w| w.text().contains('\n'));
    let control = |k| {
        matches!(
            k,
            IF_STMT
                | WHILE_STMT
                | FOR_STMT
                | FOREACH_STMT
                | SWITCH_STMT
                | DO_WHILE_STMT
                | ELSE_BRANCH
        )
    };
    for line in text.lines() {
        let w: usize = line.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum();
        s.add("line>80", w > 80);
        s.add("line>100", w > 100);
        s.add("line>120", w > 120);
    }
    for el in parse.syntax().descendants_with_tokens() {
        let Some(t) = el.into_token() else { continue };
        let Some(p) = t.parent() else { continue };
        match t.kind() {
            L_CURLY if p.kind() == BLOCK => {
                let gp = p.parent().map(|g| g.kind());
                match gp {
                    Some(FUNCTION_DEF | CONSTRUCTOR | DESTRUCTOR) => {
                        s.add("fn { on next line", nl_before(&t))
                    }
                    Some(k) if control(k) => s.add("control { on next line", nl_before(&t)),
                    _ => {}
                }
            }
            L_CURLY if p.kind() == SWITCH_STMT => s.add("switch { on next line", nl_before(&t)),
            ELSE_KW => {
                if t.prev_token()
                    .and_then(|w| {
                        if w.kind() == WHITESPACE {
                            w.prev_token()
                        } else {
                            Some(w)
                        }
                    })
                    .is_some_and(|x| x.kind() == R_CURLY)
                {
                    s.add("} else cuddled", !nl_before(&t));
                }
                if let Some(n) = p.children().next() {
                    if n.kind() == IF_STMT {
                        s.add(
                            "else if on one line",
                            !n.first_token().is_some_and(|f| nl_before(&f)),
                        );
                    }
                }
            }
            IF_KW | WHILE_KW | FOR_KW | FOREACH_KW | SWITCH_KW if control(p.kind()) => {
                if t.next_token()
                    .is_some_and(|n| n.kind() == WHITESPACE || n.kind() == L_PAREN)
                {
                    s.add("keyword space before (", sp_after(&t));
                }
            }
            R_PAREN
                if control(p.kind()) && p.kind() != SWITCH_STMT && p.kind() != DO_WHILE_STMT =>
            {
                let body = p.children().last();
                if let Some(b) = body.filter(|b| b.kind() != BLOCK && b.kind() != ELSE_BRANCH) {
                    if let Some(f) = b.first_token() {
                        s.add("unbraced body on next line", nl_before(&f) || nl_after(&t));
                    }
                }
            }
            L_BRACK if p.kind() == INDEX_EXPR => s.add("index [ padded", sp_after(&t)),
            L_BRACK if p.kind() == ARRAY_EXPR => {
                if t.next_token().is_some_and(|n| n.kind() != R_BRACK) {
                    s.add("array [ padded", sp_after(&t));
                }
            }
            L_PAREN if p.kind() == PAREN_EXPR => s.add("paren expr ( padded", sp_after(&t)),
            L_PAREN if p.kind() == VECTOR_EXPR => s.add("vector ( padded", sp_after(&t)),
            L_PAREN if p.kind() == ARG_LIST => {
                if t.next_token().is_some_and(|n| n.kind() != R_PAREN) {
                    s.add("call ( padded", sp_after(&t));
                }
            }
            R_PAREN if p.kind() == ARG_LIST => {
                if p.text().to_string().contains('\n') {
                    s.add("multi-line args: ) on own line", nl_before(&t));
                }
            }
            k if p.kind() == BIN_EXPR && k.is_punct() => {
                if nl_after(&t) || nl_before(&t) {
                    let logical = matches!(k, AMP2 | PIPE2);
                    s.add(
                        if logical {
                            "broken && ||: op at line end"
                        } else {
                            "broken other op: op at line end"
                        },
                        nl_after(&t),
                    );
                }
                let _ = sp_before;
            }
            _ => {}
        }
    }
}

/// Style measurements per corpus folder: brace placement, spacing, line widths.
#[test]
#[ignore = "needs the local corpus; run with --ignored"]
fn measure_style() {
    let corpus = corpus_dir();
    for game_dir in subdirs(&corpus) {
        let game = game_dir.file_name().unwrap().to_string_lossy().into_owned();
        let dialect = Dialect::find(&profiles_dir(), profile_id(&game)).unwrap();
        let dec = dialect.clone().with_decompiled(true);
        for sub in subdirs(&game_dir) {
            let mut files = Vec::new();
            let _ = collect_files(&sub, &mut files);
            let mut s = StyleCounts::default();
            for f in &files {
                let bytes = std::fs::read(f).unwrap();
                if is_binary(&bytes) {
                    continue;
                }
                let src = SourceText::decode(&bytes);
                let d = if looks_decompiled(&src.text) {
                    &dec
                } else {
                    &dialect
                };
                measure_file(&src.text, d, &mut s);
            }
            println!("== {game}/{}", sub.file_name().unwrap().to_string_lossy());
            for (k, [y, n]) in &s.c {
                println!(
                    "   {k:<34} {:>7} / {:>7}  {:>5.1}%",
                    y,
                    y + n,
                    pct(*y, y + n)
                );
            }
        }
    }
}
