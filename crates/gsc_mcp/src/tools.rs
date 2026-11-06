//! The tools: their schemas and their compact text answers.

use std::collections::{BTreeMap, HashMap};
use std::thread::JoinHandle;

use gsc_sema::events::pattern_matches;
use gsc_sema::ide::Symbol;
use gsc_sema::{Callee, DefId, DefKind, EventClass, EventName, FileId, How};
use serde_json::{json, Value};

use crate::catalogue::Raw;
use crate::world::{Config, World};

/// Most items a page may hold.
const MAX_LIMIT: usize = 200;
/// Characters of one answer before it is cut (the footer says where to continue).
const MAX_CHARS: usize = 8000;
const DEFAULT_LIMIT: usize = 25;

fn schema(props: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false,
    })
}

fn paging() -> [(&'static str, Value); 2] {
    [
        (
            "limit",
            json!({ "type": "integer", "minimum": 1, "maximum": MAX_LIMIT,
                    "description": "Items per page (default 25)." }),
        ),
        (
            "offset",
            json!({ "type": "integer", "minimum": 0,
                    "description": "Items to skip: the `offset=` of the previous page's last line." }),
        ),
    ]
}

fn with_paging(mut props: serde_json::Map<String, Value>) -> Value {
    for (k, v) in paging() {
        props.insert(k.to_owned(), v);
    }
    Value::Object(props)
}

fn props(v: Value) -> serde_json::Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => serde_json::Map::new(),
    }
}

fn target_props() -> serde_json::Map<String, Value> {
    props(json!({
        "name": { "type": "string",
                  "description": "`ns::function`, `function`, `class::method`, a macro, a builtin, or `.field`." },
        "file": { "type": "string", "description": "Instead of a name: a file (path as printed)..." },
        "line": { "type": "integer", "minimum": 1, "description": "...its 1-based line..." },
        "column": { "type": "integer", "minimum": 1,
                    "description": "...and 1-based column (default: the name's first use on the line, else the first non-blank)." },
    }))
}

/// The tool list, in a fixed order.
pub fn list() -> Value {
    let ro = json!({ "readOnlyHint": true, "idempotentHint": true, "openWorldHint": false });
    let tool = |name: &str, title: &str, description: &str, input: Value| {
        json!({ "name": name, "title": title, "description": description,
                "inputSchema": input, "annotations": ro })
    };
    json!([
        tool(
            "gsc_workspace",
            "Workspace summary",
            "Game, roots (with the labels used in paths), file and definition counts, and the diagnostics by code.",
            schema(json!({}), &[]),
        ),
        tool(
            "gsc_find_symbol",
            "Find symbols",
            "Definitions (functions, methods, classes, macros) whose name matches a query: exact, prefix, then substring, case-insensitive. `ns::` in the query matches qualified names.",
            schema(
                with_paging(props(json!({
                    "query": { "type": "string" },
                    "kind": { "type": "string", "enum": ["function", "method", "class", "macro", "const"] },
                }))),
                &["query"],
            ),
        ),
        tool(
            "gsc_definition",
            "Go to definition",
            "Where a name (or the name at file:line:column) is defined, with its signature and doc summary.",
            schema(Value::Object(target_props()), &[]),
        ),
        tool(
            "gsc_references",
            "Find references",
            "Every use of a function, method, macro, builtin, field (`.name`) or, by position, a local variable: path:line and the line's code.",
            schema(with_paging(target_props()), &[]),
        ),
        tool(
            "gsc_callers",
            "Callers",
            "The functions that call (or take a pointer to) a function, with the call lines.",
            schema(with_paging(target_props()), &[]),
        ),
        tool(
            "gsc_callees",
            "Callees",
            "What a function calls: script functions with their definition, builtins, unresolved names.",
            schema(with_paging(target_props()), &[]),
        ),
        tool(
            "gsc_check",
            "Check (lint)",
            "Diagnostics of the workspace, a directory or a file: `path:line:col CODE message`. Library roots are not checked.",
            schema(
                with_paging(props(json!({
                    "path": { "type": "string", "description": "A file or directory (default: everything)." },
                    "codes": { "type": "string", "description": "Comma-separated codes or prefixes to keep (`B002,L`)." },
                }))),
                &[],
            ),
        ),
        tool(
            "gsc_events",
            "Events",
            "Notify / waittill / endon sites of an event name (pattern notifies included), and whether the engine sends it. Without `name`, the event names with their counts.",
            schema(
                with_paging(props(json!({
                    "name": { "type": "string", "description": "The event name (any case)." },
                    "class": { "type": "string", "enum": ["notify", "wait", "endon"] },
                    "query": { "type": "string", "description": "Without `name`: names containing this." },
                }))),
                &[],
            ),
        ),
        tool(
            "gsc_builtin",
            "Builtin lookup",
            "An engine builtin of the game: VM, function or method, arity, parameter types and how the arity was verified.",
            schema(json!({ "name": { "type": "string" } }), &["name"]),
        ),
        tool(
            "gsc_hash",
            "Hash / reverse hash",
            "Hash a string with the game's hash families, or name a hash value (`0x1a2b3c4d`, `function_1a2b3c4d`, `hash_...`) from every name the workspace and the catalogue know.",
            schema(
                json!({
                    "text": { "type": "string", "description": "A string to hash." },
                    "value": { "type": "string", "description": "A hash to name." },
                    "family": { "type": "string", "description": "A family id (`t7.canon`...); default: the game's." },
                }),
                &[],
            ),
        ),
    ])
}

enum State {
    Loading(Option<JoinHandle<World>>),
    Ready(Box<World>),
}

pub struct Tools {
    state: State,
    raw: Option<Result<Raw, String>>,
    /// Reverse hash tables by family id, with the file count they were built at.
    names: HashMap<&'static str, (usize, gsc_hash::NameTable)>,
}

fn text(s: String) -> Value {
    json!({ "content": [{ "type": "text", "text": s }], "isError": false })
}

fn fail(s: String) -> Value {
    json!({ "content": [{ "type": "text", "text": s }], "isError": true })
}

fn str_arg<'a>(args: &'a Value, k: &str) -> Option<&'a str> {
    args.get(k)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn num_arg(args: &Value, k: &str) -> Option<usize> {
    args.get(k).and_then(|v| {
        v.as_u64()
            .map(|n| n as usize)
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
    })
}

/// A page of `items` under `header`: `limit` from `offset`, cut at [`MAX_CHARS`], with a footer
/// saying how to get the rest.
fn page(header: String, items: &[String], args: &Value) -> String {
    let limit = num_arg(args, "limit")
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT);
    let offset = num_arg(args, "offset").unwrap_or(0).min(items.len());
    let mut out = header;
    let mut end = offset;
    for it in items.iter().skip(offset).take(limit) {
        if out.len() + it.len() + 1 > MAX_CHARS && end > offset {
            break;
        }
        out.push('\n');
        out.push_str(it);
        end += 1;
    }
    if end < items.len() {
        out.push_str(&format!("\n... {} more (offset={end})", items.len() - end));
    }
    out
}

fn kind_label(k: DefKind) -> &'static str {
    match k {
        DefKind::Function => "function",
        DefKind::Method => "method",
        DefKind::Class => "class",
        DefKind::Define => "macro",
        DefKind::Const => "const",
    }
}

fn pad(params: &str) -> String {
    if params.is_empty() {
        "()".to_owned()
    } else {
        format!("( {params} )")
    }
}

impl Tools {
    pub fn new(config: Config) -> Tools {
        let handle = std::thread::Builder::new()
            .name("gsc-mcp-load".to_owned())
            .stack_size(256 << 20)
            .spawn(move || World::load(&config))
            .ok();
        Tools {
            state: State::Loading(handle),
            raw: None,
            names: HashMap::new(),
        }
    }

    fn world(&mut self) -> Result<&mut World, String> {
        if let State::Loading(h) = &mut self.state {
            let w = h
                .take()
                .ok_or("the workspace failed to load")?
                .join()
                .map_err(|_| "the workspace failed to load (see stderr)".to_owned())?;
            self.state = State::Ready(Box::new(w));
        }
        match &mut self.state {
            State::Ready(w) => Ok(w),
            State::Loading(_) => Err("the workspace failed to load".to_owned()),
        }
    }

    /// Run tool `name`; `None` for an unknown tool.
    pub fn call(&mut self, name: &str, args: &Value) -> Option<Value> {
        let known = [
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
        ];
        if !known.contains(&name) {
            return None;
        }
        if let Err(e) = self.world().map(|w| w.refresh()) {
            return Some(fail(e));
        }
        let r = match name {
            "gsc_workspace" => self.workspace(),
            "gsc_find_symbol" => self.find_symbol(args),
            "gsc_definition" => self.definition(args),
            "gsc_references" => self.references(args),
            "gsc_callers" => self.callers(args),
            "gsc_callees" => self.callees(args),
            "gsc_check" => self.check(args),
            "gsc_events" => self.events(args),
            "gsc_builtin" => self.builtin(args),
            _ => self.hash(args),
        };
        Some(match r {
            Ok(s) => text(s),
            Err(e) => fail(e),
        })
    }

    fn ready(&self) -> &World {
        match &self.state {
            State::Ready(w) => w,
            State::Loading(_) => unreachable!("call() loads the world first"),
        }
    }

    fn raw(&mut self) -> Result<&Raw, String> {
        if self.raw.is_none() {
            let w = self.ready();
            let s = &w.project;
            let data = w.settings.data_dir.as_deref();
            let path = gsc_lsp::project::profiles_dir(data, std::slice::from_ref(&w.base))
                .and_then(|d| {
                    let game = w.settings.game.as_deref().or(Some(s.dialect.id.as_str()));
                    gsc_sema::data::catalogue_file(&d, &s.dialect.id, game)
                });
            self.raw = Some(match path {
                Some(p) => Raw::load(&p),
                None => Err(format!("no builtin catalogue for {}", s.dialect.id)),
            });
        }
        self.raw
            .as_ref()
            .expect("set above")
            .as_ref()
            .map_err(Clone::clone)
    }

    fn workspace(&mut self) -> Result<String, String> {
        let w = self.world()?;
        let ws = &w.ws;
        let mut out = format!(
            "gruff {} | {} | loaded in {} ms",
            env!("CARGO_PKG_VERSION"),
            w.project.describe(),
            w.load_ms
        );
        for (i, r) in ws.roots().iter().enumerate() {
            let n = ws.files().filter(|&f| ws.file(f).meta.root == i).count();
            out.push_str(&format!(
                "\nroot `{}`{}: {} ({n} files)",
                r.label,
                if r.library {
                    " (library, not checked)"
                } else {
                    ""
                },
                r.path.display()
            ));
        }
        let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
        for d in ws.all_defs() {
            *kinds.entry(kind_label(ws.def(d).kind)).or_default() += 1;
        }
        let kinds: Vec<String> = kinds
            .iter()
            .map(|(k, n)| format!("{n} {k}{}", if *k == "class" { "es" } else { "s" }))
            .collect();
        out.push_str(&format!("\ndefinitions: {}", kinds.join(", ")));
        out.push_str(&format!(
            "\npaths: relative to {} when under it, else `<root label>:<path>`",
            w.base.display()
        ));
        if let Some(e) = w.load_errors.first() {
            out.push_str(&format!(
                "\nunreadable: {} file(s), e.g. {e}",
                w.load_errors.len()
            ));
        }
        for wn in w.project.warnings.iter().take(5) {
            out.push_str(&format!("\nwarning: {wn}"));
        }
        let primary: Vec<FileId> = w.ws.primary_files().collect();
        let s = w.check();
        let mut by_code: BTreeMap<&str, usize> = BTreeMap::new();
        let mut files = 0;
        for f in &primary {
            let d = s.diagnostics(*f).unwrap_or_default();
            files += usize::from(!d.is_empty());
            for x in d {
                *by_code.entry(x.code.as_str()).or_default() += 1;
            }
        }
        let total: usize = by_code.values().sum();
        let codes: Vec<String> = by_code
            .iter()
            .map(|(c, n)| format!("{c} {} x{n}", gsc_check::registry::name_of(c)))
            .collect();
        out.push_str(&format!(
            "\ndiagnostics: {total} in {files} of {} checked files{}{}",
            primary.len(),
            if codes.is_empty() { "" } else { ": " },
            codes.join(", ")
        ));
        Ok(out)
    }

    fn def_line(w: &World, d: DefId) -> String {
        let def = w.ws.def(d);
        let name = match def.kind {
            DefKind::Define | DefKind::Const => def.name.clone(),
            _ => def.qualified(),
        };
        let params = match def.kind {
            DefKind::Function | DefKind::Method => pad(&def.params),
            DefKind::Class if !def.params.is_empty() => format!(" : {}", def.params),
            DefKind::Define if !def.params.is_empty() => format!("({})", def.params),
            _ => String::new(),
        };
        let flags = if def.flags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", def.flags)
        };
        format!(
            "{} {name}{params}{flags}  {}",
            kind_label(def.kind),
            w.at(d.file, def.name_range.start())
        )
    }

    fn find_symbol(&mut self, args: &Value) -> Result<String, String> {
        let query = str_arg(args, "query")
            .ok_or("`query` is required")?
            .to_owned();
        let kind = str_arg(args, "kind").map(str::to_owned);
        let w = self.world()?;
        let ws = &w.ws;
        let q = query.to_ascii_lowercase();
        let qualified = q.contains("::");
        let mut hits: Vec<(u8, bool, usize, DefId)> = ws
            .all_defs()
            .filter_map(|d| {
                let def = ws.def(d);
                if kind.as_deref().is_some_and(|k| k != kind_label(def.kind)) {
                    return None;
                }
                let name = if qualified {
                    def.qualified().to_ascii_lowercase()
                } else {
                    def.short_name().to_ascii_lowercase()
                };
                let rank = if name == q {
                    0
                } else if name.starts_with(&q) {
                    1
                } else if name.contains(&q) {
                    2
                } else {
                    return None;
                };
                Some((rank, w.is_library(d.file), name.len(), d))
            })
            .collect();
        hits.sort();
        let items: Vec<String> = hits.iter().map(|h| Self::def_line(w, h.3)).collect();
        Ok(page(
            format!("{} definitions match `{query}`", items.len()),
            &items,
            args,
        ))
    }

    /// The definitions a name argument names, primary files first.
    fn defs_named(w: &World, name: &str) -> Vec<DefId> {
        let name = name.trim().trim_end_matches("()").trim();
        let lower = name.to_ascii_lowercase();
        let ws = &w.ws;
        let mut v: Vec<DefId> = ws
            .all_defs()
            .filter(|&d| {
                let def = ws.def(d);
                if lower.contains("::") {
                    def.qualified().eq_ignore_ascii_case(&lower)
                        || def.name.eq_ignore_ascii_case(&lower)
                } else {
                    def.short_name().eq_ignore_ascii_case(&lower)
                }
            })
            .collect();
        v.sort_by_key(|d| (w.is_library(d.file), *d));
        v
    }

    /// What a target argument (`name`, or `file` + `line` + `column`) designates.
    fn target(&mut self, args: &Value) -> Result<(Option<Symbol>, Vec<DefId>, String), String> {
        let name = str_arg(args, "name").map(str::to_owned);
        let file = str_arg(args, "file").map(str::to_owned);
        let has_builtin = {
            let lower = name.as_deref().map(|n| n.to_ascii_lowercase());
            match (&lower, self.raw()) {
                (Some(l), Ok(raw)) => raw.by_name.contains_key(l.trim_end_matches("()")),
                _ => false,
            }
        };
        let w = self.world()?;
        if let Some(file) = file {
            let f = w.find_file(&file)?;
            let line = num_arg(args, "line").ok_or("`line` goes with `file`")?;
            let mut column = num_arg(args, "column");
            if column.is_none() {
                if let Some(n) = &name {
                    let at = w.offset(f, line, Some(1))?;
                    let text = w.ws.file(f).text();
                    let start = usize::from(at);
                    let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
                    let short = n.rsplit("::").next().unwrap_or(n);
                    column = text[start..end]
                        .to_ascii_lowercase()
                        .find(&short.to_ascii_lowercase())
                        .map(|i| i + 1);
                }
            }
            let off = w.offset(f, line, column)?;
            let sym = w.ws.symbol_at(f, off);
            let defs = match &sym {
                Some(Symbol::Def(d)) => vec![*d],
                _ => Vec::new(),
            };
            let what = format!("{}:{line}", w.show(f));
            return match sym {
                Some(s) => Ok((Some(s), defs, what)),
                None => Err(format!("nothing to resolve at {what}")),
            };
        }
        let name = name.ok_or("give `name`, or `file` and `line`")?;
        if let Some(field) = name.strip_prefix('.') {
            let cfg = w.ws.profile(0).config;
            return Ok((
                Some(Symbol::Field(cfg.ident(field))),
                Vec::new(),
                name.clone(),
            ));
        }
        let defs = Self::defs_named(w, &name);
        if defs.is_empty() {
            if has_builtin {
                let lower = name.trim_end_matches("()").to_ascii_lowercase();
                return Ok((Some(Symbol::Builtin(lower)), defs, name));
            }
            return Err(format!(
                "no definition named `{name}` (try gsc_find_symbol; fields are `.name`)"
            ));
        }
        Ok((Some(Symbol::Def(defs[0])), defs, name))
    }

    fn definition(&mut self, args: &Value) -> Result<String, String> {
        let (sym, defs, what) = self.target(args)?;
        let w = self.ready();
        let mut out = Vec::new();
        for &d in defs.iter().take(10) {
            let mut s = Self::def_line(w, d);
            let def = w.ws.def(d);
            if !def.doc.is_empty() {
                let doc: String = def
                    .doc
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(160)
                    .collect();
                s.push_str(&format!("\n  {doc}"));
            }
            out.push(s);
        }
        if defs.len() > 10 {
            out.push(format!("... {} more definitions", defs.len() - 10));
        }
        match sym {
            Some(Symbol::Local {
                file,
                decl,
                param,
                name,
                ..
            }) => out.push(format!(
                "{} {name}  {}",
                if param { "parameter" } else { "local" },
                w.at(file, decl.start())
            )),
            Some(Symbol::Builtin(b)) => {
                out.push(format!("builtin {b}: an engine function (see gsc_builtin)"))
            }
            Some(Symbol::Field(f)) => out.push(format!(
                "field .{f}: no single definition (see gsc_references)"
            )),
            Some(Symbol::File(f)) => out.push(format!("file {}", w.show(f))),
            _ => {}
        }
        if out.is_empty() {
            return Err(format!("no definition for `{what}`"));
        }
        Ok(out.join("\n"))
    }

    fn references(&mut self, args: &Value) -> Result<String, String> {
        let (sym, defs, what) = self.target(args)?;
        let w = self.ready();
        let syms: Vec<Symbol> = if defs.len() > 1 {
            defs.iter().map(|&d| Symbol::Def(d)).collect()
        } else {
            sym.into_iter().collect()
        };
        let mut refs = Vec::new();
        for s in &syms {
            refs.extend(w.ws.references(s, false));
        }
        refs.sort_by_key(|r| (w.is_library(r.file), *r));
        refs.dedup();
        let items: Vec<String> = refs
            .iter()
            .map(|r| {
                format!(
                    "{}  {}",
                    w.at(r.file, r.range.start()),
                    w.snippet(r.file, r.range.start(), 100)
                )
            })
            .collect();
        let note = if defs.len() > 1 {
            format!(" ({} definitions share the name)", defs.len())
        } else {
            String::new()
        };
        Ok(page(
            format!("{} references to `{what}`{note}", items.len()),
            &items,
            args,
        ))
    }

    fn callable(&mut self, args: &Value) -> Result<(Vec<DefId>, String), String> {
        let (_, defs, what) = self.target(args)?;
        let w = self.ready();
        let defs: Vec<DefId> = defs
            .into_iter()
            .filter(|&d| w.ws.def(d).kind.is_callable())
            .collect();
        if defs.is_empty() {
            return Err(format!("`{what}` is not a script function or method"));
        }
        Ok((defs, what))
    }

    fn callers(&mut self, args: &Value) -> Result<String, String> {
        let (defs, what) = self.callable(args)?;
        let w = self.ready();
        let mut items = Vec::new();
        for &d in &defs {
            let mut calls = w.ws.incoming_calls(d);
            calls.sort_by_key(|c| {
                (
                    w.is_library(c.file),
                    c.file,
                    c.ranges.first().copied().map(|r| r.start()),
                )
            });
            for c in calls {
                let from = c
                    .from
                    .map_or("<file scope>".to_owned(), |f| w.ws.def(f).qualified());
                let lines: Vec<String> = c
                    .ranges
                    .iter()
                    .map(|r| w.line_col(c.file, r.start()).0.to_string())
                    .collect();
                let target = if defs.len() > 1 {
                    format!(" -> {}", w.ws.def(d).qualified())
                } else {
                    String::new()
                };
                items.push(format!(
                    "{from}  {}:{}{target}",
                    w.show(c.file),
                    lines.join(",")
                ));
            }
        }
        Ok(page(
            format!("{} callers of `{what}`", items.len()),
            &items,
            args,
        ))
    }

    fn callees(&mut self, args: &Value) -> Result<String, String> {
        let (defs, _) = self.callable(args)?;
        let w = self.ready();
        let d = defs[0];
        let res = w.ws.resolution(d.file);
        let facts = &w.ws.file(d.file).facts;
        let unresolved: Vec<&str> = res
            .refs
            .iter()
            .zip(&facts.refs)
            .filter(|(r, x)| x.caller == Some(d.index) && r.how == How::Unresolved)
            .map(|(r, _)| r.callee.as_str())
            .collect();
        let items: Vec<String> =
            w.ws.outgoing_calls(d)
                .into_iter()
                .map(|c| {
                    let n = c.ranges.len();
                    let line = c
                        .ranges
                        .first()
                        .map_or(0, |r| w.line_col(d.file, r.start()).0);
                    let times = if n > 1 {
                        format!(" x{n}")
                    } else {
                        String::new()
                    };
                    match c.to {
                        Callee::Def(t) => format!(
                            "{}{times}  line {line} -> {}",
                            w.ws.def(t).qualified(),
                            w.at(t.file, w.ws.def(t).name_range.start())
                        ),
                        Callee::Builtin(b) => format!("builtin {b}{times}  line {line}"),
                        Callee::Other(o) => {
                            let tag = if unresolved.contains(&o.as_str()) {
                                "unresolved"
                            } else {
                                "dynamic"
                            };
                            format!("{tag} {o}{times}  line {line}")
                        }
                    }
                })
                .collect();
        let note = if defs.len() > 1 {
            format!(" (the first of {} definitions)", defs.len())
        } else {
            String::new()
        };
        Ok(page(
            format!("{} callees of {}{note}", items.len(), Self::def_line(w, d)),
            &items,
            args,
        ))
    }

    fn check(&mut self, args: &Value) -> Result<String, String> {
        let path = str_arg(args, "path").map(str::to_owned);
        let codes: Vec<String> = str_arg(args, "codes")
            .map(|c| {
                c.split(',')
                    .map(|s| s.trim().to_ascii_uppercase())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let w = self.world()?;
        // A file, or a directory prefix of printed paths.
        let mut only_file = None;
        let mut prefix = None;
        if let Some(p) = &path {
            match w.find_file(p) {
                Ok(f) => only_file = Some(f),
                Err(_) => {
                    let p = p.replace('\\', "/");
                    let abs = std::path::Path::new(&p);
                    let full = if abs.is_absolute() {
                        abs.to_path_buf()
                    } else {
                        w.base.join(abs)
                    };
                    prefix = Some((p.trim_end_matches('/').to_ascii_lowercase(), full));
                }
            }
        }
        if only_file.is_some_and(|f| w.is_library(f)) {
            return Err(format!(
                "{} is in a library root: library files are not checked",
                path.unwrap_or_default()
            ));
        }
        let files: Vec<FileId> = w
            .ws
            .primary_files()
            .filter(|&f| match (&only_file, &prefix) {
                (Some(o), _) => f == *o,
                (None, Some((p, full))) => {
                    let shown = w.show(f).to_ascii_lowercase();
                    shown.starts_with(&format!("{p}/")) || w.ws.file(f).meta.path.starts_with(full)
                }
                (None, None) => true,
            })
            .collect();
        if files.is_empty() {
            return Err(format!(
                "no checked file under `{}`",
                path.unwrap_or_default()
            ));
        }
        let s = w.check();
        let mut found = Vec::new();
        for &f in &files {
            for d in s.diagnostics(f).unwrap_or_default() {
                if codes.is_empty() || codes.iter().any(|c| d.code.starts_with(c.as_str())) {
                    found.push((f, d.clone()));
                }
            }
        }
        let w = self.ready();
        let mut by_code: BTreeMap<&str, usize> = BTreeMap::new();
        for (_, d) in &found {
            *by_code.entry(d.code.as_str()).or_default() += 1;
        }
        let summary: Vec<String> = by_code.iter().map(|(c, n)| format!("{c} x{n}")).collect();
        let items: Vec<String> = found
            .iter()
            .map(|(f, d)| {
                format!(
                    "{}:{}:{} {} {}",
                    w.show(*f),
                    d.start.line,
                    d.start.col,
                    d.code,
                    d.message
                )
            })
            .collect();
        let nfiles = {
            let mut v: Vec<FileId> = found.iter().map(|(f, _)| *f).collect();
            v.dedup();
            v.len()
        };
        Ok(page(
            format!(
                "{} diagnostics in {nfiles} of {} files{}{}",
                items.len(),
                files.len(),
                if summary.is_empty() { "" } else { ": " },
                summary.join(", ")
            ),
            &items,
            args,
        ))
    }

    fn events(&mut self, args: &Value) -> Result<String, String> {
        let name = str_arg(args, "name").map(str::to_ascii_lowercase);
        let class = match str_arg(args, "class") {
            None => None,
            Some("notify") => Some(EventClass::Emit),
            Some("wait") => Some(EventClass::Wait),
            Some("endon") => Some(EventClass::Endon),
            Some(c) => return Err(format!("unknown class `{c}`: notify, wait or endon")),
        };
        let query = str_arg(args, "query").map(str::to_ascii_lowercase);
        let w = self.world()?;
        let ws = &w.ws;
        let label = |c: EventClass| match c {
            EventClass::Emit => "notify",
            EventClass::Wait => "wait",
            EventClass::Endon => "endon",
        };
        let Some(name) = name else {
            // Every event name with its counts.
            let mut counts: BTreeMap<String, [usize; 3]> = BTreeMap::new();
            for f in ws.files() {
                for s in &ws.file(f).facts.events {
                    if let EventName::Literal(n) = &s.name {
                        let n = n.to_ascii_lowercase();
                        if query.as_deref().is_some_and(|q| !n.contains(q)) {
                            continue;
                        }
                        let c = counts.entry(n).or_default();
                        c[s.class as usize] += 1;
                    }
                }
            }
            let mut v: Vec<(String, [usize; 3])> = counts.into_iter().collect();
            v.sort_by_key(|(n, c)| (std::cmp::Reverse(c.iter().sum::<usize>()), n.clone()));
            let items: Vec<String> = v
                .iter()
                .map(|(n, c)| format!("{n}  notify {} wait {} endon {}", c[0], c[1], c[2]))
                .collect();
            return Ok(page(format!("{} event names", items.len()), &items, args));
        };
        let mut sites = Vec::new();
        for f in ws.files() {
            for s in &ws.file(f).facts.events {
                let hit = match &s.name {
                    EventName::Literal(n) => n.eq_ignore_ascii_case(&name),
                    EventName::Pattern(p) => pattern_matches(p, &name),
                    EventName::Dynamic => false,
                };
                if hit && class.is_none_or(|c| c == s.class) {
                    sites.push((f, s));
                }
            }
        }
        sites.sort_by_key(|(f, s)| {
            let pattern = !matches!(s.name, EventName::Literal(_));
            (s.class as u8, pattern, w.is_library(*f), *f, s.focus)
        });
        let mut counts = [0usize; 3];
        for (_, s) in &sites {
            counts[s.class as usize] += 1;
        }
        let items: Vec<String> = sites
            .iter()
            .map(|(f, s)| {
                let caller = s.caller.map_or("<file scope>".to_owned(), |c| {
                    ws.file(*f).facts.defs[c as usize].qualified()
                });
                let pat = match &s.name {
                    EventName::Pattern(p) => format!(" (pattern {p})"),
                    _ => String::new(),
                };
                let args = if s.argc > 0 {
                    format!(" +{} arg{}", s.argc, if s.argc == 1 { "" } else { "s" })
                } else {
                    String::new()
                };
                let op = if s.op == label(s.class) || s.is_keyword() {
                    s.op.clone()
                } else {
                    format!("{} ({})", s.op, label(s.class))
                };
                format!(
                    "{op} {}{args}{pat}  {}  in {caller}",
                    s.receiver_text,
                    w.at(*f, s.focus)
                )
            })
            .collect();
        let engine = match ws.profile(0).events.as_ref() {
            Some(cat) => {
                let e = cat.lookup(&name);
                if e.is_empty() {
                    "; engine: not in the catalogue".to_owned()
                } else {
                    format!("; engine: sends it ({} entries)", e.len())
                }
            }
            None => String::new(),
        };
        Ok(page(
            format!(
                "event `{name}`: {} notify, {} wait, {} endon{engine}",
                counts[0], counts[1], counts[2]
            ),
            &items,
            args,
        ))
    }

    fn builtin(&mut self, args: &Value) -> Result<String, String> {
        let name = str_arg(args, "name")
            .ok_or("`name` is required")?
            .to_owned();
        let lower = name.trim_end_matches("()").to_ascii_lowercase();
        let script: Vec<String> = {
            let w = self.world()?;
            Self::defs_named(w, &lower)
                .into_iter()
                .take(3)
                .map(|d| Self::def_line(w, d))
                .collect()
        };
        let raw = self.raw()?;
        let Some(entries) = raw.by_name.get(&lower) else {
            let mut sugg: Vec<(usize, &str)> = raw
                .names()
                .filter_map(|n| {
                    gsc_sema::data::levenshtein(&lower, &n.to_ascii_lowercase(), 2).map(|d| (d, n))
                })
                .collect();
            sugg.sort();
            sugg.dedup();
            let sugg: Vec<&str> = sugg.into_iter().take(5).map(|(_, n)| n).collect();
            let mut out = format!("`{name}` is not a builtin in {}", raw.file);
            if !sugg.is_empty() {
                out.push_str(&format!("; did you mean {}?", sugg.join(", ")));
            }
            for s in script {
                out.push_str(&format!("\nscript {s}"));
            }
            return Ok(out);
        };
        let mut out = Vec::new();
        for e in entries {
            let params = if e.param_types.is_empty() {
                String::new()
            } else {
                e.param_types.join(", ")
            };
            out.push(format!(
                "{} {} {}({params}){}\n  {}\n  {}",
                e.vm,
                e.kind,
                e.name,
                if e.dev_only { " [dev only]" } else { "" },
                e.arity(raw.variadic_max),
                e.verification(raw)
            ));
        }
        let mut s = format!("{} ({}):\n{}", name, raw.file, out.join("\n"));
        for d in script {
            s.push_str(&format!("\nscript function of the same name: {d}"));
        }
        Ok(s)
    }

    fn hash(&mut self, args: &Value) -> Result<String, String> {
        let text_arg = str_arg(args, "text").map(str::to_owned);
        let value = str_arg(args, "value").map(str::to_owned);
        let family = str_arg(args, "family").map(str::to_owned);
        let game_id = self.world()?.project.dialect.id.clone();
        let families: Vec<&'static gsc_hash::Family> = match &family {
            Some(id) => vec![gsc_hash::family(id)
                .ok_or_else(|| format!("unknown family `{id}` (see `gruff hash --list`)"))?],
            None => {
                let game = gsc_hash::Game::from_profile_id(&game_id).ok_or_else(|| {
                    format!("no hash families known for {game_id}; pass `family`")
                })?;
                gsc_hash::families_for(game).collect()
            }
        };
        if let Some(t) = text_arg {
            let lines: Vec<String> = families
                .iter()
                .map(|f| format!("{}  0x{}  ({})", f.id, f.format(f.hash(&t)), f.kinds))
                .collect();
            return Ok(format!("`{t}`:\n{}", lines.join("\n")));
        }
        let v = value.ok_or("give `text` to hash or `value` to name")?;
        let digits = v
            .rsplit('_')
            .next()
            .unwrap_or(&v)
            .trim_start_matches("0x")
            .trim_start_matches("0X");
        let n = u64::from_str_radix(digits, 16).map_err(|_| format!("`{v}` is not a hex hash"))?;
        let packaged: Vec<String> = match &self.world()?.project.names {
            Some(d) => d
                .reverse(n)
                .into_iter()
                .map(|(f, name)| format!("{}: `{name}` (hash package)", f.id))
                .collect(),
            None => Vec::new(),
        };
        let known = self.known_names();
        let w = self.ready();
        let nfiles = w.ws.len();
        let mut out = packaged;
        for f in families {
            let stale = self.names.get(f.id).is_none_or(|(at, _)| *at != nfiles);
            if stale {
                let mut t = gsc_hash::NameTable::new();
                for k in &known {
                    t.insert_name(f, k);
                }
                self.names.insert(f.id, (nfiles, t));
            }
            let (_, t) = &self.names[f.id];
            if let Some((name, bits)) = t.lookup(n, 64) {
                out.push(format!("{}: `{name}` (matched at {bits} bits)", f.id));
            }
        }
        if out.is_empty() {
            return Ok(format!(
                "0x{n:x}: no known name hashes to it ({} names tried: definitions, namespaces, files, fields, events, builtins)",
                known.len()
            ));
        }
        Ok(format!("0x{n:x}:\n{}", out.join("\n")))
    }

    /// Every name the workspace and the catalogue know, for reverse lookups.
    fn known_names(&mut self) -> Vec<String> {
        let mut names: Vec<String> = self
            .raw()
            .map(|r| r.names().map(str::to_owned).collect())
            .unwrap_or_default();
        let w = self.ready();
        let ws = &w.ws;
        for f in ws.files() {
            let data = ws.file(f);
            let facts = &data.facts;
            names.push(facts.namespace.clone());
            names.push(data.meta.key.clone());
            names.push(data.meta.key.replace('/', "\\"));
            for d in &facts.defs {
                names.push(d.short_name().to_owned());
                if let Some(c) = d.class() {
                    names.push(c.to_owned());
                }
            }
            names.extend(facts.fields.iter().cloned());
            for e in &facts.events {
                if let EventName::Literal(n) = &e.name {
                    names.push(n.clone());
                }
            }
            names.extend(facts.strings.iter().cloned());
        }
        names.sort();
        names.dedup();
        names
    }
}
