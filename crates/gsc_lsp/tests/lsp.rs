//! Drive the server like an editor: real LSP JSON messages, framed with `Content-Length`, over
//! in-memory pipes. The fixtures (`tests/fixtures`) are a two-file mod and a tiny made-up
//! builtin catalogue, read as the superset dialect.

mod common;

use std::collections::VecDeque;
use std::io::{BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use common::{read_message, uri};
use serde_json::{json, Value};

/// The read end of an in-memory pipe.
struct PipeReader {
    rx: mpsc::Receiver<Vec<u8>>,
    buf: VecDeque<u8>,
}

impl Read for PipeReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.buf.is_empty() {
            match self.rx.recv() {
                Ok(chunk) => self.buf.extend(chunk),
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.buf.len());
        for (i, b) in self.buf.drain(..n).enumerate() {
            out[i] = b;
        }
        Ok(n)
    }
}

/// The write end of an in-memory pipe.
struct PipeWriter(mpsc::Sender<Vec<u8>>);

impl Write for PipeWriter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0
            .send(b.to_vec())
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?;
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn pipe() -> (PipeWriter, PipeReader) {
    let (tx, rx) = mpsc::channel();
    (
        PipeWriter(tx),
        PipeReader {
            rx,
            buf: VecDeque::new(),
        },
    )
}

struct Client {
    to_server: PipeWriter,
    incoming: mpsc::Receiver<Value>,
    /// Notifications and server requests seen while waiting for responses.
    seen: Vec<Value>,
    next: i64,
    server: Option<std::thread::JoinHandle<Result<(), String>>>,
}

impl Client {
    fn start() -> Client {
        let (to_server, server_in) = pipe();
        let (server_out, from_server) = pipe();
        let server = std::thread::Builder::new()
            .stack_size(256 << 20)
            .spawn(move || gsc_lsp::serve(BufReader::new(server_in), server_out))
            .unwrap();
        let (tx, incoming) = mpsc::channel();
        std::thread::spawn(move || {
            let mut r = BufReader::new(from_server);
            while let Some(m) = read_message(&mut r) {
                if tx.send(m).is_err() {
                    break;
                }
            }
        });
        Client {
            to_server,
            incoming,
            seen: Vec::new(),
            next: 0,
            server: Some(server),
        }
    }

    fn send(&mut self, v: Value) {
        let body = serde_json::to_string(&v).unwrap();
        let framed = format!("Content-Length: {}\r\n\r\n{body}", body.len());
        self.to_server.write_all(framed.as_bytes()).unwrap();
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    fn next_message(&mut self, deadline: Instant) -> Value {
        let left = deadline.saturating_duration_since(Instant::now());
        let m = self
            .incoming
            .recv_timeout(left)
            .unwrap_or_else(|_| panic!("timed out; seen: {:#?}", self.seen));
        // Answer the server's own requests (progress tokens) like a client would.
        if m.get("method").is_some() && m.get("id").is_some() {
            let id = m["id"].clone();
            self.send(json!({ "jsonrpc": "2.0", "id": id, "result": null }));
        }
        m
    }

    /// Send a request and wait for its response; returns `result` or `error`.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, Value> {
        self.next += 1;
        let id = self.next;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let m = self.next_message(deadline);
            if m.get("id") == Some(&json!(id)) && m.get("method").is_none() {
                return match m.get("error") {
                    Some(e) => Err(e.clone()),
                    None => Ok(m["result"].clone()),
                };
            }
            self.seen.push(m);
        }
    }

    fn ok(&mut self, method: &str, params: Value) -> Value {
        self.call(method, params)
            .unwrap_or_else(|e| panic!("{method} failed: {e}"))
    }

    /// Wait for a notification matching `pred` (also searching those already seen).
    fn wait_for(&mut self, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
        if let Some(i) = self.seen.iter().position(&pred) {
            return self.seen.remove(i);
        }
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let m = self.next_message(deadline);
            if pred(&m) {
                return m;
            }
            if Instant::now() > deadline {
                panic!("no {what}");
            }
            self.seen.push(m);
        }
    }
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// The diagnostics of a `publishDiagnostics` notification for `uri`, else None.
fn diagnostics_for<'a>(m: &'a Value, uri: &str) -> Option<&'a Vec<Value>> {
    (m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri)
        .then(|| m["params"]["diagnostics"].as_array())
        .flatten()
}

/// Is `m` a `$/progress` notification of this `kind` (begin, report, end)?
fn is_progress(m: &Value, kind: &str) -> bool {
    m["method"] == "$/progress" && m["params"]["value"]["kind"] == kind
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

fn start_session() -> (Client, String, String, String) {
    let ws = fixtures().join("ws");
    let main = ws.join("scripts/main.gsc");
    let util = ws.join("scripts/util.gsc");
    let mut c = Client::start();
    let init = c.ok(
        "initialize",
        json!({
            "processId": null,
            "rootUri": uri(&ws),
            "workspaceFolders": [{ "uri": uri(&ws), "name": "ws" }],
            "capabilities": { "window": { "workDoneProgress": true } },
            "initializationOptions": {
                "dataDir": fixtures().join("data"),
                "gscTestHooks": true,
                "debounceMs": 20,
            },
        }),
    );
    let caps = &init["capabilities"];
    assert_eq!(caps["textDocumentSync"]["change"], 2, "incremental sync");
    assert_eq!(
        caps["completionProvider"]["triggerCharacters"],
        json!([".", ":", ">"])
    );
    assert_eq!(caps["renameProvider"]["prepareProvider"], true);
    c.notify("initialized", json!({}));
    let text = std::fs::read_to_string(&main).unwrap();
    // Progress goes begin -> end before the workspace is ready.
    let begin = c.wait_for("progress begin", |m| is_progress(m, "begin"));
    assert_eq!(begin["params"]["value"]["title"], "Loading GSC workspace");
    c.wait_for("progress end", |m| is_progress(m, "end"));
    c.wait_for("ready", |m| m["method"] == "gsc/ready");
    let main_uri = uri(&main);
    c.notify(
        "textDocument/didOpen",
        json!({ "textDocument": {
            "uri": main_uri, "languageId": "gsc", "version": 1, "text": text
        }}),
    );
    (c, main_uri, uri(&util), text)
}

#[test]
fn editor_session() {
    let (mut c, main, util, text) = start_session();

    let status = c.ok("gsc/status", json!(null));
    assert_eq!(status["game"], "superset");
    assert_eq!(status["gameSource"], "gsc.toml");
    assert_eq!(status["files"], 2);
    assert_eq!(status["builtins"], 4);

    // Diagnostics: L001 (enabled by gruff.toml) on the loop of `spin`.
    let diags = c.wait_for("main.gsc diagnostics", |m| {
        diagnostics_for(m, &main).is_some()
    });
    let list = diags["params"]["diagnostics"].as_array().unwrap().clone();
    let l001 = list
        .iter()
        .find(|d| d["code"] == "L001")
        .unwrap_or_else(|| panic!("{list:#?}"));
    assert_eq!(l001["range"]["start"], pos(14, 1));
    assert_eq!(l001["source"], "gruff");

    // Completion after `util::` lists the namespace's functions.
    let items = c.ok(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": main }, "position": pos(6, 15),
            "context": { "triggerKind": 2, "triggerCharacter": ":" },
        }),
    );
    let labels: Vec<&str> = items["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["label"].as_str().unwrap())
        .collect();
    assert!(labels.contains(&"helper"), "{labels:?}");
    // A lone `:` or `>` is not a completion trigger.
    let none = c.ok(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": main }, "position": pos(6, 9),
            "context": { "triggerKind": 2, "triggerCharacter": ">" },
        }),
    );
    assert!(none.is_null());
    // General completion: locals, parameters, file functions and builtins.
    let items = c.ok("textDocument/completion", doc_pos(&main, 7, 15));
    let labels: Vec<&str> = items["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["label"].as_str().unwrap())
        .collect();
    for want in ["total", "count", "spin", "spawn", "randomint"] {
        assert!(labels.contains(&want), "{want} not in {labels:?}");
    }

    // Definition of `util::helper` jumps into util.gsc.
    let def = c.ok("textDocument/definition", doc_pos(&main, 6, 17));
    let def = &def[0];
    assert_eq!(def["targetUri"], json!(util));
    assert_eq!(def["targetSelectionRange"]["start"], pos(5, 9));

    // Hover shows the signature and the doc comment.
    let hover = c.ok("textDocument/hover", doc_pos(&main, 6, 17));
    let md = hover["contents"]["value"].as_str().unwrap();
    assert!(md.contains("function util::helper( a, b )"), "{md}");
    assert!(md.contains("Adds two numbers"), "{md}");

    // References: the declaration, the call in main.gsc and the one in util.gsc.
    let refs = c.ok(
        "textDocument/references",
        json!({
            "textDocument": { "uri": util }, "position": pos(5, 10),
            "context": { "includeDeclaration": true },
        }),
    );
    assert_eq!(refs.as_array().unwrap().len(), 3, "{refs:#?}");

    // Signature help of a builtin: parameter types from the catalogue.
    let sig = c.ok("textDocument/signatureHelp", doc_pos(&main, 8, 29));
    let s0 = &sig["signatures"][0];
    assert_eq!(s0["label"], "spawn( string, [vector], [int] )");
    assert_eq!(sig["activeParameter"], 1);
    // ... and of a script function: its parameters.
    let sig = c.ok("textDocument/signatureHelp", doc_pos(&main, 6, 23));
    assert_eq!(sig["signatures"][0]["label"], "util::helper( a, b )");
    assert_eq!(sig["activeParameter"], 0);

    // Document and workspace symbols.
    let syms = c.ok(
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": main } }),
    );
    let names: Vec<&str> = syms
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["init", "spin", "messy"]);
    let ws_syms = c.ok("workspace/symbol", json!({ "query": "help" }));
    assert_eq!(ws_syms[0]["name"], "helper");

    // Call hierarchy: `helper` is called from `init` and `private_thing`.
    let items = c.ok("textDocument/prepareCallHierarchy", doc_pos(&util, 5, 10));
    let incoming = c.ok("callHierarchy/incomingCalls", json!({ "item": items[0] }));
    let mut callers: Vec<&str> = incoming
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["from"]["name"].as_str().unwrap())
        .collect();
    callers.sort();
    assert_eq!(callers, ["init", "private_thing"]);
    let items = c.ok("textDocument/prepareCallHierarchy", doc_pos(&main, 4, 10));
    let outgoing = c.ok("callHierarchy/outgoingCalls", json!({ "item": items[0] }));
    let callees: Vec<&str> = outgoing
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["to"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(callees, ["helper", "spin"]);

    // Rename a script function: every file changes; builtins are refused.
    let prep = c.ok("textDocument/prepareRename", doc_pos(&main, 6, 17));
    assert_eq!(prep["placeholder"], "helper");
    let edit = c.ok(
        "textDocument/rename",
        json!({
            "textDocument": { "uri": main }, "position": pos(6, 17), "newName": "add_two",
        }),
    );
    let changes = edit["changes"].as_object().unwrap();
    assert_eq!(changes.len(), 2);
    let new_main = apply_edits(&text, changes[&main].as_array().unwrap());
    assert!(new_main.contains("util::add_two( count, 2 )"), "{new_main}");
    assert_eq!(changes[&util].as_array().unwrap().len(), 2);
    let err = c
        .call(
            "textDocument/rename",
            json!({
                "textDocument": { "uri": main }, "position": pos(8, 9), "newName": "x",
            }),
        )
        .unwrap_err();
    assert!(
        err["message"].as_str().unwrap().contains("builtin"),
        "{err}"
    );
    assert!(c
        .call("textDocument/prepareRename", doc_pos(&main, 8, 9))
        .is_err());
    // Rename a local: only its uses in its function.
    let edit = c.ok(
        "textDocument/rename",
        json!({
            "textDocument": { "uri": main }, "position": pos(6, 2), "newName": "sum",
        }),
    );
    let edits = edit["changes"][&main].as_array().unwrap();
    assert_eq!(edits.len(), 2);
    let renamed = apply_edits(&text, edits);
    assert!(renamed.contains("sum = util::helper") && renamed.contains("self.health = sum;"));
    // Not an identifier.
    assert!(c
        .call(
            "textDocument/rename",
            json!({
                "textDocument": { "uri": main }, "position": pos(6, 2), "newName": "1x",
            }),
        )
        .is_err());

    // Formatting: one edit, and the result is what `gruff format` writes.
    let edits = c.ok(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": main },
            "options": { "tabSize": 4, "insertSpaces": false },
        }),
    );
    let edits = edits.as_array().unwrap();
    assert_eq!(edits.len(), 1);
    let formatted = apply_edits(&text, edits);
    assert!(
        formatted.contains("function messy( a )\n{\n\treturn a;\n}"),
        "{formatted}"
    );
    // Range formatting: only the statement under the range changes.
    let edits = c.ok(
        "textDocument/rangeFormatting",
        json!({
            "textDocument": { "uri": main },
            "range": { "start": pos(20, 0), "end": pos(20, 1) },
            "options": { "tabSize": 4, "insertSpaces": false },
        }),
    );
    let edits = edits.as_array().unwrap();
    assert_eq!(edits.len(), 1);
    let ranged = apply_edits(&text, edits);
    assert!(
        ranged.contains("function messy( a )\n{\n\treturn a;\n}"),
        "{ranged}"
    );

    // Code actions: the L001 fix, marked unsafe.
    let actions = c.ok(
        "textDocument/codeAction",
        json!({
            "textDocument": { "uri": main },
            "range": { "start": pos(14, 3), "end": pos(14, 3) },
            "context": { "diagnostics": [] },
        }),
    );
    let fix = actions
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["title"].as_str().unwrap().contains("L001"))
        .unwrap_or_else(|| panic!("{actions:#?}"));
    assert!(fix["title"].as_str().unwrap().contains("unsafe"));
    assert_eq!(fix["isPreferred"], false);
    let fixed = apply_edits(&text, fix["edit"]["changes"][&main].as_array().unwrap());
    assert!(
        fixed.contains("n = randomint( 10 );\n\t\twait 0.05;\n"),
        "{fixed}"
    );

    // Semantic tokens: 5 integers per token; `helper` is a function, `spawn` a builtin.
    let toks = c.ok(
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": main } }),
    );
    let data: Vec<u32> = toks["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u32)
        .collect();
    assert_eq!(data.len() % 5, 0);
    let mut decoded = Vec::new();
    let (mut line, mut col) = (0, 0);
    for t in data.chunks(5) {
        if t[0] > 0 {
            line += t[0];
            col = t[1];
        } else {
            col += t[1];
        }
        decoded.push((line, col, t[2], t[3], t[4]));
    }
    let at = |l: u32, c: u32| decoded.iter().find(|t| t.0 == l && t.1 == c).copied();
    assert_eq!(at(6, 9).map(|t| t.3), Some(0), "util: namespace");
    assert_eq!(at(6, 15).map(|t| t.3), Some(1), "helper: function");
    assert_eq!(at(8, 7).map(|t| (t.3, t.4)), Some((1, 4)), "spawn: builtin");
    assert_eq!(at(6, 23).map(|t| t.3), Some(3), "count: parameter");
    assert_eq!(at(6, 1).map(|t| t.3), Some(4), "total: local");
    assert_eq!(at(7, 6).map(|t| t.3), Some(5), "health: field");

    // Incremental edits: add a function, then complete it.
    c.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": main, "version": 2 },
            "contentChanges": [
                { "range": { "start": pos(21, 0), "end": pos(21, 0) },
                  "text": "function fresh_one() {}\n" },
                { "range": { "start": pos(21, 9), "end": pos(21, 14) },
                  "text": "brand" },
            ],
        }),
    );
    let items = c.ok("textDocument/completion", doc_pos(&main, 7, 15));
    let labels: Vec<&str> = items["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["label"].as_str().unwrap())
        .collect();
    assert!(labels.contains(&"brand_one"), "{labels:?}");
    // The lint worker re-checks after the edit: L001 is still there.
    c.wait_for("re-check", |m| m["method"] == "gsc/checked");

    // A queued request cancelled in the same batch is answered with RequestCancelled.
    let id = 999;
    c.send(
        json!({ "jsonrpc": "2.0", "id": id, "method": "textDocument/hover",
        "params": doc_pos(&main, 6, 17) }),
    );
    c.notify("$/cancelRequest", json!({ "id": id }));
    let resp = c.wait_for("cancel response", |m| m["id"] == json!(id));
    // The request may have run before the cancellation arrived.
    assert!(
        resp.get("result").is_some() || resp["error"]["code"] == -32800,
        "{resp}"
    );

    c.ok("shutdown", json!(null));
    c.notify("exit", json!(null));
    let server = c.server.take().unwrap();
    server.join().unwrap().unwrap();
}

#[test]
fn closing_reverts_to_disk_and_unknown_files_work() {
    let (mut c, main, _util, _text) = start_session();
    // An open buffer that breaks the file, then closed: diagnostics return to the disk text.
    c.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": main, "version": 2 },
            "contentChanges": [{ "text": "function broken( {\n" }],
        }),
    );
    let d = c.wait_for("syntax error", |m| {
        diagnostics_for(m, &main).is_some_and(|v| {
            v.iter()
                .any(|d| d["code"].as_str().is_some_and(|c| c.starts_with('E')))
        })
    });
    assert_eq!(d["params"]["diagnostics"][0]["severity"], 1);
    // Formatting refuses a file with syntax errors.
    let err = c
        .call(
            "textDocument/formatting",
            json!({ "textDocument": { "uri": main }, "options": { "tabSize": 4, "insertSpaces": false } }),
        )
        .unwrap_err();
    assert!(
        err["message"].as_str().unwrap().contains("syntax error"),
        "{err}"
    );
    c.notify(
        "textDocument/didClose",
        json!({ "textDocument": { "uri": main } }),
    );
    c.wait_for("disk diagnostics", |m| {
        diagnostics_for(m, &main).is_some_and(|v| v.iter().any(|d| d["code"] == "L001"))
    });
    // A file outside every folder still gets diagnostics and queries.
    let loose = std::env::temp_dir().join(format!("gsc_lsp_loose_{}.gsc", std::process::id()));
    let loose_uri = uri(&loose);
    c.notify(
        "textDocument/didOpen",
        json!({ "textDocument": {
            "uri": loose_uri, "languageId": "gsc", "version": 1,
            "text": "function f( a )\n{\n\twhile ( 1 ) { a = a + 1; }\n}\n"
        }}),
    );
    c.wait_for("loose diagnostics", |m| {
        diagnostics_for(m, &loose_uri).is_some()
    });
    let refs = c.ok(
        "textDocument/references",
        json!({
            "textDocument": { "uri": loose_uri }, "position": pos(2, 15),
            "context": { "includeDeclaration": true },
        }),
    );
    assert_eq!(refs.as_array().unwrap().len(), 3, "{refs:#?}");
    // Closed, it leaves the workspace even though it exists on disk: its diagnostics clear.
    std::fs::write(&loose, "function f() { while ( 1 ) {} }\n").unwrap();
    c.notify(
        "textDocument/didClose",
        json!({ "textDocument": { "uri": loose_uri } }),
    );
    c.wait_for("cleared loose diagnostics", |m| {
        diagnostics_for(m, &loose_uri).is_some_and(Vec::is_empty)
    });
    let _ = std::fs::remove_file(&loose);
    let status = c.ok("gsc/status", json!(null));
    assert_eq!(status["files"], 2);
    c.ok("shutdown", json!(null));
    c.notify("exit", json!(null));
    c.server.take().unwrap().join().unwrap().unwrap();
}
