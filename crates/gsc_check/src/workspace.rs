//! What the rules need beyond the [`gsc_sema`] workspace: per-function "may yield" summaries
//! (what each function and macro calls and whether it waits) with their fixpoint over resolved
//! calls, and the field names the scripts set.

use std::collections::{HashMap, HashSet};

use gsc_sema::facts::{def_node, DefKind, FileFacts};
use gsc_sema::{par_map, DefId, FileId, How, Workspace};
use gsc_syntax::ast::{self, AstNode};
use gsc_syntax::{Dialect, SyntaxKind, SyntaxKind::*, SyntaxNode, TextRange};

/// What a call names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Callee {
    /// `foo()`: original spelling and range of the name.
    Bare(String, TextRange),
    /// `ns::foo()`: lower-case namespace and name.
    Qualified(String, String, TextRange),
    /// `::foo()`: a function of this file.
    Local(String, TextRange),
    /// `isdefined()`, `notify()`, `waittill()`...: a keyword callee.
    Keyword(SyntaxKind),
    /// Anything else (recovered trees).
    Other,
}

/// The callee of a call expression.
pub fn callee(call: &ast::CallExpr) -> Callee {
    match call.callee() {
        Some(ast::Expr::NameRef(n)) => {
            let Some(tok) = n.syntax().first_token() else {
                return Callee::Other;
            };
            if tok.kind() == IDENT {
                Callee::Bare(tok.text().to_owned(), tok.text_range())
            } else if tok.kind().is_keyword() {
                Callee::Keyword(tok.kind())
            } else {
                Callee::Other
            }
        }
        Some(ast::Expr::QualifiedRef(q)) => {
            let Some(name) = q.name_ref() else {
                return Callee::Other;
            };
            let range = name.syntax().text_range();
            let text = name.text().to_ascii_lowercase();
            match q.qualifier() {
                Some(qual) => Callee::Qualified(
                    qual.syntax().text().to_string().trim().to_ascii_lowercase(),
                    text,
                    range,
                ),
                None => Callee::Local(text, range),
            }
        }
        _ => Callee::Other,
    }
}

/// Is the call started as a new thread (`thread`, `childthread`, `threadendon`)?
pub fn is_threaded(modifier: Option<gsc_syntax::SyntaxToken>) -> bool {
    modifier.is_some_and(|m| matches!(m.kind(), THREAD_KW | CHILDTHREAD_KW | THREADENDON_KW))
}

/// A decompiler's spelling of a hashed name (`function_1a2b3c4d`, `_id_1A2B`, `@name`...).
pub fn is_hashed_name(name: &str) -> bool {
    if name.starts_with('@') || name.starts_with('#') {
        return true;
    }
    const PREFIXES: &[&str] = &[
        "function_",
        "namespace_",
        "script_",
        "var_",
        "_id_",
        "func_",
        "sub_",
        "hash_",
    ];
    PREFIXES.iter().any(|p| {
        name.len() > p.len()
            && name[..p.len()].eq_ignore_ascii_case(p)
            && name[p.len()..].bytes().all(|b| b.is_ascii_hexdigit())
    })
}

/// Upper-case names (`WAIT_SERVER_FRAME`, `IS_TRUE`) are macros by convention.
pub fn looks_like_macro(name: &str) -> bool {
    name.bytes().any(|b| b.is_ascii_uppercase()) && !name.bytes().any(|b| b.is_ascii_lowercase())
}

/// Names whose call waits, whatever the game spells them as. The later games compile `waittill`
/// and its family to opcodes instead of parsing them as keywords (the catalogues' `[[intrinsic]]`
/// entries), and a decompiler writes them back as ordinary calls: `self waittill(#"hash_...")`
/// reaches [`summarize`] as a bare callee, not as `WAITTILL_KW`. Left out of this list, every
/// `while (true)` whose only wait is a `waittill` call looked like an infinite loop (L001: 510
/// reports on the Black Ops 4 corpus, 486 on Cold War, 435 on Modern Warfare III, against 5 on
/// Black Ops III, which has the keyword).
const WAIT_WORDS: &[&str] = &[
    "wait",
    "waitframe",
    "waittillframeend",
    "waitrealtime",
    "waittill",
    "waittillmatch",
    "waittilltimeout",
];

/// A sync call target, for the yield fixpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Original spelling.
    Bare(String),
    Qualified(String, String),
    Local(String),
    /// A bare name used as a statement (`WAIT_SERVER_FRAME;`): a macro if anything.
    Macro(String),
}

/// What a piece of code does about yielding.
#[derive(Clone, Debug, Default)]
pub struct Yields {
    /// It waits itself (or calls something unknowable, like a pointer).
    pub direct: bool,
    /// Sync calls whose summary decides.
    pub calls: Vec<Target>,
}

/// Summarise the code under `node`.
pub fn summarize(node: &SyntaxNode) -> Yields {
    let mut y = Yields::default();
    for n in node.descendants() {
        match n.kind() {
            WAIT_STMT | WAIT_REAL_TIME_STMT | WAIT_FRAME_STMT | WAITTILL_FRAME_END_STMT => {
                y.direct = true
            }
            CALL_EXPR => {
                let Some(call) = ast::CallExpr::cast(n) else {
                    continue;
                };
                if is_threaded(call.modifier()) {
                    continue;
                }
                match callee(&call) {
                    Callee::Keyword(WAITTILL_KW | WAITTILLMATCH_KW) => y.direct = true,
                    Callee::Keyword(_) => {}
                    Callee::Bare(name, _) => {
                        if WAIT_WORDS.iter().any(|w| w.eq_ignore_ascii_case(&name)) {
                            y.direct = true;
                        } else {
                            y.calls.push(Target::Bare(name));
                        }
                    }
                    Callee::Qualified(ns, name, _) => y.calls.push(Target::Qualified(ns, name)),
                    Callee::Local(name, _) => y.calls.push(Target::Local(name)),
                    Callee::Other => y.direct = true,
                }
            }
            POINTER_CALL_EXPR => {
                let threaded =
                    ast::PointerCallExpr::cast(n).is_some_and(|c| is_threaded(c.modifier()));
                y.direct |= !threaded;
            }
            METHOD_CALL_EXPR => {
                let threaded =
                    ast::MethodCallExpr::cast(n).is_some_and(|c| is_threaded(c.modifier()));
                y.direct |= !threaded;
            }
            EXPR_STMT | MACRO_STMT => {
                if let Some(name) = bare_name_stmt(&n) {
                    y.calls.push(Target::Macro(name));
                }
            }
            _ => {}
        }
    }
    y
}

/// `NAME;` as a statement.
fn bare_name_stmt(stmt: &SyntaxNode) -> Option<String> {
    let e = stmt.first_child()?;
    if e.kind() != NAME_REF {
        return None;
    }
    let t = e.first_token()?;
    (t.kind() == IDENT).then(|| t.text().to_owned())
}

/// Summarise a `#define` body, which the parser keeps as text.
pub fn summarize_macro(body: &str, params: &[String], dialect: &Dialect) -> Yields {
    let mut y = Yields::default();
    if body.contains("[[") {
        y.direct = true;
    }
    let b = body.as_bytes();
    let mut words: Vec<(String, usize)> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'"' {
            i += 1;
            while i < b.len() && b[i] != b'"' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let s = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            words.push((body[s..i].to_owned(), i));
        } else {
            i += 1;
        }
    }
    let next_char = |end: usize| body[end..].trim_start().chars().next();
    let after = |end: usize| body[end..].trim_start();
    let mut skip_next_call = false;
    for (k, (w, end)) in words.iter().enumerate() {
        let lower = w.to_ascii_lowercase();
        if WAIT_WORDS.contains(&lower.as_str()) || lower == "waittill" || lower == "waittillmatch" {
            y.direct = true;
            continue;
        }
        if matches!(lower.as_str(), "thread" | "childthread") {
            skip_next_call = true;
            continue;
        }
        if dialect.keyword(&lower).is_some() {
            continue;
        }
        if after(*end).starts_with("::") {
            continue; // the namespace of the next word
        }
        let qualified = k > 0 && body[..end - w.len()].trim_end().ends_with("::");
        if next_char(*end) == Some('(') {
            if std::mem::take(&mut skip_next_call) {
                continue;
            }
            if params.iter().any(|p| p == w) {
                y.direct = true; // a callee passed in: unknown
            } else if qualified {
                let ns_end = body[..end - w.len()].trim_end().len() - 2;
                let ns = body[..ns_end]
                    .trim_end()
                    .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                y.calls.push(Target::Qualified(ns, lower));
            } else {
                y.calls.push(Target::Bare(w.clone()));
            }
        } else if looks_like_macro(w) && !params.iter().any(|p| p == w) {
            y.calls.push(Target::Macro(w.clone()));
        }
    }
    y
}

/// The lower-case field name of a `FIELD_EXPR` (`None` for `obj.( expr )`).
pub fn field_name(n: &SyntaxNode) -> Option<String> {
    let fe = ast::FieldExpr::cast(n.clone())?;
    let tok = fe.field()?.syntax().first_token()?;
    (tok.kind() == IDENT).then(|| tok.text().to_ascii_lowercase())
}

/// Is this field expression assigned (`=`, `+=`..., `++`, `--`, a `foreach` binding), directly
/// or through an index (`x.f[ i ] = v`)?
pub fn is_field_write(n: &SyntaxNode) -> bool {
    let mut cur = n.clone();
    loop {
        let Some(p) = cur.parent() else { return false };
        match p.kind() {
            ASSIGN_EXPR => return p.first_child().as_ref() == Some(&cur),
            PREFIX_EXPR | POSTFIX_EXPR => {
                return p
                    .children_with_tokens()
                    .filter_map(|e| e.into_token())
                    .any(|t| matches!(t.kind(), PLUS2 | MINUS2))
            }
            FOREACH_BINDINGS => return true,
            INDEX_EXPR if p.first_child().as_ref() == Some(&cur) => cur = p,
            _ => return false,
        }
    }
}

/// Is this field expression the argument of `isdefined( x.f )` or of an upper-case macro
/// (`IS_TRUE( x.f )`)? Such reads say the field is optional, set or not by data outside the
/// scripts, so they count as "known".
pub fn is_guarded(n: &SyntaxNode) -> bool {
    let Some(args) = n.parent().filter(|p| p.kind() == ARG_LIST) else {
        return false;
    };
    let Some(call) = args.parent().and_then(ast::CallExpr::cast) else {
        return false;
    };
    match callee(&call) {
        Callee::Keyword(ISDEFINED_KW) => true,
        Callee::Bare(name, _) => looks_like_macro(&name),
        _ => false,
    }
}

/// What the first pass keeps of a file beside the workspace's facts.
#[derive(Clone, Debug, Default)]
pub struct CheckFacts {
    /// `(definition index, summary)` of every function and method.
    pub functions: Vec<(u32, Yields)>,
    /// `(definition index, summary)` of every `#define`.
    pub macros: Vec<(u32, Yields)>,
    /// Lower-case field names this file assigns (`x.f = ...`, `x.f++`, `{ f: ... }`) or tests
    /// with `isdefined( x.f )`.
    pub fields_written: Vec<String>,
}

/// Collect the check facts of one parsed file.
pub fn check_facts(root: &SyntaxNode, facts: &FileFacts, dialect: &Dialect) -> CheckFacts {
    let mut c = CheckFacts::default();
    for (i, d) in facts.defs.iter().enumerate() {
        match d.kind {
            DefKind::Function | DefKind::Method => {
                let Some(def) = def_node(root, d).and_then(ast::FunctionDef::cast) else {
                    continue;
                };
                let yields = def
                    .body()
                    .map(|b| summarize(b.syntax()))
                    .unwrap_or_default();
                c.functions.push((i as u32, yields));
            }
            DefKind::Define => {
                let Some(def) = def_node(root, d).and_then(ast::PreprocDefine::cast) else {
                    continue;
                };
                let params: Vec<String> = def
                    .macro_params()
                    .map(|p| {
                        p.syntax()
                            .children_with_tokens()
                            .filter_map(|e| e.into_token())
                            .filter(|t| t.kind() == IDENT)
                            .map(|t| t.text().to_owned())
                            .collect()
                    })
                    .unwrap_or_default();
                let body = def
                    .macro_body_token()
                    .map(|t| t.text().to_owned())
                    .unwrap_or_default();
                c.macros
                    .push((i as u32, summarize_macro(&body, &params, dialect)));
            }
            _ => {}
        }
    }
    for n in root.descendants() {
        match n.kind() {
            FIELD_EXPR => {
                if is_field_write(&n) || is_guarded(&n) {
                    if let Some(name) = field_name(&n) {
                        c.fields_written.push(name);
                    }
                }
            }
            STRUCT_FIELD | ARRAY_ELEMENT => {
                if let Some(key) = n.first_child() {
                    let k = key.text().to_string();
                    let k = k.trim().trim_matches('"').to_ascii_lowercase();
                    if !k.is_empty() {
                        c.fields_written.push(k);
                    }
                }
            }
            _ => {}
        }
    }
    c
}

/// What a call contributes to its caller's summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Edge {
    Known(bool),
    /// Yields when this node does (most calls: no allocation).
    One(usize),
    /// Yields when any of these nodes does.
    Any(Vec<usize>),
}

impl Edge {
    fn nodes(mut v: Vec<usize>) -> Edge {
        if v.len() == 1 {
            Edge::One(v.pop().expect("one node"))
        } else {
            Edge::Any(v)
        }
    }

    /// The nodes the edge depends on.
    pub(crate) fn targets(&self) -> &[usize] {
        match self {
            Edge::Known(_) => &[],
            Edge::One(n) => std::slice::from_ref(n),
            Edge::Any(v) => v,
        }
    }
}

/// The edges of one file's nodes (functions then macros, in [`CheckFacts`] order).
pub(crate) type FileEdges = Vec<Vec<Edge>>;

/// The key of a retired summary node.
pub(crate) const DEAD: DefId = DefId {
    file: FileId(u32::MAX),
    index: u32::MAX,
};

/// "May yield" summaries of every function and macro of a workspace, plus the fields its
/// scripts set.
#[derive(Debug, Default)]
pub struct Summaries {
    node: HashMap<DefId, usize>,
    /// Macro nodes by exact spelling (macros are case-sensitive).
    macros: HashMap<String, Vec<usize>>,
    /// The definition of each node.
    pub(crate) keys: Vec<DefId>,
    /// Whether each node waits itself.
    pub(crate) direct: Vec<bool>,
    pub(crate) yields: Vec<bool>,
    pub(crate) fields_written: HashSet<String>,
}

impl Summaries {
    /// Summarise the workspace from the check facts of its files, then solve the least
    /// fixpoint: a node yields when it waits or a sync callee may yield.
    pub fn build(ws: &Workspace, facts: &[(FileId, CheckFacts)], jobs: usize) -> Summaries {
        let facts: Vec<(FileId, &CheckFacts)> = facts.iter().map(|(f, c)| (*f, c)).collect();
        let mut s = Summaries::number(ws, &facts);
        let edges = par_map(&facts, jobs, |(f, c)| s.file_edges(ws, *f, c));
        s.solve(edges.iter());
        s
    }

    /// Number the nodes of `facts` (file order), with their direct waits and the fields
    /// written; no edges, nothing solved.
    pub(crate) fn number(ws: &Workspace, facts: &[(FileId, &CheckFacts)]) -> Summaries {
        let mut s = Summaries::default();
        for (f, c) in facts {
            s.push_file(ws, *f, c);
            s.fields_written.extend(c.fields_written.iter().cloned());
        }
        s.yields = s.direct.clone();
        s
    }

    /// Append the nodes of file `f`; returns the first one.
    fn push_file(&mut self, ws: &Workspace, f: FileId, c: &CheckFacts) -> usize {
        let start = self.keys.len();
        for (index, y) in &c.functions {
            let def = DefId {
                file: f,
                index: *index,
            };
            self.node.insert(def, self.keys.len());
            self.keys.push(def);
            self.direct.push(y.direct);
        }
        for (index, y) in &c.macros {
            let name = &ws.file(f).facts.defs[*index as usize].name;
            self.macros
                .entry(name.clone())
                .or_default()
                .push(self.keys.len());
            self.keys.push(DefId {
                file: f,
                index: *index,
            });
            self.direct.push(y.direct);
        }
        self.yields.resize(self.keys.len(), false);
        start
    }

    /// Retire the nodes `old` of file `f` (they stay as dead slots: no wait, no edge) and
    /// append its nodes from `c`; returns the first new one. Fields are not touched.
    pub(crate) fn replace_file(
        &mut self,
        ws: &Workspace,
        f: FileId,
        old: std::ops::Range<usize>,
        c: &CheckFacts,
    ) -> usize {
        for i in old.clone() {
            self.node.remove(&self.keys[i]);
            self.keys[i] = DEAD;
            self.direct[i] = false;
            self.yields[i] = false;
        }
        if !old.is_empty() {
            self.macros.retain(|_, v| {
                v.retain(|n| !old.contains(n));
                !v.is_empty()
            });
        }
        let start = self.push_file(ws, f, c);
        for i in start..self.keys.len() {
            self.yields[i] = self.direct[i];
        }
        start
    }

    /// Dead slots, from [`Summaries::replace_file`].
    pub(crate) fn dead(&self) -> usize {
        self.keys.iter().filter(|k| **k == DEAD).count()
    }

    /// The edges of the nodes of file `f`, in node order.
    pub(crate) fn file_edges(&self, ws: &Workspace, f: FileId, c: &CheckFacts) -> FileEdges {
        let ff = &ws.file(f).facts;
        let mut out = Vec::new();
        for (index, y) in &c.functions {
            let def = &ff.defs[*index as usize];
            let (ns, class) = (def.ns.as_str(), def.class());
            out.push(
                y.calls
                    .iter()
                    .map(|t| self.edge(ws, f, ns, class, t))
                    .filter(|e| *e != Edge::Known(false))
                    .collect(),
            );
        }
        for (_, y) in &c.macros {
            let ns = ff.namespace.as_str();
            out.push(
                y.calls
                    .iter()
                    .map(|t| self.edge(ws, f, ns, None, t))
                    .filter(|e| *e != Edge::Known(false))
                    .collect(),
            );
        }
        out
    }

    /// Solve the yields from the direct waits and the edges of every file, in node order.
    pub(crate) fn solve<'a>(&mut self, edges: impl Iterator<Item = &'a FileEdges>) {
        let edges: Vec<&Vec<Edge>> = edges.flatten().collect();
        debug_assert_eq!(edges.len(), self.direct.len());
        self.yields = self.direct.clone();
        loop {
            let mut changed = false;
            for (i, es) in edges.iter().enumerate() {
                if !self.yields[i] && es.iter().any(|e| self.edge_yields(e)) {
                    self.yields[i] = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Solve again the nodes of `cone` only, the others fixed: right when no node outside the
    /// cone has an edge into it (the cone is closed under "is called by"). `edges` per node.
    pub(crate) fn solve_cone(&mut self, edges: &[&Vec<Edge>], cone: &[usize]) {
        for &i in cone {
            self.yields[i] = self.direct[i];
        }
        loop {
            let mut changed = false;
            for &i in cone {
                if !self.yields[i] && edges[i].iter().any(|e| self.edge_yields(e)) {
                    self.yields[i] = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Does node `i` yield?
    pub(crate) fn node_yields(&self, i: usize) -> bool {
        self.yields[i]
    }

    pub(crate) fn edge_yields(&self, e: &Edge) -> bool {
        match e {
            Edge::Known(b) => *b,
            Edge::One(n) => self.yields[*n],
            Edge::Any(nodes) => nodes.iter().any(|&n| self.yields[n]),
        }
    }

    fn nodes(&self, defs: impl IntoIterator<Item = DefId>) -> Vec<usize> {
        defs.into_iter()
            .filter_map(|d| self.node.get(&d).copied())
            .collect()
    }

    /// What a sync call to `t` from file `f` (namespace `ns`, class `class`) contributes.
    /// Unknown callees count as yielding.
    pub(crate) fn edge(
        &self,
        ws: &Workspace,
        f: FileId,
        ns: &str,
        class: Option<&str>,
        t: &Target,
    ) -> Edge {
        let profile = ws.profile_of(f);
        let known = |name: &str| profile.builtins.as_ref().is_some_and(|c| c.is_known(name));
        let reached = |r: gsc_sema::Resolved| match (r.def, r.how) {
            (Some(d), _) if self.node.contains_key(&d) => Some(Edge::One(self.node[&d])),
            (_, How::Builtin) => Some(Edge::Known(false)),
            _ => None,
        };
        match t {
            Target::Macro(name) => match self.macros.get(name) {
                Some(m) => Edge::nodes(m.clone()),
                None => Edge::Known(true),
            },
            Target::Bare(name) => {
                if let Some(m) = self.macros.get(name) {
                    return Edge::nodes(m.clone());
                }
                let ident = profile.config.ident(name);
                let target = gsc_sema::Target::Bare(ident.clone());
                if let Some(e) = reached(ws.resolve_target(f, ns, class, &target)) {
                    return e;
                }
                // Not reachable as far as resolution can tell: any function of that name.
                let any = self.nodes(ws.callables_matching(f, &ident).iter().copied());
                if any.is_empty() {
                    Edge::Known(!known(name))
                } else {
                    Edge::nodes(any)
                }
            }
            Target::Qualified(q, name) => {
                let target = gsc_sema::Target::Qualified(Some(q.clone()), name.clone());
                reached(ws.resolve_target(f, ns, class, &target)).unwrap_or(Edge::Known(true))
            }
            Target::Local(name) => {
                let target = gsc_sema::Target::Qualified(None, name.clone());
                reached(ws.resolve_target(f, ns, class, &target)).unwrap_or(Edge::Known(true))
            }
        }
    }

    /// May code of file `f` with summary `y` yield?
    pub fn yields(&self, ws: &Workspace, f: FileId, y: &Yields) -> bool {
        let ns = &ws.file(f).facts.namespace;
        y.direct
            || y.calls
                .iter()
                .any(|t| self.edge_yields(&self.edge(ws, f, ns, None, t)))
    }

    /// Field names (lower case) some file of the workspace assigns.
    pub fn fields_written(&self) -> &HashSet<String> {
        &self.fields_written
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(files: &[(&str, &str)]) -> Workspace {
        let mut ws =
            Workspace::new(gsc_sema::Profile::new(Dialect::superset(), None, None)).with_jobs(1);
        let root = ws.add_root(gsc_sema::Root::new("."));
        for (path, text) in files {
            ws.add_file(root, path.into(), path, text);
        }
        ws
    }

    fn summaries(ws: &Workspace) -> (Summaries, Vec<(FileId, CheckFacts)>) {
        let d = Dialect::superset();
        let facts: Vec<(FileId, CheckFacts)> = ws
            .files()
            .map(|f| {
                let data = ws.file(f);
                (f, check_facts(&data.syntax(), &data.facts, &d))
            })
            .collect();
        (Summaries::build(ws, &facts, 1), facts)
    }

    #[test]
    fn hashed_names() {
        assert!(is_hashed_name("function_1a2b3c4d"));
        assert!(is_hashed_name("_id_1A2B"));
        assert!(!is_hashed_name("function_name"));
        assert!(!is_hashed_name("spawn"));
    }

    #[test]
    fn collects_facts() {
        let ws = workspace(&[(
            "scripts/zm/a.gsc",
            "#using scripts\\shared\\util_shared;\n#namespace foo;\n#define W wait 1\nfunction f() { thread g(); h(); self.hp = 1; }\nfunction g() { self waittill(\"x\"); }\n",
        )]);
        let (_, facts) = summaries(&ws);
        let c = &facts[0].1;
        assert_eq!(ws.file(FileId(0)).facts.namespace, "foo");
        assert_eq!(c.functions.len(), 2);
        assert_eq!(c.functions[0].1.calls, vec![Target::Bare("h".into())]);
        assert!(c.functions[1].1.direct);
        assert!(c.macros[0].1.direct);
        assert_eq!(c.fields_written, vec!["hp"]);
    }

    /// A wait the game compiles to an opcode comes back from a decompiler as a plain call, so
    /// `summarize` meets it as a bare callee and not as its keyword. It still waits.
    #[test]
    fn a_wait_spelled_as_a_call_yields() {
        let ws = workspace(&[(
            "a.gsc",
            "function f() { self waittilltimeout(5, \"x\"); }\nfunction g() { notacall(); }\n",
        )]);
        let (_, facts) = summaries(&ws);
        let c = &facts[0].1;
        assert!(c.functions[0].1.direct, "waittilltimeout(...) waits");
        assert!(!c.functions[1].1.direct, "an ordinary call does not");
    }

    #[test]
    fn yield_fixpoint() {
        let ws = workspace(&[
            (
                "a.gsc",
                "function top() { mid(); }\nfunction mid() { other_file(); }\nfunction rec() { rec(); }\n",
            ),
            ("b.gsc", "#namespace a;\nfunction other_file() { wait 1; }\n"),
        ]);
        let (s, _) = summaries(&ws);
        let y = |name: &str| {
            s.yields(
                &ws,
                FileId(0),
                &Yields {
                    direct: false,
                    calls: vec![Target::Bare(name.into())],
                },
            )
        };
        assert!(y("top"));
        assert!(!y("rec"));
        assert!(y("unknown_function"));
    }

    #[test]
    fn macro_bodies() {
        let d = Dialect::superset();
        let y = summarize_macro("( isdefined( x ) && x )", &["x".into()], &d);
        assert!(!y.direct);
        assert!(y.calls.is_empty());
        let y = summarize_macro("util::wait_network_frame()", &[], &d);
        assert_eq!(
            y.calls,
            vec![Target::Qualified(
                "util".into(),
                "wait_network_frame".into()
            )]
        );
        let y = summarize_macro("self thread foo()", &[], &d);
        assert!(y.calls.is_empty());
    }
}
