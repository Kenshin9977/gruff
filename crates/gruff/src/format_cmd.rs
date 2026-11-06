//! `gruff format`: reformat in place (full style by default, `--style minimal` for whitespace
//! only), or check and diff.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use gsc_fmt::{FormatConfig, FormatError, Options, Style};
use gsc_sema::vfs::collect_files;
use gsc_sema::{default_jobs, par_map};
use gsc_syntax::{is_binary, looks_decompiled, Dialect, LineIndex, SourceText};

/// Arguments of `gruff format`.
#[derive(clap::Args)]
pub struct Args {
    /// Files or directories (searched for .gsc, .csc and .gsh). `-` reads standard input and
    /// writes the result to standard output.
    #[arg(required_unless_present = "stdin_filename")]
    pub paths: Vec<PathBuf>,
    /// Write nothing; list the files that would change and exit 1 if there are any.
    #[arg(long)]
    pub check: bool,
    /// Write nothing; print a unified diff of every change (exit 1 if there are any).
    #[arg(long)]
    pub diff: bool,
    /// Read standard input as this file (for the config lookup and messages); the result goes
    /// to standard output.
    #[arg(long)]
    pub stdin_filename: Option<PathBuf>,
    /// Game id or alias of the dialect (`t7`, `bo3`, `iw5`...). Default: `game` in gruff.toml, or
    /// `t7`. Also picks the default style of the engine family.
    #[arg(long)]
    pub game: Option<String>,
    /// Profile file to use instead of looking the game up in gruff-data.
    #[arg(long)]
    pub profile: Option<PathBuf>,
    /// The gruff-data directory (default: `$GRUFF_DATA` or `$GSC_DATA`, else a
    /// `gruff-data` or `gsc-data` folder found upwards).
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
    /// `full` reprints with line breaking at `--max-width`; `minimal` keeps every line break and
    /// only normalises whitespace. Default: `format.style` in gruff.toml, or `full`.
    #[arg(long, value_parser = ["full", "minimal"])]
    pub style: Option<String>,
    /// The line width of the full style (default: `format.max_width` in gruff.toml, or 100).
    #[arg(long, value_parser = clap::value_parser!(u16).range(20..=1000))]
    pub max_width: Option<u16>,
    /// The gruff.toml to read (default: the nearest one above the first path).
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Print only the summary and errors.
    #[arg(long, short)]
    pub quiet: bool,
    /// Worker threads (default: every core). The output order does not depend on it.
    #[arg(long, short)]
    pub jobs: Option<usize>,
}

/// Everything a worker needs.
struct Ctx {
    dialect: Dialect,
    dialect_dec: Dialect,
    opts: Options,
    write: bool,
    diff: bool,
}

/// What happened to one file.
enum Outcome {
    Unchanged,
    Changed { diff: String },
    Skipped(String),
    Failed(String),
}

/// Format `text`; `name` is only used in messages and diff headers.
fn format_text(ctx: &Ctx, name: &str, text: &str) -> (Outcome, Option<String>) {
    let d = if looks_decompiled(text) {
        &ctx.dialect_dec
    } else {
        &ctx.dialect
    };
    match gsc_fmt::format(text, d, &ctx.opts) {
        Ok(out) if out == text => (Outcome::Unchanged, Some(out)),
        Ok(out) => {
            let diff = if ctx.diff {
                gsc_fmt::unified_diff(&name.replace('\\', "/"), text, &out)
            } else {
                String::new()
            };
            (Outcome::Changed { diff }, Some(out))
        }
        Err(FormatError::Syntax(errors)) => {
            let idx = LineIndex::new(text);
            let first = &errors[0];
            let lc = idx.line_col(text, first.range.start());
            let msg = format!(
                "skipped: {} syntax error(s), first at {}:{}: {} {}",
                errors.len(),
                lc.line,
                lc.col,
                first.code,
                first.message
            );
            (Outcome::Skipped(msg), None)
        }
        Err(e @ FormatError::Unsafe(_)) => (Outcome::Failed(e.to_string()), None),
    }
}

fn process(ctx: &Ctx, path: &Path) -> Outcome {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return Outcome::Failed(format!("cannot read: {e}")),
    };
    if is_binary(&bytes) {
        return Outcome::Skipped("skipped: compiled (binary) script".to_owned());
    }
    let src = SourceText::decode(&bytes);
    let (outcome, out) = format_text(ctx, &path.display().to_string(), &src.text);
    if let (Outcome::Changed { .. }, Some(out), true) = (&outcome, &out, ctx.write) {
        if let Err(e) = write_atomic(path, &src.encode(out)) {
            return Outcome::Failed(format!("cannot write: {e}"));
        }
    }
    outcome
}

/// Write through a temporary file next to `path`, then rename it over `path`.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".gsc-format.tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

fn load_config(args: &Args) -> Result<FormatConfig, String> {
    let path = match &args.config {
        Some(p) => Some(p.clone()),
        None => {
            let start = args
                .stdin_filename
                .as_ref()
                .or(args.paths.iter().find(|p| p.as_os_str() != "-"))
                .map(|p| std::path::absolute(p).unwrap_or_else(|_| p.clone()));
            let dir = match start {
                Some(p) if p.is_dir() => Some(p),
                Some(p) => p.parent().map(Path::to_path_buf),
                None => std::env::current_dir().ok(),
            };
            dir.and_then(|d| gsc_fmt::find_config(&d))
        }
    };
    match path {
        Some(p) => FormatConfig::load(&p).map_err(|e| e.to_string()),
        None => Ok(FormatConfig::default()),
    }
}

fn build_ctx(args: &Args) -> Result<Ctx, String> {
    let config = load_config(args)?;
    let game = args.game.clone().or(config.game.clone());
    let dialect = crate::parse_cmd::load_dialect(
        game.as_deref(),
        args.profile.as_deref(),
        args.data_dir.as_deref(),
    )?;
    let mut opts = config.apply(Options::for_game(&dialect.id));
    if let Some(style) = args.style.as_deref().and_then(Style::from_name) {
        opts.style = style;
    }
    if let Some(w) = args.max_width {
        opts.max_width = w;
    }
    Ok(Ctx {
        dialect_dec: dialect.clone().with_decompiled(true),
        dialect,
        opts,
        write: !args.check && !args.diff,
        diff: args.diff,
    })
}

fn run_stdin(ctx: &Ctx, args: &Args) -> ExitCode {
    let name = args
        .stdin_filename
        .as_ref()
        .map_or_else(|| "<stdin>".to_owned(), |p| p.display().to_string());
    let mut bytes = Vec::new();
    if let Err(e) = std::io::stdin().read_to_end(&mut bytes) {
        eprintln!("error: cannot read standard input: {e}");
        return ExitCode::from(2);
    }
    let src = SourceText::decode(&bytes);
    let (outcome, out) = format_text(ctx, &name, &src.text);
    let mut stdout = std::io::stdout().lock();
    match outcome {
        Outcome::Unchanged | Outcome::Changed { .. } if ctx.write => {
            let out = out.expect("formatted text");
            let _ = stdout.write_all(&src.encode(&out));
            ExitCode::SUCCESS
        }
        Outcome::Unchanged => ExitCode::SUCCESS,
        Outcome::Changed { diff } => {
            if args.check && !args.quiet {
                let _ = writeln!(stdout, "would reformat: {name}");
            }
            let _ = stdout.write_all(diff.as_bytes());
            ExitCode::from(1)
        }
        Outcome::Skipped(msg) => {
            eprintln!("{name}: {msg}");
            if ctx.write {
                // Pass the input through, so editors never lose text.
                let _ = stdout.write_all(&bytes);
            }
            ExitCode::SUCCESS
        }
        Outcome::Failed(msg) => {
            eprintln!("{name}: error: {msg}");
            if ctx.write {
                let _ = stdout.write_all(&bytes);
            }
            ExitCode::from(2)
        }
    }
}

pub fn run(args: Args) -> ExitCode {
    let ctx = match build_ctx(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let stdin = args.stdin_filename.is_some() || args.paths.iter().any(|p| p.as_os_str() == "-");
    if stdin {
        if args.paths.iter().any(|p| p.as_os_str() != "-") {
            eprintln!("error: standard input (`-` or --stdin-filename) cannot be mixed with paths");
            return ExitCode::from(2);
        }
        return run_stdin(&ctx, &args);
    }

    let mut files = Vec::new();
    for p in &args.paths {
        if let Err(e) = collect_files(p, &mut files) {
            eprintln!("{}: error: {e}", p.display());
            return ExitCode::from(2);
        }
    }
    let jobs = args.jobs.unwrap_or_else(default_jobs);
    let outcomes = par_map(&files, jobs, |path| process(&ctx, path));

    let (mut changed, mut unchanged, mut skipped, mut failed) = (0usize, 0usize, 0usize, 0usize);
    let mut stdout = std::io::stdout().lock();
    for (path, o) in files.iter().zip(outcomes) {
        match o {
            Outcome::Unchanged => unchanged += 1,
            Outcome::Changed { diff } => {
                changed += 1;
                if args.check && !args.quiet {
                    let _ = writeln!(stdout, "would reformat: {}", path.display());
                }
                let _ = stdout.write_all(diff.as_bytes());
            }
            Outcome::Skipped(msg) => {
                skipped += 1;
                eprintln!("{}: {msg}", path.display());
            }
            Outcome::Failed(msg) => {
                failed += 1;
                eprintln!("{}: error: {msg}", path.display());
            }
        }
    }
    let verb = if ctx.write {
        "reformatted"
    } else {
        "would be reformatted"
    };
    eprintln!(
        "{changed} file(s) {verb}, {unchanged} already formatted, {skipped} skipped, {failed} failed"
    );
    if failed > 0 {
        ExitCode::from(2)
    } else if changed > 0 && !ctx.write {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        args: Args,
    }

    #[test]
    fn parses_flags() {
        let c = Cli::try_parse_from(["format", "a.gsc", "--check", "--diff"]).unwrap();
        assert!(c.args.check && c.args.diff);
        let c = Cli::try_parse_from(["format", "--stdin-filename", "x.gsc"]).unwrap();
        assert!(c.args.paths.is_empty());
        let c = Cli::try_parse_from(["format", "a.gsc", "--style", "minimal", "--max-width", "80"])
            .unwrap();
        assert_eq!(c.args.style.as_deref(), Some("minimal"));
        assert_eq!(c.args.max_width, Some(80));
        assert!(Cli::try_parse_from(["format", "a.gsc", "--style", "pretty"]).is_err());
        assert!(Cli::try_parse_from(["format"]).is_err());
    }

    #[test]
    fn check_mode_does_not_write() {
        let dir = std::env::temp_dir().join(format!("gsc-format-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.gsc");
        std::fs::write(&file, "f()\r\n{\r\nx=1;  \r\n}").unwrap();
        let ctx = Ctx {
            dialect_dec: Dialect::superset().with_decompiled(true),
            dialect: Dialect::superset(),
            opts: Options::default(),
            write: false,
            diff: true,
        };
        let Outcome::Changed { diff } = process(&ctx, &file) else {
            panic!("expected a change");
        };
        assert!(diff.contains("+\tx = 1;\r\n"), "{diff}");
        assert_eq!(std::fs::read(&file).unwrap(), b"f()\r\n{\r\nx=1;  \r\n}");
        let ctx = Ctx { write: true, ..ctx };
        assert!(matches!(process(&ctx, &file), Outcome::Changed { .. }));
        assert_eq!(
            std::fs::read(&file).unwrap(),
            b"f()\r\n{\r\n\tx = 1;\r\n}\r\n"
        );
        assert!(matches!(process(&ctx, &file), Outcome::Unchanged));
        std::fs::write(&file, "f( {").unwrap();
        assert!(matches!(process(&ctx, &file), Outcome::Skipped(_)));
        assert_eq!(std::fs::read(&file).unwrap(), b"f( {");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
