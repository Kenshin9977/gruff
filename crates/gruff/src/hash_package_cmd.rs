//! `gruff hash-package`: where the packages are, and `fetch`, the one command of this tool that
//! reaches the network.
//!
//! Nothing else in Gruff opens a connection, and `check`, `index`, `lsp` and the rest never run
//! this: it happens when asked for, naming what it fetched and under which licence. The transport
//! is `git`, so no HTTP client is linked in and the sources are repositories the user names in
//! `gruff.toml`. Names recovered from a game's files belong to their publisher, so the packages
//! this writes carry the source's own licence line and stay on the machine: see
//! `docs/hash-packages.md`.

use std::path::{Path, PathBuf};
use std::process::{Command as Proc, ExitCode};

use gsc_check::config::HashSource;

use crate::common;

/// Arguments of `gruff hash-package`.
#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub command: Sub,
}

#[derive(clap::Subcommand)]
pub enum Sub {
    /// Clone or update the `[[hash-source]]` repositories of the configuration and write a
    /// package from each file they name. The only command that uses the network.
    Fetch(FetchArgs),
    /// Print where the packages read without a flag are kept, and what is there.
    Dir,
}

#[derive(clap::Args)]
pub struct FetchArgs {
    /// Configuration file holding the `[[hash-source]]` tables (default: the nearest gruff.toml).
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Only this source (repeatable); default: every one of them.
    #[arg(long = "source", value_name = "NAME")]
    pub sources: Vec<String>,
    /// Where to write the packages (default: the directory `hash-package dir` prints).
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Say what would be fetched and written, and do neither.
    #[arg(long)]
    pub dry_run: bool,
}

pub fn run(args: Args) -> ExitCode {
    common::run(|| {
        match args.command {
            Sub::Fetch(a) => fetch(&a)?,
            Sub::Dir => dir()?,
        }
        Ok(ExitCode::SUCCESS)
    })
}

fn dir() -> Result<(), String> {
    let Some(d) = common::home_packages() else {
        return Err("no home directory: set $GRUFF_HOME".to_owned());
    };
    println!("{}", d.display());
    if !d.is_dir() {
        println!("(nothing there yet)");
        return Ok(());
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&d)
        .map_err(|e| format!("{}: {e}", d.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    for f in &files {
        let lines = std::fs::read_to_string(f)
            .map(|t| {
                t.lines()
                    .filter(|l| !l.starts_with('#') && !l.is_empty())
                    .count()
            })
            .unwrap_or(0);
        println!(
            "  {}  {lines} entr{}",
            f.file_name().unwrap_or_default().to_string_lossy(),
            if lines == 1 { "y" } else { "ies" }
        );
    }
    Ok(())
}

/// `git` in `dir`, or a clone of `url` into it when it is not there yet.
fn sync(dir: &Path, url: &str, dry_run: bool) -> Result<String, String> {
    let git = |args: &[&str], cwd: Option<&Path>| -> Result<String, String> {
        let mut c = Proc::new("git");
        c.args(args);
        if let Some(d) = cwd {
            c.current_dir(d);
        }
        let out = c.output().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                "`git` is not on PATH, and it is how this command fetches".to_owned()
            }
            _ => format!("git: {e}"),
        })?;
        if !out.status.success() {
            return Err(format!(
                "git {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    };
    if dir.join(".git").is_dir() {
        if dry_run {
            return Ok("would update".to_owned());
        }
        git(&["fetch", "--depth", "1", "origin", "HEAD"], Some(dir))?;
        git(&["reset", "--hard", "FETCH_HEAD"], Some(dir))?;
    } else {
        if dry_run {
            return Ok(format!("would clone {url}"));
        }
        if let Some(p) = dir.parent() {
            std::fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()))?;
        }
        let d = dir.to_string_lossy().to_string();
        git(&["clone", "--depth", "1", url, &d], None)?;
    }
    git(&["rev-parse", "--short", "HEAD"], Some(dir))
}

/// Rewrite one file of a source as a hash package: a header saying where the names come from and
/// under what licence, then the entries, tabs between the columns as the format wants.
fn package(src: &HashSource, body: &str, rev: &str, from: &str) -> String {
    let mut out = String::from("# gsc-hash-package: 1\n");
    if let Some(g) = &src.game {
        out.push_str(&format!("# game: {g}\n"));
    }
    if let Some(f) = &src.family {
        out.push_str(&format!("# family: {f}\n"));
    }
    out.push_str(&format!(
        "# source: {} {from} at {rev} ({})\n",
        src.name, src.git
    ));
    out.push_str(&format!(
        "# licence: {}\n",
        src.licence.as_deref().unwrap_or("not stated by the source")
    ));
    out.push_str("# fetched-by: gruff hash-package fetch\n");
    let comma = src.separator.as_deref() == Some("comma");
    let names_only = src.columns.as_deref() == Some("names");
    for line in body.lines() {
        let line = line.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        if names_only {
            out.push_str(line);
        } else if comma {
            // A value and a name; a third column is ignored by the reader anyway.
            out.push_str(&line.replacen(',', "\t", 1));
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

fn fetch(a: &FetchArgs) -> Result<(), String> {
    let cfg = match &a.config {
        Some(p) => gsc_check::config::load(p)?,
        None => match gsc_check::config::find(&std::env::current_dir().unwrap_or_default()) {
            Some(p) => gsc_check::config::load(&p)?,
            None => return Err(no_sources()),
        },
    };
    let sources: Vec<&HashSource> = cfg
        .hash_sources
        .iter()
        .filter(|s| a.sources.is_empty() || a.sources.contains(&s.name))
        .collect();
    if sources.is_empty() {
        return Err(no_sources());
    }
    let out = match &a.out {
        Some(p) => p.clone(),
        None => {
            common::home_packages().ok_or("no home directory: set $GRUFF_HOME or pass --out")?
        }
    };
    let cache = common::home_dir()
        .ok_or("no home directory: set $GRUFF_HOME")?
        .join("hash-sources");
    if !a.dry_run {
        std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    let mut written = 0usize;
    for src in sources {
        let dir = cache.join(&src.name);
        let rev = sync(&dir, &src.git, a.dry_run)?;
        println!("{}: {rev}", src.name);
        for rel in &src.paths {
            let from = dir.join(rel);
            if a.dry_run {
                println!("  {rel} -> a package in {}", out.display());
                continue;
            }
            let body = match std::fs::read_to_string(&from) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("warning: {}: {e}", from.display());
                    continue;
                }
            };
            let stem = Path::new(rel)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "names".to_owned());
            let name = format!("{}__{stem}.tsv", src.name);
            let path = out.join(&name);
            let text = package(src, &body, &rev, rel);
            let entries = text.lines().filter(|l| !l.starts_with('#')).count();
            std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
            println!("  {name}: {entries} entries, {}", licence_line(src));
            written += 1;
        }
    }
    if a.dry_run {
        println!("\nnothing fetched or written (--dry-run).");
    } else {
        println!(
            "\n{written} package{} in {}; every command reads them from there.",
            common::plural(written),
            out.display()
        );
    }
    Ok(())
}

fn licence_line(src: &HashSource) -> String {
    match &src.licence {
        Some(l) => format!("licence: {l}"),
        None => "licence not stated by the source: treat the names as its own".to_owned(),
    }
}

fn no_sources() -> String {
    "no `[[hash-source]]` in the configuration. A source names a git repository of names and the \
     files to take from it:\n\n\
     [[hash-source]]\n\
     name = \"an-index\"\n\
     git = \"https://example.invalid/an-index.git\"\n\
     paths = [\"names/t8.tsv\"]\n\
     game = \"t8, t9\"\n\
     licence = \"local only\"\n\n\
     Gruff ships none: a list of names recovered from a game's files is the publisher's \
     material, so which one you use, and whether you may, is yours to decide. \
     docs/hash-packages.md explains the format and names the community indexes."
        .to_owned()
}
