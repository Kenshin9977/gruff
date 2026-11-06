//! `gruff mcp`: the MCP server for coding agents, on standard input and output.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

/// Arguments of `gruff mcp`.
#[derive(clap::Args)]
pub struct Args {
    /// Workspace folders (default: the working directory). Their files are checked.
    pub folders: Vec<PathBuf>,
    /// Game id or alias (`t7`, `bo3`...); default: gruff.toml, else detected from the folder.
    #[arg(long)]
    pub game: Option<String>,
    /// The gruff-data directory (default: `$GRUFF_DATA` or `$GSC_DATA`, else a
    /// `gruff-data` or `gsc-data` folder found upwards).
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
    /// A library root: game scripts read for name resolution, never checked (repeatable).
    #[arg(long = "library", value_name = "DIR")]
    pub library: Vec<PathBuf>,
    /// `retail` or `community`.
    #[arg(long)]
    pub mode: Option<String>,
    /// A hash package (file or directory) naming the game's hashed identifiers (repeatable).
    #[arg(long = "hash-package", value_name = "PATH")]
    pub hash_packages: Vec<PathBuf>,
    /// Least seconds between two scans of the disk for changed files.
    #[arg(long, default_value_t = 2.0)]
    pub rescan: f64,
}

pub fn run(args: Args) -> ExitCode {
    let config = gsc_mcp::Config {
        folders: args.folders,
        settings: gsc_lsp::project::Settings {
            game: args.game,
            data_dir: args.data_dir.map(|d| d.display().to_string()),
            library_roots: args
                .library
                .iter()
                .map(|l| {
                    std::path::absolute(l)
                        .unwrap_or_else(|_| l.clone())
                        .display()
                        .to_string()
                })
                .collect(),
            mode: args.mode,
            hash_packages: args
                .hash_packages
                .iter()
                .map(|p| {
                    std::path::absolute(p)
                        .unwrap_or_else(|_| p.clone())
                        .display()
                        .to_string()
                })
                .collect(),
        },
        rescan: Duration::from_secs_f64(args.rescan.max(0.0)),
    };
    match gsc_mcp::run_stdio(config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
