//! Time a full workspace load and a one-file update.
//!
//! `cargo run --release -p gsc_sema --example workspace_bench -- <gruff-data> <game> <root>...`

use std::path::PathBuf;
use std::time::Instant;

use gsc_sema::{Catalogue, FieldCatalogue, Profile, Root, Symbol, Workspace};
use gsc_syntax::{Dialect, TextSize};

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [data, game, roots @ ..] = args.as_slice() else {
        eprintln!("usage: workspace_bench <gruff-data> <game> <root>...");
        std::process::exit(2);
    };
    let data = PathBuf::from(data);
    let dialect = Dialect::find(&data.join("profiles"), game).expect("profile");
    let builtins = Catalogue::load(&data.join(format!("builtins/{game}.toml"))).ok();
    let fields = FieldCatalogue::load(&data.join(format!("fields/{game}.toml"))).ok();

    let t = Instant::now();
    let mut ws = Workspace::new(Profile::new(dialect, builtins, fields));
    for r in roots {
        ws.add_root(Root::new(r));
    }
    let report = ws.load();
    let load = ms(t);
    let t = Instant::now();
    ws.resolve_all();
    let resolve = ms(t);
    let refs: usize = ws.files().map(|f| ws.file(f).facts.refs.len()).sum();
    let defs: usize = ws.files().map(|f| ws.file(f).facts.defs.len()).sum();
    println!(
        "{} files ({} binary skipped, {} errors), {defs} definitions, {refs} references, {} threads",
        ws.len(),
        report.binary,
        report.errors.len(),
        ws.jobs()
    );
    println!("load (discover, read, parse, facts, tables): {load:.0} ms");
    println!("resolve every file: {resolve:.0} ms");

    // A one-file edit: the largest shared utility file gains a function.
    let target = ws
        .files()
        .filter(|&f| ws.file(f).meta.key.ends_with("util_shared"))
        .max_by_key(|&f| ws.file(f).text().len())
        .unwrap_or_else(|| ws.files().next().expect("a file"));
    let text = format!(
        "{}\nfunction bench_added( a )\n{{\n\treturn a;\n}}\n",
        ws.file(target).text()
    );
    let t = Instant::now();
    let p = gsc_syntax::parse(&text, ws.profile_of(target).dialect(false));
    let parse = ms(t);
    let t = Instant::now();
    gsc_sema::facts::extract(
        &p.syntax(),
        &ws.file(target).meta.key,
        &ws.profile_of(target).config,
    );
    let extract = ms(t);
    let t = Instant::now();
    ws.set_text(target, &text);
    let update = ms(t);
    println!("one-file update breakdown: parse {parse:.1} ms, facts {extract:.1} ms");
    let t = Instant::now();
    ws.resolution(target);
    let one = ms(t);
    let t = Instant::now();
    ws.resolve_all();
    let all = ms(t);
    println!(
        "update `{}` ({} KB): set_text {update:.1} ms, re-resolve that file {one:.1} ms, re-resolve all {all:.0} ms",
        ws.file(target).meta.rel,
        text.len() / 1024
    );

    // Queries on the updated workspace.
    let f = ws.file(target);
    let def = f
        .facts
        .defs
        .iter()
        .position(|d| d.name == "bench_added")
        .expect("the new function");
    let at = f.facts.defs[def].name_range.start();
    let t = Instant::now();
    let sym = ws.symbol_at(target, at);
    let refs = sym.as_ref().map(|s| ws.references(s, true));
    println!(
        "references of the new function: {} in {:.1} ms",
        refs.map_or(0, |r| r.len()),
        ms(t)
    );
    let busiest = ws
        .files()
        .flat_map(|f| {
            (0..ws.file(f).facts.defs.len()).map(move |i| gsc_sema::DefId {
                file: f,
                index: i as u32,
            })
        })
        .find(|&d| ws.def(d).name == "is_bot")
        .map(Symbol::Def);
    if let Some(s) = busiest {
        let t = Instant::now();
        let n = ws.references(&s, false).len();
        println!("references of util::is_bot: {n} in {:.1} ms", ms(t));
    }
    let t = Instant::now();
    let syms = ws.workspace_symbols("spawn", 50);
    println!(
        "workspace_symbols(\"spawn\"): {} in {:.1} ms",
        syms.len(),
        ms(t)
    );
    let end = TextSize::from(text.len() as u32 - 12);
    let t = Instant::now();
    let items = ws.completions(target, end);
    println!(
        "completions in the new function: {} in {:.1} ms",
        items.len(),
        ms(t)
    );
}
