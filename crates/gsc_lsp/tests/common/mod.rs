//! Helpers shared by the LSP integration tests and benchmarks: LSP framing, a session with a
//! `gruff lsp` child process, and the inputs' default locations.
#![allow(dead_code)] // each test binary uses its own subset

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// The toolchain folder (the repository's parent): `corpus/` and `gruff-data/` (or the older
/// `gruff-data/`) sit there.
pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The path in environment variable `var`, else `default`.
pub fn env_path(var: &str, default: PathBuf) -> PathBuf {
    std::env::var_os(var).map_or(default, PathBuf::from)
}

/// `$GRUFF_DATA` or `$GSC_DATA`, else a `gruff-data` (or `gsc-data`) folder in [`root`].
pub fn data_dir() -> PathBuf {
    if let Some(d) = ["GRUFF_DATA", "GSC_DATA"]
        .iter()
        .find_map(|v| std::env::var_os(v).filter(|d| !d.is_empty()))
    {
        return PathBuf::from(d);
    }
    ["gruff-data", "gsc-data"]
        .iter()
        .map(|n| root().join(n))
        .find(|d| d.is_dir())
        .unwrap_or_else(|| root().join("gruff-data"))
}

/// `GSC_BIN`, else the release `gruff` of this workspace.
pub fn release_bin() -> PathBuf {
    let exe = if cfg!(windows) { "gruff.exe" } else { "gruff" };
    env_path(
        "GSC_BIN",
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/release")
            .join(exe),
    )
}

pub fn uri(p: &Path) -> String {
    gsc_lsp::convert::path_to_uri(p).as_str().to_owned()
}

/// One `Content-Length` framed message, or None at the end of the stream.
pub fn read_message(r: &mut impl BufRead) -> Option<Value> {
    let mut len = None;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(v) = line.strip_prefix("Content-Length: ") {
            len = v.parse::<usize>().ok();
        }
    }
    let mut body = vec![0; len?];
    r.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

/// "median .. p90 .. max .." of millisecond samples, with `decimals` digits.
pub fn stats(mut v: Vec<f64>, decimals: usize) -> String {
    v.sort_by(f64::total_cmp);
    format!(
        "median {:7.d$} ms   p90 {:7.d$} ms   max {:7.d$} ms   (n={})",
        v[v.len() / 2],
        v[(v.len() * 9 / 10).min(v.len() - 1)],
        v[v.len() - 1],
        v.len(),
        d = decimals
    )
}

/// A `gruff lsp --stdio` child process driven like an editor.
pub struct Session {
    pub child: Child,
    stdin: ChildStdin,
    rx: mpsc::Receiver<Value>,
    next: i64,
    /// How long `recv` waits for a server message before failing.
    pub timeout: Duration,
}

impl Session {
    pub fn spawn(bin: &Path, timeout: Duration) -> Session {
        let mut child = Command::new(bin)
            .args(["lsp", "--stdio"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start gruff lsp");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            while let Some(m) = read_message(&mut r) {
                if tx.send(m).is_err() {
                    break;
                }
            }
        });
        Session {
            child,
            stdin,
            rx,
            next: 0,
            timeout,
        }
    }

    pub fn send(&mut self, v: Value) {
        let body = serde_json::to_string(&v).unwrap();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        self.stdin.flush().unwrap();
    }

    pub fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// Answer the server's own requests (progress tokens) like a client would.
    fn answer(&mut self, m: &Value) {
        if m.get("method").is_some() && m.get("id").is_some() {
            let id = m["id"].clone();
            self.send(json!({ "jsonrpc": "2.0", "id": id, "result": null }));
        }
    }

    pub fn recv(&mut self) -> Value {
        let m = self.rx.recv_timeout(self.timeout).expect("server message");
        self.answer(&m);
        m
    }

    /// Send a request and wait for its result (an error response fails the test), with the
    /// round-trip time.
    pub fn call(&mut self, method: &str, params: Value) -> (Value, Duration) {
        self.next += 1;
        let id = self.next;
        let t0 = Instant::now();
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let m = self.recv();
            if m.get("id") == Some(&json!(id)) && m.get("method").is_none() {
                assert!(m.get("error").is_none(), "{method}: {m}");
                return (m["result"].clone(), t0.elapsed());
            }
        }
    }

    pub fn wait_for(&mut self, pred: impl Fn(&Value) -> bool) -> Value {
        loop {
            let m = self.recv();
            if pred(&m) {
                return m;
            }
        }
    }

    /// Wait for the next notification `method`.
    pub fn wait_method(&mut self, method: &str) -> Value {
        self.wait_for(|m| m["method"] == method)
    }

    /// Wait until the server has been quiet for `quiet`.
    pub fn settle(&mut self, quiet: Duration) {
        while let Ok(m) = self.rx.recv_timeout(quiet) {
            self.answer(&m);
        }
    }

    /// Working set and peak working set, in MiB.
    pub fn memory(&self) -> (f64, f64) {
        let pid = self.child.id();
        if cfg!(windows) {
            let script = format!(
                "$p = Get-Process -Id {pid}; \"$($p.WorkingSet64) $($p.PeakWorkingSet64)\""
            );
            let out = Command::new("powershell")
                .args(["-NoProfile", "-Command", &script])
                .output()
                .expect("powershell");
            let v: Vec<f64> = String::from_utf8_lossy(&out.stdout)
                .split_whitespace()
                .filter_map(|x| x.parse().ok())
                .collect();
            (v[0] / 1048576.0, v[1] / 1048576.0)
        } else {
            let s = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
            let kb = |k: &str| {
                s.lines()
                    .find(|l| l.starts_with(k))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|x| x.parse::<f64>().ok())
                    .unwrap_or(0.0)
                    / 1024.0
            };
            (kb("VmRSS:"), kb("VmHWM:"))
        }
    }

    /// `shutdown`, `exit`, and wait for the process.
    pub fn shutdown(mut self) {
        self.call("shutdown", json!(null));
        self.notify("exit", json!(null));
        let _ = self.child.wait();
    }
}
