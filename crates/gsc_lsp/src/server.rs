//! The main loop: the handshake, open documents, workspace loading, and dispatch.
//!
//! The loop drains every message waiting on the connection into a batch before acting:
//! `$/cancelRequest`s cancel requests of the same batch, and the edits of a batch reach the
//! workspace in one `set_text` per file, just before the next request needs them (or at the end
//! of the batch). Requests run on this thread against a read lock; the lint worker reads the
//! same workspace on its own thread.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crossbeam_channel::{select, Receiver, Sender};
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::*;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::analysis::Analysis;
use crate::convert::{path_key, uri_to_path, Encoding, LineIndex};
use crate::diagnostics::{Job, Worker};
use crate::handlers::{Ctx, Refused};
use crate::project::{Features, Project, Settings};

/// How long the lint worker waits after the last change before checking.
pub const DEBOUNCE: Duration = Duration::from_millis(75);

const PROGRESS_TOKEN: &str = "gsc/load";

struct Doc {
    path: PathBuf,
    text: String,
    dirty: bool,
}

struct World {
    analysis: Arc<RwLock<Analysis>>,
    project: Arc<Project>,
}

struct Loaded {
    generation: u64,
    analysis: Analysis,
    project: Project,
    elapsed: Duration,
    /// The first check, and the files it checked.
    session: Box<gsc_check::incremental::Session>,
    checked: Vec<gsc_sema::FileId>,
    /// `(phase, milliseconds)`.
    phases: Vec<(&'static str, u64)>,
    errors: Vec<String>,
}

struct Server {
    out: Sender<Message>,
    enc: Encoding,
    folders: Vec<PathBuf>,
    settings: Settings,
    features: Features,
    docs: HashMap<Uri, Doc>,
    world: Option<World>,
    load_generation: u64,
    load_started: Instant,
    loaded_tx: Sender<Loaded>,
    loaded_rx: Receiver<Loaded>,
    lint: Worker,
    progress: bool,
    next_id: i32,
}

fn negotiate(caps: &ClientCapabilities) -> Encoding {
    let utf8 = caps
        .general
        .as_ref()
        .and_then(|g| g.position_encodings.as_ref())
        .is_some_and(|v| v.contains(&PositionEncodingKind::UTF8));
    if utf8 {
        Encoding::Utf8
    } else {
        Encoding::Utf16
    }
}

fn capabilities(enc: Encoding) -> Value {
    let caps = ServerCapabilities {
        position_encoding: Some(match enc {
            Encoding::Utf8 => PositionEncodingKind::UTF8,
            Encoding::Utf16 => PositionEncodingKind::UTF16,
        }),
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::INCREMENTAL),
                will_save: None,
                will_save_wait_until: None,
                save: None,
            },
        )),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".into(), ":".into(), ">".into()]),
            ..Default::default()
        }),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".into(), ",".into()]),
            retrigger_characters: None,
            work_done_progress_options: Default::default(),
        }),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![
                CodeActionKind::QUICKFIX,
                CodeActionKind::SOURCE_ORGANIZE_IMPORTS,
                CodeActionKind::new("source.fixAll.gsc"),
            ]),
            ..Default::default()
        })),
        document_formatting_provider: Some(OneOf::Left(true)),
        document_range_formatting_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                legend: crate::semantic::legend(),
                full: Some(SemanticTokensFullOptions::Bool(true)),
                range: None,
                work_done_progress_options: Default::default(),
            },
        )),
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
        document_highlight_provider: Some(OneOf::Left(true)),
        document_link_provider: Some(DocumentLinkOptions {
            resolve_provider: Some(false),
            work_done_progress_options: Default::default(),
        }),
        code_lens_provider: Some(CodeLensOptions {
            resolve_provider: Some(false),
        }),
        inlay_hint_provider: Some(OneOf::Left(true)),
        implementation_provider: Some(ImplementationProviderCapability::Simple(true)),
        type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
        ..Default::default()
    };
    let mut v = serde_json::to_value(caps).unwrap_or_default();
    // Not in lsp-types' ServerCapabilities yet.
    v["typeHierarchyProvider"] = Value::Bool(true);
    v
}

/// Run the server on an initialized-to-be connection until `exit`.
pub fn run(conn: Connection) -> Result<(), String> {
    let (id, params) = conn
        .initialize_start()
        .map_err(|e| format!("initialize: {e}"))?;
    let init: InitializeParams =
        serde_json::from_value(params).map_err(|e| format!("initialize params: {e}"))?;
    let enc = negotiate(&init.capabilities);
    let result = serde_json::json!({
        "capabilities": capabilities(enc),
        "serverInfo": { "name": "gruff", "version": env!("CARGO_PKG_VERSION") },
    });
    conn.initialize_finish(id, result)
        .map_err(|e| format!("initialize: {e}"))?;

    let opts = init.initialization_options.clone().unwrap_or(Value::Null);
    let hooks = opts
        .get("gscTestHooks")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let debounce = opts
        .get("debounceMs")
        .and_then(Value::as_u64)
        .map_or(DEBOUNCE, Duration::from_millis);
    let mut folders: Vec<PathBuf> = init
        .workspace_folders
        .iter()
        .flatten()
        .filter_map(|f| uri_to_path(&f.uri))
        .collect();
    #[allow(deprecated)]
    if folders.is_empty() {
        folders.extend(init.root_uri.as_ref().and_then(uri_to_path));
    }
    let progress = init
        .capabilities
        .window
        .as_ref()
        .and_then(|w| w.work_done_progress)
        .unwrap_or(false);
    let (loaded_tx, loaded_rx) = crossbeam_channel::unbounded();
    let mut s = Server {
        out: conn.sender.clone(),
        enc,
        folders,
        settings: Settings::from_json(&opts),
        features: Features::from_json(&opts),
        docs: HashMap::new(),
        world: None,
        load_generation: 0,
        load_started: Instant::now(),
        loaded_tx,
        loaded_rx,
        lint: Worker::spawn(conn.sender.clone(), enc, debounce, hooks),
        progress,
        next_id: 0,
    };
    s.start_load();
    let r = s.main_loop(&conn);
    s.lint.stop();
    r
}

fn params<T: DeserializeOwned>(req: &Request) -> Result<T, Response> {
    serde_json::from_value(req.params.clone()).map_err(|e| {
        Response::new_err(
            req.id.clone(),
            ErrorCode::InvalidParams as i32,
            format!("{}: {e}", req.method),
        )
    })
}

fn ok<T: serde::Serialize>(id: RequestId, v: T) -> Response {
    Response::new_ok(id, v)
}

impl Server {
    fn notify(&self, method: &str, params: impl serde::Serialize) {
        let _ = self.out.send(Message::Notification(Notification::new(
            method.to_owned(),
            params,
        )));
    }

    fn log(&self, typ: MessageType, message: String) {
        self.notify("window/logMessage", LogMessageParams { typ, message });
    }

    fn request(&mut self, method: &str, params: impl serde::Serialize) {
        self.next_id += 1;
        let id = RequestId::from(format!("gsc-{}", self.next_id));
        let _ = self.out.send(Message::Request(Request::new(
            id,
            method.to_owned(),
            params,
        )));
    }

    /// Load (or reload) the workspace on a background thread, reporting progress.
    fn start_load(&mut self) {
        self.load_generation += 1;
        self.load_started = Instant::now();
        let generation = self.load_generation;
        let token = NumberOrString::String(format!("{PROGRESS_TOKEN}/{generation}"));
        if self.progress {
            self.request(
                "window/workDoneProgress/create",
                WorkDoneProgressCreateParams {
                    token: token.clone(),
                },
            );
        }
        let progress = self.progress;
        let out = self.out.clone();
        let tx = self.loaded_tx.clone();
        let settings = self.settings.clone();
        let folders = self.folders.clone();
        let send = move |p: WorkDoneProgress| {
            if progress {
                let _ = out.send(Message::Notification(Notification::new(
                    "$/progress".to_owned(),
                    ProgressParams {
                        token: token.clone(),
                        value: ProgressParamsValue::WorkDone(p),
                    },
                )));
            }
        };
        let spawned = std::thread::Builder::new()
            .name("gsc-load".to_owned())
            .stack_size(256 << 20)
            .spawn(move || {
                let t0 = Instant::now();
                send(WorkDoneProgress::Begin(WorkDoneProgressBegin {
                    title: "Loading GSC workspace".to_owned(),
                    cancellable: Some(false),
                    message: Some("reading settings".to_owned()),
                    percentage: Some(0),
                }));
                let mut phases = Vec::new();
                let mut lap = Instant::now();
                let mut phase = |name: &'static str| {
                    phases.push((name, lap.elapsed().as_millis() as u64));
                    lap = Instant::now();
                };
                let project = Project::resolve(&settings, &folders);
                phase("settings and data");
                let mut ws = project.workspace();
                send(WorkDoneProgress::Report(WorkDoneProgressReport {
                    cancellable: Some(false),
                    message: Some(format!(
                        "parsing {} root(s) as {}",
                        ws.roots().len(),
                        project.dialect.id
                    )),
                    percentage: Some(10),
                }));
                let report = ws.load();
                phase("discover and parse");
                send(WorkDoneProgress::Report(WorkDoneProgressReport {
                    cancellable: Some(false),
                    message: Some("resolving names".to_owned()),
                    percentage: Some(70),
                }));
                ws.resolve_all();
                phase("resolve");
                // The first check runs here, before the library trees go (the summaries need
                // them once; later changes re-parse what they touch).
                let mut session = Box::new(gsc_check::incremental::Session::new());
                let selection = crate::diagnostics::selection(&project);
                let up = session.update(&ws, &crate::diagnostics::options(&project, &selection));
                phase("check");
                ws.drop_library_trees();
                phase("free library trees");
                let n = ws.len();
                send(WorkDoneProgress::End(WorkDoneProgressEnd {
                    message: Some(format!("{n} files")),
                }));
                let analysis = Analysis::new(ws);
                phase("index");
                let _ = tx.send(Loaded {
                    generation,
                    analysis,
                    project,
                    elapsed: t0.elapsed(),
                    session,
                    checked: up.rechecked,
                    phases,
                    errors: report.errors,
                });
            });
        if let Err(e) = spawned {
            self.log(MessageType::ERROR, format!("cannot start loading: {e}"));
        }
    }

    fn install(&mut self, l: Loaded) {
        if l.generation != self.load_generation {
            return;
        }
        let phases: Vec<String> = l
            .phases
            .iter()
            .map(|(n, ms)| format!("{n} {ms} ms"))
            .collect();
        let msg = format!(
            "gruff: loaded {} files in {} ms ({}); {}",
            l.analysis.ws.len(),
            l.elapsed.as_millis(),
            phases.join(", "),
            l.project.describe()
        );
        self.log(MessageType::INFO, msg);
        for e in l.errors.iter().take(20) {
            self.log(MessageType::WARNING, format!("gruff: {e}"));
        }
        for w in &l.project.warnings {
            self.log(MessageType::WARNING, format!("gruff: {w}"));
        }
        if let Some(w) = l.project.warnings.first() {
            self.notify(
                "window/showMessage",
                ShowMessageParams {
                    typ: MessageType::WARNING,
                    message: format!("gruff: {w}"),
                },
            );
        }
        let world = World {
            analysis: Arc::new(RwLock::new(l.analysis)),
            project: Arc::new(l.project),
        };
        let _ = self.lint.tx.send(Job::World(
            world.analysis.clone(),
            world.project.clone(),
            l.session,
            l.checked,
        ));
        self.world = Some(world);
        for d in self.docs.values_mut() {
            d.dirty = true;
        }
        self.flush();
        self.notify(
            "gsc/ready",
            serde_json::json!({
                "ms": self.load_started.elapsed().as_millis() as u64,
                "phases": l.phases.iter().map(|(n, ms)| serde_json::json!([n, ms])).collect::<Vec<_>>(),
            }),
        );
    }

    /// Bring the workspace up to date with the open documents.
    fn flush(&mut self) {
        let Some(w) = &self.world else { return };
        if !self.docs.values().any(|d| d.dirty) {
            return;
        }
        let mut a = match w.analysis.write() {
            Ok(a) => a,
            Err(p) => p.into_inner(),
        };
        let mut changed = false;
        for (uri, d) in self.docs.iter_mut().filter(|(_, d)| d.dirty) {
            changed |= a.open(uri, &d.path, &d.text).1;
            d.dirty = false;
        }
        drop(a);
        if changed {
            self.lint.changed();
        }
    }

    fn main_loop(&mut self, conn: &Connection) -> Result<(), String> {
        loop {
            let mut batch: VecDeque<Message> = VecDeque::new();
            select! {
                recv(conn.receiver) -> m => match m {
                    Ok(m) => batch.push_back(m),
                    Err(_) => return Ok(()),
                },
                recv(self.loaded_rx) -> l => {
                    if let Ok(l) = l {
                        self.install(l);
                    }
                    continue;
                }
            }
            while let Ok(m) = conn.receiver.try_recv() {
                batch.push_back(m);
            }
            let cancelled: HashSet<RequestId> = batch
                .iter()
                .filter_map(|m| match m {
                    Message::Notification(n) if n.method == "$/cancelRequest" => {
                        let p: CancelParams = serde_json::from_value(n.params.clone()).ok()?;
                        Some(match p.id {
                            NumberOrString::Number(i) => RequestId::from(i),
                            NumberOrString::String(s) => RequestId::from(s),
                        })
                    }
                    _ => None,
                })
                .collect();
            for msg in batch {
                match msg {
                    Message::Request(req) => {
                        if conn
                            .handle_shutdown(&req)
                            .map_err(|e| format!("shutdown: {e}"))?
                        {
                            return Ok(());
                        }
                        if cancelled.contains(&req.id) {
                            let _ = self.out.send(Message::Response(Response::new_err(
                                req.id,
                                ErrorCode::RequestCanceled as i32,
                                "cancelled".to_owned(),
                            )));
                            continue;
                        }
                        self.flush();
                        let resp = self.handle(req);
                        let _ = self.out.send(Message::Response(resp));
                    }
                    Message::Notification(n) => {
                        if n.method == "exit" {
                            return Ok(());
                        }
                        self.on_notification(n);
                    }
                    Message::Response(_) => {}
                }
            }
            self.flush();
        }
    }

    fn on_notification(&mut self, n: Notification) {
        match n.method.as_str() {
            "textDocument/didOpen" => {
                let Ok(p) = serde_json::from_value::<DidOpenTextDocumentParams>(n.params) else {
                    return;
                };
                let Some(path) = uri_to_path(&p.text_document.uri) else {
                    return;
                };
                self.docs.insert(
                    p.text_document.uri,
                    Doc {
                        path,
                        text: p.text_document.text,
                        dirty: true,
                    },
                );
            }
            "textDocument/didChange" => {
                let Ok(p) = serde_json::from_value::<DidChangeTextDocumentParams>(n.params) else {
                    return;
                };
                let enc = self.enc;
                let Some(d) = self.docs.get_mut(&p.text_document.uri) else {
                    return;
                };
                for c in p.content_changes {
                    match c.range {
                        Some(r) => {
                            let idx = LineIndex::new(&d.text);
                            let tr = idx.text_range(&d.text, r, enc);
                            d.text.replace_range(
                                usize::from(tr.start())..usize::from(tr.end()),
                                &c.text,
                            );
                        }
                        None => d.text = c.text,
                    }
                }
                d.dirty = true;
            }
            "textDocument/didClose" => {
                let Ok(p) = serde_json::from_value::<DidCloseTextDocumentParams>(n.params) else {
                    return;
                };
                let Some(d) = self.docs.remove(&p.text_document.uri) else {
                    return;
                };
                self.reload_from_disk(&d.path, true);
            }
            "workspace/didChangeWatchedFiles" => {
                let Ok(p) = serde_json::from_value::<DidChangeWatchedFilesParams>(n.params) else {
                    return;
                };
                let mut reload = false;
                for c in p.changes {
                    let Some(path) = uri_to_path(&c.uri) else {
                        continue;
                    };
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_ascii_lowercase())
                        .unwrap_or_default();
                    if gsc_check::config::CONFIG_FILES.contains(&name.as_str()) {
                        reload = true;
                        continue;
                    }
                    let open = self
                        .docs
                        .values()
                        .any(|d| path_key(&d.path) == path_key(&path));
                    if !open && gsc_sema::vfs::is_script(&path) {
                        self.reload_from_disk(&path, false);
                    }
                }
                if reload {
                    self.log(
                        MessageType::INFO,
                        "gruff: configuration changed, reloading".into(),
                    );
                    self.start_load();
                }
            }
            "workspace/didChangeConfiguration" => {
                let Ok(p) = serde_json::from_value::<DidChangeConfigurationParams>(n.params) else {
                    return;
                };
                self.features = Features::from_json(&p.settings);
                let s = Settings::from_json(&p.settings);
                if s != self.settings {
                    self.settings = s;
                    self.log(
                        MessageType::INFO,
                        "gruff: settings changed, reloading".into(),
                    );
                    self.start_load();
                }
            }
            _ => {}
        }
    }

    /// Make the workspace match the disk for `path` (a closed editor, a watched file event).
    fn reload_from_disk(&mut self, path: &std::path::Path, closed: bool) {
        let Some(w) = &self.world else { return };
        let mut a = match w.analysis.write() {
            Ok(a) => a,
            Err(p) => p.into_inner(),
        };
        if closed {
            if let Some(f) = a.file_id(path) {
                a.close(f);
            }
        }
        // A file outside every folder (a root of its own) stays only while it is open.
        let loose = a.file_id(path).is_some_and(|f| {
            let root = a.ws.file(f).meta.root;
            a.ws.root(root).path.is_file()
        });
        let changed = match std::fs::read(path) {
            _ if closed && loose => a.remove(path).is_some(),
            Ok(bytes) if !gsc_syntax::is_binary(&bytes) => {
                let text = gsc_syntax::SourceText::decode(&bytes).text;
                let known = a.file_id(path).is_some();
                let key = path_key(path);
                let mut roots = a.ws.roots().iter().filter(|r| r.path.is_dir());
                if known || roots.any(|r| key.starts_with(&path_key(&r.path))) {
                    a.set_file(path, &text).1
                } else {
                    false
                }
            }
            _ => a.remove(path).is_some(),
        };
        drop(a);
        if changed {
            self.lint.changed();
        }
    }

    fn handle(&mut self, req: Request) -> Response {
        let id = req.id.clone();
        let Some(w) = &self.world else {
            if req.method == "gsc/status" {
                return ok(id, serde_json::json!({ "loading": true }));
            }
            return ok(id, Value::Null);
        };
        let a = match w.analysis.read() {
            Ok(a) => a,
            Err(p) => p.into_inner(),
        };
        let ctx = Ctx {
            a: &a,
            project: &w.project,
            enc: self.enc,
            lint: &self.lint.shared,
            features: self.features,
            lines: Default::default(),
        };
        let refused =
            |id: RequestId, r: Refused| Response::new_err(id, ErrorCode::RequestFailed as i32, r.0);
        macro_rules! with {
            ($f:expr) => {
                match params(&req) {
                    Ok(p) => ok(id, $f(p)),
                    Err(e) => e,
                }
            };
        }
        macro_rules! with_result {
            ($f:expr) => {
                match params(&req) {
                    Ok(p) => match $f(p) {
                        Ok(v) => ok(id, v),
                        Err(r) => refused(id, r),
                    },
                    Err(e) => e,
                }
            };
        }
        match req.method.as_str() {
            "textDocument/definition" => with!(|p| ctx.definition(p)),
            "textDocument/references" => with!(|p| ctx.references(p)),
            "textDocument/hover" => with!(|p| ctx.hover(p)),
            "textDocument/documentSymbol" => with!(|p| ctx.document_symbols(p)),
            "workspace/symbol" => with!(|p| ctx.workspace_symbols(p)),
            "textDocument/completion" => with!(|p| ctx.completion(p)),
            "textDocument/signatureHelp" => with!(|p| ctx.signature_help(p)),
            "textDocument/prepareCallHierarchy" => with!(|p| ctx.prepare_call_hierarchy(p)),
            "callHierarchy/incomingCalls" => with!(|p| ctx.incoming_calls(p)),
            "callHierarchy/outgoingCalls" => with!(|p| ctx.outgoing_calls(p)),
            "textDocument/formatting" => with_result!(|p| ctx.formatting(p)),
            "textDocument/rangeFormatting" => with_result!(|p| ctx.range_formatting(p)),
            "textDocument/codeAction" => with!(|p| ctx.code_actions(p)),
            "textDocument/prepareRename" => with_result!(|p| ctx.prepare_rename(p)),
            "textDocument/rename" => with_result!(|p| ctx.rename(p)),
            "textDocument/semanticTokens/full" => with!(|p| ctx.semantic_tokens(p)),
            "textDocument/foldingRange" => with!(|p| ctx.folding_ranges(p)),
            "textDocument/selectionRange" => with!(|p| ctx.selection_ranges(p)),
            "textDocument/documentHighlight" => with!(|p| ctx.document_highlight(p)),
            "textDocument/documentLink" => with!(|p| ctx.document_links(p)),
            "textDocument/codeLens" => with!(|p| ctx.code_lens(p)),
            "textDocument/inlayHint" => with!(|p| ctx.inlay_hints(p)),
            "textDocument/implementation" => with!(|p| ctx.implementation(p)),
            "textDocument/typeDefinition" => with!(|p| ctx.type_definition(p)),
            "textDocument/prepareTypeHierarchy" => with!(|p| ctx.prepare_type_hierarchy(p)),
            "typeHierarchy/supertypes" => with!(|p| ctx.supertypes(p)),
            "typeHierarchy/subtypes" => with!(|p| ctx.subtypes(p)),
            "gsc/status" => ok(id, ctx.status()),
            _ => Response::new_err(
                id,
                ErrorCode::MethodNotFound as i32,
                format!("unknown method {}", req.method),
            ),
        }
    }
}
