//! The event rules (T001-T004) over a hand-written fixture (`tests/fixtures/events`) and a tiny
//! made-up engine event catalogue.

use std::path::PathBuf;

use gsc_check::output::{self, Format};
use gsc_check::{check, Options, Selection};
use gsc_sema::{BareCall, Config, EventArgs, EventCatalogue, Profile, Resolution, Root, Workspace};
use gsc_syntax::Dialect;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/events")
}

fn workspace(events: Option<EventCatalogue>) -> Workspace {
    let profile = Profile::new(Dialect::superset(), None, None).with_events(events);
    load(profile, fixtures().join("scripts"))
}

/// BO4 semantics (gruff-data `profiles/t8.toml`, measured in the emulated VM): `waittill`
/// returns the payload struct and takes several events.
fn t8_workspace() -> Workspace {
    let mut cfg = Config::names(Resolution::Namespace, BareCall::NamespaceFirst, true);
    cfg.waittill_args = EventArgs::Events;
    cfg.endon_args = EventArgs::Events;
    cfg.waittill_struct = true;
    let profile = Profile::new(Dialect::superset(), None, None).with_config(cfg);
    load(profile, fixtures().join("../events_t8/scripts"))
}

fn load(profile: Profile, root: PathBuf) -> Workspace {
    let mut ws = Workspace::new(profile).with_jobs(1);
    ws.add_root(Root::new(root));
    assert!(ws.load().errors.is_empty());
    let files: Vec<_> = ws.files().collect();
    for f in files {
        let rel = ws.file(f).meta.rel.clone();
        ws.set_display(f, rel);
    }
    ws
}

fn run(ws: &Workspace) -> String {
    let selection = Selection {
        layers: vec![gsc_check::Layer {
            select: Some(gsc_check::select::parse_list(&["T"]).unwrap()),
            ..Default::default()
        }],
        preview: true,
    };
    let opts = Options {
        selection: &selection,
        per_file_ignores: &[],
        root: None,
    };
    output::render(&check(ws, &opts), Format::Concise)
}

fn catalogue() -> EventCatalogue {
    EventCatalogue::load(&fixtures().join("events.toml")).unwrap()
}

#[test]
fn with_a_complete_catalogue() {
    insta::assert_snapshot!(run(&workspace(Some(catalogue()))));
}

#[test]
fn without_a_catalogue() {
    // T001 and T003 are silent: nothing says what the engine notifies, or with what payload,
    // and a script that re-notifies an engine event sends part of it. T002 still finds the
    // typos of script events, whose notifies the workspace can see.
    let out = run(&workspace(None));
    assert!(!out.contains("T001"), "{out}");
    assert!(!out.contains("T003"), "{out}");
    insta::assert_snapshot!(out);
}

#[test]
fn incomplete_catalogue_keeps_t001_silent() {
    let mut c = catalogue();
    c.complete = false;
    let out = run(&workspace(Some(c)));
    assert!(!out.contains("T001"), "{out}");
}

#[test]
fn graph_queries() {
    let ws = workspace(None);
    assert!(ws.event_emitted("round_started"));
    assert!(ws.event_emitted("wave_7"), "pattern notify");
    assert!(!ws.event_emitted("late_event"));
    assert!(ws.event_mentioned("late_event"));
    assert_eq!(
        ws.event_payload("picked_up"),
        gsc_sema::events::Payload::AtMost(1)
    );
    assert_eq!(ws.dynamic_emit_payload(), Some(0));
}

#[test]
fn t8_struct_waittill() {
    // T003 reads the result's fields: `who` and `loser` are never sent; a read after an `if`
    // whose branches wait for different events, a computed event and a written field are not
    // judged.
    insta::assert_snapshot!(run(&t8_workspace()));
}

#[test]
fn t8_event_arguments() {
    let ws = t8_workspace();
    let sites = |name: &str| -> Vec<(String, usize)> {
        ws.event_sites(name)
            .iter()
            .map(|&s| ws.event_site(s))
            .filter(|s| s.class != gsc_sema::EventClass::Emit)
            .map(|s| (s.op.clone(), s.argc))
            .collect()
    };
    // Every waittill argument is an event, the computed one included; nothing is bound.
    assert_eq!(
        sites("picked_up"),
        vec![
            ("waittill".to_owned(), 0),
            ("waittill".to_owned(), 0),
            ("waittillmatch".to_owned(), 0)
        ]
    );
    assert_eq!(sites("match_over"), vec![("waittill".to_owned(), 0); 2]);
    let fields = ws.event_payload_fields("picked_up").unwrap();
    assert_eq!(fields.into_iter().collect::<Vec<_>>(), ["count", "item"]);
    assert!(ws.event_payload_fields("never_sent").is_none());
}
