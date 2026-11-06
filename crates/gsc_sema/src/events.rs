//! Events: the sites that notify, wait for or end on a named event (`notify`, `waittill`,
//! `waittillmatch`, `endon` and the usual wrappers such as `waittill_any`), the engine's event
//! catalogue (`gruff-data/events/<game>.toml`) and the workspace event graph that links them.
//!
//! Sites are facts of one file ([`crate::FileFacts::events`]), read during fact extraction. The
//! workspace keeps them in global tables patched per file like the name tables, so that the
//! graph queries ([`Workspace::event_emitted`], [`Workspace::event_sites`]...) are lookups, not
//! walks: they run on every lint of the language server.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use gsc_syntax::ast::{AstNode, BinExpr, CallExpr, Expr};
use gsc_syntax::{SyntaxKind, TextRange, TextSize};

use crate::facts::{code_text, focus_of, receiver_kind, string_content, strip_parens};
use crate::ide::{FileRange, Hover, NavTarget, Symbol, SymbolKind};
use crate::par::par_map;
use crate::profile::{Config, EventArgs};
use crate::workspace::{DefId, FileId, Workspace};

/// What a site does with an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventClass {
    Emit,
    Wait,
    Endon,
}

/// An event name: a literal, a pattern built from a concatenation, or unknown.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EventName {
    Literal(String),
    /// `"trigger_" + n` is `trigger_*`.
    Pattern(String),
    Dynamic,
}

impl EventName {
    pub fn render(&self) -> &str {
        match self {
            EventName::Literal(s) | EventName::Pattern(s) => s,
            EventName::Dynamic => "<dynamic>",
        }
    }

    pub fn literal(&self) -> Option<&str> {
        match self {
            EventName::Literal(s) => Some(s),
            _ => None,
        }
    }
}

/// One event site.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventSite {
    pub name: EventName,
    /// `notify`, `waittill`, `waittillmatch`, `endon` or a wrapper name (`waittill_any`...).
    pub op: String,
    pub class: EventClass,
    /// [`receiver_kind`] of the receiver; `self` when there is none (the implicit self).
    pub receiver: &'static str,
    /// The receiver as written (`-` for the implicit self).
    pub receiver_text: String,
    /// Payload of a `notify`, variables bound by a `waittill`, other arguments of a wrapper.
    pub argc: usize,
    /// The enclosing function or method (index into the file's definitions).
    pub caller: Option<u32>,
    /// The first significant token of the call.
    pub focus: TextSize,
    /// The name argument.
    pub name_range: TextRange,
    /// The source text of the name argument when it is not a plain literal.
    pub expr: String,
    /// The fields a `notify` sends when its payload is a struct literal (`notify( "e",
    /// { #a: 1 } )`) or nothing: the keys, lower case, without `#`. `None` for any other
    /// payload and for every other site.
    pub fields: Option<Vec<String>>,
}

impl EventSite {
    /// A `notify`, `waittill`, `waittillmatch` or `endon` keyword (not a wrapper).
    pub fn is_keyword(&self) -> bool {
        is_event_op(&self.op)
    }

    /// The literal name in lower case (event names compare without case: T7 and later hash
    /// them, and a case-only difference is no evidence of a typo).
    pub fn key(&self) -> Option<String> {
        self.name.literal().map(str::to_ascii_lowercase)
    }
}

fn is_event_op(op: &str) -> bool {
    matches!(op, "notify" | "waittill" | "waittillmatch" | "endon")
}

/// Event wrappers recognised by name (last segment, lower case). Every string argument is an
/// event name; other arguments (timeouts, entities) are skipped.
pub const WRAPPERS: &[(&str, EventClass)] = &[
    ("waittill_any", EventClass::Wait),
    ("waittill_any_return", EventClass::Wait),
    ("waittill_any_timeout", EventClass::Wait),
    ("waittill_any_array", EventClass::Wait),
    ("waittill_any_array_return", EventClass::Wait),
    ("waittill_any_ex", EventClass::Wait),
    ("waittill_any_ents", EventClass::Wait),
    ("waittill_any_ents_two", EventClass::Wait),
    ("waittill_either", EventClass::Wait),
    ("waittill_multiple", EventClass::Wait),
    ("waittill_multiple_ents", EventClass::Wait),
    ("waittill_notify_or_timeout", EventClass::Wait),
    ("waittill_timeout", EventClass::Wait),
    ("waittill_any_2", EventClass::Wait),
    ("waittill_any_3", EventClass::Wait),
    ("waittill_any_timeout_1", EventClass::Wait),
    ("waittill_any_timeout_2", EventClass::Wait),
    ("waittill_any_in_array_return", EventClass::Wait),
    ("waittill_any_in_array_or_timeout", EventClass::Wait),
    ("delay_notify", EventClass::Emit),
    ("notify_delay", EventClass::Emit),
    ("notify_all", EventClass::Emit),
    ("wait_endon", EventClass::Endon),
];

/// The class of a wrapper, by lower-case last name segment.
pub fn wrapper_class(lower: &str) -> Option<EventClass> {
    WRAPPERS.iter().find(|(w, _)| *w == lower).map(|(_, c)| *c)
}

fn is_plus(e: &BinExpr) -> bool {
    e.op().is_some_and(|t| t.kind() == SyntaxKind::PLUS)
}

fn pattern_pieces(e: &Expr, out: &mut String, literal: &mut bool) {
    match strip_parens(e.clone()) {
        Expr::BinExpr(b) if is_plus(&b) => {
            if let Some(l) = b.lhs() {
                pattern_pieces(&l, out, literal);
            }
            if let Some(r) = b.rhs() {
                pattern_pieces(&r, out, literal);
            }
        }
        Expr::Literal(l) => {
            *literal = true;
            match string_content(&l) {
                Some(s) => out.push_str(&s),
                None => out.push_str(l.syntax().text().to_string().trim()),
            }
        }
        _ => {
            if !out.ends_with('*') {
                out.push('*');
            }
        }
    }
}

/// The event name an argument denotes. `strings_only` rejects numbers and other non-string
/// literals (for wrappers, whose other arguments are timeouts).
pub fn event_name(e: &Expr, strings_only: bool) -> Option<EventName> {
    match strip_parens(e.clone()) {
        Expr::Literal(l) => match string_content(&l) {
            Some(s) => Some(EventName::Literal(s)),
            None if strings_only => None,
            None => Some(EventName::Literal(
                l.syntax().text().to_string().trim().to_owned(),
            )),
        },
        Expr::BinExpr(b) if is_plus(&b) => {
            let mut out = String::new();
            let mut literal = false;
            pattern_pieces(&Expr::BinExpr(b), &mut out, &mut literal);
            if literal && out != "*" {
                Some(EventName::Pattern(out))
            } else if strings_only {
                None
            } else {
                Some(EventName::Dynamic)
            }
        }
        _ if strings_only => None,
        _ => Some(EventName::Dynamic),
    }
}

/// Does the pattern `pat` (`*` for any text) match the lower-case name `name`?
pub fn pattern_matches(pat: &str, name: &str) -> bool {
    let pat = pat.to_ascii_lowercase();
    let parts: Vec<&str> = pat.split('*').collect();
    if parts.len() == 1 {
        return pat == name;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !name.starts_with(first) || !name[first.len()..].ends_with(last) {
        return false;
    }
    let mut rest = &name[first.len()..name.len() - last.len()];
    for p in &parts[1..parts.len() - 1] {
        match rest.find(p) {
            Some(i) => rest = &rest[i + p.len()..],
            None => return false,
        }
    }
    true
}

/// A struct key or a field as written (`#name`, `name`, `"name"`, `#"name"`): lower case, without
/// `#` and quotes.
pub fn field_key(text: &str) -> String {
    let t = text.trim().trim_start_matches('#');
    t.trim_matches('"').to_ascii_lowercase()
}

/// The fields of a notify's payload: none sent, or the keys of one struct literal.
fn payload_fields(payload: &[Expr]) -> Option<Vec<String>> {
    match payload {
        [] => Some(Vec::new()),
        [Expr::StructExpr(s)] => s
            .struct_fields()
            .map(|f| f.key().map(|k| field_key(&k.syntax().text().to_string())))
            .collect(),
        _ => None,
    }
}

/// The event sites of one call: an event keyword (`op` lower case) or a wrapper. `cfg` says
/// what the arguments after a `waittill`'s or an `endon`'s first are in the game.
pub(crate) fn call_sites(
    c: &CallExpr,
    op: &str,
    class: EventClass,
    caller: Option<u32>,
    cfg: &Config,
    out: &mut Vec<EventSite>,
) {
    let args: Vec<Expr> = c
        .arg_list()
        .map(|a| a.exprs().collect())
        .unwrap_or_default();
    let receiver = c.receiver();
    let focus = focus_of(c.syntax());
    let push = |out: &mut Vec<EventSite>, name: EventName, arg: &Expr, argc: usize| {
        let expr = match name {
            EventName::Literal(_) => String::new(),
            _ => code_text(arg.syntax(), 80),
        };
        let (rk, rt) = match &receiver {
            Some(r) => (receiver_kind(Some(r)), code_text(r.syntax(), 60)),
            None => ("self", "-".to_owned()),
        };
        out.push(EventSite {
            name,
            op: op.to_owned(),
            class,
            receiver: rk,
            receiver_text: rt,
            argc,
            caller,
            focus,
            name_range: strip_parens(arg.clone()).syntax().text_range(),
            expr,
            fields: None,
        });
    };
    if is_event_op(op) {
        let Some(first) = args.first() else { return };
        match op {
            // BO4 and later take every argument as an event, T7 binds the rest to names; for
            // other games every string argument is an event and the rest are bindings.
            "waittill" | "endon" => {
                let rest = if op == "waittill" {
                    cfg.waittill_args
                } else {
                    cfg.endon_args
                };
                let mut names = Vec::new();
                for (i, a) in args.iter().enumerate() {
                    let name = match rest {
                        _ if i == 0 => event_name(a, false).or(Some(EventName::Dynamic)),
                        EventArgs::Events => event_name(a, false),
                        EventArgs::Bindings => None,
                        EventArgs::Guess => event_name(a, true),
                    };
                    if let Some(n) = name {
                        names.push((n, a));
                    }
                }
                let argc = args.len() - names.len();
                for (n, a) in names {
                    push(out, n, a, argc);
                }
            }
            // BO4 and later: `waittillmatch( { #name: v }, "e" )`, the event second.
            "waittillmatch" if cfg.waittill_struct && args.len() > 1 => {
                let n = event_name(&args[1], false).unwrap_or(EventName::Dynamic);
                push(out, n, &args[1], 0);
            }
            _ => {
                let n = event_name(first, false).unwrap_or(EventName::Dynamic);
                push(out, n, first, args.len() - 1);
                if op == "notify" {
                    if let Some(s) = out.last_mut() {
                        s.fields = payload_fields(&args[1..]);
                    }
                }
            }
        }
        return;
    }
    let names: Vec<(EventName, &Expr)> = args
        .iter()
        .filter_map(|a| event_name(a, true).map(|n| (n, a)))
        .collect();
    let argc = args.len() - names.len();
    if names.is_empty() {
        if let Some(first) = args.first() {
            push(out, EventName::Dynamic, first, argc);
        }
        return;
    }
    for (n, a) in names {
        push(out, n, a, argc);
    }
}

/// A string literal worth remembering for the event graph: identifier-like, short. Such a
/// string used anywhere but as a waited name may reach a `notify( name )` through a variable.
pub(crate) fn event_like_string(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b' '))
}

/// A decompiler's spelling of a hashed event name (`hash_1a2b3c4d`, `_id_1A2B`), as its value.
pub fn hashed_event(name: &str) -> Option<u64> {
    let lower = name.to_ascii_lowercase();
    ["hash_", "event_", "_id_"].iter().find_map(|p| {
        let rest = lower.strip_prefix(p)?;
        (!rest.is_empty() && rest.len() <= 16 && rest.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| u64::from_str_radix(rest, 16).ok())
            .flatten()
    })
}

// ---------------------------------------------------------------------------------------------
// The engine catalogue.

/// Who the engine notifies an event on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EngineReceiver {
    Level,
    Entity,
    Player,
    Any,
}

impl EngineReceiver {
    fn parse(s: &str) -> EngineReceiver {
        match s {
            "level" => EngineReceiver::Level,
            "entity" => EngineReceiver::Entity,
            "player" => EngineReceiver::Player,
            _ => EngineReceiver::Any,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            EngineReceiver::Level => "level",
            EngineReceiver::Entity => "entity",
            EngineReceiver::Player => "player",
            EngineReceiver::Any => "any",
        }
    }
}

/// One event the engine notifies.
#[derive(Clone, Debug)]
pub struct EngineEvent {
    /// Lower case, when known.
    pub name: Option<String>,
    pub receiver: EngineReceiver,
    /// The payload the engine pushes.
    pub args: usize,
    /// The payload varies between sites (then `args` is the largest seen).
    pub args_variable: bool,
}

/// The events a game's engine notifies (`gruff-data/events/<game>.toml`).
#[derive(Clone, Debug, Default)]
pub struct EventCatalogue {
    pub game: String,
    /// `[coverage] complete = true`: every engine notify was found, so an event missing here
    /// and from every script is notified by nothing.
    pub complete: bool,
    pub events: Vec<EngineEvent>,
    by_name: HashMap<String, Vec<usize>>,
    /// Hashed entries by hash value.
    by_hash: HashMap<u64, Vec<usize>>,
    families: Vec<&'static gsc_hash::Family>,
}

fn parse_hash(v: &toml::Value) -> Option<u64> {
    match v {
        toml::Value::String(s) => {
            let t = s.trim().trim_start_matches("0x").trim_start_matches("0X");
            u64::from_str_radix(t, 16).ok()
        }
        toml::Value::Integer(i) => Some(*i as u64),
        _ => None,
    }
}

impl EventCatalogue {
    pub fn load(path: &Path) -> Result<EventCatalogue, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        EventCatalogue::from_toml_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn from_toml_str(text: &str) -> Result<EventCatalogue, String> {
        let doc: toml::Table = text.parse().map_err(|e| format!("invalid TOML: {e}"))?;
        let meta = doc.get("meta").and_then(|v| v.as_table());
        let get = |k: &str| {
            meta.and_then(|m| m.get(k))
                .or_else(|| doc.get(k))
                .and_then(|v| v.as_str())
        };
        let mut c = EventCatalogue {
            game: get("game").unwrap_or_default().to_owned(),
            ..EventCatalogue::default()
        };
        let default_family = get("hash_family");
        if let Some(cov) = doc.get("coverage").and_then(|v| v.as_table()) {
            c.complete = crate::data::toml_get::flag(cov, "complete");
        }
        let entries = doc
            .get("event")
            .and_then(|v| v.as_array())
            .map(Vec::as_slice)
            .unwrap_or_default();
        for e in entries {
            let Some(t) = e.as_table() else { continue };
            let name = t
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::to_ascii_lowercase);
            let hash = t.get("hash").and_then(parse_hash);
            if name.is_none() && hash.is_none() {
                continue;
            }
            let args = t.get("args").and_then(|v| v.as_array()).map_or(0, Vec::len);
            let ev = EngineEvent {
                name: name.clone(),
                receiver: EngineReceiver::parse(
                    t.get("receiver").and_then(|v| v.as_str()).unwrap_or("any"),
                ),
                args,
                args_variable: crate::data::toml_get::flag(t, "args_variable"),
            };
            let i = c.events.len();
            c.events.push(ev);
            if let Some(n) = name {
                c.by_name.entry(n).or_default().push(i);
            }
            if let Some(h) = hash {
                let fam = t
                    .get("hash_family")
                    .and_then(|v| v.as_str())
                    .or(default_family)
                    .and_then(gsc_hash::family);
                if let Some(f) = fam {
                    if !c.families.iter().any(|x| std::ptr::eq(*x, f)) {
                        c.families.push(f);
                    }
                }
                c.by_hash.entry(h).or_default().push(i);
            }
        }
        Ok(c)
    }

    /// The entries for a script name (any case), by name, by hash, or by a decompiler's
    /// `hash_<hex>` spelling.
    pub fn lookup(&self, name: &str) -> Vec<&EngineEvent> {
        let lower = name.to_ascii_lowercase();
        let mut idx: Vec<usize> = self.by_name.get(&lower).cloned().unwrap_or_default();
        if !self.by_hash.is_empty() {
            let mut hashes: Vec<u64> = self.families.iter().map(|f| f.hash(&lower)).collect();
            hashes.extend(hashed_event(&lower));
            for h in hashes {
                if let Some(v) = self.by_hash.get(&h) {
                    idx.extend(v);
                }
            }
        }
        idx.sort_unstable();
        idx.dedup();
        idx.into_iter().map(|i| &self.events[i]).collect()
    }

    pub fn contains(&self, name: &str) -> bool {
        !self.lookup(name).is_empty()
    }

    /// Named events, for suggestions.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.by_name.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

// ---------------------------------------------------------------------------------------------
// The workspace graph.

/// A site: its file and its index in the file's `facts.events`.
pub type SiteId = (FileId, u32);

/// The global event tables, patched per file.
#[derive(Debug, Default)]
pub(crate) struct EventTables {
    /// Literal sites by lower-case name, sorted by file then index.
    by_name: HashMap<String, Vec<SiteId>>,
    /// Pattern sites (`"trigger_" + n`), sorted.
    patterns: Vec<SiteId>,
    /// Emits whose name is unknown (`self notify( msg )`), per file: the largest payload seen.
    dynamic_emits: BTreeMap<FileId, usize>,
    /// Event-like string literals used elsewhere than as an event name: how many files hold
    /// each (lower case).
    strings: HashMap<String, u32>,
}

impl EventTables {
    pub(crate) fn index(&mut self, id: FileId, facts: &crate::FileFacts) {
        let mut dynamic: Option<usize> = None;
        for (i, s) in facts.events.iter().enumerate() {
            let site = (id, i as u32);
            match &s.name {
                EventName::Literal(n) => {
                    let v = self.by_name.entry(n.to_ascii_lowercase()).or_default();
                    let at = v.partition_point(|x| *x <= site);
                    v.insert(at, site);
                }
                EventName::Pattern(_) => {
                    let at = self.patterns.partition_point(|x| *x <= site);
                    self.patterns.insert(at, site);
                }
                EventName::Dynamic if s.class == EventClass::Emit => {
                    let argc = if s.is_keyword() { s.argc } else { usize::MAX };
                    dynamic = Some(dynamic.map_or(argc, |d| d.max(argc)));
                }
                EventName::Dynamic => {}
            }
        }
        if let Some(d) = dynamic {
            self.dynamic_emits.insert(id, d);
        }
        for s in &facts.strings {
            *self.strings.entry(s.clone()).or_default() += 1;
        }
    }

    pub(crate) fn unindex(&mut self, id: FileId, facts: &crate::FileFacts) {
        for s in &facts.events {
            if let EventName::Literal(n) = &s.name {
                let k = n.to_ascii_lowercase();
                if let Some(v) = self.by_name.get_mut(&k) {
                    v.retain(|x| x.0 != id);
                    if v.is_empty() {
                        self.by_name.remove(&k);
                    }
                }
            }
        }
        self.patterns.retain(|x| x.0 != id);
        self.dynamic_emits.remove(&id);
        for s in &facts.strings {
            if let Some(n) = self.strings.get_mut(s) {
                *n -= 1;
                if *n == 0 {
                    self.strings.remove(s);
                }
            }
        }
    }
}

/// What the scripts send with an event, over every notifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Payload {
    /// No script notifies it.
    None,
    /// The largest payload of the script notifiers (all with known argument counts).
    AtMost(usize),
    /// Some notifier's payload is unknown (a wrapper such as `delay_notify`, or a pattern
    /// such as `"done_" + n` that may be another event).
    Unknown,
}

/// How a function is started, over every resolved reference to it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Starts {
    /// `ent thread f()` on an object that is neither `level` nor `self` (`player`, `ent`,
    /// `self.owner`...).
    pub thread_on_entity: u32,
    /// The part of `thread_on_entity` on an expression naming a player (`player thread f()`,
    /// `players[ i ] thread f()`).
    pub thread_on_player: u32,
    /// `thread f()` or `self thread f()`: on whatever the caller's `self` is.
    pub thread_on_self: u32,
    /// `level thread f()` (and `game`, `anim`).
    pub thread_on_level: u32,
    /// `childthread f()`: inherits the `endon`s of the thread that starts it (BO4 and Cold War,
    /// measured; it does not end with that thread).
    pub childthread: u32,
    /// Synchronous calls: the caller's thread runs the body, with the caller's `endon`s.
    pub sync: u32,
    /// Function pointers (`::f`): started in ways the call sites do not show.
    pub pointer: u32,
}

/// [`Starts`] of every referenced function.
pub type StartIndex = HashMap<DefId, Starts>;

/// Receiver classes, for matching a wait with its notifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Level,
    Entity,
    /// The implicit or explicit `self`: either.
    Unknown,
}

impl Side {
    /// The side of a [`receiver_kind`].
    pub fn of(receiver: &str) -> Side {
        match receiver {
            "level" | "game" | "anim" => Side::Level,
            "self" | "-" => Side::Unknown,
            _ => Side::Entity,
        }
    }

    pub fn compatible(self, other: Side) -> bool {
        self == Side::Unknown || other == Side::Unknown || self == other
    }

    fn bit(self) -> u8 {
        match self {
            Side::Level => 1,
            Side::Entity => 2,
            Side::Unknown => 4,
        }
    }
}

fn vm_bit(vm: Option<crate::Vm>) -> u8 {
    match vm {
        Some(crate::Vm::Gsc) => 1,
        Some(crate::Vm::Csc) => 2,
        None => 4,
    }
}

/// Who notifies one literal event, over every script.
#[derive(Clone, Copy, Debug, Default)]
pub struct Emitted {
    sides: u8,
    vms: u8,
}

impl Emitted {
    /// Some notifier is on a receiver compatible with `side`, in a VM compatible with `vm`.
    pub fn reaches(&self, side: Side, vm: Option<crate::Vm>) -> bool {
        let side_ok = [Side::Level, Side::Entity, Side::Unknown]
            .iter()
            .any(|s| self.sides & s.bit() != 0 && s.compatible(side));
        let vm_ok = vm.is_none() || self.vms & (vm_bit(vm) | 4) != 0;
        side_ok && vm_ok
    }
}

/// What the rules ask of the whole graph, indexed once per workspace state.
#[derive(Debug, Default)]
pub struct EventIndex {
    /// Literal notified names (lower case).
    pub emitted: HashMap<String, Emitted>,
    /// The same names by length, for edit-distance scans.
    by_len: BTreeMap<usize, Vec<String>>,
    /// Notified patterns (lower case, unique).
    patterns: Vec<String>,
    /// Files holding a wait or an `endon` per literal name.
    waiting_files: HashMap<String, u32>,
}

impl EventIndex {
    /// Notified names whose length is within `d` of `len`.
    pub fn emitted_near_len(&self, len: usize, d: usize) -> impl Iterator<Item = &str> {
        self.by_len
            .range(len.saturating_sub(d)..=len + d)
            .flat_map(|(_, v)| v.iter().map(String::as_str))
    }

    /// Does a notify name `lower`, literally or through a pattern?
    pub fn emitted(&self, lower: &str) -> bool {
        self.emitted.contains_key(lower) || self.patterns.iter().any(|p| pattern_matches(p, lower))
    }

    /// How many files wait for (or end on) `lower`.
    pub fn waiting_files(&self, lower: &str) -> u32 {
        self.waiting_files.get(lower).copied().unwrap_or(0)
    }
}

/// The derived indexes, built on first use.
#[derive(Debug, Default)]
pub(crate) struct Caches {
    starts: std::sync::OnceLock<std::sync::Arc<StartIndex>>,
    index: std::sync::OnceLock<std::sync::Arc<EventIndex>>,
}

impl Workspace {
    /// How the function `d` is started (all zero when nothing references it). The index over
    /// every file is built on first use after a change.
    pub fn thread_starts(&self, d: DefId) -> Starts {
        let index = self.event_cache.starts.get_or_init(|| {
            let files: Vec<FileId> = self.files().collect();
            let per_file = par_map(&files, self.jobs(), |&f| {
                let res = self.resolution(f);
                let refs = &self.file(f).facts.refs;
                res.refs
                    .iter()
                    .zip(refs)
                    .filter_map(|(r, x)| Some((r.def?, x.kind, x.receiver)))
                    .collect::<Vec<_>>()
            });
            let mut m = StartIndex::new();
            for (d, kind, receiver) in per_file.into_iter().flatten() {
                let st = m.entry(d).or_default();
                let n = match (kind, receiver) {
                    ("thread", "level" | "game" | "anim") => &mut st.thread_on_level,
                    ("thread", "self" | "-") => &mut st.thread_on_self,
                    ("thread", "player") => {
                        st.thread_on_entity += 1;
                        &mut st.thread_on_player
                    }
                    ("thread", _) => &mut st.thread_on_entity,
                    ("childthread", _) => &mut st.childthread,
                    ("pointer", _) => &mut st.pointer,
                    _ => &mut st.sync,
                };
                *n += 1;
            }
            std::sync::Arc::new(m)
        });
        index.get(&d).copied().unwrap_or_default()
    }

    /// The derived event index (built on first use after a change; O(sites)).
    pub fn event_index(&self) -> &EventIndex {
        self.event_cache.index.get_or_init(|| {
            let t = &self.tables.events;
            let mut ix = EventIndex::default();
            for (name, sites) in &t.by_name {
                let mut files: Vec<FileId> = Vec::new();
                for &s in sites {
                    let site = self.event_site(s);
                    if site.class == EventClass::Emit {
                        let e = ix.emitted.entry(name.clone()).or_default();
                        e.sides |= Side::of(site.receiver).bit();
                        e.vms |= vm_bit(self.file(s.0).meta.vm);
                    } else if files.last() != Some(&s.0) {
                        files.push(s.0);
                    }
                }
                if !files.is_empty() {
                    ix.waiting_files.insert(name.clone(), files.len() as u32);
                }
            }
            for name in ix.emitted.keys() {
                ix.by_len.entry(name.len()).or_default().push(name.clone());
            }
            for v in ix.by_len.values_mut() {
                v.sort();
            }
            for &s in &t.patterns {
                let site = self.event_site(s);
                if site.class == EventClass::Emit {
                    ix.patterns.push(site.name.render().to_ascii_lowercase());
                }
            }
            ix.patterns.sort();
            ix.patterns.dedup();
            std::sync::Arc::new(ix)
        })
    }

    /// A site.
    pub fn event_site(&self, (f, i): SiteId) -> &EventSite {
        &self.file(f).facts.events[i as usize]
    }

    /// The literal sites named `name` (any case), every class, sorted by file.
    pub fn event_sites(&self, name: &str) -> &[SiteId] {
        self.tables
            .events
            .by_name
            .get(&name.to_ascii_lowercase())
            .map_or(&[], Vec::as_slice)
    }

    /// Pattern sites (`"trigger_" + n`) that may name `name`, every class.
    pub fn event_pattern_sites(&self, name: &str) -> Vec<SiteId> {
        let lower = name.to_ascii_lowercase();
        self.tables
            .events
            .patterns
            .iter()
            .copied()
            .filter(|&s| pattern_matches(self.event_site(s).name.render(), &lower))
            .collect()
    }

    /// The script sites that notify `name`: literal ones, then patterns that may.
    pub fn event_emitters(&self, name: &str) -> Vec<SiteId> {
        let mut v: Vec<SiteId> = self
            .event_sites(name)
            .iter()
            .copied()
            .filter(|&s| self.event_site(s).class == EventClass::Emit)
            .collect();
        v.extend(
            self.event_pattern_sites(name)
                .into_iter()
                .filter(|&s| self.event_site(s).class == EventClass::Emit),
        );
        v
    }

    /// Does a script notify `name` (literally or through a pattern)?
    pub fn event_emitted(&self, name: &str) -> bool {
        self.event_sites(name)
            .iter()
            .any(|&s| self.event_site(s).class == EventClass::Emit)
            || self.tables.events.patterns.iter().any(|&s| {
                let site = self.event_site(s);
                site.class == EventClass::Emit
                    && pattern_matches(site.name.render(), &name.to_ascii_lowercase())
            })
    }

    /// Is `name` written as a string literal somewhere other than as an event name (passed to a
    /// helper, stored in a field)? It may then reach a `notify( var )`.
    pub fn event_mentioned(&self, name: &str) -> bool {
        self.tables
            .events
            .strings
            .contains_key(&name.to_ascii_lowercase())
    }

    /// The largest payload of a notify whose name is unknown (`self notify( msg, a )`), if any:
    /// such a notify may send any event. `usize::MAX` when a wrapper does.
    pub fn dynamic_emit_payload(&self) -> Option<usize> {
        self.tables.events.dynamic_emits.values().copied().max()
    }

    /// The fields the scripts' notifies of `name` send, in games whose `waittill` returns the
    /// payload struct: `None` when no script notifies it, or one notifier is a wrapper, a
    /// pattern or a `notify` whose payload is not a struct literal.
    pub fn event_payload_fields(&self, name: &str) -> Option<BTreeSet<String>> {
        let emitters = self.event_emitters(name);
        if emitters.is_empty() {
            return None;
        }
        let mut out = BTreeSet::new();
        for s in emitters {
            let site = self.event_site(s);
            if !site.is_keyword() || !matches!(site.name, EventName::Literal(_)) {
                return None;
            }
            out.extend(site.fields.as_ref()?.iter().cloned());
        }
        Some(out)
    }

    /// The fields a notify whose name is unknown (`self notify( msg, { #a: 1 } )`) may send:
    /// `None` when one sends another payload (or a wrapper does).
    pub fn dynamic_emit_fields(&self) -> Option<BTreeSet<String>> {
        let mut out = BTreeSet::new();
        for &f in self.tables.events.dynamic_emits.keys() {
            for s in &self.file(f).facts.events {
                if s.class == EventClass::Emit && s.name == EventName::Dynamic {
                    if !s.is_keyword() {
                        return None;
                    }
                    out.extend(s.fields.clone()?);
                }
            }
        }
        Some(out)
    }

    /// What the scripts send with `name`.
    pub fn event_payload(&self, name: &str) -> Payload {
        let mut max: Option<usize> = None;
        for s in self.event_emitters(name) {
            let site = self.event_site(s);
            // A wrapper's payload is unknown; a pattern may name another event.
            if !site.is_keyword() || !matches!(site.name, EventName::Literal(_)) {
                return Payload::Unknown;
            }
            max = Some(max.map_or(site.argc, |m| m.max(site.argc)));
        }
        max.map_or(Payload::None, Payload::AtMost)
    }

    /// Lower-case names some script notifies literally.
    pub fn emitted_event_names(&self) -> impl Iterator<Item = &str> {
        self.tables
            .events
            .by_name
            .iter()
            .filter(|(_, v)| {
                v.iter()
                    .any(|&s| self.event_site(s).class == EventClass::Emit)
            })
            .map(|(k, _)| k.as_str())
    }

    /// The site whose name argument holds `offset` in `file`.
    pub fn event_at(&self, file: FileId, offset: TextSize) -> Option<u32> {
        self.file(file)
            .facts
            .events
            .iter()
            .position(|s| s.name_range.contains_inclusive(offset))
            .map(|i| i as u32)
    }

    /// The event whose literal name holds `offset`, as a symbol.
    pub(crate) fn event_symbol_at(&self, file: FileId, offset: TextSize) -> Option<Symbol> {
        let i = self.event_at(file, offset)?;
        self.event_site((file, i)).key().map(Symbol::Event)
    }

    fn site_nav(&self, (f, i): SiteId) -> NavTarget {
        let s = self.event_site((f, i));
        NavTarget {
            file: f,
            full_range: s.name_range,
            focus_range: s.name_range,
            name: format!("{} {}", s.op, s.name.render()),
            kind: SymbolKind::Event,
            container: Some(self.file(f).facts.caller_name(s.caller)),
        }
    }

    /// Every literal site of `name` and every pattern site that may name it.
    fn all_event_sites(&self, name: &str) -> Vec<SiteId> {
        let mut v: Vec<SiteId> = self.event_sites(name).to_vec();
        v.extend(self.event_pattern_sites(name));
        v.sort();
        v
    }

    /// Go to definition on an event name: from a wait or an `endon`, its notifiers; from a
    /// notify, its waiters and `endon`s. Every other site when the other side is empty.
    pub(crate) fn event_definition(
        &self,
        file: FileId,
        offset: TextSize,
        name: &str,
    ) -> Vec<NavTarget> {
        let here = self.event_at(file, offset).map(|i| (file, i));
        let emit_here = here.is_some_and(|s| self.event_site(s).class == EventClass::Emit);
        let sites = self.all_event_sites(name);
        let other: Vec<SiteId> = sites
            .iter()
            .copied()
            .filter(|&s| (self.event_site(s).class == EventClass::Emit) != emit_here)
            .collect();
        let pick = if other.is_empty() { sites } else { other };
        pick.into_iter()
            .filter(|&s| Some(s) != here)
            .map(|s| self.site_nav(s))
            .collect()
    }

    /// Every site of an event (its name argument).
    pub(crate) fn event_references(&self, name: &str) -> Vec<FileRange> {
        self.all_event_sites(name)
            .into_iter()
            .map(|s| FileRange {
                file: s.0,
                range: self.event_site(s).name_range,
            })
            .collect()
    }

    /// Hover on an event name: who notifies it, with what, who waits, what the engine does.
    pub(crate) fn event_hover(&self, file: FileId, offset: TextSize) -> Option<Hover> {
        let i = self.event_at(file, offset)?;
        let site = self.event_site((file, i));
        let name = site.key()?;
        let mut emit: BTreeMap<&str, usize> = BTreeMap::new();
        let (mut waits, mut endons) = (0, 0);
        for s in self.all_event_sites(&name) {
            let x = self.event_site(s);
            match x.class {
                EventClass::Emit => *emit.entry(x.receiver).or_default() += 1,
                EventClass::Wait => waits += 1,
                EventClass::Endon => endons += 1,
            }
        }
        let n_emit: usize = emit.values().sum();
        let mut detail = Vec::new();
        if n_emit == 0 {
            detail.push("notified by no script".to_owned());
        } else {
            let by: Vec<String> = emit.iter().map(|(r, n)| format!("{r} ×{n}")).collect();
            let payload = match self.event_payload(&name) {
                Payload::AtMost(0) => ", no value".to_owned(),
                Payload::AtMost(1) => ", up to 1 value".to_owned(),
                Payload::AtMost(n) => format!(", up to {n} values"),
                Payload::Unknown | Payload::None => String::new(),
            };
            detail.push(format!(
                "notified at {n_emit} site{} ({}){payload}",
                if n_emit == 1 { "" } else { "s" },
                by.join(", ")
            ));
        }
        detail.push(format!("waited at {waits}, `endon` at {endons}"));
        match self.profile_of(file).config.notify_inline {
            Some(true) => detail.push(
                "a notify runs the woken waiters inside the call, each up to its next wait, \
                 before the notifier's next statement"
                    .into(),
            ),
            Some(false) => detail.push(
                "a notify queues the woken waiters: they run once the notifier waits or ends"
                    .into(),
            ),
            None => {}
        }
        if self.event_mentioned(&name) {
            detail
                .push("also written as a string elsewhere (it may reach a `notify( var )`)".into());
        }
        match self.profile_of(file).events.as_ref() {
            Some(c) => {
                let e = c.lookup(&name);
                if e.is_empty() {
                    detail.push(format!("not in the engine catalogue ({})", c.game));
                } else {
                    for x in e {
                        detail.push(format!(
                            "engine notifies it on {} with {}{} value{}",
                            x.receiver.label(),
                            if x.args_variable { "up to " } else { "" },
                            x.args,
                            if x.args == 1 { "" } else { "s" }
                        ));
                    }
                }
            }
            None => detail.push("no engine event catalogue for this game".to_owned()),
        }
        Some(Hover {
            range: site.name_range,
            signature: format!("event \"{}\"", site.name.render()),
            doc: None,
            detail,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns() {
        assert!(pattern_matches("trigger_*", "trigger_on"));
        assert!(pattern_matches("*_done", "move_done"));
        assert!(pattern_matches("a*b*c", "axxbyyc"));
        assert!(!pattern_matches("trigger_*", "triggered"));
        assert!(pattern_matches("Exact", "exact"));
    }

    fn ws(files: &[(&str, &str)]) -> Workspace {
        let p = crate::Profile::new(gsc_syntax::Dialect::superset(), None, None);
        let mut ws = Workspace::new(p).with_jobs(1);
        let root = ws.add_root(crate::Root::new("."));
        for (path, text) in files {
            ws.add_file(root, path.into(), path, text);
        }
        ws
    }

    #[test]
    fn navigation_and_incremental_tables() {
        let a = "function f() { level notify( \"go\", 1 ); self thread g( \"go_msg\" ); }\n";
        let b = "function w() { level waittill( \"go\", x ); self endon( \"GO\" ); }\n";
        let mut ws = ws(&[("a.gsc", a), ("b.gsc", b)]);
        let (fa, fb) = (FileId(0), FileId(1));
        let at = |t: &str, pat: &str| TextSize::from((t.find(pat).unwrap() + 1) as u32);
        // From the waittill string to the notify, and back.
        let defs = ws.definition(fb, at(b, "\"go\""));
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].file, fa);
        assert_eq!(&a[defs[0].focus_range], "\"go\"");
        let back = ws.definition(fa, at(a, "\"go\""));
        assert_eq!(back.len(), 2, "the waittill and the endon (any case)");
        let sym = ws.symbol_at(fb, at(b, "\"GO\"")).unwrap();
        assert_eq!(sym, Symbol::Event("go".into()));
        assert_eq!(ws.references(&sym, true).len(), 3);
        let h = ws.hover(fb, at(b, "\"go\"")).unwrap();
        assert!(h.detail[0].contains("notified at 1 site (level ×1), up to 1 value"));
        assert!(ws.event_mentioned("go_msg"));
        // Editing a file patches the tables.
        ws.set_text(fa, "function f() { }\n");
        assert!(!ws.event_emitted("go"));
        assert!(!ws.event_mentioned("go_msg"));
        assert_eq!(ws.event_sites("go").len(), 2);
    }

    #[test]
    fn hashed() {
        assert_eq!(hashed_event("hash_1a2b"), Some(0x1a2b));
        assert_eq!(hashed_event("_id_FF"), Some(0xff));
        assert_eq!(hashed_event("hash_name"), None);
    }

    #[test]
    fn catalogue() {
        let c = EventCatalogue::from_toml_str(
            r#"
[meta]
game = "t7"
[coverage]
complete = true
[[event]]
name = "Death"
receiver = "entity"
args = [{ type = "entity" }]
[[event]]
name = "damage"
receiver = "entity"
args = [{ type = "int" }, { type = "entity" }]
args_variable = true
"#,
        )
        .unwrap();
        assert!(c.complete);
        assert_eq!(c.game, "t7");
        assert!(c.contains("DEATH"));
        assert_eq!(c.lookup("death")[0].args, 1);
        assert!(c.lookup("damage")[0].args_variable);
        assert!(!c.contains("nope"));
        let partial = EventCatalogue::from_toml_str("[[event]]\nname = \"x\"\n").unwrap();
        assert!(!partial.complete);
    }
}
