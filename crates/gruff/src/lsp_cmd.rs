//! `gruff lsp`: the language server, on standard input and output.

use std::process::ExitCode;

/// Arguments of `gruff lsp`.
#[derive(clap::Args)]
pub struct Args {
    /// Speak LSP on standard input and output (the only transport; accepted because clients
    /// such as vscode-languageclient pass it).
    #[arg(long)]
    pub stdio: bool,
}

pub fn run(_args: Args) -> ExitCode {
    match gsc_lsp::run_stdio() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
