//! `gruff index`: write the grep-able workspace index (SYMBOLS, CALLS, EVENTS...).

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use gsc_sema::{Catalogue, Profile};
use gsc_syntax::Dialect;

use crate::{common, parse_cmd};

/// Arguments of `gruff index`.
#[derive(clap::Args)]
pub struct Args {
    /// Root directories (or files) to index; paths in the index are relative to them.
    #[arg(required = true)]
    paths: Vec<PathBuf>,
    /// Game id or alias of the dialect (`t7`, `bo3`, `iw5`, `t11`...).
    #[arg(long, default_value = "t7")]
    game: String,
    /// Output directory.
    #[arg(long, default_value = ".gsc-index")]
    out: PathBuf,
    /// Profile file to use instead of looking the game up in gruff-data.
    #[arg(long)]
    profile: Option<PathBuf>,
    /// The gruff-data directory (default: `$GRUFF_DATA` or `$GSC_DATA`, else a
    /// `gruff-data` or `gsc-data` folder found upwards).
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// Builtins catalogue (default: gruff-data `builtins/<game>.toml` when it exists).
    #[arg(long)]
    builtins: Option<PathBuf>,
    /// Do not load any builtins catalogue.
    #[arg(long, conflicts_with = "builtins")]
    no_builtins: bool,
    /// A hash package (file or directory) naming the game's hashed identifiers: calls written by
    /// name and by hash then resolve to the same definition. Repeatable; adds to
    /// `$GSC_HASH_PACKAGES`. The output keeps the spellings of the scripts.
    #[arg(long = "hash-package", value_name = "PATH")]
    hash_packages: Vec<PathBuf>,
    /// Also write UNKNOWNS.tsv, VOCAB.tsv and STRINGS.tsv: the hashes nothing names and what
    /// constrains each of them, the clear vocabulary of every file, and the clear strings of each
    /// use (`docs/hash-recovery.md`). Empty for a game that does not hash identifiers.
    #[arg(long)]
    unknowns: bool,
    /// Whether the inputs are decompiler output.
    #[arg(long, value_enum, default_value = "auto")]
    decompiled: parse_cmd::Decompiled,
    /// Worker threads (default: every core). The output does not depend on it.
    #[arg(long, short)]
    jobs: Option<usize>,
    /// Print nothing but errors.
    #[arg(long, short)]
    quiet: bool,
}

fn builtins_for(args: &Args, dialect: &Dialect) -> Result<Option<Catalogue>, String> {
    if args.no_builtins {
        return Ok(None);
    }
    if let Some(p) = &args.builtins {
        return common::load_catalogue(p).map(Some);
    }
    // A catalogue of another game (a profile file named like it) is left out.
    Ok(
        common::find_catalogue(args.data_dir.as_deref(), dialect, Some(&args.game))?
            .filter(|b| b.game.starts_with(dialect.id.as_str())),
    )
}

fn run_inner(args: &Args) -> Result<(), String> {
    let wall = Instant::now();
    let dialect = parse_cmd::load_dialect(
        Some(&args.game),
        args.profile.as_deref(),
        args.data_dir.as_deref(),
    )?;
    let builtins = builtins_for(args, &dialect)?;
    let builtins_len = builtins.as_ref().map_or(0, Catalogue::len);
    let names = common::load_hash_names(&dialect.id, &args.hash_packages, args.quiet)?;
    let profile = Profile::new(dialect, builtins, None).with_names(names);
    let resolution = profile.config.resolution;
    let game = profile.dialect.id.clone();
    let mut ws = gsc_index::workspace(
        &args.paths,
        profile,
        common::decompiled(args.decompiled),
        common::jobs(args.jobs),
    );
    let stats = gsc_index::build(&mut ws);
    for e in &stats.unreadable {
        eprintln!("error: {e}");
    }
    let index = gsc_index::index(&ws);
    let t = Instant::now();
    let sizes = gsc_index::write::write_dir(&index, &args.out, &game, args.unknowns)
        .map_err(|e| format!("{}: {e}", args.out.display()))?;
    let write_time = t.elapsed();
    if !args.quiet {
        println!(
            "{} files ({:.1} MB) indexed as {} ({:?} resolution, builtins: {}){}",
            stats.files,
            stats.bytes as f64 / 1e6,
            game,
            resolution,
            builtins_len,
            if stats.skipped_binary > 0 {
                format!(", {} compiled files skipped", stats.skipped_binary)
            } else {
                String::new()
            }
        );
        if stats.syntax_errors > 0 {
            println!(
                "{} syntax errors in {} files (facts are still extracted from the recovered tree)",
                stats.syntax_errors, stats.files_with_errors
            );
        }
        for w in &sizes {
            println!("  {:<20} {:>12} bytes  {:>9} rows", w.name, w.bytes, w.rows);
        }
        println!(
            "time: read+parse+extract {:.3} s on {} threads, resolve {:.3} s, write {:.3} s, total {:.3} s",
            stats.extract.as_secs_f64(),
            stats.jobs,
            stats.resolve.as_secs_f64(),
            write_time.as_secs_f64(),
            wall.elapsed().as_secs_f64()
        );
        println!("wrote {}", args.out.display());
    }
    if stats.unreadable.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} files could not be read",
            stats.unreadable.len()
        ))
    }
}

pub fn run(args: Args) -> ExitCode {
    common::run(|| run_inner(&args).map(|()| ExitCode::SUCCESS))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Wrap {
        #[command(flatten)]
        args: Args,
    }

    #[test]
    fn parses_index_args() {
        let w = Wrap::try_parse_from(["x", "a", "b", "--game", "t11", "--out", "o", "-j", "2"])
            .unwrap();
        assert_eq!(w.args.paths.len(), 2);
        assert_eq!(w.args.game, "t11");
        assert_eq!(w.args.out, PathBuf::from("o"));
        assert!(Wrap::try_parse_from(["x"]).is_err());
    }
}
