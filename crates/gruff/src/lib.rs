//! `gruff`: the GSC toolchain command line. The `gruff` binary (`src/main.rs`) and the
//! deprecated `gsc` alias (`src/bin/gsc.rs`) both call [`main`].

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod check_cmd;
mod common;
mod format_cmd;
mod hash_cmd;
mod index_cmd;
mod lsp_cmd;
mod mcp_cmd;
mod parse_cmd;
mod report_cmd;
mod types_cmd;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// The binary's former name, kept as a deprecated alias for one version.
pub const OLD_NAME: &str = "gsc";

#[derive(Parser)]
#[command(
    name = "gruff",
    version,
    about = "Gruff: GSC language tools (CoD1 to Black Ops 7)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Hash strings with a family (`gruff hash t10.function main`), list the families, name
    /// hashes from the hash packages (`--reverse`) or measure name coverage (`--coverage`).
    Hash(hash_cmd::Args),
    /// Parse files or directories: print diagnostics (`path:line:col: code message`) and a
    /// corpus summary.
    Parse {
        /// Files or directories (searched for .gsc, .csc and .gsh).
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Game id or alias of the dialect (`t7`, `bo3`, `iw5`...), or `superset`.
        #[arg(long, default_value = "t7")]
        game: String,
        /// Profile file to use instead of looking the game up in gruff-data.
        #[arg(long)]
        profile: Option<PathBuf>,
        /// The gruff-data directory (default: `$GRUFF_DATA` or `$GSC_DATA`, else a `gruff-data`
        /// or `gsc-data` folder found upwards).
        #[arg(long)]
        data_dir: Option<PathBuf>,
        /// Print the syntax tree of every file.
        #[arg(long)]
        tree: bool,
        /// Print timing, throughput and counts per diagnostic.
        #[arg(long)]
        stats: bool,
        /// Print only the summary.
        #[arg(long, short)]
        quiet: bool,
        /// Print at most this many diagnostics per file.
        #[arg(long, default_value_t = 50)]
        max_per_file: usize,
        /// Whether the inputs are decompiler output.
        #[arg(long, value_enum, default_value = "auto")]
        decompiled: parse_cmd::Decompiled,
        /// Worker threads (default: every core). The output order does not depend on it.
        #[arg(long, short)]
        jobs: Option<usize>,
        /// Compiler semantics: `retail` (the game's own compiler) or `community` (gsc-tool,
        /// Plutonium...: reads the profiles' `community` values, e.g. the BO2 preprocessor).
        #[arg(long, default_value = "retail")]
        mode: gsc_syntax::Mode,
    },
    /// Lint files or directories.
    Check(check_cmd::Args),
    /// Write the grep-able workspace index (SYMBOLS, CALLS, EVENTS).
    Index(index_cmd::Args),
    /// Format files in place, or check that they are formatted.
    Format(format_cmd::Args),
    /// Run the language server on standard input and output.
    Lsp(lsp_cmd::Args),
    /// Run the MCP server for coding agents on standard input and output (read-only tools).
    Mcp(mcp_cmd::Args),
    /// Print, as JSON, the argument types the scripts pass to each builtin.
    Types(types_cmd::Args),
    /// Write a shareable corpus report (counts and identifiers, no code) for community data.
    Report(report_cmd::Args),
}

/// Run the command line (`std::env::args`).
pub fn main() -> ExitCode {
    let cli = Cli::parse();
    // Parsing recurses; give it room for deeply nested files.
    let worker = std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(move || run(cli));
    match worker.map(|h| h.join()) {
        Ok(Ok(code)) => code,
        _ => {
            eprintln!("error: the worker thread failed");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> ExitCode {
    match cli.command {
        Command::Check(args) => check_cmd::run(args),
        Command::Hash(args) => hash_cmd::run(args),
        Command::Index(args) => index_cmd::run(args),
        Command::Format(args) => format_cmd::run(args),
        Command::Lsp(args) => lsp_cmd::run(args),
        Command::Mcp(args) => mcp_cmd::run(args),
        Command::Types(args) => types_cmd::run(args),
        Command::Report(args) => report_cmd::run(args),
        Command::Parse {
            paths,
            game,
            profile,
            data_dir,
            tree,
            stats,
            quiet,
            max_per_file,
            decompiled,
            jobs,
            mode,
        } => {
            let jobs = common::jobs(jobs);
            let opts = parse_cmd::Options {
                paths,
                game: Some(game),
                profile,
                data_dir,
                tree,
                stats,
                quiet,
                max_per_file,
                decompiled,
                jobs,
                mode,
            };
            match parse_cmd::run(&opts) {
                Ok(true) => ExitCode::SUCCESS,
                Ok(false) => ExitCode::from(1),
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::from(2)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_hash_and_list() {
        let c = Cli::try_parse_from(["gruff", "hash", "t7.canon", "main"]).unwrap();
        assert!(matches!(c.command, Command::Hash(ref a) if !a.list && a.args.len() == 2));
        let c = Cli::try_parse_from(["gruff", "hash", "--list"]).unwrap();
        assert!(matches!(c.command, Command::Hash(ref a) if a.list));
        assert!(Cli::try_parse_from(["gruff", "hash"]).is_err());
        let c = Cli::try_parse_from([
            "gruff",
            "hash",
            "--reverse",
            "--game",
            "t11",
            "--hash-package",
            "p.tsv",
            "function_1a2b",
        ])
        .unwrap();
        assert!(
            matches!(c.command, Command::Hash(ref a) if a.reverse && a.hash_packages.len() == 1)
        );
    }

    #[test]
    fn parses_parse() {
        let c =
            Cli::try_parse_from(["gruff", "parse", "a.gsc", "--game", "t7", "--stats"]).unwrap();
        assert!(matches!(c.command, Command::Parse { stats: true, .. }));
        assert!(Cli::try_parse_from(["gruff", "parse"]).is_err());
    }
}
