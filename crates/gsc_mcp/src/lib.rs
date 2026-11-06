//! `gruff mcp`: a Model Context Protocol server for coding agents, over stdio.
//!
//! Read-only tools over the language server's core ([`gsc_sema`], [`gsc_check`], the event
//! tables, the gruff-data catalogue, [`gsc_hash`]), with compact paginated text outputs.
//! Newline-delimited JSON-RPC 2.0, dual-era: modern requests (2026-07-28, version in `_meta`)
//! are served statelessly; `initialize` selects a legacy revision. See `docs/mcp.md`.

mod catalogue;
mod tools;
mod world;

use std::io::{BufRead, Write};

use serde_json::{json, Value};

pub use world::Config;

/// The modern revision this server implements.
pub const MODERN: &str = "2026-07-28";
/// Handshake-based revisions answered on `initialize`, newest first.
pub const LEGACY: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";

const INSTRUCTIONS: &str = "Read-only queries over a GSC (Call of Duty script) workspace. \
Start with gsc_workspace. Names are `ns::function` or `function`; locations are `path:line`. \
Results are paginated: pass the `offset` named on the last line to continue.";

fn supported() -> Vec<&'static str> {
    std::iter::once(MODERN)
        .chain(LEGACY.iter().copied())
        .collect()
}

fn error(id: &Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut e = json!({ "code": code, "message": message });
    if let Some(d) = data {
        e["data"] = d;
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": e })
}

fn server_info() -> Value {
    json!({ "name": "gruff", "title": "Gruff (GSC toolchain)", "version": env!("CARGO_PKG_VERSION") })
}

/// The protocol state of one connection: the legacy revision once `initialize` chose one.
pub struct Server {
    tools: tools::Tools,
    legacy: Option<String>,
}

impl Server {
    pub fn new(config: Config) -> Server {
        Server {
            tools: tools::Tools::new(config),
            legacy: None,
        }
    }

    /// The legacy revision `initialize` negotiated, if any.
    pub fn legacy(&self) -> Option<&str> {
        self.legacy.as_deref()
    }

    /// Handle one message; the response to write, if any.
    pub fn handle(&mut self, line: &str) -> Option<Value> {
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                return Some(error(
                    &Value::Null,
                    -32700,
                    &format!("parse error: {e}"),
                    None,
                ))
            }
        };
        if !msg.is_object() {
            return Some(error(
                &Value::Null,
                -32600,
                "invalid request: one JSON-RPC object per line",
                None,
            ));
        }
        let method = msg.get("method").and_then(Value::as_str);
        let id = msg.get("id").cloned();
        let (Some(method), Some(id)) = (method, id) else {
            // A notification or a response: nothing to answer.
            return None;
        };
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let requested = params
            .get("_meta")
            .and_then(|m| m.get(META_VERSION))
            .and_then(Value::as_str);
        let modern = match requested {
            Some(v) if v == MODERN => true,
            Some(v) if LEGACY.contains(&v) => false,
            Some(v) => {
                return Some(error(
                    &id,
                    -32022,
                    "Unsupported protocol version",
                    Some(json!({ "supported": supported(), "requested": v })),
                ))
            }
            None => false,
        };
        let result = match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str);
                let v = asked
                    .filter(|a| LEGACY.contains(a))
                    .unwrap_or(LEGACY[0])
                    .to_owned();
                self.legacy = Some(v.clone());
                Ok(json!({
                    "protocolVersion": v,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": server_info(),
                    "instructions": INSTRUCTIONS,
                }))
            }
            "server/discover" => Ok(json!({
                "supportedVersions": supported(),
                "capabilities": { "tools": {} },
                "_meta": { "io.modelcontextprotocol/serverInfo": server_info() },
                "instructions": INSTRUCTIONS,
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools::list() })),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                match self.tools.call(name, &args) {
                    Some(r) => Ok(r),
                    None => Err((-32602, format!("Unknown tool: {name}"))),
                }
            }
            _ => Err((-32601, format!("Method not found: {method}"))),
        };
        Some(match result {
            Ok(mut r) => {
                if modern {
                    r["resultType"] = json!("complete");
                }
                json!({ "jsonrpc": "2.0", "id": id, "result": r })
            }
            Err((code, m)) => error(&id, code, &m, None),
        })
    }
}

/// Serve newline-delimited JSON-RPC over `reader` and `writer` until the end of `reader`.
pub fn serve(config: Config, reader: impl BufRead, mut writer: impl Write) -> Result<(), String> {
    let mut server = Server::new(config);
    for line in reader.lines() {
        let line = line.map_err(|e| format!("read: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(resp) = server.handle(&line) {
            let s = serde_json::to_string(&resp).map_err(|e| e.to_string())?;
            writeln!(writer, "{s}").map_err(|e| format!("write: {e}"))?;
            writer.flush().map_err(|e| format!("write: {e}"))?;
        }
    }
    Ok(())
}

/// Serve on standard input and output.
pub fn run_stdio(config: Config) -> Result<(), String> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve(config, stdin.lock(), stdout.lock())
}
