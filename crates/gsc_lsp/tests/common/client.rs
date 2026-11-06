//! An in-process client: the server runs on a thread of the test, over in-memory pipes.

use std::collections::VecDeque;
use std::io::{BufReader, Read, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{read_message, uri};

/// The read end of an in-memory pipe.
pub struct PipeReader {
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
pub struct PipeWriter(mpsc::Sender<Vec<u8>>);

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

pub struct Client {
    to_server: PipeWriter,
    incoming: mpsc::Receiver<Value>,
    /// Notifications and server requests seen while waiting for responses.
    pub seen: Vec<Value>,
    next: i64,
    pub server: Option<std::thread::JoinHandle<Result<(), String>>>,
}

impl Client {
    pub fn start() -> Client {
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

    /// Start a server on `folder` read as `game`, with the data in `data`, open `open` and
    /// wait until the server has checked them (the workspace is loaded then).
    pub fn session(folder: &Path, data: &Path, game: &str, open: &[&Path]) -> Client {
        let mut c = Client::start();
        c.ok(
            "initialize",
            json!({
                "processId": null,
                "rootUri": uri(folder),
                "workspaceFolders": [{ "uri": uri(folder), "name": "ws" }],
                "capabilities": {},
                "initializationOptions": { "dataDir": data, "game": game, "debounceMs": 20 },
            }),
        );
        c.notify("initialized", json!({}));
        for p in open {
            let text = std::fs::read_to_string(p).unwrap();
            c.notify(
                "textDocument/didOpen",
                json!({ "textDocument": {
                    "uri": uri(p), "languageId": "gsc", "version": 1, "text": text
                }}),
            );
        }
        // Requests answer `null` until the workspace is loaded.
        let first = uri(open[0]);
        let deadline = Instant::now() + Duration::from_secs(60);
        while c
            .ok(
                "textDocument/documentSymbol",
                json!({ "textDocument": { "uri": first } }),
            )
            .is_null()
        {
            assert!(Instant::now() < deadline, "the workspace did not load");
            std::thread::sleep(Duration::from_millis(10));
        }
        c
    }

    pub fn send(&mut self, v: Value) {
        let body = serde_json::to_string(&v).unwrap();
        let framed = format!("Content-Length: {}\r\n\r\n{body}", body.len());
        self.to_server.write_all(framed.as_bytes()).unwrap();
    }

    pub fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    fn next_message(&mut self, deadline: Instant) -> Value {
        let left = deadline.saturating_duration_since(Instant::now());
        let m = self
            .incoming
            .recv_timeout(left)
            .unwrap_or_else(|_| panic!("timed out; seen: {:#?}", self.seen));
        if m.get("method").is_some() && m.get("id").is_some() {
            let id = m["id"].clone();
            self.send(json!({ "jsonrpc": "2.0", "id": id, "result": null }));
        }
        m
    }

    /// Send a request and wait for its response; returns `result` or `error`.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, Value> {
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

    pub fn ok(&mut self, method: &str, params: Value) -> Value {
        self.call(method, params)
            .unwrap_or_else(|e| panic!("{method} failed: {e}"))
    }

    /// Wait for a notification matching `pred` (also searching those already seen).
    pub fn wait_for(&mut self, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
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

    pub fn shutdown(mut self) {
        self.ok("shutdown", json!(null));
        self.notify("exit", json!(null));
        self.server.take().unwrap().join().unwrap().unwrap();
    }
}
