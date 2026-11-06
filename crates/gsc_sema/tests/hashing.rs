//! Names through hashes: a definition the decompiler named by hash meets a call written by name,
//! hash packages name what is left, and the IDE shows `name (#hash)`. Hand-written fixtures
//! with made-up names, no game code.

use std::sync::Arc;

use gsc_hash::{Dictionary, Game, Package, Role, Scheme};
use gsc_sema::{BareCall, Catalogue, Config, How, Profile, Resolution, Root, Workspace};
use gsc_syntax::{Dialect, TextSize};

fn scheme(game: Game) -> Scheme {
    Scheme::for_game(game).unwrap()
}

fn hex(game: Game, role: Role, name: &str) -> String {
    format!("{:x}", scheme(game).key(role, name).unwrap())
}

fn profile(game: Game, names: Option<Arc<Dictionary>>) -> Profile {
    let cat = Catalogue::from_toml_str(&format!(
        "game = \"fixture\"\nhash_family = \"{}\"\n[[builtin]]\nname = \"spawnstruct\"\nvm = \"gsc\"\nkind = \"function\"\nmin_args = 0\nmax_args = 0\n",
        scheme(game).family(Role::Function).unwrap().id
    ))
    .unwrap();
    Profile::new(Dialect::superset(), Some(cat), None)
        .with_config(Config::names(
            Resolution::Namespace,
            BareCall::BuiltinFirst,
            true,
        ))
        .with_scheme(Some(scheme(game)))
        .with_names(names)
}

fn ws(game: Game, names: Option<Arc<Dictionary>>, files: &[(&str, String)]) -> Workspace {
    let mut ws = Workspace::new(profile(game, names)).with_jobs(1);
    let root = ws.add_root(Root::new("."));
    for (path, text) in files {
        ws.add_file(root, path.into(), path, text);
    }
    ws
}

fn hows(ws: &Workspace, f: gsc_sema::FileId) -> Vec<(String, How)> {
    ws.resolution(f)
        .refs
        .iter()
        .map(|r| (r.callee.clone(), r.how))
        .collect()
}

#[test]
fn hashed_definitions_meet_named_calls_and_the_reverse() {
    let g = Game::T8;
    let fn_h = hex(g, Role::Function, "do_thing");
    let ns_h = hex(g, Role::Namespace, "my_util");
    let other = hex(g, Role::Function, "other_thing");
    let lib = format!(
        "#namespace my_util;\nfunction function_{fn_h}() {{}}\nfunction other_thing() {{}}\n"
    );
    let user = format!(
        "#using scripts\\my_util;\n#namespace user;\nfunction main() {{\n    my_util::do_thing();\n    namespace_{ns_h}::function_{fn_h}();\n    namespace_{ns_h}::function_{other}();\n    function_{}();\n}}\n",
        hex(g, Role::Function, "spawnstruct")
    );
    let ws = ws(
        g,
        None,
        &[("scripts/my_util.gsc", lib), ("scripts/user.gsc", user)],
    );
    let user = ws.files().nth(1).unwrap();
    let got = hows(&ws, user);
    assert_eq!(got[0].1, How::Qualified, "{got:?}");
    assert_eq!(got[1].1, How::Qualified, "{got:?}");
    // The callee is the definition's spelling, whichever spelling the call used.
    assert_eq!(got[2], ("my_util::other_thing".to_owned(), How::Qualified));
    assert_eq!(got[0].0, format!("my_util::function_{fn_h}"));
    assert_eq!(got[0].0, got[1].0);
    // A builtin spelled by hash is the builtin.
    assert_eq!(got[3].1, How::Builtin, "{got:?}");
    // B001's question: is there a script function of that name, by hash?
    assert_eq!(ws.callables_matching(user, "do_thing").len(), 1);
    assert!(ws.callables_named("do_thing").is_empty());
}

#[test]
fn script_names_by_hash_reach_clear_paths() {
    let g = Game::Bo7;
    let s = scheme(g);
    let h = s
        .family(Role::Script)
        .unwrap()
        .hash("scripts/common/thing.gsc")
        & gsc_hash::MASK60;
    let a = "function helper() {}\n".to_owned();
    let b = format!("#using script_{h:x};\nfunction main() {{ helper(); }}\n");
    let c = "#using scripts\\common\\other;\nfunction main2() { other_fn(); }\n".to_owned();
    let other_h = s
        .family(Role::Script)
        .unwrap()
        .hash("scripts/common/other.gsc")
        & gsc_hash::MASK60;
    let d = "function other_fn() {}\n".to_owned();
    let ws = ws(
        g,
        None,
        &[
            ("scripts/common/thing.gsc", a),
            ("b.gsc", b),
            ("c.gsc", c),
            (&format!("script_{other_h:x}.gsc"), d),
        ],
    );
    let ids: Vec<_> = ws.files().collect();
    assert!(ws.resolution(ids[1]).imports[0].is_some());
    assert_eq!(hows(&ws, ids[1])[0].1, How::Using);
    assert_eq!(ws.resolution(ids[2]).imports[0], Some(ids[3]));
    assert_eq!(hows(&ws, ids[2])[0].1, How::Using);
}

#[test]
fn packages_name_hashes_in_hover_completion_and_messages() {
    let g = Game::Bo7;
    let fn_h = hex(g, Role::Function, "spawn_wave");
    let unknown = "function_1234567890abc";
    let pkg = Package::parse(
        "# gsc-hash-package: 1\n# game: t11\n# source: test\n# licence: CC0-1.0\nspawn_wave\n",
    )
    .unwrap();
    let mut d = Dictionary::new(scheme(g));
    d.add_package(&pkg);
    let text = format!(
        "function function_{fn_h}() {{}}\nfunction main() {{ function_{fn_h}(); {unknown}(); }}\n"
    );
    let ws = ws(g, Some(Arc::new(d)), &[("a.gsc", text.clone())]);
    let f = ws.files().next().unwrap();
    let spelled = format!("function_{fn_h}");
    assert_eq!(
        ws.display_name(f, Role::Function, &spelled),
        format!("spawn_wave (#{fn_h})")
    );
    assert_eq!(
        ws.display_name(f, Role::Function, unknown),
        "#1234567890abc"
    );
    assert_eq!(ws.display_name(f, Role::Function, "main"), "main");
    let at = TextSize::from(text.rfind(&spelled).unwrap() as u32 + 2);
    let h = ws.hover(f, at).unwrap();
    assert!(
        h.signature.contains(&format!("spawn_wave (#{fn_h})")),
        "{h:?}"
    );
    assert!(
        h.detail.iter().any(|l| l.contains("hash packages")),
        "{h:?}"
    );
    let msg = ws.display_message(f, &format!("call to `{unknown}` and `{spelled}`"));
    assert_eq!(
        msg,
        format!("call to `#1234567890abc` and `spawn_wave (#{fn_h})`")
    );
    // Completion offers the recovered name.
    let at = TextSize::from(text.find("{ function_").unwrap() as u32 + 2);
    let items = ws.completions(f, at);
    assert!(items.iter().any(|i| i.label == "spawn_wave"), "{items:?}");
}

#[test]
fn workspace_names_name_hashed_definitions() {
    // JUP: the definition by hash, a call by name elsewhere: hover names the definition.
    let g = Game::Jup;
    let fn_h = hex(g, Role::Function, "getteamdata");
    let a = format!("#namespace shared;\nfunction function_{fn_h}() {{}}\n");
    let b = "#namespace shared;\nfunction main() { getteamdata(); }\n".to_owned();
    let ws = ws(g, None, &[("a.gsc", a), ("b.gsc", b)]);
    let ids: Vec<_> = ws.files().collect();
    assert_eq!(hows(&ws, ids[1])[0].1, How::Namespace);
    let (name, src) = ws
        .recover(ids[0], Role::Function, &format!("function_{fn_h}"))
        .unwrap();
    assert_eq!(
        (name.as_str(), src),
        ("getteamdata", gsc_sema::NameSource::Workspace)
    );
    let cov = ws.hash_coverage();
    assert_eq!(
        cov.count(Some(Role::Function), gsc_sema::NameSource::Workspace),
        1
    );
}
