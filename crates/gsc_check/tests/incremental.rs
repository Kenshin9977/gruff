//! The incremental session must give the results of a fresh full check after every edit:
//! scripted edits that cross each dependency (yields, names, fields, macros, imports, events,
//! library files), then a few hundred pseudo-random edits.

use std::path::PathBuf;

use gsc_check::incremental::Session;
use gsc_check::output::{self, Format};
use gsc_check::{check, Catalogue, Layer, Options, Selection};
use gsc_sema::{EventCatalogue, FileId, Profile, Root, Workspace};
use gsc_syntax::Dialect;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn profile() -> Profile {
    let cat = Catalogue::load(&fixtures().join("builtins.toml"))
        .unwrap()
        .with_intrinsics(["gettime".to_owned()]);
    let fields = gsc_check::FieldCatalogue::load(&fixtures().join("fields.toml")).unwrap();
    let events = EventCatalogue::load(&fixtures().join("events/events.toml")).ok();
    Profile::new(Dialect::superset(), Some(cat), Some(fields)).with_events(events)
}

const FILES: &[(&str, &str)] = &[
    (
        "scripts/a.gsc",
        "#using scripts\\b;\n#insert scripts\\c.gsh;\n#namespace a;\n\nfunction waits()\n{\n\twait 1;\n}\n\nfunction nowait()\n{\n}\n\nfunction loop1()\n{\n\twhile ( true )\n\t{\n\t\tb::maybe();\n\t\tx = self.foo_bar;\n\t}\n}\n\nfunction listen()\n{\n\tself endon( \"stop_it\" );\n\tself waittill( \"go_now\" );\n}\n",
    ),
    (
        "scripts/b.gsc",
        "#using scripts\\a;\n#namespace b;\n\nfunction maybe()\n{\n\ta::nowait();\n}\n\nfunction spin()\n{\n\tfor ( ;; )\n\t{\n\t\tmaybe();\n\t}\n}\n\nfunction go()\n{\n\tself thread a::listen();\n\tself notify( \"go_now\" );\n}\n",
    ),
    (
        "scripts/c.gsh",
        "#define WAITM wait 0.05\n#define NOWAITM x = 1\n",
    ),
    (
        "scripts/d.gsc",
        "#insert scripts\\c.gsh;\n#namespace d;\n\nfunction m()\n{\n\twhile ( 1 )\n\t{\n\t\tWAITM;\n\t}\n\twhile ( 1 )\n\t{\n\t\tNOWAITM;\n\t}\n\twhile ( 1 )\n\t{\n\t\tlib_fn();\n\t\tspawn( 1, 2, 3, 4, 5 );\n\t}\n}\n",
    ),
];

const LIBRARY: &[(&str, &str)] = &[(
    "lib/lib.gsc",
    "#namespace d;\n\nfunction lib_fn()\n{\n}\n\nfunction set_it()\n{\n\tself.lib_field = 1;\n}\n",
)];

fn workspace() -> Workspace {
    let mut ws = Workspace::new(profile()).with_jobs(2);
    let main = ws.add_root(Root::new("mod"));
    let lib = ws.add_root(Root::new("game").library(true));
    for (p, t) in FILES {
        ws.add_file(main, PathBuf::from("mod").join(p), p, t);
    }
    for (p, t) in LIBRARY {
        ws.add_file(lib, PathBuf::from("game").join(p), p, t);
    }
    // Filler, so that an edit moves few of the summary nodes (the in-place renumbering).
    for i in 0..12 {
        let mut text = format!("#namespace fill{i};\n");
        for j in 0..6 {
            let body = match j % 3 {
                0 => "wait 1;".to_owned(),
                1 => format!("f{}();", j - 1),
                _ => "maybe();".to_owned(),
            };
            text.push_str(&format!("function f{j}() {{ {body} }}\n"));
        }
        let rel = format!("fill/fill{i}.gsc");
        ws.add_file(lib, PathBuf::from("game").join(&rel), &rel, &text);
    }
    ws
}

fn selection() -> Selection {
    Selection {
        layers: vec![Layer {
            select: Some(gsc_check::select::parse_list(&["ALL"]).unwrap()),
            ..Default::default()
        }],
        preview: true,
    }
}

fn render(r: &[gsc_check::FileResult]) -> String {
    output::render(r, Format::Concise)
}

struct Harness {
    ws: Workspace,
    session: Session,
    sel: Selection,
    steps: usize,
    partial: usize,
    modes: std::collections::BTreeMap<&'static str, usize>,
}

impl Harness {
    fn new() -> Harness {
        let mut h = Harness {
            ws: workspace(),
            session: Session::new(),
            sel: selection(),
            steps: 0,
            partial: 0,
            modes: Default::default(),
        };
        h.compare("initial");
        h
    }

    fn compare(&mut self, what: &str) -> String {
        let opts = Options {
            selection: &self.sel,
            per_file_ignores: &[],
            root: None,
        };
        let up = self.session.update(&self.ws, &opts);
        let full = render(&check(&self.ws, &opts));
        let inc = render(&self.session.results(&self.ws));
        assert_eq!(inc, full, "after {what} ({up:?})");
        self.steps += 1;
        *self.modes.entry(up.summaries).or_default() += 1;
        let primary = self.ws.primary_files().count();
        if up.rechecked.len() < primary {
            self.partial += 1;
        }
        full
    }

    fn file(&self, rel: &str) -> FileId {
        self.ws
            .files()
            .find(|&f| self.ws.file(f).meta.rel == rel)
            .unwrap_or_else(|| panic!("no {rel}"))
    }

    fn edit(&mut self, rel: &str, from: &str, to: &str) -> String {
        let f = self.file(rel);
        let text = self.ws.file(f).text().to_owned();
        assert!(text.contains(from), "{rel} has no {from:?}");
        self.ws.set_text(f, &text.replacen(from, to, 1));
        self.compare(&format!("{rel}: {from:?} -> {to:?}"))
    }
}

#[test]
fn scripted_edits_match_a_full_check() {
    let mut h = Harness::new();
    let base = h.compare("nothing");
    assert!(
        base.contains("L001"),
        "the fixture has loop findings:\n{base}"
    );

    // A yield bit flips in another file.
    let out = h.edit("scripts/b.gsc", "\ta::nowait();", "\ta::waits();");
    assert_ne!(out, base);
    h.edit("scripts/b.gsc", "\ta::waits();", "\ta::nowait();");
    h.edit(
        "scripts/a.gsc",
        "function nowait()\n{\n",
        "function nowait()\n{\n\twait 1;\n",
    );
    h.edit(
        "scripts/a.gsc",
        "function nowait()\n{\n\twait 1;\n",
        "function nowait()\n{\n",
    );
    // Names: a renamed function, a new one, a removed one.
    h.edit("scripts/b.gsc", "function maybe()", "function maybe2()");
    h.edit("scripts/b.gsc", "function maybe2()", "function maybe()");
    h.edit(
        "scripts/a.gsc",
        "function waits()",
        "function spawn()\n{\n}\n\nfunction waits()",
    );
    h.edit("scripts/a.gsc", "function spawn()\n{\n}\n\n", "");
    // Fields written somewhere.
    h.edit(
        "scripts/b.gsc",
        "function go()\n{\n",
        "function go()\n{\n\tself.foo_bar = 2;\n",
    );
    h.edit("scripts/b.gsc", "\tself.foo_bar = 2;\n", "");
    // Macros.
    h.edit(
        "scripts/c.gsh",
        "#define NOWAITM x = 1",
        "#define NOWAITM wait 1",
    );
    h.edit(
        "scripts/c.gsh",
        "#define WAITM wait 0.05",
        "#define WAITM y = 2",
    );
    // Imports and namespaces.
    h.edit("scripts/a.gsc", "#using scripts\\b;\n", "");
    h.edit("scripts/b.gsc", "#namespace b;", "#namespace bb;");
    // Events.
    h.edit("scripts/b.gsc", "\tself notify( \"go_now\" );\n", "");
    h.edit(
        "scripts/b.gsc",
        "function go()\n{\n",
        "function go()\n{\n\tself notify( \"go_\" + 1 );\n",
    );
    h.edit(
        "scripts/b.gsc",
        "\tself thread a::listen();\n",
        "\ta::listen();\n",
    );
    // Library files.
    h.edit(
        "lib/lib.gsc",
        "function lib_fn()\n{\n}",
        "function lib_fn()\n{\n\tnowait_lib();\n}",
    );
    h.edit("lib/lib.gsc", "self.lib_field = 1;", "self.foo_bar = 1;");
    // Files added and removed.
    let main = 0;
    let f = h.ws.add_file(
        main,
        PathBuf::from("mod/scripts/e.gsc"),
        "scripts/e.gsc",
        "#namespace e;\nfunction lib_fn() { wait 1; }\n",
    );
    h.compare("e.gsc added");
    h.ws.remove_file(f);
    h.compare("e.gsc removed");
    assert!(
        h.partial > 0,
        "some updates re-checked only part of the files"
    );
}

/// xorshift64*: deterministic, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const SNIPPETS: &[&str] = &[
    "\twait 1;\n",
    "\tb::maybe();\n",
    "\tmaybe();\n",
    "\ta::waits();\n",
    "\tself thread spin();\n",
    "\tx = self.foo_bar;\n",
    "\tself.foo_bar = 1;\n",
    "\tself.other_field = 1;\n",
    "\ty = self.other_field;\n",
    "\tself notify( \"go_now\" );\n",
    "\tself waittill( \"go_nwo\" );\n",
    "\tself endon( \"stop_it\" );\n",
    "\tlevel notify( \"lvl_\" + x );\n",
    "\tWAITM;\n",
    "\tNOWAITM;\n",
    "\tspawn( 1 );\n",
    "\tlib_fn();\n",
    "function extra_fn()\n{\n\twait 1;\n}\n",
    "function maybe()\n{\n}\n",
    "function lib_fn()\n{\n\twhile ( true ) { maybe(); }\n}\n",
    "#define WAITM wait 2\n",
    "#using scripts\\d;\n",
    "#namespace a;\n",
    "\twhile ( true )\n\t{\n\t\tmaybe();\n\t}\n",
    "\twhile ( true )\n\t{\n\t\tlib_fn();\n\t}\n",
    "\tself waittill( \"go_now\", a, b );\n",
    "\tself notify( \"go_now\", 1 );\n",
    "\tlevel waittill( \"stop_it\" );\n",
    "\tself notify( \"7\" );\n",
    "\tself waittill( \"7\" );\n",
    "\tself waittill( \"go 2\" );\n",
    "\tself waittill_any( \"go_now\", \"stop_it\" );\n",
    "\tplayer thread listen();\n",
    "\tplayer thread spin();\n",
    "\tthread listen();\n",
    "\tlisten();\n",
    "\tlevel notify( \"go_noww\" );\n",
    "}\n",
    "{\n",
];

#[test]
fn random_edits_match_a_full_check() {
    for seed in [
        0x9E37_79B9_7F4A_7C15,
        0x2545_F491_4F6C_DD1D,
        0xDEAD_BEEF_1234_5678,
    ] {
        random_edits(seed);
    }
}

fn random_edits(seed: u64) {
    let mut h = Harness::new();
    let mut rng = Rng(seed);
    let rels: Vec<String> =
        h.ws.files()
            .map(|f| h.ws.file(f).meta.rel.clone())
            .collect();
    for step in 0..400 {
        let rel = &rels[rng.below(rels.len())];
        let f = h.file(rel);
        let text = h.ws.file(f).text().to_owned();
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let at = rng.below(lines.len() + 1);
        let mut out: Vec<&str> = lines.clone();
        let what = if rng.below(3) == 0 && !lines.is_empty() {
            let i = at.min(lines.len() - 1);
            out.remove(i);
            format!("delete line {i} of {rel}")
        } else {
            let s = SNIPPETS[rng.below(SNIPPETS.len())];
            out.insert(at, s);
            format!("insert {s:?} at line {at} of {rel}")
        };
        h.ws.set_text(f, &out.concat());
        h.compare(&format!("seed {seed:x} step {step}: {what}"));
    }
    assert!(h.partial > 50, "partial re-checks: {}", h.partial);
    for mode in ["full", "renumbered", "in place", "cone"] {
        assert!(
            h.modes.contains_key(mode),
            "{mode} never ran: {:?}",
            h.modes
        );
    }
}
