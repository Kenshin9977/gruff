//! The GSC semantic core, shared by `gruff check`, `gruff index` and the language server.
//!
//! A [`Workspace`] holds roots (a mod plus optional game script trees), one or more game
//! [`Profile`]s, and the files found under the roots, each parsed once with its
//! [`facts::FileFacts`] (definitions, namespace, imports, references). Global name tables are
//! patched per file on [`Workspace::set_text`]; [`resolve`] links references across files per
//! engine family; [`ide`] answers editor queries (definition, references, hover, symbols,
//! completion, call hierarchy) in offsets and ranges.

pub mod data;
pub mod events;
pub mod facts;
pub mod hashing;
pub mod ide;
pub mod nav;
pub mod par;
pub mod profile;
pub mod resolve;
pub mod types;
pub mod vfs;
pub mod workspace;

pub use data::{Builtin, Catalogue, FieldCatalogue, Intrinsic, Kind};
pub use events::{EventCatalogue, EventClass, EventName, EventSite};
pub use facts::{Def, DefKind, FileFacts, Import, Ref, Target};
pub use gsc_hash::{Dictionary, Role, Scheme};
pub use hashing::{load_packages, Coverage, NameSource, PackageReport};
pub use ide::{
    Callee, CompletionItem, DocumentSymbol, FileRange, Hover, IncomingCall, NavTarget,
    OutgoingCall, Symbol, SymbolKind,
};
pub use par::{default_jobs, par_map};
pub use profile::{BareCall, Config, Decompiled, EventArgs, Profile, Resolution};
pub use resolve::{FileResolution, How, Resolved};
pub use vfs::{Root, RootId, Vm};
pub use workspace::{DefId, FileData, FileId, LoadReport, Workspace};
