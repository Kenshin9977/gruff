//! Glue shared by `gruff check` and `gruff index`: thread count, decompiled mode, the game's engine
//! data (builtins and fields next to the profiles) and the error-to-exit-code wrapper.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use gsc_sema::{Catalogue, FieldCatalogue};
use gsc_syntax::Dialect;

use crate::parse_cmd;

/// `--jobs`, or every core.
pub fn jobs(arg: Option<usize>) -> usize {
    arg.unwrap_or_else(gsc_sema::default_jobs)
}

pub fn decompiled(d: parse_cmd::Decompiled) -> gsc_sema::Decompiled {
    match d {
        parse_cmd::Decompiled::Auto => gsc_sema::Decompiled::Auto,
        parse_cmd::Decompiled::On => gsc_sema::Decompiled::On,
        parse_cmd::Decompiled::Off => gsc_sema::Decompiled::Off,
    }
}

/// Run a command; an error is printed and exits with 2.
pub fn run(f: impl FnOnce() -> Result<ExitCode, String>) -> ExitCode {
    match f() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

/// Load a builtin catalogue file (its `[[intrinsic]]` list included).
pub fn load_catalogue(path: &Path) -> Result<Catalogue, String> {
    Catalogue::load(path)
}

/// The builtin catalogue next to the profiles (`gruff-data/builtins/<game>.toml`), if any; `game`
/// as given picks a variant such as `jup-sp` (see `gsc_sema::data::catalogue_file`).
pub fn find_catalogue(
    data_dir: Option<&Path>,
    dialect: &Dialect,
    game: Option<&str>,
) -> Result<Option<Catalogue>, String> {
    let Some(dir) = parse_cmd::profiles_dir(data_dir) else {
        return Ok(None);
    };
    match gsc_sema::data::catalogue_file(&dir, &dialect.id, game) {
        Some(p) => load_catalogue(&p).map(Some),
        None => Ok(None),
    }
}

/// The field catalogue next to the profiles (`gruff-data/fields/<game>.toml`), if any.
pub fn find_fields(
    data_dir: Option<&Path>,
    dialect: &Dialect,
) -> Result<Option<FieldCatalogue>, String> {
    let Some(dir) = parse_cmd::profiles_dir(data_dir) else {
        return Ok(None);
    };
    gsc_sema::data::data_file(&dir, "fields", &dialect.id)
        .map(|p| FieldCatalogue::load(&p))
        .transpose()
}

/// The engine event catalogue next to the profiles (`gruff-data/events/<game>.toml`), if any.
pub fn find_events(
    data_dir: Option<&Path>,
    dialect: &Dialect,
) -> Result<Option<gsc_sema::EventCatalogue>, String> {
    let Some(dir) = parse_cmd::profiles_dir(data_dir) else {
        return Ok(None);
    };
    gsc_sema::data::data_file(&dir, "events", &dialect.id)
        .map(|p| gsc_sema::EventCatalogue::load(&p))
        .transpose()
}

/// The hash packages of a game (`dialect_id`): `paths`, then `$GSC_HASH_PACKAGES`. `None` when the
/// game keeps identifiers in clear or no package is given. A summary goes to stderr unless
/// `quiet`; a missing or malformed package is a warning, not an error.
pub fn load_hash_names(
    dialect_id: &str,
    paths: &[PathBuf],
    quiet: bool,
) -> Result<Option<Arc<gsc_sema::Dictionary>>, String> {
    let Some(scheme) = gsc_sema::Scheme::for_profile(dialect_id) else {
        return Ok(None);
    };
    let mut all = paths.to_vec();
    if let Some(env) = std::env::var_os("GSC_HASH_PACKAGES") {
        all.extend(std::env::split_paths(&env).filter(|p| !p.as_os_str().is_empty()));
    }
    if all.is_empty() {
        return Ok(None);
    }
    let (names, report) = gsc_sema::load_packages(scheme, &all);
    for e in &report.errors {
        eprintln!("warning: hash package {e}");
    }
    if !quiet {
        let s = report.stats;
        eprintln!(
            "hash packages: {} file{}, {} names for {dialect_id} ({} plain, {} pairs verified, {} kept unverified, {} dropped, {} for other games)",
            report.files.len(),
            plural(report.files.len()),
            names.len(),
            s.names,
            s.verified,
            s.unverified,
            s.dropped,
            s.other_game
        );
    }
    Ok((!names.is_empty()).then(|| Arc::new(names)))
}

pub fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}
