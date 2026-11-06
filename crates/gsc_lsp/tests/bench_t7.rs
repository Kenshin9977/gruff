//! Scripted session against the release `gruff lsp` on the T7 workspace (mod-tools sources plus
//! the decompiled dump as a library root). Ignored by default; see `docs/lsp.md` for how to run
//! it and the variables (`GSC_CORPUS`, `GRUFF_DATA` or `GSC_DATA`, `GSC_BIN`) that relocate its inputs.

mod common;

use std::time::{Duration, Instant};

use common::{data_dir, env_path, release_bin, root, uri, Session};
use serde_json::{json, Value};

/// The position of the first `pat` in `text`, plus `delta` columns (ASCII text).
fn find(text: &str, pat: &str, delta: usize) -> Value {
    let at = text
        .find(pat)
        .unwrap_or_else(|| panic!("{pat:?} not found"))
        + delta;
    let line = text[..at].matches('\n').count();
    let col = at - text[..at].rfind('\n').map_or(0, |i| i + 1);
    json!({ "line": line, "character": col })
}

fn stats(v: Vec<Duration>) -> String {
    common::stats(v.iter().map(|d| d.as_secs_f64() * 1000.0).collect(), 2)
}

fn with_context(mut p: Value) -> Value {
    p["context"] = json!({ "includeDeclaration": true });
    p
}

#[test]
#[ignore = "needs the local T7 corpus and a release build"]
fn t7_session() {
    let corpus = env_path("GSC_CORPUS", root().join("corpus")).join("t7");
    let data = data_dir();
    let bin = release_bin();
    let folder = corpus.join("source-modtools");
    let library = corpus.join("decompiled");
    assert!(folder.is_dir() && library.is_dir() && bin.is_file());

    let mut s = Session::spawn(&bin, Duration::from_secs(300));

    let t0 = Instant::now();
    let (init, _) = s.call(
        "initialize",
        json!({
            "processId": null,
            "rootUri": uri(&folder),
            "workspaceFolders": [{ "uri": uri(&folder), "name": "t7" }],
            "capabilities": { "window": { "workDoneProgress": true } },
            "initializationOptions": {
                "game": "t7",
                "dataDir": data,
                "libraryRoots": [library],
                "gscTestHooks": true,
            },
        }),
    );
    s.notify("initialized", json!({}));
    let ready = s.wait_for(|m| m["method"] == "gsc/ready");
    let t_ready = t0.elapsed();
    let (status, _) = s.call("gsc/status", json!(null));
    let checked = s.wait_for(|m| m["method"] == "gsc/checked");
    let t_checked = t0.elapsed();
    let mem_ready = s.memory();

    let file = folder.join("scripts/mp/gametypes/_globallogic.gsc");
    let text = std::fs::read_to_string(&file).unwrap();
    let u = uri(&file);
    s.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": u, "languageId": "gsc", "version": 1, "text": text }}),
    );
    let at = |p: &Value| json!({ "textDocument": { "uri": u }, "position": p });
    let def_pos = find(&text, "visionset_mgr::register_info(", 17);
    let builtin_pos = find(&text, "isSplitScreen()", 2);
    let field_pos = find(&text, "level.splitscreen", 6);
    let qual_pos = find(&text, "visionset_mgr::register_info(", 15);

    let (d, _) = s.call("textDocument/definition", at(&def_pos));
    assert!(!d.as_array().unwrap().is_empty(), "definition: {d}");

    let mut comp_general = Vec::new();
    let mut comp_field = Vec::new();
    let mut comp_qual = Vec::new();
    let mut defs = Vec::new();
    let mut refs_fn = Vec::new();
    let mut refs_builtin = Vec::new();
    let (mut n_general, mut n_refs_fn, mut n_refs_builtin) = (0, 0, 0);
    for _ in 0..20 {
        let (r, t) = s.call("textDocument/completion", at(&builtin_pos));
        n_general = r["items"].as_array().map_or(0, Vec::len);
        comp_general.push(t);
        comp_field.push(s.call("textDocument/completion", at(&field_pos)).1);
        comp_qual.push(s.call("textDocument/completion", at(&qual_pos)).1);
        defs.push(s.call("textDocument/definition", at(&def_pos)).1);
        let (r, t) = s.call("textDocument/references", with_context(at(&def_pos)));
        n_refs_fn = r.as_array().map_or(0, Vec::len);
        refs_fn.push(t);
        let (r, t) = s.call("textDocument/references", with_context(at(&builtin_pos)));
        n_refs_builtin = r.as_array().map_or(0, Vec::len);
        refs_builtin.push(t);
    }

    // After an edit, a query pays for the re-parse and the lazy re-resolution.
    let mut after: [Vec<Duration>; 3] = Default::default();
    let mut version = 1;
    let end = text.lines().count();
    for i in 0..10 {
        for (j, out) in after.iter_mut().enumerate() {
            version += 1;
            s.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": u, "version": version },
                    "contentChanges": [{
                        "range": { "start": { "line": end, "character": 0 },
                                   "end": { "line": end, "character": 0 } },
                        "text": format!("// {i}{j}\n"),
                    }],
                }),
            );
            let (method, p) = match j {
                0 => ("textDocument/completion", at(&builtin_pos)),
                1 => ("textDocument/definition", at(&def_pos)),
                _ => ("textDocument/references", with_context(at(&def_pos))),
            };
            out.push(s.call(method, p).1);
        }
    }
    let checked_after = s.wait_for(|m| m["method"] == "gsc/checked");
    let mem_end = s.memory();

    // Whole-file requests on the 4,200-line file.
    let doc = json!({ "textDocument": { "uri": u } });
    let mut sem = Vec::new();
    let mut n_tokens = 0;
    let mut syms = Vec::new();
    let mut hovers = Vec::new();
    for _ in 0..5 {
        let (r, t) = s.call("textDocument/semanticTokens/full", doc.clone());
        n_tokens = r["data"].as_array().map_or(0, Vec::len) / 5;
        sem.push(t);
        syms.push(s.call("textDocument/documentSymbol", doc.clone()).1);
        hovers.push(s.call("textDocument/hover", at(&def_pos)).1);
    }

    s.shutdown();

    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let [edit_comp, edit_def, edit_refs] = after;
    println!(
        "\n== gruff lsp on T7: {} files ({} primary) ==",
        status["files"], status["primaryFiles"]
    );
    println!("server: {}", init["serverInfo"]);
    println!(
        "initialize -> ready (loaded, resolved): {:6.0} ms (server-side {} ms: {})",
        ms(t_ready),
        ready["params"]["ms"],
        ready["params"]["phases"]
    );
    println!(
        "initialize -> first diagnostics:        {:6.0} ms (check {} ms)",
        ms(t_checked),
        checked["params"]["ms"]
    );
    println!(
        "re-check after the edits:               {:>6} ms",
        checked_after["params"]["ms"]
    );
    println!(
        "memory after the first check: {:.0} MiB (peak {:.0}); at the end: {:.0} MiB (peak {:.0})",
        mem_ready.0, mem_ready.1, mem_end.0, mem_end.1
    );
    println!(
        "completion, general ({n_general} items): {}",
        stats(comp_general)
    );
    println!("completion after `level.`:       {}", stats(comp_field));
    println!("completion after `ns::`:         {}", stats(comp_qual));
    println!("definition:                      {}", stats(defs));
    println!("hover:                           {}", stats(hovers));
    println!("semantic tokens ({n_tokens}): {}", stats(sem));
    println!("document symbols:                {}", stats(syms));
    println!("references, function ({n_refs_fn}): {}", stats(refs_fn));
    println!(
        "references, builtin ({n_refs_builtin}): {}",
        stats(refs_builtin)
    );
    println!("after an edit, completion:       {}", stats(edit_comp));
    println!("after an edit, definition:       {}", stats(edit_def));
    println!("after an edit, references:       {}", stats(edit_refs));
}
