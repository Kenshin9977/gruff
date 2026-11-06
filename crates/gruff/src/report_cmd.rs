//! `gruff report`: a shareable corpus report (counts and identifiers, no code) of a local script
//! tree, for games whose data comes from the community. See docs/community-reports.md.

use std::path::PathBuf;
use std::process::ExitCode;

use gsc_index::report::{self, Options, ScriptNames};
use gsc_sema::Profile;

use crate::{common, parse_cmd};

/// How scripts are named in the report.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum Scripts {
    /// xxh3 of each relative path (dedupes identical installs).
    Hash,
    /// The relative paths.
    Names,
    /// No script list.
    Drop,
}

/// Arguments of `gruff report`.
#[derive(clap::Args)]
pub struct Args {
    /// Root directories (or files) of the game's scripts.
    #[arg(required = true)]
    paths: Vec<PathBuf>,
    /// Game id or alias of the dialect (`iw1-uo`, `s4`, `iw9`...).
    #[arg(long)]
    game: String,
    /// The gruff-data directory (default: `$GRUFF_DATA` or `$GSC_DATA`, else a
    /// `gruff-data` or `gsc-data` folder found upwards).
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// Write the JSON report here (default: standard output; the summary then goes to
    /// standard error).
    #[arg(long, short)]
    output: Option<PathBuf>,
    /// How to name the scripts.
    #[arg(long, value_enum, default_value = "hash")]
    scripts: Scripts,
    /// Whether the inputs are decompiler output.
    #[arg(long, value_enum, default_value = "auto")]
    decompiled: parse_cmd::Decompiled,
    /// Leave out event names of several words (`single anim`, common in CoD2): one-word names
    /// only.
    #[arg(long)]
    one_word_events: bool,
    /// Compiler semantics: `retail` or `community` (see `gruff parse --mode`).
    #[arg(long, default_value = "retail")]
    mode: gsc_syntax::Mode,
    /// Worker threads (default: every core). The report does not depend on it.
    #[arg(long, short)]
    jobs: Option<usize>,
}

fn run_inner(args: &Args) -> Result<(), String> {
    let dialect = parse_cmd::load_dialect(Some(&args.game), None, args.data_dir.as_deref())?
        .with_mode(args.mode);
    let dir = parse_cmd::profiles_dir(args.data_dir.as_deref())
        .ok_or("cannot find gruff-data/profiles; pass --data-dir or set GRUFF_DATA")?;
    let path = dir.join(format!("{}.toml", dialect.id));
    let profile_text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read the profile {}: {e}", path.display()))?;
    // No catalogue: the report must not depend on gruff-data's builtins (absent for the games
    // it is for), so every call nothing defines is a candidate.
    let mut ws = gsc_index::workspace(
        &args.paths,
        Profile::new(dialect, None, None),
        common::decompiled(args.decompiled),
        common::jobs(args.jobs),
    );
    let stats = gsc_index::build(&mut ws);
    if stats.files == 0 {
        return Err("no .gsc, .csc or .gsh file found".to_owned());
    }
    let opts = Options {
        toolchain: env!("CARGO_PKG_VERSION").to_owned(),
        game: args.game.to_ascii_lowercase(),
        profile_text,
        mode: args.mode,
        scripts: match args.scripts {
            Scripts::Hash => ScriptNames::Hash,
            Scripts::Names => ScriptNames::Names,
            Scripts::Drop => ScriptNames::Drop,
        },
        event_spaces: !args.one_word_events,
    };
    let rep = report::report(&ws, &opts)?;
    let json = rep.to_json();
    match &args.output {
        Some(p) => {
            std::fs::write(p, &json).map_err(|e| format!("cannot write {}: {e}", p.display()))?;
            print!("{}", rep.summary());
            println!("  written to {}", p.display());
        }
        None => {
            print!("{json}");
            eprint!("{}", rep.summary());
        }
    }
    Ok(())
}

pub fn run(args: Args) -> ExitCode {
    common::run(|| run_inner(&args).map(|()| ExitCode::SUCCESS))
}
