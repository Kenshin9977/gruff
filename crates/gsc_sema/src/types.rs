//! Argument types at builtin call sites, inferred from the scripts alone (`gruff types`; the
//! catalogues' corpus measure, tools/re/types_corpus.py documents the method).
//!
//! Types come from literals, known builtin results ([`builtin_return`]), a few universal
//! fields, local variables (join of their assignments), and script functions' parameters and
//! results (join over resolved call sites and `return`s, to a workspace fixpoint). Lattice:
//! `Bottom` < one type < `Top`; `int` and `float` join to `number`, `undefined` is the identity.

use std::collections::{BTreeMap, HashMap, HashSet};

use gsc_syntax::ast::{
    AnyParam, AstNode, CallExpr, Expr, ForeachStmt, Literal, MethodCallExpr, ParamList,
    PointerCallExpr,
};
use gsc_syntax::{SyntaxKind, SyntaxKind::*, SyntaxNode, TextRange};

use crate::data::Kind;
use crate::facts::def_node;
use crate::par::par_map;
use crate::profile::EventArgs;
use crate::resolve::How;
use crate::vfs::Vm;
use crate::workspace::{DefId, FileId, Workspace};

/// A value's type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ty {
    /// Nothing known yet (a parameter nobody passes, before the fixpoint reaches it).
    Bottom,
    Undefined,
    Bool,
    Int,
    Float,
    /// `int` and `float` mixed.
    Number,
    String,
    IString,
    Hash,
    Vector,
    Array,
    Struct,
    Entity,
    Function,
    Anim,
    Animtree,
    /// A weapon object (T7+, IW8+; weapons are strings before).
    Weapon,
    /// A class instance (`new C()`).
    Object,
    /// Unknown.
    Top,
}

impl Ty {
    /// The label of the catalogues' `corpus_arg_types` (Bottom and Top are `unknown`).
    pub fn label(self) -> &'static str {
        match self {
            Ty::Bottom | Ty::Top => "unknown",
            Ty::Undefined => "undefined",
            Ty::Bool => "bool",
            Ty::Int => "int",
            Ty::Float => "float",
            Ty::Number => "number",
            Ty::String => "string",
            Ty::IString => "istring",
            Ty::Hash => "hash",
            Ty::Vector => "vector",
            Ty::Array => "array",
            Ty::Struct => "struct",
            Ty::Entity => "entity",
            Ty::Function => "function",
            Ty::Anim => "anim",
            Ty::Animtree => "animtree",
            Ty::Weapon => "weapon",
            Ty::Object => "object",
        }
    }

    fn numeric(self) -> bool {
        matches!(self, Ty::Bool | Ty::Int | Ty::Float | Ty::Number)
    }

    /// Least upper bound.
    pub fn join(self, o: Ty) -> Ty {
        use Ty::*;
        match (self, o) {
            (a, b) if a == b => a,
            (Bottom, x) | (x, Bottom) | (Undefined, x) | (x, Undefined) => x,
            (Top, _) | (_, Top) => Top,
            (Bool, Int) | (Int, Bool) => Int,
            (a, b) if a.numeric() && b.numeric() => Number,
            _ => Top,
        }
    }
}

/// Numeric result of an arithmetic operator on two numbers.
fn arith(a: Ty, b: Ty) -> Ty {
    match (a, b) {
        (Ty::Float, _) | (_, Ty::Float) => Ty::Float,
        (Ty::Number, _) | (_, Ty::Number) => Ty::Number,
        _ => Ty::Int,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Add,
    /// `-`, `*`.
    SubMul,
    Div,
    Mod,
    /// `&`, `|`, `^`, `<<`, `>>`.
    Bits,
    /// Comparisons, `&&`, `||`.
    Bool,
    /// `??`.
    Coalesce,
}

fn bin(op: Op, a: Ty, b: Ty) -> Ty {
    use Ty::*;
    if op == Op::Coalesce {
        return a.join(b);
    }
    if op == Op::Bool {
        return Bool;
    }
    if op == Op::Add && (a == String || b == String) {
        return String;
    }
    if a == Bottom || b == Bottom {
        return Bottom;
    }
    let (na, nb) = (a.numeric(), b.numeric());
    match op {
        Op::Bits if na && nb => Int,
        Op::Mod if na && nb => arith(a, b),
        Op::Div if na && nb => match arith(a, b) {
            Int => Number,
            t => t,
        },
        Op::Div if a == Vector && nb => Vector,
        Op::Add | Op::SubMul if na && nb => arith(a, b),
        Op::Add | Op::SubMul if a == Vector && b == Vector => Vector,
        Op::SubMul if (a == Vector && nb) || (na && b == Vector) => Vector,
        _ => Top,
    }
}

/// A type expression of the function IR, evaluated against the fixpoint state.
#[derive(Clone, Debug)]
enum TE {
    C(Ty),
    /// A local variable (or parameter) of the function.
    Var(u32),
    /// The result of a script function.
    Ret(u32),
    Bin(Op, Box<TE>, Box<TE>),
    /// Unary `-`/`+`: numbers and vectors keep their type.
    Neg(Box<TE>),
    Join(Box<TE>, Box<TE>),
}

/// One function, lowered.
#[derive(Debug, Default)]
struct Func {
    /// Variable index of each parameter (`None` for `...`).
    params: Vec<Option<u32>>,
    /// Per variable: what is assigned to it.
    defs: Vec<Vec<TE>>,
    /// Per variable: unknowable whatever its assignments (foreach/waittill bindings).
    top: Vec<bool>,
    returns: Vec<TE>,
    /// Script calls: callee function index, argument types.
    calls: Vec<(u32, Vec<TE>)>,
    /// Builtin calls: key index, argument types.
    sites: Vec<(u32, Vec<TE>)>,
    /// The engine or a pointer may call it with anything.
    escapes: bool,
}

/// A builtin call site key: lower-case name as written, VM (`gsc`, `csc`), kind (`function`,
/// `method`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SiteKey {
    pub name: String,
    pub vm: &'static str,
    pub kind: &'static str,
}

/// What the corpus says about one builtin.
#[derive(Clone, Debug, Default)]
pub struct BuiltinTypes {
    pub sites: u32,
    pub files: u32,
    /// Per argument index: count per type label.
    pub args: Vec<BTreeMap<&'static str, u32>>,
}

/// The result of [`infer`].
#[derive(Debug, Default)]
pub struct Inference {
    pub builtins: BTreeMap<SiteKey, BuiltinTypes>,
    /// Script functions, their parameters, and how many parameters got a type.
    pub functions: usize,
    pub params: usize,
    pub params_typed: usize,
    /// Fixpoint rounds.
    pub rounds: usize,
}

/// Result types of builtins and intrinsics every engine shares (lower-case names); the weapon
/// getters return objects only where `weapon_objects`.
pub fn builtin_return(name: &str, weapon_objects: bool) -> Option<Ty> {
    use Ty::*;
    Some(match name {
        "getweapon" | "getcurrentweapon" | "getcurrentoffhand" | "makeweapon" => {
            if weapon_objects {
                Weapon
            } else {
                String
            }
        }
        "isdefined" | "isalive" | "isplayer" | "isai" | "isarray" | "isstring" | "isint"
        | "isfloat" | "isvec" | "isistring" | "isfunctionptr" | "issubstr" | "isweapon"
        | "isvehicle" | "issentient" | "isactor" | "istrue" | "isbot" | "istestclient"
        | "bullettracepassed" | "sighttracepassed" | "isspawner" | "isstruct" => Bool,
        "int" | "randomint" | "randomintrange" | "gettime" | "getdvarint" | "getentitynumber"
        | "strtol" | "getstarttime" => Int,
        "float" | "randomfloat" | "randomfloatrange" | "getdvarfloat" | "distance"
        | "distancesquared" | "distance2d" | "distance2dsquared" | "length" | "lengthsquared"
        | "length2d" | "length2dsquared" | "vectordot" | "sqrt" | "sin" | "cos" | "tan"
        | "asin" | "acos" | "atan" | "angleclamp180" | "angleclamp" | "absangleclamp180"
        | "absangleclamp360" | "angleclamp360" | "squared" | "log" | "exp" | "pow" => Float,
        "vectornormalize"
        | "vectorscale"
        | "vectorcross"
        | "anglestoforward"
        | "anglestoright"
        | "anglestoup"
        | "vectortoangles"
        | "vectorlerp"
        | "rotatepoint"
        | "gettagorigin"
        | "gettagangles"
        | "geteye"
        | "getplayerangles"
        | "getorigin"
        | "getangles"
        | "getvelocity"
        | "playerphysicstrace"
        | "physicstrace"
        | "getstartorigin"
        | "getstartangles"
        | "getgroundpos"
        | "worldtolocalcoords"
        | "localtoworldcoords"
        | "combineangles"
        | "invertangles"
        | "vectorfromlinetopoint"
        | "pointonsegmentnearesttopoint"
        | "getclosestpointonnavmesh"
        | "getcentroid" => Vector,
        "getdvar"
        | "getdvarstring"
        | "tolower"
        | "toupper"
        | "getsubstr"
        | "strip"
        | "getsubstring"
        | "tablelookup"
        | "tablelookupcolumnforrow"
        | "getmapname" => String,
        "tablelookupistring" => IString,
        "getentarray"
        | "getaiarray"
        | "getplayers"
        | "strtok"
        | "getarraykeys"
        | "array"
        | "getspawnerarray"
        | "getvehiclenodearray"
        | "getnodearray"
        | "getallnodes"
        | "getstructarray"
        | "getentarraybytype"
        | "getaispeciesarray"
        | "getvehiclearray"
        | "getcorpsearray"
        | "getweaponslist"
        | "getweaponslistprimaries" => Array,
        "spawnstruct" => Struct,
        "spawn" | "getent" | "spawnvehicle" | "spawnturret" | "spawnplane" | "spawnhelicopter"
        | "getvehiclenode" | "getnode" | "spawnactor" | "spawnbot" => Entity,
        "hash" => Hash,
        _ => return None,
    })
}

/// The type of a few fields every engine gives the same type.
fn field_type(name: &str) -> Option<Ty> {
    match name {
        "size" | "health" | "maxhealth" => Some(Ty::Int),
        "origin" | "angles" => Some(Ty::Vector),
        _ => None,
    }
}

fn literal(lit: &Literal) -> Ty {
    let Some(t) = lit.token() else { return Ty::Top };
    match t.kind() {
        INT_NUMBER => Ty::Int,
        FLOAT_NUMBER => Ty::Float,
        STRING => Ty::String,
        ISTRING => Ty::IString,
        HASH_STRING | HASH_IDENT => Ty::Hash,
        PREFIXED_STRING if t.text().starts_with('%') => Ty::Anim,
        PREFIXED_STRING => Ty::Hash,
        TRUE_KW | FALSE_KW => Ty::Bool,
        UNDEFINED_KW => Ty::Undefined,
        _ => Ty::Top,
    }
}

fn bin_op(k: SyntaxKind) -> Op {
    match k {
        PLUS => Op::Add,
        MINUS | STAR => Op::SubMul,
        SLASH => Op::Div,
        PERCENT => Op::Mod,
        AMP | PIPE | CARET | SHL | SHR => Op::Bits,
        QUESTION2 => Op::Coalesce,
        _ => Op::Bool,
    }
}

fn assign_op(k: SyntaxKind) -> Option<Op> {
    Some(match k {
        PLUSEQ => Op::Add,
        MINUSEQ | STAREQ => Op::SubMul,
        SLASHEQ => Op::Div,
        PERCENTEQ => Op::Mod,
        AMPEQ | PIPEEQ | CARETEQ | SHLEQ | SHREQ => Op::Bits,
        _ => return None,
    })
}

/// The resolution of every call and pointer of a file, by the range of the name.
type FileCalls = HashMap<TextRange, (How, Option<DefId>, String)>;

fn file_calls(ws: &Workspace, f: FileId) -> FileCalls {
    let facts = &ws.file(f).facts;
    let res = ws.resolution(f);
    facts
        .refs
        .iter()
        .zip(res.refs.iter())
        .map(|(x, r)| (x.range, (r.how, r.def, r.callee.clone())))
        .collect()
}

/// The range facts use for a call's name.
fn callee_range(call: &CallExpr) -> Option<TextRange> {
    match call.callee()? {
        Expr::NameRef(n) => Some(n.syntax().text_range()),
        Expr::QualifiedRef(q) => Some(q.name_ref()?.syntax().text_range()),
        _ => None,
    }
}

/// A plain identifier (not `self`, `level`...).
fn is_ident(e: &SyntaxNode) -> bool {
    e.first_token().is_some_and(|t| t.kind() == IDENT)
}

struct Lower<'a> {
    ws: &'a Workspace,
    file: FileId,
    vm: Vm,
    calls: &'a FileCalls,
    fn_index: &'a HashMap<DefId, u32>,
    keys: &'a mut Vec<SiteKey>,
    key_index: &'a mut HashMap<SiteKey, u32>,
    vars: HashMap<String, u32>,
    weapon_objects: bool,
    out: Func,
}

impl Lower<'_> {
    fn var(&mut self, name: &str) -> u32 {
        let name = name.to_ascii_lowercase();
        if let Some(&v) = self.vars.get(&name) {
            return v;
        }
        let v = self.out.defs.len() as u32;
        self.out.defs.push(Vec::new());
        self.out.top.push(false);
        self.vars.insert(name, v);
        v
    }

    fn bind_top(&mut self, e: &Expr) {
        if let Expr::NameRef(n) = e {
            let v = self.var(&n.text());
            self.out.top[v as usize] = true;
        }
    }

    fn key(&mut self, name: String, kind: Kind) -> u32 {
        let k = SiteKey {
            name,
            vm: match self.vm {
                Vm::Gsc => "gsc",
                Vm::Csc => "csc",
            },
            kind: kind.label(),
        };
        if let Some(&i) = self.key_index.get(&k) {
            return i;
        }
        let i = self.keys.len() as u32;
        self.keys.push(k.clone());
        self.key_index.insert(k, i);
        i
    }

    fn args(&mut self, list: Option<gsc_syntax::ast::ArgList>) -> Vec<TE> {
        list.map(|l| l.exprs().map(|e| self.expr(&e)).collect())
            .unwrap_or_default()
    }

    fn is_catalogued(&self, name: &str, kind: Kind) -> bool {
        self.ws
            .profile_of(self.file)
            .builtins
            .as_ref()
            .is_some_and(|cat| {
                cat.intrinsic(name).is_none()
                    && cat
                        .lookup(name)
                        .iter()
                        .any(|b| b.vm == self.vm && b.kind == kind)
            })
    }

    /// A decompiler's `function_<hex>` naming a builtin of this VM and kind by hash: the entry's
    /// name if it has one, else the spelling.
    fn hashed_builtin(&self, written: &str, kind: Kind) -> Option<String> {
        let hex = written.strip_prefix("function_")?;
        let h = u64::from_str_radix(hex, 16).ok()?;
        let cat = self.ws.profile_of(self.file).builtins.as_ref()?;
        let b = cat
            .lookup_hash(h)
            .iter()
            .find(|b| b.vm == self.vm && b.kind == kind)?;
        Some(b.name.clone().unwrap_or_else(|| written.to_owned()))
    }

    /// A call: records the site, returns the result's type.
    fn call(&mut self, c: &CallExpr) -> TE {
        let threaded = c
            .modifier()
            .is_some_and(|m| matches!(m.kind(), THREAD_KW | CHILDTHREAD_KW | THREADENDON_KW));
        let written = match c.callee() {
            Some(Expr::NameRef(n)) => n.text().to_ascii_lowercase(),
            _ => String::new(),
        };
        if matches!(written.as_str(), "waittill" | "waittillmatch") {
            // The bindings receive what the notify passes: unknowable here. BO4 and later bind
            // nothing: the waittill returns a struct.
            let skip = if written == "waittill" { 1 } else { 2 };
            let binds = self.ws.profile_of(self.file).config.waittill_args != EventArgs::Events;
            if let Some(l) = c.arg_list().filter(|_| binds) {
                for e in l.exprs().skip(skip) {
                    self.bind_top(&e);
                }
            }
            return TE::C(Ty::Struct);
        }
        let res = callee_range(c).and_then(|r| self.calls.get(&r)).cloned();
        let kind = if c.receiver().is_some() {
            Kind::Method
        } else {
            Kind::Function
        };
        let Some((how, def, callee)) = res else {
            self.args(c.arg_list());
            return TE::C(Ty::Top);
        };
        if how == How::Unresolved {
            if let Some(name) = self.hashed_builtin(&written, kind) {
                let args = self.args(c.arg_list());
                let k = self.key(name, kind);
                self.out.sites.push((k, args));
                return TE::C(Ty::Top);
            }
        }
        let lower = callee.to_ascii_lowercase();
        let args = self.args(c.arg_list());
        let keyword = matches!(c.callee(), Some(Expr::NameRef(n)) if !is_ident(n.syntax()));
        let ret = if threaded {
            TE::C(Ty::Top)
        } else {
            builtin_return(&lower, self.weapon_objects).map_or(TE::C(Ty::Top), TE::C)
        };
        match (how, def) {
            (How::Builtin, _) => {
                if !keyword && self.is_catalogued(&lower, kind) {
                    let k = self.key(lower, kind);
                    self.out.sites.push((k, args));
                }
                ret
            }
            (h, Some(d)) if h.is_script() && h != How::Macro => match self.fn_index.get(&d) {
                Some(&i) => {
                    self.out.calls.push((i, args));
                    if threaded {
                        TE::C(Ty::Top)
                    } else {
                        TE::Ret(i)
                    }
                }
                None => TE::C(Ty::Top),
            },
            _ => TE::C(Ty::Top),
        }
    }

    fn assign(&mut self, lhs: Option<Expr>, op: Option<SyntaxKind>, rhs: TE) -> TE {
        match lhs {
            Some(Expr::NameRef(n)) if is_ident(n.syntax()) => {
                let v = self.var(&n.text());
                let value = match op.and_then(assign_op) {
                    Some(o) => TE::Bin(o, Box::new(TE::Var(v)), Box::new(rhs)),
                    None if op == Some(EQ) => rhs,
                    None => TE::C(Ty::Top),
                };
                self.out.defs[v as usize].push(value.clone());
                value
            }
            Some(Expr::IndexExpr(i)) => {
                // `x[k] = v` on an undefined `x` makes it an array.
                match i.base() {
                    Some(Expr::NameRef(n)) if is_ident(n.syntax()) => {
                        let v = self.var(&n.text());
                        self.out.defs[v as usize].push(TE::C(Ty::Array));
                    }
                    Some(b) => {
                        self.expr(&b);
                    }
                    None => {}
                }
                if let Some(x) = i.index() {
                    self.expr(&x);
                }
                rhs
            }
            Some(other) => {
                self.expr(&other);
                rhs
            }
            None => rhs,
        }
    }

    /// `x++`, `--x`.
    fn increment(&mut self, e: Option<Expr>) {
        if let Some(Expr::NameRef(n)) = e {
            if is_ident(n.syntax()) {
                let v = self.var(&n.text());
                self.out.defs[v as usize].push(TE::Bin(
                    Op::Add,
                    Box::new(TE::Var(v)),
                    Box::new(TE::C(Ty::Int)),
                ));
            }
        }
    }

    fn opt(&mut self, e: Option<Expr>) -> TE {
        e.map_or(TE::C(Ty::Top), |x| self.expr(&x))
    }

    fn expr(&mut self, e: &Expr) -> TE {
        match e {
            Expr::Literal(l) => TE::C(literal(l)),
            Expr::NameRef(n) if is_ident(n.syntax()) => TE::Var(self.var(&n.text())),
            Expr::NameRef(_) | Expr::ThisThreadExpr(_) => TE::C(Ty::Top),
            Expr::QualifiedRef(_) | Expr::FunctionRef(_) => TE::C(Ty::Function),
            Expr::CallExpr(c) => {
                if let Some(r) = c.receiver() {
                    self.expr(&r);
                }
                self.call(c)
            }
            Expr::PointerCallExpr(c) => self.pointer_call(c),
            Expr::MethodCallExpr(c) => self.method_call(c),
            Expr::FieldExpr(f) => {
                self.opt(f.base());
                let name = f.field().map(|n| n.text().to_ascii_lowercase());
                TE::C(name.and_then(|n| field_type(&n)).unwrap_or(Ty::Top))
            }
            Expr::IndexExpr(i) => {
                self.opt(i.base());
                self.opt(i.index());
                TE::C(Ty::Top)
            }
            Expr::PrefixExpr(p) => {
                let inner = self.opt(p.expr());
                if p.bang_token().is_some() {
                    TE::C(Ty::Bool)
                } else if p.tilde_token().is_some() {
                    TE::C(Ty::Int)
                } else if p.plus2_token().is_some() || p.minus2_token().is_some() {
                    self.increment(p.expr());
                    inner
                } else {
                    TE::Neg(Box::new(inner))
                }
            }
            Expr::PostfixExpr(p) => {
                let inner = self.opt(p.expr());
                self.increment(p.expr());
                inner
            }
            Expr::BinExpr(b) => {
                let l = self.opt(b.lhs());
                let r = self.opt(b.rhs());
                let op = b.op().map_or(Op::Bool, |t| bin_op(t.kind()));
                TE::Bin(op, Box::new(l), Box::new(r))
            }
            Expr::AssignExpr(a) => {
                let rhs = self.opt(a.rhs());
                self.assign(a.lhs(), a.op().map(|t| t.kind()), rhs)
            }
            Expr::TernaryExpr(t) => {
                self.opt(t.condition());
                let a = self.opt(t.then_value());
                let b = self.opt(t.else_value());
                TE::Join(Box::new(a), Box::new(b))
            }
            Expr::IsExpr(i) => {
                self.opt(i.expr());
                TE::C(Ty::Bool)
            }
            Expr::ParenExpr(p) => self.opt(p.expr()),
            Expr::VectorExpr(v) => {
                for x in [v.x(), v.y(), v.z()] {
                    self.opt(x);
                }
                TE::C(Ty::Vector)
            }
            Expr::ArrayExpr(a) => {
                self.children(a.syntax());
                TE::C(Ty::Array)
            }
            Expr::StructExpr(s) => {
                self.children(s.syntax());
                TE::C(Ty::Struct)
            }
            Expr::NewExpr(n) => {
                self.args(n.arg_list());
                TE::C(Ty::Object)
            }
            Expr::AnimRef(_) => TE::C(Ty::Anim),
            Expr::AnimtreeExpr(_) => TE::C(Ty::Animtree),
            Expr::DollarName(_) => TE::C(Ty::Entity),
            Expr::CastExpr(c) => {
                self.opt(c.expr());
                match c.ty().map(|t| t.text().to_ascii_lowercase()).as_deref() {
                    Some("int") => TE::C(Ty::Int),
                    Some("float") => TE::C(Ty::Float),
                    _ => TE::C(Ty::Top),
                }
            }
        }
    }

    /// The expressions of array elements and struct fields (keys and values).
    fn children(&mut self, node: &SyntaxNode) {
        for c in node.children() {
            match Expr::cast(c.clone()) {
                Some(e) => {
                    self.expr(&e);
                }
                None => {
                    for e in c.children().filter_map(Expr::cast) {
                        self.expr(&e);
                    }
                }
            }
        }
    }

    fn pointer_call(&mut self, c: &PointerCallExpr) -> TE {
        self.opt(c.receiver());
        self.opt(c.target());
        self.args(c.arg_list());
        TE::C(Ty::Top)
    }

    fn method_call(&mut self, c: &MethodCallExpr) -> TE {
        self.opt(c.receiver());
        self.opt(c.object());
        let def = c
            .method()
            .and_then(|m| self.calls.get(&m.syntax().text_range()))
            .and_then(|(how, def, _)| if *how == How::Method { *def } else { None });
        let args = self.args(c.arg_list());
        match def.and_then(|d| self.fn_index.get(&d)) {
            Some(&i) => {
                self.out.calls.push((i, args));
                TE::Ret(i)
            }
            None => TE::C(Ty::Top),
        }
    }

    /// Lower the code under `body`: each top expression (one whose parent is not an expression
    /// or an argument list) is walked once, with everything inside it.
    fn body(&mut self, body: &SyntaxNode) {
        let mut stack = vec![body.clone()];
        while let Some(n) = stack.pop() {
            if let Some(e) = Expr::cast(n.clone()) {
                let t = self.expr(&e);
                if n.parent().is_some_and(|p| p.kind() == RETURN_STMT) {
                    self.out.returns.push(t);
                }
                continue;
            }
            match n.kind() {
                RETURN_STMT if !n.children().any(|c| Expr::can_cast(c.kind())) => {
                    self.out.returns.push(TE::C(Ty::Undefined));
                }
                FOREACH_STMT => {
                    let b = ForeachStmt::cast(n.clone()).and_then(|f| f.foreach_bindings());
                    if let Some(b) = b {
                        for name in b.names() {
                            let v = self.var(&name.text());
                            self.out.top[v as usize] = true;
                        }
                        for e in b.exprs() {
                            self.bind_top(&e);
                        }
                    }
                }
                _ => {}
            }
            let kids: Vec<SyntaxNode> = n
                .children()
                .filter(|c| c.kind() != FOREACH_BINDINGS)
                .collect();
            stack.extend(kids.into_iter().rev());
        }
    }
}

/// Are weapons objects in this game (T7+, IW8+)?
pub fn weapon_objects(game: &str) -> bool {
    let num = |p: &str| {
        game.strip_prefix(p).and_then(|r| {
            r.split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse::<u32>()
                .ok()
        })
    };
    num("t").is_some_and(|n| n >= 7) || num("iw").is_some_and(|n| n >= 8) || game.starts_with("jup")
}

/// Does the engine (or anything we cannot see) call the function?
fn escapes_by_flags(flags: &str, name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    flags.contains("autoexec")
        || flags.contains("codecall")
        || flags.contains("event_handler")
        || lower.starts_with("codecallback")
        || lower.starts_with("callback")
}

fn lower_fn(
    ws: &Workspace,
    d: DefId,
    calls: &FileCalls,
    fn_index: &HashMap<DefId, u32>,
    keys: &mut Vec<SiteKey>,
    key_index: &mut HashMap<SiteKey, u32>,
) -> Func {
    let data = ws.file(d.file);
    let def = ws.def(d);
    let mut l = Lower {
        ws,
        file: d.file,
        vm: data.meta.vm.unwrap_or(Vm::Gsc),
        calls,
        fn_index,
        keys,
        key_index,
        vars: HashMap::new(),
        weapon_objects: weapon_objects(&ws.profile_of(d.file).dialect.id),
        out: Func {
            escapes: escapes_by_flags(&def.flags, def.short_name()),
            ..Func::default()
        },
    };
    let root = data.syntax();
    let Some(node) = def_node(&root, def) else {
        return l.out;
    };
    if let Some(list) = node.children().find_map(ParamList::cast) {
        for p in list.any_params() {
            match p {
                AnyParam::Param(p) => {
                    let Some(name) = p.name() else {
                        l.out.params.push(None);
                        continue;
                    };
                    let v = l.var(&name.text());
                    if let Some(e) = p.default_value().and_then(|d| d.expr()) {
                        let t = l.expr(&e);
                        l.out.defs[v as usize].push(t);
                    }
                    l.out.params.push(Some(v));
                }
                AnyParam::VarargParam(_) => l.out.params.push(None),
            }
        }
    }
    if let Some(body) = node.children().find(|c| c.kind() == BLOCK) {
        l.body(&body);
    }
    l.out
}

/// Evaluate a type expression.
fn eval(t: &TE, vars: &[Ty], rets: &[Ty]) -> Ty {
    match t {
        TE::C(x) => *x,
        TE::Var(v) => vars[*v as usize],
        TE::Ret(f) => rets[*f as usize],
        TE::Bin(op, a, b) => bin(*op, eval(a, vars, rets), eval(b, vars, rets)),
        TE::Neg(a) => match eval(a, vars, rets) {
            Ty::Bool => Ty::Int,
            x if x.numeric() || x == Ty::Vector || x == Ty::Bottom => x,
            _ => Ty::Top,
        },
        TE::Join(a, b) => eval(a, vars, rets).join(eval(b, vars, rets)),
    }
}

/// The variables' types of a function, given its parameters' and every function's results.
fn local_fixpoint(f: &Func, params: &[Ty], rets: &[Ty]) -> Vec<Ty> {
    let mut vars = vec![Ty::Bottom; f.defs.len()];
    let is_param: HashSet<u32> = f.params.iter().flatten().copied().collect();
    for (p, t) in f.params.iter().zip(params) {
        if let Some(v) = p {
            vars[*v as usize] = *t;
        }
    }
    for (v, defs) in f.defs.iter().enumerate() {
        // Bound by foreach/waittill, or read and never assigned: unknown.
        if f.top[v] || (defs.is_empty() && !is_param.contains(&(v as u32))) {
            vars[v] = Ty::Top;
        }
    }
    for _ in 0..32 {
        let mut changed = false;
        for (v, defs) in f.defs.iter().enumerate() {
            let t = defs
                .iter()
                .fold(vars[v], |t, d| t.join(eval(d, &vars, rets)));
            if t != vars[v] {
                vars[v] = t;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    vars
}

fn initial_params(funcs: &[Func]) -> Vec<Vec<Ty>> {
    funcs
        .iter()
        .map(|f| {
            let t = if f.escapes { Ty::Top } else { Ty::Bottom };
            f.params
                .iter()
                .map(|p| if p.is_some() { t } else { Ty::Top })
                .collect()
        })
        .collect()
}

/// Infer the argument types of every builtin call site of the workspace.
pub fn infer(ws: &Workspace) -> Inference {
    let defs: Vec<DefId> = ws
        .files()
        .flat_map(|f| {
            ws.file(f)
                .facts
                .defs
                .iter()
                .enumerate()
                .filter(|(_, d)| d.kind.is_callable())
                .map(move |(i, _)| DefId {
                    file: f,
                    index: i as u32,
                })
        })
        .collect();
    let fn_index: HashMap<DefId, u32> = defs
        .iter()
        .enumerate()
        .map(|(i, d)| (*d, i as u32))
        .collect();
    // Lower per file on the workspace's threads; site keys are merged after.
    let files: Vec<FileId> = ws.files().collect();
    let per_file = par_map(&files, ws.jobs(), |&f| {
        let calls = file_calls(ws, f);
        let mut keys = Vec::new();
        let mut key_index = HashMap::new();
        let mut funcs = Vec::new();
        for (i, d) in ws.file(f).facts.defs.iter().enumerate() {
            if !d.kind.is_callable() {
                continue;
            }
            let id = DefId {
                file: f,
                index: i as u32,
            };
            let func = lower_fn(ws, id, &calls, &fn_index, &mut keys, &mut key_index);
            funcs.push((fn_index[&id], func));
        }
        // Functions whose address is taken: a pointer may pass anything.
        let res = ws.resolution(f);
        let pointed: Vec<u32> = ws
            .file(f)
            .facts
            .refs
            .iter()
            .zip(res.refs.iter())
            .filter(|(x, _)| x.kind == "pointer")
            .filter_map(|(_, r)| r.def.and_then(|d| fn_index.get(&d).copied()))
            .collect();
        (f, keys, funcs, pointed)
    });
    let mut funcs: Vec<Func> = (0..defs.len()).map(|_| Func::default()).collect();
    let mut sites: Vec<(u32, FileId, Vec<TE>, u32)> = Vec::new(); // (fn, file, args, key)
    let mut keys: Vec<SiteKey> = Vec::new();
    let mut key_index: HashMap<SiteKey, u32> = HashMap::new();
    let mut escaped: HashSet<u32> = HashSet::new();
    for (f, local_keys, fs, pointed) in per_file {
        let map: Vec<u32> = local_keys
            .into_iter()
            .map(|k| {
                *key_index.entry(k.clone()).or_insert_with(|| {
                    keys.push(k);
                    keys.len() as u32 - 1
                })
            })
            .collect();
        for (i, mut func) in fs {
            for (k, args) in std::mem::take(&mut func.sites) {
                sites.push((i, f, args, map[k as usize]));
            }
            funcs[i as usize] = func;
        }
        escaped.extend(pointed);
    }
    for i in escaped {
        funcs[i as usize].escapes = true;
    }

    // The workspace fixpoint over parameter and result types (monotone: each round recomputes
    // from the previous state and joins it).
    let n = funcs.len();
    let mut params = initial_params(&funcs);
    let mut rets = vec![Ty::Bottom; n];
    let idx: Vec<usize> = (0..n).collect();
    let mut rounds = 0;
    loop {
        rounds += 1;
        let results = par_map(&idx, ws.jobs(), |&i| {
            let f = &funcs[i];
            let vars = local_fixpoint(f, &params[i], &rets);
            let ret = f
                .returns
                .iter()
                .fold(Ty::Bottom, |t, r| t.join(eval(r, &vars, &rets)));
            let contrib: Vec<(u32, Vec<Ty>)> = f
                .calls
                .iter()
                .map(|(c, args)| (*c, args.iter().map(|a| eval(a, &vars, &rets)).collect()))
                .collect();
            (ret, contrib)
        });
        let mut new_params = params.clone();
        let mut new_rets = rets.clone();
        for (i, (ret, contrib)) in results.into_iter().enumerate() {
            new_rets[i] = new_rets[i].join(ret);
            for (c, args) in contrib {
                // A missing argument is undefined: the identity of the join.
                for (slot, a) in new_params[c as usize].iter_mut().zip(&args) {
                    *slot = slot.join(*a);
                }
            }
        }
        let done = new_params == params && new_rets == rets;
        params = new_params;
        rets = new_rets;
        if done || rounds >= 64 {
            break;
        }
    }

    let mut out = Inference {
        functions: n,
        rounds,
        ..Inference::default()
    };
    for (f, p) in funcs.iter().zip(&params) {
        for (slot, t) in f.params.iter().zip(p) {
            out.params += 1;
            if slot.is_some() && !matches!(t, Ty::Bottom | Ty::Top | Ty::Undefined) {
                out.params_typed += 1;
            }
        }
    }
    // Builtin sites, evaluated with the final state.
    let mut vars_cache: HashMap<u32, Vec<Ty>> = HashMap::new();
    let mut files_of: HashMap<u32, HashSet<FileId>> = HashMap::new();
    for (fi, file, args, key) in &sites {
        let vars = vars_cache
            .entry(*fi)
            .or_insert_with(|| local_fixpoint(&funcs[*fi as usize], &params[*fi as usize], &rets));
        let entry = out.builtins.entry(keys[*key as usize].clone()).or_default();
        entry.sites += 1;
        files_of.entry(*key).or_default().insert(*file);
        if entry.args.len() < args.len() {
            entry.args.resize_with(args.len(), BTreeMap::new);
        }
        for (j, a) in args.iter().enumerate() {
            *entry.args[j]
                .entry(eval(a, vars, &rets).label())
                .or_default() += 1;
        }
    }
    for (k, fs) in files_of {
        if let Some(e) = out.builtins.get_mut(&keys[k as usize]) {
            e.files = fs.len() as u32;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_is_a_lattice() {
        assert_eq!(Ty::Int.join(Ty::Float), Ty::Number);
        assert_eq!(Ty::Bool.join(Ty::Int), Ty::Int);
        assert_eq!(Ty::Bottom.join(Ty::String), Ty::String);
        assert_eq!(Ty::Undefined.join(Ty::Vector), Ty::Vector);
        assert_eq!(Ty::String.join(Ty::Vector), Ty::Top);
        assert_eq!(Ty::Number.join(Ty::Int), Ty::Number);
    }

    #[test]
    fn weapon_games() {
        assert!(weapon_objects("t7") && weapon_objects("t11") && weapon_objects("iw8"));
        assert!(weapon_objects("jup-sp"));
        assert!(!weapon_objects("t6") && !weapon_objects("iw7") && !weapon_objects("s1"));
    }

    const CAT: &str = r#"
game = "fixture"

[[builtin]]
name = "earthquake"
vm = "gsc"
kind = "function"
min_args = 4
max_args = 4

[[builtin]]
name = "randomint"
vm = "gsc"
kind = "function"
min_args = 1
max_args = 1

[[builtin]]
name = "setmodel"
vm = "gsc"
kind = "method"
min_args = 1
max_args = 1
"#;

    const SRC: &str = r#"
main()
{
    x = 0.5;
    helper( x, "m", ( 1, 2, 3 ) );
    n = randomint( 3 );
    earthquake( n, x * 2, self.origin, undefined );
    self waittill( "e", w );
    self setmodel( w );
}

helper( a, b, c )
{
    self setmodel( b );
    earthquake( a, 1, c, level.x );
}
"#;

    #[test]
    fn infers_through_locals_and_parameters() {
        use crate::{BareCall, Catalogue, Config, Profile, Resolution, Root};
        let cat = Catalogue::from_toml_str(CAT).unwrap();
        let profile = Profile::new(gsc_syntax::Dialect::superset(), Some(cat), None).with_config(
            Config::names(Resolution::Include, BareCall::BuiltinFirst, true),
        );
        let mut ws = Workspace::new(profile).with_jobs(1);
        let root = ws.add_root(Root::new("."));
        ws.add_file(root, "a.gsc".into(), "a.gsc", SRC);
        let inf = infer(&ws);
        let get = |name: &str, kind: &'static str| {
            let k = SiteKey {
                name: name.to_owned(),
                vm: "gsc",
                kind,
            };
            inf.builtins[&k].clone()
        };
        let eq = get("earthquake", "function");
        assert_eq!(eq.sites, 2);
        let arg = |i: usize| {
            eq.args[i]
                .iter()
                .map(|(t, n)| format!("{t}={n}"))
                .collect::<Vec<_>>()
        };
        assert_eq!(arg(0), ["float=1", "int=1"]);
        assert_eq!(arg(1), ["float=1", "int=1"]);
        assert_eq!(arg(2), ["vector=2"]);
        assert_eq!(arg(3), ["undefined=1", "unknown=1"]);
        let sm = get("setmodel", "method");
        assert_eq!(sm.args[0].get("string"), Some(&1));
        assert_eq!(sm.args[0].get("unknown"), Some(&1));
    }

    #[test]
    fn operators() {
        assert_eq!(bin(Op::Add, Ty::String, Ty::Int), Ty::String);
        assert_eq!(bin(Op::Add, Ty::Int, Ty::Int), Ty::Int);
        assert_eq!(bin(Op::Div, Ty::Int, Ty::Int), Ty::Number);
        assert_eq!(bin(Op::SubMul, Ty::Vector, Ty::Float), Ty::Vector);
        assert_eq!(bin(Op::Add, Ty::Vector, Ty::Int), Ty::Top);
        assert_eq!(bin(Op::Bool, Ty::Top, Ty::Top), Ty::Bool);
    }
}
