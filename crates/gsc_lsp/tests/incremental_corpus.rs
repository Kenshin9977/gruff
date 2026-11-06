//! The incremental check against a full `gruff check` on a real corpus (default: the T7 mod-tools
//! sources with the decompiled dump as a library root): pseudo-random edits of primary files,
//! the results compared after each, with the time of both. Ignored by default; it reads
//! `GSC_BENCH_FOLDER`, `GSC_BENCH_LIBRARY`, `GSC_BENCH_GAME` and `GRUFF_DATA`/`GSC_DATA` like
//! `bench_corpus.rs`, and `GSC_INCREMENTAL_STEPS` (default 40).

mod common;

use std::path::PathBuf;
use std::time::Instant;

use common::root;
use gsc_check::incremental::Session;
use gsc_check::{check, Options, Selection};
use gsc_lsp::project::{Project, Settings};

const SNIPPETS: &[&str] = &[
    "\twait 0.05;\n",
    "\tself notify( \"bench_event\" );\n",
    "\tself waittill( \"spawned_player\" );\n",
    "\tself.bench_field = 1;\n",
    "\tx = self.bench_field;\n",
    "\twhile ( true ) { foo(); }\n",
    "function bench_fn() { wait 1; }\n",
    "// a comment\n",
    "}\n",
];

#[test]
#[ignore = "needs a local corpus"]
fn incremental_equals_full_on_a_corpus() {
    let folder = std::env::var_os("GSC_BENCH_FOLDER")
        .map_or_else(|| root().join("corpus/t7/source-modtools"), PathBuf::from);
    let library = std::env::var("GSC_BENCH_LIBRARY").map_or_else(
        |_| vec![root().join("corpus/t7/decompiled").display().to_string()],
        |v| if v.is_empty() { Vec::new() } else { vec![v] },
    );
    let settings = Settings {
        game: Some(std::env::var("GSC_BENCH_GAME").unwrap_or_else(|_| "t7".to_owned())),
        data_dir: Some(common::data_dir().display().to_string()),
        library_roots: library,
        mode: None,
        hash_packages: Vec::new(),
    };
    let steps: usize = std::env::var("GSC_INCREMENTAL_STEPS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(40);
    let project = Project::resolve(&settings, &[folder]);
    let mut ws = project.workspace();
    ws.load();
    let selection = Selection {
        layers: vec![project.check.layer.clone()],
        preview: project.check.preview.unwrap_or(false),
    };
    let opts = Options {
        selection: &selection,
        per_file_ignores: &project.check.per_file_ignores,
        root: None,
    };
    let mut session = Session::new();
    let t = Instant::now();
    session.update(&ws, &opts);
    println!("first update: {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    // As the language server does: the incremental path must not need them.
    let dropped = ws.drop_library_trees();
    println!("library trees dropped: {dropped}");
    let primary: Vec<_> = ws.primary_files().collect();
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = |n: usize| {
        seed ^= seed >> 12;
        seed ^= seed << 25;
        seed ^= seed >> 27;
        (seed.wrapping_mul(0x2545_F491_4F6C_DD1D) % n as u64) as usize
    };
    let (mut t_inc, mut t_full) = (Vec::new(), Vec::new());
    for step in 0..steps {
        let f = primary[next(primary.len())];
        let text = ws.file(f).text().to_owned();
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let at = next(lines.len() + 1);
        let mut out = lines.clone();
        out.insert(at, SNIPPETS[next(SNIPPETS.len())]);
        ws.set_text(f, &out.concat());
        let t = Instant::now();
        let up = session.update(&ws, &opts);
        t_inc.push(t.elapsed().as_secs_f64() * 1000.0);
        let t = Instant::now();
        let full = check(&ws, &opts);
        t_full.push(t.elapsed().as_secs_f64() * 1000.0);
        let inc = session.results(&ws);
        assert_eq!(inc.len(), full.len());
        for (a, b) in inc.iter().zip(&full) {
            assert_eq!(
                a.diagnostics,
                b.diagnostics,
                "step {step}: {} differs ({} re-checked)",
                b.display,
                up.rechecked.len()
            );
        }
        println!(
            "step {step:2}: {:>4} re-checked, {:10}, incremental {:6.1} ms {:?}, full {:6.1} ms",
            up.rechecked.len(),
            up.summaries,
            t_inc[step],
            up.phases
                .iter()
                .map(|(n, ms)| format!("{n} {ms:.1}"))
                .collect::<Vec<_>>(),
            t_full[step]
        );
    }
    let median = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    println!(
        "median: incremental {:.1} ms, full {:.1} ms",
        median(t_inc),
        median(t_full)
    );
}
