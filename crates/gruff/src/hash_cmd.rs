//! `gruff hash`: hash strings with a family, list the families, name hashes from the hash
//! packages (`--reverse`), or measure how many identifiers of a script tree have a name
//! (`--coverage`).

use std::path::PathBuf;
use std::process::ExitCode;

use gsc_hash::{Game, Role, Scheme};
use gsc_sema::{NameSource, Profile, Root, Workspace};

use crate::common;

/// Arguments of `gruff hash`.
#[derive(clap::Args)]
pub struct Args {
    /// List every family with its games, width and algorithm.
    #[arg(long, conflicts_with_all = ["reverse", "coverage"])]
    pub list: bool,
    /// Name each value (`0x1a2b`, `function_1a2b`, `#"hash_1a2b"`) from the hash packages: one
    /// line per family that names it.
    #[arg(long, conflicts_with = "coverage")]
    pub reverse: bool,
    /// Count the identifiers of the given script trees by where their name comes from (written
    /// in clear, a hash package, the clear names of the trees, unknown).
    #[arg(long)]
    pub coverage: bool,
    /// The game (`t8`, `jup`, `t11`...): the families `--list` shows, the scheme `--reverse` and
    /// `--coverage` use.
    #[arg(long)]
    pub game: Option<String>,
    /// A hash package file or directory (repeatable); adds to `hash_packages` in gruff.toml and
    /// `$GSC_HASH_PACKAGES`.
    #[arg(long = "hash-package", value_name = "PATH")]
    pub hash_packages: Vec<PathBuf>,
    /// Configuration file for `hash_packages` and `game` (default: the nearest gruff.toml).
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// The gruff-data directory, for `--coverage` (default: `$GRUFF_DATA` or `$GSC_DATA`, else a
    /// `gruff-data` or `gsc-data` folder found upwards).
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
    /// Also print the unmasked value.
    #[arg(long)]
    pub full: bool,
    /// Family id then strings (`t10.function main`); the values with `--reverse`; the script
    /// directories with `--coverage`.
    #[arg(required_unless_present = "list")]
    pub args: Vec<String>,
}

pub fn run(args: Args) -> ExitCode {
    common::run(|| {
        if args.list {
            list(args.game.as_deref())?;
        } else if args.reverse {
            reverse(&args)?;
        } else if args.coverage {
            coverage(&args)?;
        } else {
            let (id, strings) = args.args.split_first().ok_or("no family given")?;
            if strings.is_empty() {
                return Err("no string to hash".into());
            }
            hash(id, strings, args.full)?;
        }
        Ok(ExitCode::SUCCESS)
    })
}

fn list(game: Option<&str>) -> Result<(), String> {
    let game = game.map(parse_game).transpose()?;
    println!(
        "{:<20} {:>4}  {:<12} {:<26} {:<22} algorithm",
        "family", "bits", "status", "games", "input"
    );
    for f in gsc_hash::families() {
        if game.is_some_and(|g| !f.applies_to(g)) {
            continue;
        }
        let games: Vec<_> = f.games.iter().map(|g| g.id()).collect();
        println!(
            "{:<20} {:>4}  {:<12} {:<26} {:<22} {}",
            f.id,
            f.bits,
            f.status.label(),
            games.join(","),
            f.canon.describe(),
            f.algo.describe()
        );
        println!("{:<20}       {}", "", f.kinds);
    }
    if let Some(s) = game.and_then(Scheme::for_game) {
        println!("\nscript identifiers of {}:", s.game.name());
        for r in Role::ALL {
            if let Some(f) = s.family(*r) {
                println!("  {:<10} {:<11} {}", r.label(), r.prefix(), f.id);
            }
        }
    }
    Ok(())
}

fn parse_game(id: &str) -> Result<Game, String> {
    Game::from_profile_id(id).ok_or_else(|| {
        let ids: Vec<_> = Game::ALL.iter().map(|g| g.id()).collect();
        format!("unknown game {id:?}; known: {}", ids.join(", "))
    })
}

fn hash(id: &str, strings: &[String], full: bool) -> Result<(), String> {
    let family = gsc_hash::family(id)
        .ok_or_else(|| format!("unknown family {id:?}; run `gruff hash --list` for the ids"))?;
    for s in strings {
        let v = family.hash(s);
        if full {
            let raw = family.hash_unmasked(s.as_bytes());
            println!("{}\t{raw:016x}\t{s}", family.format(v));
        } else {
            println!("{}\t{s}", family.format(v));
        }
    }
    Ok(())
}

/// The game and hash packages from the flags and the configuration file.
fn game_and_packages(args: &Args) -> Result<(String, Vec<PathBuf>), String> {
    let cfg = match args.config.clone().or_else(|| {
        let start = args
            .args
            .first()
            .filter(|_| args.coverage)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        gsc_check::config::find(&start)
    }) {
        Some(p) => gsc_check::config::load(&p)?,
        None => gsc_check::config::Config::default(),
    };
    let game = args
        .game
        .clone()
        .or(cfg.game.clone())
        .ok_or("pass --game (or set `game` in gruff.toml)")?;
    let mut packages = args.hash_packages.clone();
    packages.extend(cfg.hash_packages.iter().cloned());
    Ok((game, packages))
}

fn reverse(args: &Args) -> Result<(), String> {
    let (game, packages) = game_and_packages(args)?;
    let Some(names) = common::load_hash_names(&game, &packages, false)? else {
        return Err(format!(
            "no hash package for {game}: pass --hash-package, set `hash_packages` in gruff.toml or $GSC_HASH_PACKAGES"
        ));
    };
    for a in &args.args {
        let written = a.trim().trim_start_matches('#').trim_matches('"');
        let value = gsc_hash::parse_any_spelling(written)
            .map(|(_, v)| v)
            .or_else(|| gsc_hash::db::parse_hex(written))
            .ok_or_else(|| format!("not a hash: {a:?}"))?;
        let hits = names.reverse(value);
        if hits.is_empty() {
            println!("{value:x}\t-\t?");
        }
        for (f, n) in hits {
            println!("{value:x}\t{}\t{n}", f.id);
        }
    }
    Ok(())
}

fn coverage(args: &Args) -> Result<(), String> {
    let (game, packages) = game_and_packages(args)?;
    let dialect = crate::parse_cmd::load_dialect(Some(&game), None, args.data_dir.as_deref())?;
    let catalogue = common::find_catalogue(args.data_dir.as_deref(), &dialect, Some(&game))?;
    let names = common::load_hash_names(&dialect.id, &packages, false)?;
    let profile = Profile::new(dialect, catalogue, None).with_names(names);
    if profile.scheme.is_none() {
        return Err(format!("{game} does not hash script identifiers"));
    }
    let mut ws = Workspace::new(profile);
    for p in &args.args {
        ws.add_root(Root::new(PathBuf::from(p)));
    }
    let loaded = ws.load();
    for e in &loaded.errors {
        eprintln!("error: {e}");
    }
    let cov = ws.hash_coverage();
    let pct = |a: usize, b: usize| {
        if b == 0 {
            0.0
        } else {
            100.0 * a as f64 / b as f64
        }
    };
    let hashed: usize = cov.counts.values().sum();
    let named = hashed - cov.count(None, NameSource::Unknown);
    let total = hashed + cov.clear_idents + cov.clear_strings;
    println!(
        "{} files, game {game}: {total} identifiers and #\"...\" strings",
        ws.len()
    );
    println!(
        "  written in clear: {} ({:.1}%)",
        cov.clear_idents + cov.clear_strings,
        pct(cov.clear_idents + cov.clear_strings, total)
    );
    println!(
        "  hashed: {hashed}, named {named} ({:.1}% of hashed): package {}, workspace {}",
        pct(named, hashed),
        cov.count(None, NameSource::Package),
        cov.count(None, NameSource::Workspace)
    );
    println!(
        "  with a name (clear or recovered): {:.1}%",
        pct(total - cov.count(None, NameSource::Unknown), total)
    );
    println!("role\toccurrences\tpackage\tworkspace\tunknown\tdistinct\tdistinct_named");
    for r in Role::ALL {
        let all: usize = cov
            .counts
            .iter()
            .filter(|((x, _), _)| x == r)
            .map(|(_, n)| n)
            .sum();
        if all == 0 {
            continue;
        }
        let (d, dn) = cov.distinct.get(r).copied().unwrap_or_default();
        println!(
            "{}\t{all}\t{}\t{}\t{}\t{d}\t{dn}",
            r.label(),
            cov.count(Some(*r), NameSource::Package),
            cov.count(Some(*r), NameSource::Workspace),
            cov.count(Some(*r), NameSource::Unknown),
        );
    }
    Ok(())
}
