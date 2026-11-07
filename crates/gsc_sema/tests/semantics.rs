//! The VM semantics a profile carries (gruff-data `semantics/<game>.toml`, read through the
//! profile features) and what they change: the event sites of `waittill`/`endon`, and hover.

use std::path::PathBuf;

use gsc_sema::events::EventName;
use gsc_sema::{BareCall, Config, EventArgs, FileId, Profile, Resolution, Root, Workspace};
use gsc_syntax::{Dialect, TextSize};

/// `$GRUFF_DATA` or `$GSC_DATA`, else a `gruff-data` (or the older `gsc-data`) folder next to the
/// repository.
fn data_root() -> PathBuf {
    if let Some(d) = ["GRUFF_DATA", "GSC_DATA"]
        .iter()
        .find_map(|v| std::env::var_os(v).filter(|d| !d.is_empty()))
    {
        return PathBuf::from(d);
    }
    let up = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    ["gruff-data", "gsc-data"]
        .iter()
        .map(|n| up.join(n))
        .find(|d| d.is_dir())
        .unwrap_or_else(|| up.join("gruff-data"))
}

fn profiles_dir() -> Option<PathBuf> {
    Some(data_root().join("profiles")).filter(|d| d.is_dir())
}

#[test]
fn profiles_carry_the_measured_semantics() {
    let Some(dir) = profiles_dir() else {
        eprintln!("gruff-data not found; skipping");
        return;
    };
    let cfg = |game: &str| Config::from_dialect(&Dialect::find(&dir, game).unwrap());
    let t7 = cfg("t7");
    assert_eq!(t7.waittill_args, EventArgs::Bindings);
    assert!(!t7.waittill_struct);
    assert_eq!(t7.notify_inline, Some(false));
    assert!(t7.hash_equality_error);
    let t8 = cfg("t8");
    assert_eq!(
        (t8.waittill_args, t8.endon_args),
        (EventArgs::Events, EventArgs::Events)
    );
    assert!(t8.waittill_struct && !t8.hash_equality_error);
    assert_eq!(t8.notify_inline, Some(false));
    // Cold War runs the woken waiters inside the notify; BO6 and BO7 are assumed from it.
    for game in ["t9", "t10", "t11"] {
        let c = cfg(game);
        assert_eq!(c.waittill_args, EventArgs::Events, "{game}");
        assert!(c.waittill_struct, "{game}");
        assert_eq!(c.notify_inline, Some(true), "{game}");
    }
    // World at War and CoD4: file-first names, and no hash literals (`hash_equality = "n/a"`,
    // read as off).
    for game in ["t4", "iw3"] {
        let d = Dialect::find(&dir, game).unwrap();
        let c = Config::from_dialect(&d);
        assert_eq!(c.bare_call, BareCall::FileFirst, "{game}");
        assert_eq!(c.resolution, Resolution::Include, "{game}");
        assert!(d.not_applicable("hash_equality"), "{game}");
        assert!(!c.hash_equality_error, "{game}");
        assert_eq!(c.notify_inline, Some(false), "{game}");
    }
    // Black Ops: file-first names too; its hashes are ints compared by value (a string against
    // a hash is a script error that `by-value` does not say, see the profile's note).
    let t5 = cfg("t5");
    assert_eq!(t5.bare_call, BareCall::FileFirst);
    assert!(!t5.hash_equality_error);
    // Black Ops II: the builtin before any script function, even of the same file.
    let t6 = cfg("t6");
    assert_eq!(t6.bare_call, BareCall::BuiltinStrict);
    assert_eq!(t6.resolution, Resolution::Include);
    assert!(t6.hash_equality_error);
    assert_eq!(cfg("t7").bare_call, BareCall::NamespaceFirst);
    // `x = thread f()`: f's result or undefined from CoD4 to BO4, a `finalization` in Cold War.
    for game in ["iw3", "t4", "t5", "t6", "t7", "t8"] {
        assert_eq!(cfg(game).thread_call_result, Some(true), "{game}");
    }
    assert_eq!(cfg("t9").thread_call_result, Some(false));
    // The x64 IW games (MW3 to IW), measured in their VMs: IW3's runtime semantics; their
    // bare-call resolution is decided at compile time and stays as documented.
    for game in ["iw5", "iw6", "s1", "h1", "iw7"] {
        let c = cfg(game);
        assert_eq!(c.thread_call_result, Some(true), "{game}");
        assert_eq!(c.notify_inline, Some(false), "{game}");
        assert_eq!(c.waittill_args, EventArgs::Bindings, "{game}");
        assert!(!c.waittill_struct, "{game}");
    }
    assert_eq!(cfg("iw5").bare_call, BareCall::BuiltinFirst);
    // Games whose VM has not been run stay unknown (MW2's x64 re-release has no loader).
    let iw4 = cfg("iw4");
    assert_eq!(iw4.thread_call_result, None);
    assert_eq!(iw4.waittill_args, EventArgs::Guess);
    assert_eq!(iw4.notify_inline, None);
}

const SRC: &str = "function f( ev )\n{\n    level waittill( \"a\", \"b\", x );\n    \
                   level endon( \"c\", ev );\n    level notify( \"a\" );\n}\n";

fn workspace(args: EventArgs, inline: Option<bool>) -> Workspace {
    let mut cfg = Config::names(Resolution::Namespace, BareCall::NamespaceFirst, true);
    cfg.waittill_args = args;
    cfg.endon_args = args;
    cfg.notify_inline = inline;
    let profile = Profile::new(Dialect::superset(), None, None).with_config(cfg);
    let mut ws = Workspace::new(profile).with_jobs(1);
    let root = ws.add_root(Root::new("."));
    ws.add_file(root, "a.gsc".into(), "a.gsc", SRC);
    ws
}

/// `op name argc` of every waittill and endon site.
fn sites(ws: &Workspace) -> Vec<String> {
    ws.file(FileId(0))
        .facts
        .events
        .iter()
        .filter(|s| s.op != "notify")
        .map(|s| {
            let name = match &s.name {
                EventName::Dynamic => "<dynamic>",
                n => n.render(),
            };
            format!("{} {} {}", s.op, name, s.argc)
        })
        .collect()
}

#[test]
fn waittill_and_endon_arguments_per_game() {
    // T7: the names after the event are bound.
    assert_eq!(
        sites(&workspace(EventArgs::Bindings, None)),
        ["waittill a 2", "endon c 1"]
    );
    // BO4 and later: every argument is an event, computed ones too.
    assert_eq!(
        sites(&workspace(EventArgs::Events, None)),
        [
            "waittill a 0",
            "waittill b 0",
            "waittill <dynamic> 0",
            "endon c 0",
            "endon <dynamic> 0"
        ]
    );
    // Unknown: string arguments are events, the rest bindings.
    assert_eq!(
        sites(&workspace(EventArgs::Guess, None)),
        ["waittill a 1", "waittill b 1", "endon c 1"]
    );
}

#[test]
fn hover_says_when_waiters_run() {
    let at = TextSize::from((SRC.rfind("\"a\"").unwrap() + 1) as u32);
    let detail = |inline: Option<bool>| {
        workspace(EventArgs::Events, inline)
            .hover(FileId(0), at)
            .unwrap()
            .detail
            .join("\n")
    };
    assert!(detail(Some(true)).contains("inside the call"));
    assert!(detail(Some(false)).contains("queues the woken waiters"));
    assert!(!detail(None).contains("woken waiters"));
}
