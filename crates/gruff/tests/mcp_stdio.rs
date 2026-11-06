//! End to end: `gruff mcp` as a subprocess over stdio, on the language server's fixture mod (a
//! copy, so the test can edit it on disk), both protocol eras.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../gsc_lsp/tests/fixtures")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &to.join(e.file_name()));
        } else {
            std::fs::copy(&p, to.join(e.file_name())).unwrap();
        }
    }
}

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: i64,
}

impl Mcp {
    fn start(ws: &Path) -> Mcp {
        let mut child = Command::new(env!("CARGO_BIN_EXE_gruff"))
            .arg("mcp")
            .arg(ws)
            .arg("--data-dir")
            .arg(fixtures().join("data"))
            .args(["--rescan", "0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start gruff mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Mcp {
            child,
            stdin,
            stdout,
            next: 0,
        }
    }

    fn send(&mut self, v: &Value) {
        let line = serde_json::to_string(v).unwrap();
        assert!(!line.contains('\n'));
        writeln!(self.stdin, "{line}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn recv(&mut self) -> Value {
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line:?}"))
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let r = self.recv();
        assert_eq!(r["id"], json!(id), "{r}");
        r
    }

    /// A modern (per-request `_meta`) tool call: its text, and whether it is an error.
    fn tool(&mut self, name: &str, args: Value) -> (String, bool) {
        let r = self.request(
            "tools/call",
            json!({
                "name": name,
                "arguments": args,
                "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" },
            }),
        );
        let res = &r["result"];
        assert_eq!(res["resultType"], "complete", "{r}");
        (
            res["content"][0]["text"].as_str().unwrap().to_owned(),
            res["isError"] == json!(true),
        )
    }
}

#[test]
fn stdio_session() {
    let tmp = std::env::temp_dir().join(format!("gsc_mcp_e2e_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    copy_dir(&fixtures().join("ws"), &tmp.join("ws"));
    let ws = tmp.join("ws");
    let mut m = Mcp::start(&ws);

    // Modern era: discover, a bad version, the tool list.
    let d = m.request(
        "server/discover",
        json!({ "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" } }),
    );
    let versions = d["result"]["supportedVersions"].as_array().unwrap();
    assert!(versions.contains(&json!("2026-07-28")) && versions.contains(&json!("2025-11-25")));
    let bad = m.request(
        "tools/list",
        json!({ "_meta": { "io.modelcontextprotocol/protocolVersion": "1900-01-01" } }),
    );
    assert_eq!(bad["error"]["code"], -32022, "{bad}");
    let list = m.request("tools/list", json!({}));
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for n in [
        "gsc_workspace",
        "gsc_find_symbol",
        "gsc_definition",
        "gsc_references",
        "gsc_callers",
        "gsc_callees",
        "gsc_check",
        "gsc_events",
        "gsc_builtin",
        "gsc_hash",
    ] {
        assert!(names.contains(&n), "{n} in {names:?}");
    }

    let (t, err) = m.tool("gsc_workspace", json!({}));
    assert!(!err, "{t}");
    assert!(t.contains("game superset") && t.contains("2 files"), "{t}");

    let (t, _) = m.tool("gsc_find_symbol", json!({ "query": "help" }));
    assert!(
        t.contains("function util::helper( a, b )  scripts/util.gsc:6"),
        "{t}"
    );

    let (t, _) = m.tool("gsc_definition", json!({ "name": "util::helper" }));
    assert!(
        t.contains("scripts/util.gsc:6") && t.contains("Adds two numbers"),
        "{t}"
    );
    let (t, _) = m.tool(
        "gsc_definition",
        json!({ "file": "scripts/main.gsc", "line": 7, "name": "helper" }),
    );
    assert!(t.contains("scripts/util.gsc:6"), "by position: {t}");

    let (t, _) = m.tool("gsc_references", json!({ "name": "helper" }));
    assert!(t.starts_with("2 references"), "{t}");
    assert!(
        t.contains("scripts/main.gsc:7  total = util::helper( count, 2 );"),
        "{t}"
    );
    let (t, _) = m.tool("gsc_references", json!({ "name": "helper", "limit": 1 }));
    assert!(t.ends_with("... 1 more (offset=1)"), "{t}");
    let (t, _) = m.tool("gsc_references", json!({ "name": ".health" }));
    assert!(t.contains("scripts/main.gsc:8"), "field: {t}");

    let (t, _) = m.tool("gsc_callers", json!({ "name": "util::helper" }));
    assert!(t.contains("main::init  scripts/main.gsc:7"), "{t}");
    assert!(
        t.contains("util::private_thing  scripts/util.gsc:13"),
        "{t}"
    );
    let (t, _) = m.tool("gsc_callees", json!({ "name": "main::init" }));
    assert!(
        t.contains("util::helper  line 7 -> scripts/util.gsc:6"),
        "{t}"
    );
    assert!(t.contains("builtin spawn  line 9"), "{t}");

    let (t, _) = m.tool("gsc_check", json!({}));
    assert!(t.contains("scripts/main.gsc:15:2 L001"), "{t}");
    let (t, _) = m.tool("gsc_check", json!({ "codes": "B" }));
    assert!(t.starts_with("0 diagnostics"), "{t}");

    let (t, _) = m.tool("gsc_builtin", json!({ "name": "spawn" }));
    assert!(
        t.contains("gsc function spawn(string, vector, int)") && t.contains("1 to 3 arguments"),
        "{t}"
    );
    let (t, _) = m.tool("gsc_builtin", json!({ "name": "spwan" }));
    assert!(t.contains("did you mean spawn"), "{t}");

    let (t, _) = m.tool(
        "gsc_hash",
        json!({ "text": "helper", "family": "t7.canon" }),
    );
    let hex = t
        .split("0x")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    let (t, _) = m.tool(
        "gsc_hash",
        json!({ "value": format!("function_{hex}"), "family": "t7.canon" }),
    );
    assert!(t.contains("`helper`"), "{t}");

    let (t, _) = m.tool("gsc_events", json!({}));
    assert!(
        t.starts_with("0 event names") || t.contains("event names"),
        "{t}"
    );

    let (t, err) = m.tool("gsc_definition", json!({ "name": "nope" }));
    assert!(err && t.contains("no definition"), "{t}");
    let unknown = m.request("tools/call", json!({ "name": "nope", "arguments": {} }));
    assert_eq!(unknown["error"]["code"], -32602);

    // The disk changes between calls: the loop now waits, and a new file appears.
    let main = ws.join("scripts/main.gsc");
    let text = std::fs::read_to_string(&main).unwrap();
    let fixed = text.replace("n = randomint( 10 );", "n = randomint( 10 );\n\t\twait 1;");
    std::fs::write(&main, fixed).unwrap();
    std::fs::write(
        ws.join("scripts/extra.gsc"),
        "#namespace extra;\nfunction more() { util::helper( 3, 4 ); }\n",
    )
    .unwrap();
    let (t, _) = m.tool("gsc_check", json!({}));
    assert!(t.starts_with("0 diagnostics in 0 of 3 files"), "{t}");
    let (t, _) = m.tool("gsc_callers", json!({ "name": "util::helper" }));
    assert!(t.contains("extra::more  scripts/extra.gsc:2"), "{t}");

    // Legacy era: the handshake, then plain requests.
    let init = m.request(
        "initialize",
        json!({ "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" } }),
    );
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    m.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    let r = m.request(
        "tools/call",
        json!({ "name": "gsc_find_symbol", "arguments": { "query": "spin" } }),
    );
    assert!(r["result"].get("resultType").is_none());
    let t = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        t.contains("function main::spin()  scripts/main.gsc:13"),
        "{t}"
    );
    let ping = m.request("ping", json!({}));
    assert_eq!(ping["result"], json!({}));

    drop(m.stdin);
    let status = m.child.wait().unwrap();
    assert!(status.success());
    let _ = std::fs::remove_dir_all(&tmp);
}
