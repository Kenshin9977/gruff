//! Community corpus reports (`gruff report`): counts and identifiers a player measures on their
//! own install and can share without sharing game code. Deterministic: no clock, no machine
//! path, no dependence on the thread count. What a report holds: `docs/community-reports.md`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;

use gsc_sema::{par_map, EventClass, EventName, FileId, How, Target, Vm, Workspace};
use gsc_syntax::ast::{AstNode, CallExpr, Expr};
use gsc_syntax::{Dialect, SyntaxKind, WalkEvent};

/// The report format. Bump on any change of meaning; merge tools refuse unknown versions.
pub const FORMAT_VERSION: u32 = 1;

/// Argument indexes recorded per builtin (the catalogues' `corpus_arg_types` bound).
const MAX_ARGS: usize = 16;

/// How the report names the scripts it measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptNames {
    /// xxh3-64 of the lower-case relative path (default). Dedupes installs; not anonymous, a
    /// guessed path can be hashed and compared.
    Hash,
    /// The relative paths as found under the root.
    Names,
    /// No per-script list (the corpus fingerprint stays).
    Drop,
}

impl ScriptNames {
    pub fn label(self) -> &'static str {
        match self {
            ScriptNames::Hash => "hash",
            ScriptNames::Names => "names",
            ScriptNames::Drop => "drop",
        }
    }
}

/// What the caller knows that the workspace does not.
pub struct Options {
    /// The toolchain version (`CARGO_PKG_VERSION` of the binary).
    pub toolchain: String,
    /// The game as asked (`t6`, `bo2`...); the profile id is the dialect's.
    pub game: String,
    /// The profile file's text: its hash is recorded, and it builds the probe dialect.
    pub profile_text: String,
    pub mode: gsc_syntax::Mode,
    pub scripts: ScriptNames,
    /// Keep event names of several words (`single anim`); false: one-word names only.
    pub event_spaces: bool,
}

/// Counts of one feature construct.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeatureUse {
    pub uses: u64,
    pub files: u64,
}

/// One external call target: a name nothing in the scripts defines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CallStat {
    /// A keyword the language treats as a call (`isdefined`...): an intrinsic.
    pub keyword: bool,
    pub sites: u64,
    pub files: u64,
    /// Sites behind `thread`/`childthread`.
    pub threaded: u64,
    /// Argument count to sites.
    pub argc: BTreeMap<u32, u64>,
    /// Receiver kind (`self`, `level`, `player`, `other`...) to sites, methods only.
    pub receivers: BTreeMap<&'static str, u64>,
    /// Per argument index: literal type (or `expr`) to sites.
    pub arg_literals: Vec<BTreeMap<&'static str, u64>>,
}

/// One event name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventStat {
    /// Built from a concatenation (`"trigger_" + n` is `trigger_*`).
    pub pattern: bool,
    pub emit: u64,
    pub wait: u64,
    pub endon: u64,
    pub files: u64,
    /// Operation (`notify`, `waittill`, `waittill_any`...) to sites.
    pub ops: BTreeMap<String, u64>,
    pub receivers: BTreeMap<&'static str, u64>,
    /// `notify` payload count to sites.
    pub payload: BTreeMap<u32, u64>,
}

/// One field name on one object kind.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldStat {
    pub reads: u64,
    pub writes: u64,
    pub files: u64,
}

/// File-level counts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CorpusStats {
    pub files: u64,
    pub gsc: u64,
    pub csc: u64,
    pub headers: u64,
    pub decompiled: u64,
    pub bytes: u64,
    pub lines: u64,
    pub files_with_syntax_errors: u64,
    pub functions: u64,
    pub imports: u64,
    pub imports_unresolved: u64,
}

/// Call-site totals.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CallTotals {
    pub sites: u64,
    /// Reaching a script definition (function, method or macro).
    pub script: u64,
    /// Bare names nothing defines, and keyword calls: the `builtins` list.
    pub external: u64,
    /// `path::f` / `ns::f` whose file or function is not in the corpus.
    pub qualified_unresolved: u64,
    /// `[[ f ]]()` and `->m()`.
    pub dynamic: u64,
    /// Names left out because they are not plain identifiers.
    pub skipped_names: u64,
}

type CallKey = (String, &'static str, &'static str);

/// A whole report.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub toolchain: String,
    pub game: String,
    pub profile_id: String,
    pub profile_hash: String,
    pub mode: String,
    pub scripts_mode: &'static str,
    pub event_spaces: bool,
    pub corpus: CorpusStats,
    /// Script ids (hash or name) with their VM, sorted; empty with [`ScriptNames::Drop`].
    pub scripts: Vec<(String, &'static str)>,
    /// xxh3 of the sorted script hashes: equal for equal script sets, whatever `scripts_mode`.
    pub fingerprint: String,
    /// `feature:construct` to counts.
    pub features: BTreeMap<String, FeatureUse>,
    pub calls: CallTotals,
    /// `(name, vm, kind)` to counts.
    pub builtins: BTreeMap<CallKey, CallStat>,
    pub events: BTreeMap<String, EventStat>,
    /// Sites whose event name is computed, per class (`emit`, `wait`, `endon`).
    pub events_dynamic: BTreeMap<&'static str, u64>,
    /// Literal event names left out because they are not identifier-like.
    pub events_skipped: u64,
    /// `(object kind, name)` to counts.
    pub fields: BTreeMap<(&'static str, String), FieldStat>,
    pub fields_skipped: u64,
}

/// Is `s` a plain identifier (what a builtin or field name may be)?
pub fn is_identifier(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Is `s` an event name a report may hold: at most 48 characters of letters, digits and
/// `_ + - . : *`, and with `spaces` (CoD2 names such as `single anim`) up to 6 words.
pub fn is_event_name(s: &str, spaces: bool) -> bool {
    let words = s.split(' ').collect::<Vec<_>>();
    !s.is_empty()
        && s.len() <= 48
        && words.iter().all(|w| !w.is_empty())
        && (words.len() == 1 || (spaces && words.len() <= 6))
        && s.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'-' | b'.' | b':' | b'*' | b' ')
        })
}

fn xxh64(s: &str) -> String {
    format!("{:016x}", xxhash_rust::xxh3::xxh3_64(s.as_bytes()))
}

/// The id a script gets with [`ScriptNames::Hash`].
pub fn script_hash(rel: &str) -> String {
    xxh64(&rel.replace('\\', "/").to_ascii_lowercase())
}

// The probe dialect: every feature off, so each gated construct is reported once per use
// (the method of gruff-data/tools/featcount.py --parser).
const ENUM_FEATURES: &[&str] = &[
    "doc_comment_style",
    "identifier_encoding",
    "import_directive",
    "name_resolution",
    "header_directive",
    "function_pointer_style",
    "isdefined_form",
    "bare_call_resolution",
    "array_argument_passing",
    "notify_wakes_waiters",
    "hash_equality",
];
const LIST_FEATURES: &[&str] = &[
    "file_kinds",
    "extra_string_prefixes",
    "hashed_identifier_spellings",
    "predefined_macros",
    "profiler_keywords",
    "assert_keywords",
    "opcode_builtins",
    "global_objects",
];

/// The probe dialect of a profile: its keywords and identifier case, every feature off (lists
/// empty, enums matching nothing), imports allowed anywhere.
pub fn probe_dialect(profile_text: &str) -> Result<Dialect, String> {
    let doc: toml::Table = profile_text
        .parse()
        .map_err(|e| format!("profile: invalid TOML: {e}"))?;
    let mut out = toml::Table::new();
    out.insert("schema_version".into(), toml::Value::Integer(1));
    out.insert(
        "id".into(),
        doc.get("id").cloned().unwrap_or_else(|| "probe".into()),
    );
    let mut kw = toml::Table::new();
    if let Some(w) = doc.get("keywords").and_then(|k| k.get("words")) {
        kw.insert("words".into(), w.clone());
    } else {
        kw.insert("words".into(), toml::Value::Array(Vec::new()));
    }
    out.insert("keywords".into(), toml::Value::Table(kw));
    let mut feats = toml::Table::new();
    if let Some(table) = doc.get("features").and_then(|f| f.as_table()) {
        for (k, e) in table {
            let probe = match k.as_str() {
                "case_insensitive_identifiers" => match e.get("value") {
                    Some(toml::Value::Boolean(b)) => toml::Value::Boolean(*b),
                    _ => toml::Value::Boolean(true),
                },
                "imports_before_declarations" => toml::Value::Boolean(true),
                _ if LIST_FEATURES.contains(&k.as_str()) => toml::Value::Array(Vec::new()),
                _ if ENUM_FEATURES.contains(&k.as_str()) => "probe-no-match".into(),
                _ => toml::Value::Boolean(false),
            };
            let mut t = toml::Table::new();
            t.insert("value".into(), probe);
            feats.insert(k.clone(), toml::Value::Table(t));
        }
    }
    out.insert("features".into(), toml::Value::Table(feats));
    let text = toml::to_string(&out).map_err(|e| format!("probe profile: {e}"))?;
    Dialect::from_toml_str(&text).map_err(|e| format!("probe profile: {e}"))
}

/// `feature:construct` of a dialect diagnostic, or `None` for other diagnostics.
fn feature_key(code: &str, message: &str) -> Option<String> {
    match code {
        "D002" => return Some("function_keyword:declaration without function".into()),
        "D003" => return Some("imports_before_declarations:import after a declaration".into()),
        "D004" => return Some("goto_labels:goto or label".into()),
        _ if !code.starts_with('D') => return None,
        _ => {}
    }
    let start = message.find("(feature `")? + "(feature `".len();
    let end = start + message[start..].find('`')?;
    let feature = &message[start..end];
    // The construct is the checker's own wording; bound it all the same.
    let what: String = message
        .split(" is not available")
        .next()
        .unwrap_or("")
        .chars()
        .take(64)
        .collect();
    Some(format!("{feature}:{what}"))
}

#[derive(Default)]
struct FileOut {
    features: BTreeMap<String, u64>,
    calls: CallTotals,
    builtins: BTreeMap<CallKey, CallStat>,
    syntax_errors: bool,
}

fn vm_label(vm: Option<Vm>) -> &'static str {
    match vm {
        Some(Vm::Gsc) => "gsc",
        Some(Vm::Csc) => "csc",
        None => "header",
    }
}

/// The type of a literal argument, `expr` for anything computed.
fn literal_kind(e: &Expr) -> &'static str {
    use SyntaxKind::*;
    match e {
        Expr::Literal(l) => match l.token().map(|t| (t.kind(), t.text().starts_with('%'))) {
            Some((INT_NUMBER, _)) => "int",
            Some((FLOAT_NUMBER, _)) => "float",
            Some((STRING, _)) => "string",
            Some((ISTRING, _)) => "istring",
            Some((HASH_STRING | HASH_IDENT, _)) => "hash",
            Some((PREFIXED_STRING, true)) => "anim",
            Some((PREFIXED_STRING, false)) => "hash",
            Some((TRUE_KW | FALSE_KW, _)) => "bool",
            Some((UNDEFINED_KW, _)) => "undefined",
            _ => "expr",
        },
        Expr::PrefixExpr(p) => {
            let minus = p
                .syntax()
                .children_with_tokens()
                .filter_map(|t| t.into_token())
                .any(|t| t.kind() == MINUS);
            match p.expr() {
                Some(inner @ Expr::Literal(_)) if minus => match literal_kind(&inner) {
                    k @ ("int" | "float") => k,
                    _ => "expr",
                },
                _ => "expr",
            }
        }
        Expr::ParenExpr(_) => match gsc_sema::facts::strip_parens(e.clone()) {
            Expr::ParenExpr(_) => "expr",
            inner => literal_kind(&inner),
        },
        Expr::VectorExpr(_) => "vector",
        Expr::ArrayExpr(_) => "array",
        Expr::StructExpr(_) => "struct",
        // `::f`, `path::f` and `&f` as values are function pointers.
        Expr::FunctionRef(_) | Expr::QualifiedRef(_) => "function",
        Expr::AnimRef(_) => "anim",
        Expr::AnimtreeExpr(_) => "animtree",
        _ => "expr",
    }
}

fn file_out(ws: &Workspace, f: FileId, probe: &Dialect) -> FileOut {
    let data = ws.file(f);
    let mut out = FileOut {
        syntax_errors: data.errors.iter().any(|e| e.is_syntax()),
        ..FileOut::default()
    };
    let probe = probe.clone().with_decompiled(data.decompiled);
    let parsed = gsc_syntax::parse(data.text(), &probe);
    let gated = gsc_syntax::check_dialect(&parsed.syntax(), &probe);
    for e in parsed.errors().iter().chain(&gated) {
        if let Some(k) = feature_key(e.code, &e.message) {
            *out.features.entry(k).or_default() += 1;
        }
    }
    let facts = &data.facts;
    let res = ws.resolution(f);
    let how = |i: usize| res.refs.get(i).map_or(How::Unresolved, |x| x.how);
    for (i, r) in facts.refs.iter().enumerate() {
        if r.kind == "pointer" {
            continue;
        }
        out.calls.sites += 1;
        match (&r.target, how(i)) {
            (_, h) if h.is_script() => out.calls.script += 1,
            (Target::Dynamic(_) | Target::Method(_), _) | (_, How::Dynamic) => {
                out.calls.dynamic += 1
            }
            (Target::Qualified(..), How::Unresolved) => out.calls.qualified_unresolved += 1,
            _ => {}
        }
    }
    let by_range: HashMap<_, usize> = facts
        .refs
        .iter()
        .enumerate()
        .map(|(i, r)| (r.range, i))
        .collect();
    let vm = match data.meta.vm {
        Some(Vm::Csc) => "csc",
        _ => "gsc",
    };
    let mut seen = std::collections::HashSet::new();
    for ev in data.syntax().preorder() {
        let WalkEvent::Enter(n) = ev else { continue };
        if n.kind() != SyntaxKind::CALL_EXPR {
            continue;
        }
        let Some(c) = CallExpr::cast(n) else { continue };
        let Some(Expr::NameRef(name)) = c.callee() else {
            continue;
        };
        // No ref: an event keyword (`notify`...), counted as an event.
        let Some(&i) = by_range.get(&name.syntax().text_range()) else {
            continue;
        };
        if !seen.insert(i) {
            continue; // the same call seen from an enclosing node
        }
        let r = &facts.refs[i];
        let keyword = matches!(r.target, Target::Keyword(_));
        let lower = match &r.target {
            Target::Keyword(n) => n.to_ascii_lowercase(),
            Target::Bare(n)
                if matches!(how(i), How::Unresolved | How::Builtin | How::BuiltinOtherVm) =>
            {
                n.to_ascii_lowercase()
            }
            _ => continue,
        };
        if !is_identifier(&lower) {
            out.calls.skipped_names += 1;
            continue;
        }
        out.calls.external += 1;
        let kind = if c.receiver().is_some() {
            "method"
        } else {
            "function"
        };
        let st = out.builtins.entry((lower, vm, kind)).or_default();
        st.keyword |= keyword;
        st.sites += 1;
        st.files = 1;
        if matches!(r.kind, "thread" | "childthread") {
            st.threaded += 1;
        }
        if kind == "method" {
            *st.receivers.entry(r.receiver).or_default() += 1;
        }
        let args: Vec<Expr> = c
            .arg_list()
            .map(|l| l.exprs().collect())
            .unwrap_or_default();
        *st.argc.entry(args.len() as u32).or_default() += 1;
        for (j, a) in args.iter().take(MAX_ARGS).enumerate() {
            if st.arg_literals.len() <= j {
                st.arg_literals.resize_with(j + 1, BTreeMap::new);
            }
            *st.arg_literals[j].entry(literal_kind(a)).or_default() += 1;
        }
    }
    out
}

fn add_map<K: Ord + Clone>(into: &mut BTreeMap<K, u64>, from: &BTreeMap<K, u64>) {
    for (k, v) in from {
        *into.entry(k.clone()).or_default() += v;
    }
}

fn merge_call(into: &mut CallStat, from: CallStat) {
    into.keyword |= from.keyword;
    into.sites += from.sites;
    into.files += from.files;
    into.threaded += from.threaded;
    add_map(&mut into.argc, &from.argc);
    add_map(&mut into.receivers, &from.receivers);
    if into.arg_literals.len() < from.arg_literals.len() {
        into.arg_literals
            .resize_with(from.arg_literals.len(), BTreeMap::new);
    }
    for (j, m) in from.arg_literals.iter().enumerate() {
        add_map(&mut into.arg_literals[j], m);
    }
}

fn class_label(c: EventClass) -> &'static str {
    match c {
        EventClass::Emit => "emit",
        EventClass::Wait => "wait",
        EventClass::Endon => "endon",
    }
}

/// Measure a loaded and resolved workspace. Build it without a builtin catalogue, so that the
/// report does not depend on gruff-data's catalogues: every call nothing defines is reported.
pub fn report(ws: &Workspace, opts: &Options) -> Result<Report, String> {
    let probe = probe_dialect(&opts.profile_text)?;
    let ids: Vec<FileId> = ws.files().collect();
    let several = ws.roots().len() > 1;
    // Root labels are directory names, which may say something about the machine: number them.
    let rel_of = |f: FileId| {
        let m = &ws.file(f).meta;
        if several {
            format!("root{}/{}", m.root, m.rel)
        } else {
            m.rel.clone()
        }
    };
    let per_file = par_map(&ids, ws.jobs(), |&f| file_out(ws, f, &probe));
    let mut rep = Report {
        toolchain: opts.toolchain.clone(),
        game: opts.game.clone(),
        profile_id: ws.profile(0).dialect.id.clone(),
        profile_hash: xxh64(&opts.profile_text.replace("\r\n", "\n")),
        mode: opts.mode.as_str().to_owned(),
        scripts_mode: opts.scripts.label(),
        event_spaces: opts.event_spaces,
        ..Report::default()
    };
    let mut hashes = Vec::new();
    let mut feature_files: BTreeMap<String, u64> = BTreeMap::new();
    for (&f, out) in ids.iter().zip(per_file) {
        let data = ws.file(f);
        let c = &mut rep.corpus;
        c.files += 1;
        match data.meta.vm {
            Some(Vm::Gsc) => c.gsc += 1,
            Some(Vm::Csc) => c.csc += 1,
            None => c.headers += 1,
        }
        c.decompiled += u64::from(data.decompiled);
        c.bytes += data.text().len() as u64;
        c.lines += data.text().lines().count() as u64;
        c.files_with_syntax_errors += u64::from(out.syntax_errors);
        c.functions += data
            .facts
            .defs
            .iter()
            .filter(|d| d.kind.is_callable())
            .count() as u64;
        let res = ws.resolution(f);
        for (imp, found) in data.facts.imports.iter().zip(res.imports.iter()) {
            if imp.is_scope() {
                c.imports += 1;
                c.imports_unresolved += u64::from(found.is_none());
            }
        }
        let rel = rel_of(f);
        let h = script_hash(&rel);
        match opts.scripts {
            ScriptNames::Hash => rep.scripts.push((h.clone(), vm_label(data.meta.vm))),
            ScriptNames::Names => rep.scripts.push((rel, vm_label(data.meta.vm))),
            ScriptNames::Drop => {}
        }
        hashes.push(h);
        for (k, n) in out.features {
            rep.features.entry(k.clone()).or_default().uses += n;
            *feature_files.entry(k).or_default() += 1;
        }
        let t = &mut rep.calls;
        t.sites += out.calls.sites;
        t.script += out.calls.script;
        t.external += out.calls.external;
        t.qualified_unresolved += out.calls.qualified_unresolved;
        t.dynamic += out.calls.dynamic;
        t.skipped_names += out.calls.skipped_names;
        for (k, s) in out.builtins {
            merge_call(rep.builtins.entry(k).or_default(), s);
        }
    }
    for (k, n) in feature_files {
        if let Some(e) = rep.features.get_mut(&k) {
            e.files = n;
        }
    }
    rep.scripts.sort();
    hashes.sort();
    rep.fingerprint = xxh64(&hashes.join("\n"));

    // Events and fields: the index's view (macro expansions included).
    let index = crate::index(ws);
    for fi in &index.files {
        let mut seen_events = BTreeSet::new();
        for e in &fi.events {
            let class = class_label(e.class);
            let (name, pattern) = match &e.name {
                EventName::Literal(s) => (s.to_ascii_lowercase(), false),
                EventName::Pattern(s) => (s.to_ascii_lowercase(), true),
                EventName::Dynamic => {
                    *rep.events_dynamic.entry(class).or_default() += 1;
                    continue;
                }
            };
            let op = e.op.to_ascii_lowercase();
            if !is_event_name(&name, opts.event_spaces) || !is_identifier(&op) {
                rep.events_skipped += 1;
                continue;
            }
            let st = rep.events.entry(name.clone()).or_default();
            st.pattern |= pattern;
            match e.class {
                EventClass::Emit => {
                    st.emit += 1;
                    if op == "notify" {
                        *st.payload.entry(e.argc as u32).or_default() += 1;
                    }
                }
                EventClass::Wait => st.wait += 1,
                EventClass::Endon => st.endon += 1,
            }
            *st.ops.entry(op).or_default() += 1;
            *st.receivers.entry(e.receiver).or_default() += 1;
            if seen_events.insert(name) {
                st.files += 1;
            }
        }
        let mut seen_fields = BTreeSet::new();
        for fl in &fi.fields {
            let name = fl.name.to_ascii_lowercase();
            if !is_identifier(&name) {
                rep.fields_skipped += 1;
                continue;
            }
            let key = (fl.obj, name);
            let st = rep.fields.entry(key.clone()).or_default();
            if fl.op == "read" {
                st.reads += 1;
            } else {
                st.writes += 1;
            }
            if seen_fields.insert(key) {
                st.files += 1;
            }
        }
    }
    Ok(rep)
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn obj<K: std::fmt::Display>(m: &BTreeMap<K, u64>) -> String {
    let kv: Vec<String> = m
        .iter()
        .map(|(k, v)| format!("{}: {v}", esc(&k.to_string())))
        .collect();
    format!("{{{}}}", kv.join(", "))
}

fn list(items: &[String]) -> String {
    if items.is_empty() {
        String::new()
    } else {
        format!("\n{}\n  ", items.join(",\n"))
    }
}

/// What a report holds, written into the report itself.
pub const PRIVACY: &[&str] = &[
    "counts and identifiers only: no source text, no comments, no line numbers",
    "builtin candidates: names the scripts call but do not define, argument counts, literal argument types (int, string...; never values)",
    "events: identifier-like literal names of notify/waittill/endon sites, counts per operation and receiver kind; computed names are counted, not named",
    "fields: identifier field names per object kind (level, self, game, anim, other), read/write counts",
    "features: per dialect construct, uses and files",
    "scripts: relative script names hashed (default), listed, or dropped; no other path",
    "toolchain version, profile id and profile hash; no time stamp, no user or machine name",
];

impl Report {
    /// The deterministic JSON text; `report_id` is an xxh3 of the rest.
    pub fn to_json(&self) -> String {
        let body = self.json_body();
        let mut out = String::new();
        out.push_str("{\n  \"format\": \"gsc-corpus-report\",\n");
        let _ = writeln!(out, "  \"format_version\": {FORMAT_VERSION},");
        let _ = writeln!(out, "  \"report_id\": \"{}\",", xxh64(&body));
        out.push_str(&body);
        out.push_str("}\n");
        out
    }

    fn json_body(&self) -> String {
        let mut o = String::new();
        let _ = writeln!(
            o,
            "  \"toolchain\": {{\"name\": \"gruff\", \"version\": {}}},",
            esc(&self.toolchain)
        );
        let _ = writeln!(o, "  \"game\": {},", esc(&self.game));
        let _ = writeln!(
            o,
            "  \"profile\": {{\"id\": {}, \"xxh3\": {}, \"mode\": {}}},",
            esc(&self.profile_id),
            esc(&self.profile_hash),
            esc(&self.mode)
        );
        let p: Vec<String> = PRIVACY.iter().map(|s| esc(s)).collect();
        let _ = writeln!(o, "  \"privacy\": [{}],", p.join(", "));
        let c = &self.corpus;
        let _ = writeln!(
            o,
            "  \"corpus\": {{\"fingerprint\": {}, \"scripts_mode\": {}, \"files\": {}, \"gsc\": {}, \"csc\": {}, \"headers\": {}, \"decompiled\": {}, \"bytes\": {}, \"lines\": {}, \"files_with_syntax_errors\": {}, \"functions\": {}, \"imports\": {}, \"imports_unresolved\": {}}},",
            esc(&self.fingerprint),
            esc(self.scripts_mode),
            c.files,
            c.gsc,
            c.csc,
            c.headers,
            c.decompiled,
            c.bytes,
            c.lines,
            c.files_with_syntax_errors,
            c.functions,
            c.imports,
            c.imports_unresolved
        );
        let s: Vec<String> = self
            .scripts
            .iter()
            .map(|(n, vm)| format!("    {{\"script\": {}, \"vm\": \"{vm}\"}}", esc(n)))
            .collect();
        let _ = writeln!(o, "  \"scripts\": [{}],", list(&s));
        let f: Vec<String> = self
            .features
            .iter()
            .map(|(k, u)| {
                let (feat, what) = k.split_once(':').unwrap_or((k, ""));
                format!(
                    "    {{\"feature\": {}, \"construct\": {}, \"uses\": {}, \"files\": {}}}",
                    esc(feat),
                    esc(what),
                    u.uses,
                    u.files
                )
            })
            .collect();
        let _ = writeln!(o, "  \"features\": [{}],", list(&f));
        let t = &self.calls;
        let _ = writeln!(
            o,
            "  \"calls\": {{\"sites\": {}, \"script\": {}, \"external\": {}, \"qualified_unresolved\": {}, \"dynamic\": {}, \"skipped_names\": {}}},",
            t.sites, t.script, t.external, t.qualified_unresolved, t.dynamic, t.skipped_names
        );
        let b: Vec<String> = self
            .builtins
            .iter()
            .map(|((name, vm, kind), s)| {
                let lits: Vec<String> = s.arg_literals.iter().map(obj).collect();
                format!(
                    "    {{\"name\": {}, \"vm\": \"{vm}\", \"kind\": \"{kind}\", \"keyword\": {}, \"sites\": {}, \"files\": {}, \"threaded\": {}, \"argc\": {}, \"receivers\": {}, \"arg_literals\": [{}]}}",
                    esc(name),
                    s.keyword,
                    s.sites,
                    s.files,
                    s.threaded,
                    obj(&s.argc),
                    obj(&s.receivers),
                    lits.join(", ")
                )
            })
            .collect();
        let _ = writeln!(o, "  \"builtins\": [{}],", list(&b));
        let e: Vec<String> = self
            .events
            .iter()
            .map(|(name, s)| {
                format!(
                    "    {{\"name\": {}, \"pattern\": {}, \"emit\": {}, \"wait\": {}, \"endon\": {}, \"files\": {}, \"ops\": {}, \"receivers\": {}, \"payload\": {}}}",
                    esc(name),
                    s.pattern,
                    s.emit,
                    s.wait,
                    s.endon,
                    s.files,
                    obj(&s.ops),
                    obj(&s.receivers),
                    obj(&s.payload)
                )
            })
            .collect();
        let _ = writeln!(
            o,
            "  \"events\": {{\"spaces\": {}, \"dynamic\": {}, \"skipped\": {}, \"names\": [{}]}},",
            self.event_spaces,
            obj(&self.events_dynamic),
            self.events_skipped,
            list(&e)
        );
        let fl: Vec<String> = self
            .fields
            .iter()
            .map(|((objk, name), s)| {
                format!(
                    "    {{\"object\": \"{objk}\", \"name\": {}, \"reads\": {}, \"writes\": {}, \"files\": {}}}",
                    esc(name),
                    s.reads,
                    s.writes,
                    s.files
                )
            })
            .collect();
        let _ = writeln!(
            o,
            "  \"fields\": {{\"skipped\": {}, \"names\": [{}]}}",
            self.fields_skipped,
            list(&fl)
        );
        o
    }

    /// A short human summary of what the report holds.
    pub fn summary(&self) -> String {
        let c = &self.corpus;
        let mut o = String::new();
        let _ = writeln!(
            o,
            "GSC corpus report: game {} (profile {}, {} mode), gruff {}",
            self.game, self.profile_id, self.mode, self.toolchain
        );
        let _ = writeln!(
            o,
            "  files: {} ({} gsc, {} csc, {} headers; {} decompiled, {} with syntax errors), {} lines",
            c.files, c.gsc, c.csc, c.headers, c.decompiled, c.files_with_syntax_errors, c.lines
        );
        let listed = match self.scripts_mode {
            "drop" => "not listed".to_owned(),
            m => format!("{} listed as {m}", self.scripts.len()),
        };
        let _ = writeln!(
            o,
            "  scripts: {listed}; corpus fingerprint {}",
            self.fingerprint
        );
        let _ = writeln!(o, "  features: {} constructs used", self.features.len());
        for (k, u) in &self.features {
            let _ = writeln!(o, "    {k}: {} uses in {} files", u.uses, u.files);
        }
        let t = &self.calls;
        let fns = self.builtins.values().filter(|s| !s.keyword).count();
        let _ = writeln!(
            o,
            "  calls: {} sites ({} to scripts, {} external, {} unresolved qualified, {} dynamic)",
            t.sites, t.script, t.external, t.qualified_unresolved, t.dynamic
        );
        let _ = writeln!(
            o,
            "  builtin candidates: {fns} (name, vm, kind), {} keyword intrinsics",
            self.builtins.len() - fns
        );
        let dynamic: u64 = self.events_dynamic.values().sum();
        let _ = writeln!(
            o,
            "  events: {} names ({dynamic} computed-name sites counted, {} names left out)",
            self.events.len(),
            self.events_skipped
        );
        let _ = writeln!(o, "  fields: {} (object, name) pairs", self.fields.len());
        o.push_str("  no code, no string values, no machine path: see docs/community-reports.md\n");
        o
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gsc_sema::Profile;

    const PROFILE: &str = r#"
schema_version = 1
id = "tx"
name = "Test"
[keywords]
words = ["if", "foreach", "in", "isdefined", "self", "level", "notify", "waittill", "endon", "thread", "wait", "return", "undefined", "true", "false"]
[features]
foreach = { value = true, confidence = "documented", source = [] }
classes = { value = false, confidence = "documented", source = [] }
import_directive = { value = "include", confidence = "documented", source = [] }
function_pointer_style = { value = "colon", confidence = "documented", source = [] }
istring_literal = { value = true, confidence = "documented", source = [] }
case_insensitive_identifiers = { value = true, confidence = "documented", source = [] }
"#;

    const A: &str = r#"#include common;
main()
{
    level.score = 0;
    self.name = "secret player name";
    foo( 1, "text", -2.5 );
    self setmodel( "a_model" );
    self thread helper( undefined );
    ent = spawn( "script_origin", ( 0, 0, 0 ) );
    foreach ( x in level.players ) { x notify( "spawned_player", 1 ); }
    self waittill( "death" );
    self endon( "disconnect" );
    if ( isdefined( level.score ) ) { wait 1; }
    self notify( "this is not an id" );
}
helper( a ) { return a; }
"#;

    fn build(src: &[(&str, &str)]) -> Workspace {
        let dialect = Dialect::from_toml_str(PROFILE).unwrap();
        let ws = crate::workspace_from_sources(src, Profile::new(dialect, None, None));
        ws.resolve_all(); // not build(): its load() would read the crate's folder
        ws
    }

    fn opts(scripts: ScriptNames) -> Options {
        Options {
            toolchain: "0.0.0-test".into(),
            game: "tx".into(),
            profile_text: PROFILE.into(),
            mode: gsc_syntax::Mode::Retail,
            scripts,
            event_spaces: false,
        }
    }

    fn key(name: &str, kind: &'static str) -> CallKey {
        (name.to_owned(), "gsc", kind)
    }

    #[test]
    fn measures_calls_events_fields_features() {
        let ws = build(&[("maps/a.gsc", A)]);
        let r = report(&ws, &opts(ScriptNames::Hash)).unwrap();
        let foo = &r.builtins[&key("foo", "function")];
        assert_eq!(foo.sites, 1);
        assert_eq!(foo.argc.get(&3), Some(&1));
        assert_eq!(foo.arg_literals[0].get("int"), Some(&1));
        assert_eq!(foo.arg_literals[1].get("string"), Some(&1));
        assert_eq!(foo.arg_literals[2].get("float"), Some(&1));
        let sm = &r.builtins[&key("setmodel", "method")];
        assert_eq!(sm.receivers.get("self"), Some(&1));
        let sp = &r.builtins[&key("spawn", "function")];
        assert_eq!(sp.arg_literals[1].get("vector"), Some(&1));
        // A script function is not a builtin candidate; a keyword call is, as a keyword.
        assert!(!r.builtins.contains_key(&key("helper", "method")));
        assert!(r.builtins[&key("isdefined", "function")].keyword);
        assert_eq!(r.events["spawned_player"].emit, 1);
        assert_eq!(r.events["spawned_player"].payload.get(&1), Some(&1));
        assert_eq!(r.events["death"].wait, 1);
        assert_eq!(r.events["disconnect"].endon, 1);
        assert!(!r.events.contains_key("this is not an id"));
        assert_eq!(r.events_skipped, 1);
        assert_eq!(r.fields[&("level", "score".to_owned())].writes, 1);
        assert!(r.features.keys().any(|k| k.starts_with("foreach:")));
        assert!(r
            .features
            .keys()
            .any(|k| k.starts_with("import_directive:")));
        assert_eq!(r.corpus.imports_unresolved, 1);
        let json = r.to_json();
        for leak in [
            "secret player name",
            "a_model",
            "script_origin",
            "\"text\"",
            "maps/a.gsc",
        ] {
            assert!(!json.contains(leak), "{leak} leaked");
        }
        assert!(json.contains(&script_hash("maps/a.gsc")));
    }

    #[test]
    fn deterministic_and_script_modes() {
        let src = [("maps/a.gsc", A), ("maps/b.gsc", "f() { g( 1 ); }\n")];
        let a = report(&build(&src), &opts(ScriptNames::Hash))
            .unwrap()
            .to_json();
        let mut rev = src;
        rev.reverse();
        let b = report(&build(&rev), &opts(ScriptNames::Hash))
            .unwrap()
            .to_json();
        assert_eq!(a, b);
        let names = report(&build(&src), &opts(ScriptNames::Names)).unwrap();
        assert!(names.to_json().contains("maps/b.gsc"));
        let dropped = report(&build(&src), &opts(ScriptNames::Drop)).unwrap();
        assert!(dropped.scripts.is_empty());
        assert_eq!(dropped.fingerprint, names.fingerprint);
        assert!(!dropped.to_json().contains(&script_hash("maps/b.gsc")));
    }

    #[test]
    fn identifier_filters() {
        assert!(is_identifier("setmodel"));
        assert!(!is_identifier("a b"));
        assert!(!is_identifier(""));
        assert!(is_event_name("+attack", false));
        assert!(is_event_name("trigger_*", false));
        assert!(!is_event_name("single anim", false));
        assert!(is_event_name("single anim", true));
        assert!(!is_event_name("a  b", true));
        assert!(!is_event_name("this is far too many words here", true));
        assert!(!is_event_name("C:/Users", true));
    }
}
