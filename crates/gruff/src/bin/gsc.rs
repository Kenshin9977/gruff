//! `gsc`: the deprecated alias of `gruff` (the binary's former name). Same program; it prints
//! a one-line notice on standard error, except for `lsp` and `mcp`, whose clients read stderr as
//! a log. It will be removed in the next minor version.

fn main() -> std::process::ExitCode {
    let sub = std::env::args().nth(1).unwrap_or_default();
    if !matches!(sub.as_str(), "lsp" | "mcp") {
        eprintln!(
            "warning: `{}` is a deprecated alias of `gruff` and will be removed; run `gruff` instead",
            gruff_cli::OLD_NAME
        );
    }
    gruff_cli::main()
}
