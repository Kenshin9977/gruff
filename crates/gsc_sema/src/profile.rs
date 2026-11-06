//! The game a file is read as: its dialect (parsing, gating), its name-resolution family and its
//! engine data (builtins, fields).

use std::sync::Arc;

use gsc_hash::{Dictionary, Scheme};
use gsc_syntax::{Dialect, FeatureValue, Gate};

use crate::data::{Catalogue, FieldCatalogue};
use crate::events::EventCatalogue;

/// How bare names reach other files: the engine family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// Treyarch T7 and later: `#namespace` and `#using`; a function's namespace is its file's
    /// (or the last `#namespace` before it).
    Namespace,
    /// IW games and Treyarch T4-T6: `#include` merges files; a function is named by its file
    /// path (`maps\mp\_utility::foo`).
    Include,
}

/// `bare_call_resolution`: where an unqualified call `foo()` looks, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BareCall {
    /// The same file, the builtins, then the imports (`#include`; with `#namespace` the
    /// current namespace and the `#using`'d files); a threaded call and `&foo` resolve as a
    /// plain call, `::foo` reaches only a function of the same file. Measured in no game (the
    /// IW games but IW3, CoD Online and MWIII carry it), and despite the name a function of the same
    /// file comes before the builtin. Also the reading of an unknown value.
    BuiltinFirst,
    /// T7 and later: the current namespace, then the builtins, never another namespace; a
    /// threaded call never reaches a builtin (measured in the T7, T8 and T9 VMs). In a game
    /// without namespaces: the same file, the `#include`d files, then the builtins.
    NamespaceFirst,
    /// T4, IW3 (measured in their VMs): no namespaces; the same file, the builtin, then the
    /// `#include`d files (an `#include`d function named like a builtin does not shadow it);
    /// `::foo` and `thread foo()` never reach a builtin.
    FileFirst,
    /// T6 (measured in its VM): no namespaces; the builtin before any script function, even
    /// one of the same file (only `path::foo` reaches it), then the same file, then the
    /// `#include`d files. `thread foo()` never reaches a builtin (the same file, then the
    /// `#include`d files); `::foo` naming a builtin takes the builtin (the game stores its
    /// native handler, which does not run as script: rule B005).
    BuiltinStrict,
}

/// What the arguments after the first of a `waittill` or `endon` are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventArgs {
    /// More events: the wait wakes on (the `endon` ends on) any of them (BO4 and later,
    /// measured in the T8/T9 VMs).
    Events,
    /// The names `waittill` binds the notify's values to (T7, measured).
    Bindings,
    /// Not known for the game: string arguments are events, the others bindings.
    Guess,
}

/// The name-resolution and VM semantics of a dialect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub resolution: Resolution,
    /// `bare_call_resolution`.
    pub bare_call: BareCall,
    /// Lower-case identifiers (functions, namespaces, fields, paths).
    pub case_insensitive: bool,
    /// `waittill_multiple_events`.
    pub waittill_args: EventArgs,
    /// `endon_multiple_events`.
    pub endon_args: EventArgs,
    /// `waittill_return_value`: `r = x waittill( "e" )` is a struct holding the payload's
    /// fields and `_notify` (BO4 and later).
    pub waittill_struct: bool,
    /// `notify_wakes_waiters`: `Some(true)` when the woken waiters run inside the `notify` call
    /// (Cold War), `Some(false)` when they run after the notifier yields (T7, T8).
    pub notify_inline: Option<bool>,
    /// `hash_equality = "script-error"`: `==` on a hash is a script error (T7). False for
    /// `by-value`, for `n/a` (no hash literals: the check has nothing to look at) and unknown.
    pub hash_equality_error: bool,
    /// `thread_call_value`: `Some(true)` when `x = thread f()` gives the caller f's return value
    /// if f ends before its first wait, else undefined (`callee-result`: IW3, T4 to T8);
    /// `Some(false)` when it is a thread value of its own (`finalization`: Cold War); `None`
    /// unknown.
    pub thread_call_result: Option<bool>,
}

impl Config {
    /// Read `name_resolution`, `import_directive`, `bare_call_resolution`,
    /// `case_insensitive_identifiers` and the VM features (`waittill_return_value`,
    /// `*_multiple_events`, `notify_wakes_waiters`, `hash_equality`, `thread_call_value`) from a profile. Unknown
    /// values fall back to the T7 model for names and stay unknown for the VM; `n/a` (does not
    /// apply) reads as off.
    pub fn from_dialect(d: &Dialect) -> Config {
        let text = |name: &str| match d.feature(name) {
            Some(FeatureValue::Text(t)) => Some(t.as_str()),
            _ => None,
        };
        let resolution = match (text("name_resolution"), text("import_directive")) {
            (Some("merge"), _) | (None, Some("include")) => Resolution::Include,
            _ => Resolution::Namespace,
        };
        let bare_call = match text("bare_call_resolution") {
            Some("namespace-first") => BareCall::NamespaceFirst,
            Some("file-first") => BareCall::FileFirst,
            Some("builtin-strict") => BareCall::BuiltinStrict,
            _ => BareCall::BuiltinFirst,
        };
        let args = |name: &str| match d.flag(name) {
            Gate::On => EventArgs::Events,
            Gate::Off => EventArgs::Bindings,
            Gate::Unknown => EventArgs::Guess,
        };
        Config {
            resolution,
            bare_call,
            case_insensitive: d.case_insensitive(),
            waittill_args: args("waittill_multiple_events"),
            endon_args: args("endon_multiple_events"),
            waittill_struct: d.flag("waittill_return_value") == Gate::On,
            notify_inline: match d.text("notify_wakes_waiters") {
                Some("inline") => Some(true),
                Some("deferred") => Some(false),
                _ => None,
            },
            hash_equality_error: d.text("hash_equality") == Some("script-error"),
            thread_call_result: match d.text("thread_call_value") {
                Some("callee-result") => Some(true),
                Some("finalization") => Some(false),
                _ => None,
            },
        }
    }

    /// Name resolution alone, the VM semantics unknown (hand-made profiles, tests).
    pub fn names(resolution: Resolution, bare_call: BareCall, case_insensitive: bool) -> Config {
        Config {
            resolution,
            bare_call,
            case_insensitive,
            ..Config::from_dialect(&Dialect::superset())
        }
    }

    /// `s` in the game's identifier case.
    pub fn ident(&self, s: &str) -> String {
        if self.case_insensitive {
            s.to_ascii_lowercase()
        } else {
            s.to_owned()
        }
    }
}

/// Whether inputs are decompiler output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Decompiled {
    /// Decide per file from the decompiler's header comments.
    #[default]
    Auto,
    On,
    Off,
}

impl Decompiled {
    pub fn decide(self, text: &str) -> bool {
        match self {
            Decompiled::On => true,
            Decompiled::Off => false,
            Decompiled::Auto => gsc_syntax::looks_decompiled(text),
        }
    }
}

/// One game: dialect, resolution semantics and engine data.
#[derive(Clone, Debug)]
pub struct Profile {
    pub dialect: Dialect,
    dialect_decompiled: Dialect,
    pub config: Config,
    pub builtins: Option<Catalogue>,
    pub fields: Option<FieldCatalogue>,
    /// The events the engine notifies (`gruff-data/events/<game>.toml`).
    pub events: Option<EventCatalogue>,
    /// How the game hashes identifiers (BO3, BO4 and later, MWIII), from the dialect id.
    pub scheme: Option<Scheme>,
    /// Names the hash packages give (`hash_packages` in gruff.toml, `--hash-package`).
    pub names: Option<Arc<Dictionary>>,
}

impl Profile {
    pub fn new(
        dialect: Dialect,
        builtins: Option<Catalogue>,
        fields: Option<FieldCatalogue>,
    ) -> Profile {
        Profile {
            dialect_decompiled: dialect.clone().with_decompiled(true),
            config: Config::from_dialect(&dialect),
            scheme: Scheme::for_profile(&dialect.id),
            dialect,
            builtins,
            fields,
            events: None,
            names: None,
        }
    }

    /// Attach the names of the hash packages.
    pub fn with_names(mut self, names: Option<Arc<Dictionary>>) -> Profile {
        self.names = names;
        self
    }

    /// Override the hash scheme read from the dialect id (`None`: names are compared as written).
    pub fn with_scheme(mut self, scheme: Option<Scheme>) -> Profile {
        self.scheme = scheme;
        self
    }

    /// Attach the engine's event catalogue.
    pub fn with_events(mut self, events: Option<EventCatalogue>) -> Profile {
        self.events = events;
        self
    }

    /// Override the resolution semantics read from the dialect.
    pub fn with_config(mut self, config: Config) -> Profile {
        self.config = config;
        self
    }

    /// The dialect a file is parsed with.
    pub fn dialect(&self, decompiled: bool) -> &Dialect {
        if decompiled {
            &self.dialect_decompiled
        } else {
            &self.dialect
        }
    }
}
