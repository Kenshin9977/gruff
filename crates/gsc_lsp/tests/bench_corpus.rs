//! Diagnostics latency and memory of the release `gruff lsp` on any corpus folder. Ignored by
//! default; see `docs/lsp.md` ("Benchmarks"). Inputs, from the environment:
//!
//! - `GSC_BENCH_FOLDER` (default `../corpus/t7/source-modtools`): the workspace folder;
//! - `GSC_BENCH_LIBRARY` (default `../corpus/t7/decompiled`, empty for none): a library root;
//! - `GSC_BENCH_GAME` (default `t7`), `GRUFF_DATA` or `GSC_DATA` (default `../gruff-data`, else `../gsc-data`), `GSC_BIN`;
//! - `GSC_BENCH_FILE`: the file to edit, relative to the folder (default: the largest file).

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::{data_dir, env_path, release_bin, root, stats, uri, Session};
use serde_json::{json, Value};

fn largest_file(folder: &Path) -> PathBuf {
    let mut files = Vec::new();
    gsc_sema::vfs::collect_files(folder, &mut files).unwrap();
    files
        .into_iter()
        .filter(|f| f.extension().is_some_and(|x| x == "gsc"))
        .max_by_key(|f| std::fs::metadata(f).map(|m| m.len()).unwrap_or(0))
        .expect("a .gsc file")
}

#[test]
#[ignore = "needs a local corpus and a release build"]
fn corpus_session() {
    let folder = env_path("GSC_BENCH_FOLDER", root().join("corpus/t7/source-modtools"));
    let library = std::env::var("GSC_BENCH_LIBRARY").map_or_else(
        |_| Some(root().join("corpus/t7/decompiled")),
        |v| (!v.is_empty()).then(|| PathBuf::from(v)),
    );
    let game = std::env::var("GSC_BENCH_GAME").unwrap_or_else(|_| "t7".to_owned());
    let data = data_dir();
    let bin = release_bin();
    let file = std::env::var_os("GSC_BENCH_FILE")
        .map_or_else(|| largest_file(&folder), |f| folder.join(f));
    assert!(folder.is_dir() && bin.is_file() && file.is_file());

    let mut s = Session::spawn(&bin, Duration::from_secs(600));
    let t0 = Instant::now();
    s.call(
        "initialize",
        json!({
            "processId": null,
            "rootUri": uri(&folder),
            "workspaceFolders": [{ "uri": uri(&folder), "name": "bench" }],
            "capabilities": {},
            "initializationOptions": {
                "game": game,
                "dataDir": data,
                "libraryRoots": library.iter().collect::<Vec<_>>(),
                "gscTestHooks": true,
            },
        }),
    );
    s.notify("initialized", json!({}));
    s.wait_method("gsc/ready");
    let t_ready = t0.elapsed();
    let (status, _) = s.call("gsc/status", json!(null));
    let first = s.wait_method("gsc/checked");
    let t_first = t0.elapsed();
    let mem_first = s.memory();

    let text = std::fs::read_to_string(&file).unwrap();
    let u = uri(&file);
    s.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": u, "languageId": "gsc", "version": 1, "text": text }}),
    );
    s.settle(Duration::from_millis(1500));

    // Edit to diagnostics: the time from didChange to the end of the check it triggers (the
    // debounce included), then the server's own check time.
    let end = text.lines().count();
    let mut version = 1;
    let mut lat: [Vec<f64>; 2] = Default::default();
    let mut server: [Vec<f64>; 2] = Default::default();
    let mut rechecked: [Vec<f64>; 2] = Default::default();
    let mut phases: [Value; 2] = Default::default();
    for i in 0..10 {
        for kind in 0..2 {
            version += 1;
            // 0: a comment (no name changes); 1: a new function (a new exported name).
            let insert = match kind {
                0 => format!("// edit {i}\n"),
                _ => format!("function bench_new_fn_{i}() {{ }}\n"),
            };
            let t = Instant::now();
            s.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": u, "version": version },
                    "contentChanges": [{
                        "range": { "start": { "line": end, "character": 0 },
                                   "end": { "line": end, "character": 0 } },
                        "text": insert,
                    }],
                }),
            );
            let c = s.wait_method("gsc/checked");
            lat[kind].push(t.elapsed().as_secs_f64() * 1000.0);
            server[kind].push(c["params"]["ms"].as_f64().unwrap_or(0.0));
            rechecked[kind].push(c["params"]["rechecked"].as_f64().unwrap_or(f64::NAN));
            phases[kind] = c["params"]["phases"].clone();
            s.settle(Duration::from_millis(300));
        }
    }
    let mem_end = s.memory();
    s.settle(Duration::from_secs(5));
    let mem_idle = s.memory();
    s.shutdown();

    println!(
        "\n== gruff lsp on {}: {} files ({} primary), game {game} ==",
        folder.display(),
        status["files"],
        status["primaryFiles"]
    );
    println!("edited file: {}", file.display());
    println!(
        "initialize -> ready: {:6.0} ms; -> first diagnostics: {:6.0} ms (check {} ms)",
        t_ready.as_secs_f64() * 1000.0,
        t_first.as_secs_f64() * 1000.0,
        first["params"]["ms"]
    );
    println!(
        "memory after the first check: {:.0} MiB (peak {:.0}); after the edits: {:.0} MiB; 5 s idle later: {:.0} MiB (peak {:.0})",
        mem_first.0, mem_first.1, mem_end.0, mem_idle.0, mem_idle.1
    );
    let names = ["comment edit", "new function"];
    for k in 0..2 {
        println!(
            "{:13} edit -> diagnostics: {}",
            names[k],
            stats(lat[k].clone(), 1)
        );
        println!(
            "{:13} server check:        {}",
            "",
            stats(server[k].clone(), 1)
        );
        let r = &rechecked[k];
        if r.iter().all(|x| x.is_finite()) {
            println!(
                "{:13} files re-checked:     {:?}",
                "",
                r.iter().map(|x| *x as u64).collect::<Vec<_>>()
            );
            println!("{:13} phases (last run):   {}", "", phases[k]);
        }
    }
}
