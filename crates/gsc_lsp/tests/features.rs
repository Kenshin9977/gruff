//! The editor features beyond the basics (folding, links, highlights, lenses, inlay hints,
//! implementations, type definitions, type hierarchy, members and fields, macros, literal and
//! auto-import completion, import code actions), driven like an editor over in-memory pipes.
//! `fixtures/ws_features` is read as the superset dialect (`#using`), `fixtures/ws_merge` as
//! a made-up `#include` dialect.

mod common;

use std::path::PathBuf;

use common::client::Client;
use common::uri;
use serde_json::{json, Value};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn pos(line: u32, character: u32) -> Value {
    json!({ "line": line, "character": character })
}

fn doc_pos(u: &str, line: u32, character: u32) -> Value {
    json!({ "textDocument": { "uri": u }, "position": pos(line, character) })
}

/// Apply LSP text edits (UTF-16 positions, ASCII text here) to `text`.
fn apply_edits(text: &str, edits: &[Value]) -> String {
    let idx = gsc_lsp::convert::LineIndex::new(text);
    let mut es: Vec<(usize, usize, String)> = edits
        .iter()
        .map(|e| {
            let r: lsp_types::Range = serde_json::from_value(e["range"].clone()).unwrap();
            let tr = idx.text_range(text, r, gsc_lsp::convert::Encoding::Utf16);
            (
                usize::from(tr.start()),
                usize::from(tr.end()),
                e["newText"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    es.sort_by_key(|e| std::cmp::Reverse(e.0));
    let mut out = text.to_owned();
    for (s, e, t) in es {
        out.replace_range(s..e, &t);
    }
    out
}

fn labels(v: &Value) -> Vec<String> {
    let items = v.get("items").unwrap_or(v);
    items
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["label"].as_str().unwrap().to_owned())
        .collect()
}

/// `(line, character)` of each location or location link.
fn places(v: &Value) -> Vec<(String, u64, u64)> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|l| {
            let (u, r) = match l.get("targetUri") {
                Some(u) => (u, &l["targetSelectionRange"]),
                None => (&l["uri"], &l["range"]),
            };
            let file = u.as_str().unwrap().rsplit('/').next().unwrap().to_owned();
            (
                file,
                r["start"]["line"].as_u64().unwrap(),
                r["start"]["character"].as_u64().unwrap(),
            )
        })
        .collect()
}

#[test]
fn navigation_and_structure() {
    let ws = fixtures().join("ws_features");
    let main = ws.join("scripts/main.gsc");
    let shapes = ws.join("scripts/shapes.gsc");
    let mut c = Client::session(&ws, &fixtures().join("data"), "superset", &[&main, &shapes]);
    let (m, s) = (uri(&main), uri(&shapes));

    // Capabilities are advertised by the requests answering at all; folding: the function body
    // folds from its `function` line to the line before its `}`, the import block folds.
    let folds = c.ok(
        "textDocument/foldingRange",
        json!({ "textDocument": { "uri": m } }),
    );
    let folds = folds.as_array().unwrap();
    assert!(
        folds
            .iter()
            .any(|f| f["startLine"] == 7 && f["endLine"] == 19),
        "{folds:#?}"
    );
    assert!(folds
        .iter()
        .any(|f| f["startLine"] == 0 && f["kind"] == "imports"));

    // Selection ranges grow from the name outwards.
    let sel = c.ok(
        "textDocument/selectionRange",
        json!({ "textDocument": { "uri": m }, "positions": [pos(15, 10)] }),
    );
    let first = &sel[0];
    assert_eq!(first["range"]["start"], pos(15, 9));
    assert_eq!(first["range"]["end"], pos(15, 14));
    assert!(first["parent"]["parent"].is_object(), "{first}");

    // Document links: the four directives resolve (the duplicate too).
    let links = c.ok(
        "textDocument/documentLink",
        json!({ "textDocument": { "uri": m } }),
    );
    let targets: Vec<&str> = links
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["target"].as_str().unwrap())
        .collect();
    assert_eq!(targets.len(), 4, "{links:#?}");
    assert!(targets.iter().any(|t| t.ends_with("macros.gsh")));

    // Highlights: the parameter `count` is written at its declaration, read twice.
    let hl = c.ok("textDocument/documentHighlight", doc_pos(&m, 7, 16));
    let kinds: Vec<u64> = hl
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["kind"].as_u64().unwrap())
        .collect();
    assert_eq!(kinds, [3, 2, 2], "{hl:#?}");

    // Code lenses: classes count `new` and base lists; methods count their calls.
    let lenses = c.ok(
        "textDocument/codeLens",
        json!({ "textDocument": { "uri": s } }),
    );
    let titles: Vec<(u64, String)> = lenses
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            (
                l["range"]["start"]["line"].as_u64().unwrap(),
                l["command"]["title"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert!(titles.contains(&(2, "1 reference".into())), "{titles:?}");
    assert!(titles.contains(&(18, "2 references".into())), "{titles:?}");
    assert_eq!(lenses[0]["command"]["command"], "gruff.showReferences");

    // Implementations: the override of a method; a field's plain writes and what they bind.
    let imp = c.ok("textDocument/implementation", doc_pos(&s, 12, 11));
    assert_eq!(places(&imp), [("shapes.gsc".into(), 20, 10)]);
    let imp = c.ok("textDocument/implementation", doc_pos(&m, 9, 8));
    let got = places(&imp);
    assert!(got.contains(&("main.gsc".into(), 9, 7)), "{got:?}");
    assert!(got.contains(&("main.gsc".into(), 27, 9)), "{got:?}");

    // Type definition: `sq` holds a `cSquare`.
    let td = c.ok("textDocument/typeDefinition", doc_pos(&m, 12, 1));
    assert_eq!(places(&td), [("shapes.gsc".into(), 18, 6)]);

    // Type hierarchy: cSquare's supertype is cShape, whose subtype is cSquare.
    let items = c.ok("textDocument/prepareTypeHierarchy", doc_pos(&s, 18, 7));
    assert_eq!(items[0]["name"], "csquare");
    let sup = c.ok("typeHierarchy/supertypes", json!({ "item": items[0] }));
    assert_eq!(sup[0]["name"], "cshape");
    let sub = c.ok("typeHierarchy/subtypes", json!({ "item": sup[0] }));
    assert_eq!(sub[0]["name"], "csquare");

    // Class members: a bare name in a method reaches the `var`, in a subclass too.
    let def = c.ok("textDocument/definition", doc_pos(&s, 22, 3));
    assert_eq!(places(&def), [("shapes.gsc".into(), 4, 5)]);
    let refs = c.ok(
        "textDocument/references",
        json!({ "textDocument": { "uri": s }, "position": pos(4, 6),
                "context": { "includeDeclaration": true } }),
    );
    assert_eq!(refs.as_array().unwrap().len(), 3, "{refs:#?}");
    let hover = c.ok("textDocument/hover", doc_pos(&s, 9, 3));
    let md = hover["contents"]["value"].as_str().unwrap();
    assert!(md.contains("var cshape::area"), "{md}");
    let edit = c.ok(
        "textDocument/rename",
        json!({ "textDocument": { "uri": s }, "position": pos(22, 3), "newName": "extent" }),
    );
    let text = std::fs::read_to_string(&shapes).unwrap();
    let renamed = apply_edits(&text, edit["changes"][&s].as_array().unwrap());
    assert!(
        renamed.contains("var extent;")
            && renamed.contains("extent = 0;")
            && renamed.contains("extent = 4;"),
        "{renamed}"
    );

    // Fields: definition lists the writes; rename reaches every use.
    let def = c.ok("textDocument/definition", doc_pos(&m, 10, 8));
    assert_eq!(
        places(&def),
        [("main.gsc".into(), 10, 7), ("main.gsc".into(), 11, 7)]
    );
    let edit = c.ok(
        "textDocument/rename",
        json!({ "textDocument": { "uri": m }, "position": pos(10, 8), "newName": "sum" }),
    );
    assert_eq!(edit["changes"][&m].as_array().unwrap().len(), 2);

    // Classes: references from `new` and the base list.
    let refs = c.ok(
        "textDocument/references",
        json!({ "textDocument": { "uri": m }, "position": pos(12, 11),
                "context": { "includeDeclaration": false } }),
    );
    assert_eq!(refs.as_array().unwrap().len(), 2, "{refs:#?}");

    c.shutdown();
}

#[test]
fn hints_macros_completion_and_imports() {
    let ws = fixtures().join("ws_features");
    let main = ws.join("scripts/main.gsc");
    let mut c = Client::session(&ws, &fixtures().join("data"), "superset", &[&main]);
    let m = uri(&main);
    let text = std::fs::read_to_string(&main).unwrap();

    // Inlay hints: parameter names (not where the argument spells the name), macro parameter
    // names, and the types of locals the value says.
    let hints = c.ok(
        "textDocument/inlayHint",
        json!({ "textDocument": { "uri": m },
                "range": { "start": pos(0, 0), "end": pos(30, 0) } }),
    );
    let got: Vec<(u64, u64, String)> = hints
        .as_array()
        .unwrap()
        .iter()
        .map(|h| {
            (
                h["position"]["line"].as_u64().unwrap(),
                h["position"]["character"].as_u64().unwrap(),
                h["label"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    for want in [
        (15, 9, "a:"),
        (15, 16, "b:"),
        (16, 9, "a:"),
        (16, 12, "b:"),
        (17, 13, "v:"),
        (13, 2, ": int"),
        (14, 6, ": string"),
        (12, 3, ": cSquare"),
    ] {
        assert!(
            got.contains(&(want.0, want.1, want.2.to_owned())),
            "{want:?} not in {got:?}"
        );
    }

    // Macros: hover and signature help show the expansion at the use.
    let hover = c.ok("textDocument/hover", doc_pos(&m, 17, 6));
    let md = hover["contents"]["value"].as_str().unwrap();
    assert!(
        md.contains("expands to") && md.contains("( count * 2 )"),
        "{md}"
    );
    let sig = c.ok("textDocument/signatureHelp", doc_pos(&m, 17, 14));
    assert_eq!(sig["signatures"][0]["label"], "DOUBLE( v )");

    // Literal completion inside a string: the event names of the workspace.
    let items = c.ok("textDocument/completion", doc_pos(&m, 14, 10));
    let got = labels(&items);
    assert!(
        got.contains(&"spawned".to_owned()) && got.contains(&"round_start".to_owned()),
        "{got:?}"
    );

    // Auto-import: type `far` in `helper`; the functions of `extra` come qualified, with the
    // `#using` added after the last one.
    c.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": m, "version": 2 },
            "contentChanges": [{ "range": { "start": pos(24, 1), "end": pos(24, 1) },
                                 "text": "far;\n\t" }],
        }),
    );
    let items = c.ok("textDocument/completion", doc_pos(&m, 24, 4));
    let item = items["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["label"] == "extra::farther_still")
        .unwrap_or_else(|| panic!("{items:#?}"))
        .clone();
    let edits = item["additionalTextEdits"].as_array().unwrap();
    let added = apply_edits(&text, edits);
    assert!(
        added.starts_with("#using scripts\\shapes;\n#using scripts\\shapes;\n#using scripts\\unused;\n#using scripts\\extra;\n#insert"),
        "{added}"
    );
    c.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": m, "version": 3 },
            "contentChanges": [{ "text": text }],
        }),
    );

    // Code actions: the duplicate `#using`, the missing one for `extra::far_away`, organize.
    let actions = c.ok(
        "textDocument/codeAction",
        json!({ "textDocument": { "uri": m },
                "range": { "start": pos(1, 3), "end": pos(1, 3) },
                "context": { "diagnostics": [] } }),
    );
    let titles: Vec<&str> = actions
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["title"].as_str().unwrap())
        .collect();
    assert!(
        titles.iter().any(|t| t.starts_with("Remove duplicate")),
        "{titles:?}"
    );
    let actions = c.ok(
        "textDocument/codeAction",
        json!({ "textDocument": { "uri": m },
                "range": { "start": pos(19, 9), "end": pos(19, 9) },
                "context": { "diagnostics": [] } }),
    );
    let add = actions
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["title"] == "Add `#using scripts\\extra;`")
        .unwrap_or_else(|| panic!("{actions:#?}"));
    let added = apply_edits(&text, add["edit"]["changes"][&m].as_array().unwrap());
    assert!(
        added.contains("#using scripts\\unused;\n#using scripts\\extra;\n"),
        "{added}"
    );
    let actions = c.ok(
        "textDocument/codeAction",
        json!({ "textDocument": { "uri": m },
                "range": { "start": pos(0, 0), "end": pos(0, 0) },
                "context": { "diagnostics": [], "only": ["source.organizeImports"] } }),
    );
    let org = &actions[0];
    assert_eq!(org["kind"], "source.organizeImports");
    let organized = apply_edits(&text, org["edit"]["changes"][&m].as_array().unwrap());
    assert!(
        organized.starts_with("#using scripts\\shapes;\n#insert scripts\\macros.gsh;\n"),
        "{organized}"
    );

    c.shutdown();
}

#[test]
fn merge_dialect_imports_with_include() {
    let ws = fixtures().join("ws_merge");
    let main = ws.join("maps/mp/main.gsc");
    let mut c = Client::session(&ws, &fixtures().join("data"), "mergetest", &[&main]);
    let m = uri(&main);
    let text = std::fs::read_to_string(&main).unwrap();

    // The unresolved bare call offers the `#include` of the file that defines it.
    let actions = c.ok(
        "textDocument/codeAction",
        json!({ "textDocument": { "uri": m },
                "range": { "start": pos(4, 3), "end": pos(4, 3) },
                "context": { "diagnostics": [] } }),
    );
    let add = actions
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["title"] == "Add `#include maps\\mp\\util;`")
        .unwrap_or_else(|| panic!("{actions:#?}"));
    let added = apply_edits(&text, add["edit"]["changes"][&m].as_array().unwrap());
    assert!(
        added.starts_with("#include maps\\mp\\other;\n#include maps\\mp\\util;\n"),
        "{added}"
    );

    // Auto-import completes the bare name and adds the `#include`.
    let items = c.ok("textDocument/completion", doc_pos(&m, 4, 4));
    let item = items["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["label"] == "helper_func")
        .unwrap_or_else(|| panic!("{items:#?}"))
        .clone();
    assert_eq!(item["insertText"], "helper_func");
    let added = apply_edits(&text, item["additionalTextEdits"].as_array().unwrap());
    assert!(added.contains("#include maps\\mp\\util;\n"), "{added}");

    c.shutdown();
}
