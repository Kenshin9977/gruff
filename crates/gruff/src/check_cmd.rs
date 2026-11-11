//! `gruff check`: the Ruff-like linter. The rules live in the `gsc_check` crate; this is the glue:
//! arguments, configuration, file collection, fixes, output and exit codes.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use gsc_check::output::{self, Format};
use gsc_check::{config, select, Applicability, Layer, Options, Selection};
use gsc_sema::{Profile, Root, Workspace};

use crate::common::{self, plural};
use crate::parse_cmd;

/// Arguments of `gruff check`.
#[derive(clap::Args)]
pub struct Args {
    /// Files or directories (searched for .gsc, .csc and .gsh). Default: the current directory.
    pub paths: Vec<PathBuf>,
    /// A library root: scripts read for name resolution but not checked, such as the game's
    /// own scripts (the mod tools' `share/raw`). Repeatable; adds to `library` in gruff.toml.
    #[arg(long, value_name = "DIR")]
    pub library: Vec<PathBuf>,
    /// A hash package (file or directory) naming the game's hashed identifiers in messages.
    /// Repeatable; adds to `hash_packages` in gruff.toml and `$GSC_HASH_PACKAGES`.
    #[arg(long = "hash-package", value_name = "PATH")]
    pub hash_packages: Vec<PathBuf>,
    /// Game id or alias of the dialect (`t7`, `bo3`...). Default: `game` in gruff.toml, else t7.
    #[arg(long)]
    pub game: Option<String>,
    /// Profile file to use instead of looking the game up in gruff-data.
    #[arg(long)]
    pub profile: Option<PathBuf>,
    /// The gruff-data directory (default: `$GRUFF_DATA` or `$GSC_DATA`, else a
    /// `gruff-data` or `gsc-data` folder found upwards).
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
    /// Configuration file (default: the nearest gruff.toml, .gruff.toml, gsc.toml, .gsc.toml, or
    /// pyproject.toml with a [tool.gruff] or [tool.gsc] table).
    #[arg(long, conflicts_with = "isolated")]
    pub config: Option<PathBuf>,
    /// Ignore every configuration file.
    #[arg(long)]
    pub isolated: bool,
    /// Rules to enable, replacing the configuration's (codes, prefixes, names or ALL).
    #[arg(long, value_delimiter = ',')]
    pub select: Option<Vec<String>>,
    /// Rules to enable on top of the selection.
    #[arg(long, value_delimiter = ',')]
    pub extend_select: Vec<String>,
    /// Rules to disable.
    #[arg(long, value_delimiter = ',')]
    pub ignore: Vec<String>,
    /// Enable preview rules.
    #[arg(long)]
    pub preview: bool,
    /// Apply safe fixes in place.
    #[arg(long)]
    pub fix: bool,
    /// With --fix, also apply unsafe fixes.
    #[arg(long, requires = "fix")]
    pub unsafe_fixes: bool,
    /// Output format.
    #[arg(long, value_enum, default_value = "text")]
    pub output_format: OutputFormat,
    /// Print the number of diagnostics per code instead of the diagnostics.
    #[arg(long)]
    pub statistics: bool,
    /// Print the documentation of a rule (code or name) and exit.
    #[arg(long, value_name = "CODE")]
    pub explain: Option<String>,
    /// List every rule and exit.
    #[arg(long)]
    pub list_rules: bool,
    /// Exit with 0 even when diagnostics remain.
    #[arg(long)]
    pub exit_zero: bool,
    /// Whether the inputs are decompiler output.
    #[arg(long, value_enum, default_value = "auto")]
    pub decompiled: parse_cmd::Decompiled,
    /// Worker threads (default: every core). The output order does not depend on it.
    #[arg(long, short)]
    pub jobs: Option<usize>,
    /// Do not print the summary.
    #[arg(long, short)]
    pub quiet: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    /// `path:line:col: CODE message`, with a source snippet.
    Text,
    /// One line per diagnostic.
    Concise,
    Json,
    /// GitHub Actions workflow commands.
    Github,
    /// SARIF 2.1.0.
    Sarif,
}

impl From<OutputFormat> for Format {
    fn from(f: OutputFormat) -> Format {
        match f {
            OutputFormat::Text => Format::Text,
            OutputFormat::Concise => Format::Concise,
            OutputFormat::Json => Format::Json,
            OutputFormat::Github => Format::Github,
            OutputFormat::Sarif => Format::Sarif,
        }
    }
}

pub fn run(args: Args) -> ExitCode {
    common::run(|| run_inner(args))
}

fn run_inner(args: Args) -> Result<ExitCode, String> {
    let mut stdout = std::io::stdout().lock();
    if let Some(code) = &args.explain {
        let page = output::explain(code)
            .ok_or_else(|| format!("unknown rule `{code}`; see `gruff check --list-rules`"))?;
        let _ = stdout.write_all(page.as_bytes());
        return Ok(ExitCode::SUCCESS);
    }
    if args.list_rules {
        let _ = stdout.write_all(output::rule_list().as_bytes());
        return Ok(ExitCode::SUCCESS);
    }
    let paths = if args.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        args.paths.clone()
    };

    let cfg = if args.isolated {
        config::Config::default()
    } else {
        match args.config.clone().or_else(|| config::find(&paths[0])) {
            Some(p) => config::load(&p)?,
            None => config::Config::default(),
        }
    };
    let game = args.game.clone().or(cfg.game.clone());
    let dialect = parse_cmd::load_dialect(
        game.as_deref(),
        args.profile.as_deref(),
        args.data_dir.as_deref(),
    )?;
    let catalogue = common::find_catalogue(args.data_dir.as_deref(), &dialect, game.as_deref())?;
    let fields = common::find_fields(args.data_dir.as_deref(), &dialect)?;
    let events = common::find_events(args.data_dir.as_deref(), &dialect)?;
    let no_events = !events.as_ref().is_some_and(|e| e.complete);
    let no_catalogue = catalogue.is_none().then(|| dialect.id.clone());

    let cli = Layer {
        select: args.select.as_deref().map(select::parse_list).transpose()?,
        extend_select: select::parse_list(&args.extend_select)?,
        ignore: select::parse_list(&args.ignore)?,
    };
    let selection = Selection {
        layers: vec![cfg.layer.clone(), cli],
        preview: args.preview || cfg.preview.unwrap_or(false),
    };
    let opts = Options {
        selection: &selection,
        per_file_ignores: &cfg.per_file_ignores,
        root: cfg.path.as_ref().map(|_| cfg.root.as_path()),
    };

    let game_id = dialect.id.clone();
    let packages: Vec<PathBuf> = args
        .hash_packages
        .iter()
        .chain(&cfg.hash_packages)
        .cloned()
        .collect();
    let names = common::load_hash_names(&game_id, &packages, args.quiet)?;
    let no_names = names.is_none();
    let profile = Profile::new(dialect, catalogue, fields)
        .with_events(events)
        .with_names(names)
        .with_modes(cfg.modes.clone());
    let mut ws = Workspace::new(profile)
        .with_decompiled(common::decompiled(args.decompiled))
        .with_jobs(common::jobs(args.jobs));
    for p in &paths {
        ws.add_root(Root::new(p.clone()));
    }
    for p in args.library.iter().chain(&cfg.library) {
        ws.add_root(Root::new(p.clone()).library(true));
    }
    let loaded = ws.load();
    for e in &loaded.errors {
        eprintln!("error: {e}");
    }
    let mut results = gsc_check::check(&ws, &opts);

    let mut fixed = 0;
    if args.fix {
        let mut changed = false;
        for result in &results {
            let (text, n) =
                gsc_check::apply_fixes(&result.source.text, &result.diagnostics, args.unsafe_fixes);
            if n == 0 {
                continue;
            }
            std::fs::write(&result.path, result.source.encode(&text))
                .map_err(|e| format!("{}: {e}", result.path.display()))?;
            ws.set_text(result.file, &text);
            fixed += n;
            changed = true;
        }
        if changed {
            results = gsc_check::check(&ws, &opts);
        }
    }

    let format: Format = args.output_format.into();
    let total: usize = results.iter().map(|r| r.diagnostics.len()).sum();
    if args.statistics {
        let _ = stdout.write_all(output::statistics(&results).as_bytes());
    } else {
        let _ = stdout.write_all(output::render(&results, format).as_bytes());
    }
    // Preview rules report as hints: they are counted apart and never fail the run.
    let hints = results
        .iter()
        .flat_map(|r| &r.diagnostics)
        .filter(|d| d.severity.is_hint())
        .count();
    let errors = total - hints;
    if !args.quiet {
        let safe = results
            .iter()
            .flat_map(|r| &r.diagnostics)
            .filter(|d| {
                d.fix
                    .as_ref()
                    .is_some_and(|f| f.applicability == Applicability::Safe)
            })
            .count();
        let and_hints = match hints {
            0 => String::new(),
            n => format!(" and {n} hint{}", plural(n)),
        };
        let mut summary = if total == 0 && fixed == 0 {
            "All checks passed!\n".to_owned()
        } else if fixed > 0 {
            let all = errors + fixed;
            format!(
                "Found {all} error{} ({fixed} fixed, {errors} remaining){and_hints}.\n",
                plural(all)
            )
        } else if errors == 0 {
            format!("Found {hints} hint{}.\n", plural(hints))
        } else {
            format!("Found {errors} error{}{and_hints}.\n", plural(errors))
        };
        if safe > 0 && !args.fix {
            summary.push_str(&format!("[*] {safe} fixable with `--fix`.\n"));
        }
        if loaded.binary > 0 {
            summary.push_str(&format!(
                "{} compiled (binary) file{} skipped.\n",
                loaded.binary,
                plural(loaded.binary)
            ));
        }
        if let Some(game) = &no_catalogue {
            summary.push_str(&format!(
                "note: no builtin catalogue for {game}: B001-B003 are silent.\n"
            ));
        }
        if no_events && opts.selection.enabled("T001") {
            summary.push_str(&format!(
                "note: no complete engine event catalogue for {game_id}: T001 is silent.\n"
            ));
        }
        // The games that compile names to hashes read as `function_1a2b3c4d` without a package,
        // and nothing used to say so: the output just looked poorer than it should. Say it once,
        // with the number, and point at what fixes it.
        if no_names && ws.profile(0).scheme.is_some() {
            let unknown = ws
                .hash_coverage()
                .count(None, gsc_sema::NameSource::Unknown);
            // A stray hashed literal in an otherwise clear workspace is not worth a note.
            if unknown >= 10 {
                summary.push_str(&format!(
                    "note: {unknown} identifier{} of this workspace {} a hash with no name, and no \
                     hash package is loaded. `gruff hash --coverage` measures it; \
                     `gruff hash-package fetch` fills it from a source you configure \
                     (docs/hash-packages.md).\n",
                    plural(unknown),
                    if unknown == 1 { "is" } else { "are" },
                ));
            }
        }
        // Machine formats keep stdout clean.
        if matches!(format, Format::Text | Format::Concise) || args.statistics {
            let _ = stdout.write_all(summary.as_bytes());
        } else {
            eprint!("{summary}");
        }
    }
    let _ = stdout.flush();
    if !loaded.errors.is_empty() {
        return Ok(ExitCode::from(2));
    }
    // Hints do not fail the run: a preview rule is not sure enough of itself to stop a build.
    Ok(if errors > 0 && !args.exit_zero {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
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
    fn parses_selector_lists() {
        let c = Cli::try_parse_from([
            "check",
            "a.gsc",
            "--select",
            "B,L001",
            "--ignore",
            "B002",
            "--output-format",
            "sarif",
        ])
        .unwrap();
        assert_eq!(c.args.select.unwrap(), vec!["B", "L001"]);
        assert_eq!(c.args.ignore, vec!["B002"]);
        assert_eq!(c.args.output_format, OutputFormat::Sarif);
        assert!(Cli::try_parse_from(["check", "--unsafe-fixes"]).is_err());
    }
}
