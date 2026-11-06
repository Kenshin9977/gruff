//! `gruff types`: the argument types the scripts pass to each builtin, as JSON (the corpus
//! measure of the catalogues' parameter types, read by tools/re/types_corpus.py).

use std::path::PathBuf;
use std::process::ExitCode;

use gsc_sema::types::infer;
use gsc_sema::Profile;

use crate::{common, parse_cmd};

/// Arguments of `gruff types`.
#[derive(clap::Args)]
pub struct Args {
    /// Root directories (or files) of one workspace.
    #[arg(required = true)]
    paths: Vec<PathBuf>,
    /// Game id or alias of the dialect (`t7`, `bo3`, `iw5`, `t11`...).
    #[arg(long, default_value = "t7")]
    game: String,
    /// The gruff-data directory (default: `$GRUFF_DATA` or `$GSC_DATA`, else a
    /// `gruff-data` or `gsc-data` folder found upwards).
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// Whether the inputs are decompiler output.
    #[arg(long, value_enum, default_value = "auto")]
    decompiled: parse_cmd::Decompiled,
    /// A hash package (file or directory) naming the game's hashed identifiers: calls written by
    /// name and by hash then resolve to the same definition. Repeatable; adds to
    /// `$GSC_HASH_PACKAGES`. The output keeps the spellings of the scripts.
    #[arg(long = "hash-package", value_name = "PATH")]
    hash_packages: Vec<PathBuf>,
    /// Worker threads (default: every core). The output does not depend on it.
    #[arg(long, short)]
    jobs: Option<usize>,
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn run_inner(args: &Args) -> Result<(), String> {
    let dialect = parse_cmd::load_dialect(Some(&args.game), None, args.data_dir.as_deref())?;
    let builtins = common::find_catalogue(args.data_dir.as_deref(), &dialect, Some(&args.game))?
        .ok_or_else(|| format!("no builtin catalogue for {}", args.game))?;
    let names = common::load_hash_names(&dialect.id, &args.hash_packages, true)?;
    let profile = Profile::new(dialect, Some(builtins), None).with_names(names);
    let mut ws = gsc_index::workspace(
        &args.paths,
        profile,
        common::decompiled(args.decompiled),
        common::jobs(args.jobs),
    );
    let stats = gsc_index::build(&mut ws);
    let inf = infer(&ws);
    let mut out = String::new();
    out.push_str(&format!(
        "{{\"game\": \"{}\", \"files\": {}, \"functions\": {}, \"params\": {}, \"params_typed\": {}, \"rounds\": {}, \"builtins\": [\n",
        esc(&args.game),
        stats.files,
        inf.functions,
        inf.params,
        inf.params_typed,
        inf.rounds
    ));
    let n = inf.builtins.len();
    for (i, (k, b)) in inf.builtins.iter().enumerate() {
        let args: Vec<String> = b
            .args
            .iter()
            .map(|m| {
                let kv: Vec<String> = m.iter().map(|(t, c)| format!("\"{t}\": {c}")).collect();
                format!("{{{}}}", kv.join(", "))
            })
            .collect();
        out.push_str(&format!(
            "{{\"name\": \"{}\", \"vm\": \"{}\", \"kind\": \"{}\", \"sites\": {}, \"files\": {}, \"args\": [{}]}}{}\n",
            esc(&k.name),
            k.vm,
            k.kind,
            b.sites,
            b.files,
            args.join(", "),
            if i + 1 < n { "," } else { "" }
        ));
    }
    out.push_str("]}\n");
    print!("{out}");
    Ok(())
}

pub fn run(args: Args) -> ExitCode {
    common::run(|| run_inner(&args).map(|()| ExitCode::SUCCESS))
}
