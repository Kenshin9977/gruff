//! The lint worker: runs `gsc_check` over the workspace on its own thread, debounced and
//! incremental ([`Session`]: only the files an edit can affect are re-checked), and publishes
//! the diagnostics that changed.
//!
//! The main loop bumps a generation counter on every change and sends [`Job::Run`]. The worker
//! waits until no change came for [`crate::DEBOUNCE`], checks under a read lock, and holds the
//! publication back when the generation moved meanwhile (a newer run is queued). It keeps the last
//! diagnostics per file, with a hash of the text they were computed on, for code actions.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use gsc_check::incremental::Session;
use gsc_check::{Diagnostic, Options, Selection};
use gsc_sema::FileId;
use lsp_server::{Message, Notification};
use lsp_types::{DiagnosticSeverity, DiagnosticTag, NumberOrString, PublishDiagnosticsParams};

use crate::analysis::Analysis;
use crate::convert::{Encoding, LineIndex};
use crate::project::Project;

/// The lint options of a project.
pub fn options<'a>(project: &'a Project, selection: &'a Selection) -> Options<'a> {
    Options {
        selection,
        per_file_ignores: &project.check.per_file_ignores,
        root: project
            .check
            .path
            .as_ref()
            .map(|_| project.check.root.as_path()),
    }
}

/// The rule selection of a project.
pub fn selection(project: &Project) -> Selection {
    Selection {
        layers: vec![project.check.layer.clone()],
        preview: project.check.preview.unwrap_or(false),
    }
}

pub fn text_hash(text: &str) -> u64 {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// The last lint results of a file.
#[derive(Clone, Debug)]
pub struct FileDiags {
    pub text_hash: u64,
    pub diags: Vec<Diagnostic>,
}

/// State shared with the main loop.
#[derive(Default)]
pub struct Shared {
    pub results: Mutex<HashMap<FileId, FileDiags>>,
    /// How long the last check took, and how many runs finished (tests and logs).
    pub last_run: Mutex<Option<Duration>>,
    pub runs: AtomicU64,
}

pub enum Job {
    /// The workspace changed: check it once changes pause.
    Run,
    /// A new workspace (after a load), with the session of its first check and the files
    /// that check produced.
    World(
        Arc<RwLock<Analysis>>,
        Arc<Project>,
        Box<Session>,
        Vec<FileId>,
    ),
    Stop,
}

pub struct Worker {
    pub tx: Sender<Job>,
    pub shared: Arc<Shared>,
    pub generation: Arc<AtomicU64>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    pub fn spawn(out: Sender<Message>, enc: Encoding, debounce: Duration, notify: bool) -> Worker {
        let (tx, rx) = crossbeam_channel::unbounded();
        let shared = Arc::new(Shared::default());
        let generation = Arc::new(AtomicU64::new(0));
        let (s, g) = (shared.clone(), generation.clone());
        let handle = std::thread::Builder::new()
            .name("gsc-lint".to_owned())
            .stack_size(256 << 20)
            .spawn(move || {
                let env = Env {
                    out,
                    shared: s,
                    generation: g,
                    enc,
                    notify,
                };
                worker(rx, env, debounce)
            })
            .expect("spawn the lint worker");
        Worker {
            tx,
            shared,
            generation,
            handle: Some(handle),
        }
    }

    /// The workspace changed: bump the generation and schedule a check.
    pub fn changed(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let _ = self.tx.send(Job::Run);
    }

    pub fn stop(&mut self) {
        let _ = self.tx.send(Job::Stop);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn severity(code: &str) -> DiagnosticSeverity {
    match code.as_bytes().first() {
        Some(b'E' | b'D') => DiagnosticSeverity::ERROR,
        Some(b'S') => DiagnosticSeverity::HINT,
        _ => DiagnosticSeverity::WARNING,
    }
}

pub fn to_lsp(d: &Diagnostic, text: &str, idx: &LineIndex, enc: Encoding) -> lsp_types::Diagnostic {
    let mut message = d.message.clone();
    if let Some(f) = &d.fix {
        message.push_str(&format!(
            " [fix: {}{}]",
            f.message,
            if f.applicability == gsc_check::Applicability::Unsafe {
                ", unsafe"
            } else {
                ""
            }
        ));
    }
    lsp_types::Diagnostic {
        range: idx.range(text, d.range, enc),
        severity: Some(severity(&d.code)),
        code: Some(NumberOrString::String(d.code.clone())),
        source: Some("gruff".to_owned()),
        message,
        tags: (d.code == "S100").then(|| vec![DiagnosticTag::UNNECESSARY]),
        ..Default::default()
    }
}

/// What a check run needs besides the workspace.
struct Env {
    out: Sender<Message>,
    shared: Arc<Shared>,
    generation: Arc<AtomicU64>,
    enc: Encoding,
    /// Send `gsc/checked` after each run.
    notify: bool,
}

/// What the worker keeps between runs.
#[derive(Default)]
struct Lint {
    world: Option<(Arc<RwLock<Analysis>>, Arc<Project>)>,
    session: Session,
    /// What each file was last published with.
    published: HashMap<FileId, (lsp_types::Uri, Vec<lsp_types::Diagnostic>)>,
    /// URIs of the previous world, cleared after the first run of the new one.
    stale: Vec<lsp_types::Uri>,
    /// Files re-checked but not yet published (a run whose text changed meanwhile).
    unpublished: HashSet<FileId>,
}

fn worker(rx: Receiver<Job>, env: Env, debounce: Duration) {
    let mut lint = Lint::default();
    let mut pending = false;
    loop {
        let job = if pending {
            match rx.recv_timeout(debounce) {
                Ok(j) => j,
                Err(RecvTimeoutError::Timeout) => {
                    pending = false;
                    run(&mut lint, &env);
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match rx.recv() {
                Ok(j) => j,
                Err(_) => return,
            }
        };
        match job {
            Job::Run => pending = true,
            Job::World(a, p, session, checked) => {
                lint.stale
                    .extend(lint.published.drain().map(|(_, (uri, _))| uri));
                lint.session = *session;
                env.shared.results.lock().expect("results lock").clear();
                lint.unpublished = checked.into_iter().collect();
                lint.world = Some((a, p));
                pending = true;
            }
            Job::Stop => return,
        }
    }
}

fn run(lint: &mut Lint, env: &Env) {
    let Some((analysis, project)) = lint.world.clone() else {
        return;
    };
    let Env {
        out,
        shared,
        generation,
        enc,
        notify,
    } = env;
    let (enc, notify) = (*enc, *notify);
    let start_gen = generation.load(Ordering::SeqCst);
    let t0 = Instant::now();
    let a = match analysis.read() {
        Ok(a) => a,
        Err(p) => p.into_inner(),
    };
    let selection = selection(&project);
    let up = lint.session.update(&a.ws, &options(&project, &selection));
    let check_ms = t0.elapsed();
    lint.unpublished.extend(up.rechecked.iter().copied());
    let mut notes = Vec::new();
    {
        let mut results = shared.results.lock().expect("results lock");
        for f in &up.removed {
            results.remove(f);
            if let Some((uri, _)) = lint.published.remove(f) {
                notes.push((uri, Vec::new()));
            }
        }
        for &f in &lint.unpublished {
            let diags = lint.session.diagnostics(f).unwrap_or_default();
            results.insert(
                f,
                FileDiags {
                    text_hash: text_hash(a.ws.file(f).text()),
                    diags: diags.to_vec(),
                },
            );
        }
    }
    if generation.load(Ordering::SeqCst) != start_gen {
        // The text changed while checking: a newer run is queued and publishes these.
        return;
    }
    let mut files: Vec<FileId> = lint.unpublished.drain().collect();
    files.sort_unstable();
    for f in files {
        if a.ws.get(f).is_none() {
            continue;
        }
        let text = a.ws.file(f).text();
        let idx = LineIndex::new(text);
        let diags: Vec<lsp_types::Diagnostic> = lint
            .session
            .diagnostics(f)
            .unwrap_or_default()
            .iter()
            .map(|d| to_lsp(d, text, &idx, enc))
            .collect();
        let uri = a.uri(f);
        match lint.published.get(&f) {
            Some((u, old)) if *u == uri && *old == diags => continue,
            Some((u, _)) if *u != uri => notes.push((u.clone(), Vec::new())),
            None if diags.is_empty() => continue,
            _ => {}
        }
        notes.push((uri.clone(), diags.clone()));
        if diags.is_empty() {
            lint.published.remove(&f);
        } else {
            lint.published.insert(f, (uri, diags));
        }
    }
    // URIs of the previous world nothing republished.
    let live: HashSet<&lsp_types::Uri> = lint.published.values().map(|(u, _)| u).collect();
    for uri in lint.stale.drain(..) {
        if !live.contains(&uri) {
            notes.push((uri, Vec::new()));
        }
    }
    drop(a);
    *shared.last_run.lock().expect("timing lock") = Some(check_ms);
    for (uri, diagnostics) in notes {
        let _ = out.send(Message::Notification(Notification::new(
            "textDocument/publishDiagnostics".to_owned(),
            PublishDiagnosticsParams {
                uri,
                diagnostics,
                version: None,
            },
        )));
    }
    shared.runs.fetch_add(1, Ordering::SeqCst);
    if !notify {
        return;
    }
    // Tell tests and benchmarks that a run finished (opt-in: `gscTestHooks` in the options).
    let _ = out.send(Message::Notification(Notification::new(
        "gsc/checked".to_owned(),
        serde_json::json!({
            "ms": t0.elapsed().as_millis() as u64,
            "generation": start_gen,
            "check_ms": check_ms.as_secs_f64() * 1000.0,
            "rechecked": up.rechecked.len(),
            "full": up.full,
            "phases": up.phases,
            "summaries": up.summaries,
        }),
    )));
}
