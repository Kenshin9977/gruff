//! Throughput of each phase over a directory:
//! `cargo run --release --example bench -- <dir> [profile.toml]`.

use std::path::{Path, PathBuf};
use std::time::Instant;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use gsc_syntax::{check_dialect, parse, tokenize, Dialect, SourceText};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p
            .extension()
            .is_some_and(|x| x == "gsc" || x == "csc" || x == "gsh")
        {
            out.push(p);
        }
    }
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: bench <dir>");
    let mut files = Vec::new();
    collect(Path::new(&dir), &mut files);
    let texts: Vec<String> = files
        .iter()
        .filter_map(|f| std::fs::read(f).ok())
        .map(|b| SourceText::decode(&b).text)
        .collect();
    let bytes: usize = texts.iter().map(|t| t.len()).sum();
    let dialect = match std::env::args().nth(2) {
        Some(profile) => Dialect::load(Path::new(&profile)).expect("profile"),
        None => Dialect::superset(),
    };
    let mb = bytes as f64 / 1e6;

    let t = Instant::now();
    let mut n = 0usize;
    for text in &texts {
        n += tokenize(text, &dialect).0.len();
    }
    let lex = t.elapsed().as_secs_f64();

    let t = Instant::now();
    let parses: Vec<_> = texts.iter().map(|text| parse(text, &dialect)).collect();
    let full = t.elapsed().as_secs_f64();

    let mut max_depth = 0usize;
    for p in &parses {
        let mut depth = 0usize;
        for e in p.syntax().preorder() {
            match e {
                gsc_syntax::WalkEvent::Enter(_) => {
                    depth += 1;
                    max_depth = max_depth.max(depth);
                }
                gsc_syntax::WalkEvent::Leave(_) => depth -= 1,
            }
        }
    }
    println!("max tree depth {max_depth}");
    let t = Instant::now();
    let mut d = 0usize;
    for p in &parses {
        d += check_dialect(&p.syntax(), &dialect).len();
    }
    let check = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let mut same = 0usize;
    for (p, text) in parses.iter().zip(&texts) {
        same += usize::from(p.syntax().to_string() == *text);
    }
    let round_trip = t.elapsed().as_secs_f64();
    println!("round-trip {round_trip:.3}s ({same} exact)");
    println!(
        "{} files, {:.1} MB, {n} tokens, {d} diags\nlex {lex:.3}s ({:.0} MB/s)\nlex+parse+tree {full:.3}s ({:.0} MB/s, {:.0} files/s)\ncheck {check:.3}s",
        texts.len(),
        mb,
        mb / lex,
        mb / full,
        texts.len() as f64 / full
    );
}
