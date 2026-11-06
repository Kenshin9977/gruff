//! Calls, events and field accesses that come from macro expansions, indexed at the use.
//!
//! The tree holds a macro use as written (`IS_ON( self.ready )`); what the `#define` body does
//! only shows up in the expansion ([`gsc_syntax::macro_expansions_with`]). Each expansion is
//! parsed on its own and walked like a file; what it yields is reported at the line of the use,
//! in the function around the use, with `via_macro` naming the macro. Facts the use site already
//! shows (a macro argument such as `self.ready`, parsed in place) are not repeated: each one
//! cancels one identical fact of the expansion.

use gsc_sema::facts::{self, LineStarts, Scope, Target};
use gsc_sema::{FileId, Workspace};
use gsc_syntax::{header_paths, macro_expansions_with, HeaderSet, MacroExpansion, TextRange};

use crate::extract::{Event, Field};

/// A call or function reference inside a macro expansion.
#[derive(Clone, Debug)]
pub struct MacroCall {
    /// `ns::fn` of the function around the use (or defined by a file-scope expansion), else
    /// `<file>`.
    pub caller: String,
    pub callee: String,
    pub resolution: &'static str,
    pub kind: &'static str,
    pub receiver: &'static str,
    /// Line of the macro use.
    pub line: u32,
    /// The macro used, as written at the use.
    pub via: String,
}

/// What a file's macro uses add to the index.
#[derive(Debug, Default)]
pub struct Expanded {
    pub calls: Vec<MacroCall>,
    pub events: Vec<Event>,
    pub fields: Vec<Field>,
}

/// The workspace's `.gsh` headers, for the macros a file sees through `#insert`/`#inline`.
pub fn headers(ws: &Workspace) -> HeaderSet {
    let mut set = HeaderSet::new();
    for f in ws.files() {
        let data = ws.file(f);
        if data.meta.vm.is_none() {
            let d = ws.profile_of(f).dialect(data.decompiled);
            set.add(&data.meta.rel, &data.syntax(), d);
        }
    }
    set
}

/// Remove one element equal to `x` from `pool`; was there one?
fn take<T: PartialEq>(pool: &mut Vec<T>, x: &T) -> bool {
    match pool.iter().position(|p| p == x) {
        Some(i) => {
            pool.swap_remove(i);
            true
        }
        None => false,
    }
}

/// The calls, events and fields the macro uses of file `id` expand to. `direct_events` and
/// `direct_fields` are what the file's own tree yields (see [`crate::extract::extract`]).
pub fn expand(
    ws: &Workspace,
    id: FileId,
    headers: &HeaderSet,
    lines: &LineStarts,
    direct_events: &[Event],
    direct_fields: &[Field],
) -> Expanded {
    let data = ws.file(id);
    let root = data.syntax();
    let profile = ws.profile_of(id);
    let dialect = profile.dialect(data.decompiled);
    let cfg = profile.config;
    let table = headers.macros_for(&header_paths(&root));
    let f = &data.facts;
    let own = f.defs.iter().any(|d| d.kind == gsc_sema::DefKind::Define);
    let mut out = Expanded::default();
    if table.is_empty() && !own {
        return out;
    }
    let key = &data.meta.key;
    // The namespace an expansion's own walk assumes (the file key's default); file-scope
    // expansions are renamed to the file's namespace.
    let default_ns = Scope::new(&cfg, key).ns;
    for e in macro_expansions_with(&root, &table, dialect) {
        expand_one(
            ws,
            id,
            &e,
            &default_ns,
            lines,
            direct_events,
            direct_fields,
            &mut out,
        );
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn expand_one(
    ws: &Workspace,
    id: FileId,
    e: &MacroExpansion,
    default_ns: &str,
    lines: &LineStarts,
    direct_events: &[Event],
    direct_fields: &[Field],
    out: &mut Expanded,
) {
    let data = ws.file(id);
    let f = &data.facts;
    let profile = ws.profile_of(id);
    let cfg = profile.config;
    let parsed = e.parse(profile.dialect(data.decompiled));
    let ex_root = parsed.syntax();
    let use_range: TextRange = e.use_range;
    let line = lines.line(use_range.start());
    // The function around the use (innermost, callable kinds only).
    let around = (!e.at_file_scope)
        .then(|| {
            f.defs
                .iter()
                .enumerate()
                .filter(|(_, d)| d.kind.is_callable() && d.range.contains_range(use_range))
                .min_by_key(|(_, d)| d.range.len())
                .map(|(i, _)| i as u32)
        })
        .flatten();
    let around_name = f.caller_name(around);
    let rename = |func: String| -> String {
        if !e.at_file_scope {
            return around_name.clone();
        }
        match func
            .strip_prefix(default_ns)
            .and_then(|s| s.strip_prefix("::"))
        {
            Some(rest) => format!("{}::{rest}", f.namespace),
            None => func,
        }
    };

    // Calls.
    let mut seen: Vec<&Target> = f
        .refs
        .iter()
        .filter(|r| use_range.contains(r.focus))
        .map(|r| &r.target)
        .collect();
    let sub = facts::extract(&ex_root, &data.meta.key, &cfg);
    for r in &sub.refs {
        if take(&mut seen, &&r.target) {
            continue;
        }
        let (caller, ns, class) = if e.at_file_scope {
            match r.caller.map(|c| &sub.defs[c as usize]) {
                Some(d) => (
                    format!("{}::{}", f.namespace, d.name),
                    f.namespace.as_str(),
                    d.class(),
                ),
                None => ("<file>".to_owned(), f.namespace.as_str(), None),
            }
        } else {
            let (ns, class) = f.scope_of(around);
            (around_name.clone(), ns, class)
        };
        let resolved = ws.resolve_target(id, ns, class, &r.target);
        out.calls.push(MacroCall {
            caller,
            callee: resolved.callee,
            resolution: resolved.how.label(),
            kind: r.kind,
            receiver: r.receiver,
            line,
            via: e.name.clone(),
        });
    }

    // Events and fields.
    let ex_lines = LineStarts::new(&parsed.text());
    let (events, fields) = crate::extract::extract(&ex_root, &data.meta.key, &cfg, &ex_lines, &sub);
    let event_key = |x: &Event| (x.name.render().to_owned(), x.op.clone(), x.receiver);
    let mut seen: Vec<_> = direct_events
        .iter()
        .filter(|x| use_range.contains(x.focus))
        .map(event_key)
        .collect();
    for mut x in events {
        if take(&mut seen, &event_key(&x)) {
            continue;
        }
        x.func = rename(std::mem::take(&mut x.func));
        x.line = line;
        x.focus = use_range.start();
        x.via_macro = Some(e.name.clone());
        out.events.push(x);
    }
    let field_key = |x: &Field| (x.obj, x.name.clone(), x.op);
    let mut seen: Vec<_> = direct_fields
        .iter()
        .filter(|x| use_range.contains(x.focus))
        .map(field_key)
        .collect();
    for mut x in fields {
        if take(&mut seen, &field_key(&x)) {
            continue;
        }
        x.func = rename(std::mem::take(&mut x.func));
        x.line = line;
        x.focus = use_range.start();
        x.via_macro = Some(e.name.clone());
        out.fields.push(x);
    }
}
