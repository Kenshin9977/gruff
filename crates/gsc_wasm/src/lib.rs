//! gruff for the web: parser, checker, formatter and hash families in WebAssembly.
//!
//! An [`Engine`] holds one game's data (profile, builtin and event catalogues), passed in as TOML
//! or, for BO3 (`t7`), embedded at build time when gruff-data was found (`build.rs`). Each call
//! takes one file's text and returns JSON; positions are byte offsets, 1-based line and column,
//! and UTF-16 offsets. Single file only: no file system, no threads, so names from other files
//! (`#using`) do not resolve. See docs/wasm.md.

mod json;

use std::path::PathBuf;

use gsc_check::select::{self, Layer, Selection};
use gsc_check::{Applicability, Diagnostic, Options};
use gsc_sema::{Catalogue, Decompiled, EventCatalogue, Profile, Root, Workspace};
use gsc_syntax::{check_dialect, looks_decompiled, Dialect, NodeOrToken, SyntaxError, SyntaxNode};
use wasm_bindgen::prelude::*;

use json::{Json, Positions};

#[cfg(gsc_embedded_t7)]
mod embedded {
    pub const PROFILE: &str = include_str!(concat!(env!("OUT_DIR"), "/t7_profile.toml"));
    pub const BUILTINS: &str = include_str!(concat!(env!("OUT_DIR"), "/t7_builtins.toml"));
    pub const EVENTS: &str = include_str!(concat!(env!("OUT_DIR"), "/t7_events.toml"));
}

/// The games whose data this build embeds.
pub fn embedded_game_ids() -> &'static [&'static str] {
    if cfg!(gsc_embedded_t7) {
        &["t7"]
    } else {
        &[]
    }
}

/// The version of gruff this module was built from.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// The games with embedded data, as a JSON array of ids.
#[wasm_bindgen(js_name = embeddedGames)]
pub fn embedded_games() -> String {
    let mut j = Json::new();
    j.array(|j| {
        for id in embedded_game_ids() {
            j.item_str(id);
        }
    });
    j.finish()
}

/// Hash `text` with the family of id `family` (`t7.canon`, `t8.fnv64`, `t10.function`...): the
/// value as lower-case hex at the family's width.
#[wasm_bindgen]
pub fn hash(family: &str, text: &str) -> Result<String, JsError> {
    hash_hex(family, text).map_err(|e| JsError::new(&e))
}

/// [`hash`] without JavaScript errors.
pub fn hash_hex(family: &str, text: &str) -> Result<String, String> {
    let f = gsc_hash::family(family).ok_or_else(|| format!("unknown hash family {family:?}"))?;
    Ok(f.format(f.hash(text)))
}

/// Every hash family, as a JSON array of `{id, bits, kinds, status}`.
#[wasm_bindgen(js_name = hashFamilies)]
pub fn hash_families() -> String {
    let mut j = Json::new();
    j.array(|j| {
        for f in gsc_hash::families() {
            j.item_object(|j| {
                j.str("id", f.id);
                j.num("bits", f.bits.into());
                j.str("kinds", f.kinds);
                j.str("status", &format!("{:?}", f.status).to_ascii_lowercase());
            });
        }
    });
    j.finish()
}

/// One game's data, loaded once.
#[wasm_bindgen]
pub struct Engine {
    profile: Profile,
}

#[wasm_bindgen]
impl Engine {
    /// `game`: an id with embedded data (`t7`), or `superset` (every dialect, no data; the
    /// default). `profile`, `builtins`, `events`: the text of gruff-data's `profiles/<game>.toml`,
    /// `builtins/<game>.toml` and `events/<game>.toml`, which replace the embedded ones.
    #[wasm_bindgen(constructor)]
    pub fn new(
        game: Option<String>,
        profile: Option<String>,
        builtins: Option<String>,
        events: Option<String>,
    ) -> Result<Engine, JsError> {
        Engine::load(
            game.as_deref(),
            profile.as_deref(),
            builtins.as_deref(),
            events.as_deref(),
        )
        .map_err(|e| JsError::new(&e))
    }

    /// The dialect id (`t7`, `superset`...).
    #[wasm_bindgen(getter)]
    pub fn game(&self) -> String {
        self.profile.dialect.id.clone()
    }

    /// Whether a builtin catalogue is loaded (the B rules need one).
    #[wasm_bindgen(getter, js_name = hasBuiltins)]
    pub fn has_builtins(&self) -> bool {
        self.profile.builtins.is_some()
    }

    /// Parse: `{decompiled, errors, tree}`. `errors` holds syntax errors and dialect diagnostics;
    /// a node is `{kind, start, end, start16, end16, children}`, and with `tokens` the children
    /// include tokens (`{kind, start, end, text}`).
    pub fn parse(&self, text: &str, tokens: bool) -> String {
        self.parse_json(text, tokens)
    }

    /// The syntax tree as indented text, one node or token per line.
    #[wasm_bindgen(js_name = syntaxTree)]
    pub fn syntax_tree(&self, text: &str) -> String {
        let (dialect, _) = self.dialect_for(text);
        gsc_syntax::parse(text, dialect).debug_tree()
    }

    /// Lint one file: a JSON array of `{code, name, message, start, end, fix?}`. `path` names
    /// the file (`.csc` selects the client VM; default `main.gsc`); `select` is a comma-separated
    /// selection (`ALL`, `B`, `L001`) replacing the default one; `preview` adds preview rules.
    pub fn check(
        &self,
        text: &str,
        path: Option<String>,
        select: Option<String>,
        preview: bool,
    ) -> Result<String, JsError> {
        self.check_json(text, path.as_deref(), select.as_deref(), preview)
            .map_err(|e| JsError::new(&e))
    }

    /// Format one file: `{ok: true, changed, text}` or `{ok: false, error}`. `style` is `full`
    /// (the default) or `minimal`.
    pub fn format(&self, text: &str, style: Option<String>) -> String {
        self.format_json(text, style.as_deref())
    }
}

impl Engine {
    /// [`Engine::new`] without JavaScript errors.
    pub fn load(
        game: Option<&str>,
        profile: Option<&str>,
        builtins: Option<&str>,
        events: Option<&str>,
    ) -> Result<Engine, String> {
        let game = game.map(str::to_ascii_lowercase);
        let embedded = embedded_texts(game.as_deref());
        let dialect = match (profile, embedded, game.as_deref()) {
            (Some(text), _, _) => Dialect::from_toml_str(text).map_err(|e| e.0)?,
            (None, Some(e), _) => Dialect::from_toml_str(e.0).map_err(|e| e.0)?,
            (None, None, None | Some("superset")) => Dialect::superset(),
            (None, None, Some(g)) => {
                return Err(format!(
                    "no embedded data for {g:?}: pass its profile TOML (embedded: [{}])",
                    embedded_game_ids().join(", ")
                ))
            }
        };
        let builtins = builtins
            .or(embedded.map(|e| e.1))
            .map(|t| Catalogue::from_toml_str(t).map_err(|e| format!("builtins: {e}")))
            .transpose()?;
        let events = events
            .or(embedded.map(|e| e.2))
            .map(|t| EventCatalogue::from_toml_str(t).map_err(|e| format!("events: {e}")))
            .transpose()?;
        Ok(Engine {
            profile: Profile::new(dialect, builtins, None).with_events(events),
        })
    }

    fn dialect_for(&self, text: &str) -> (&Dialect, bool) {
        let decompiled = looks_decompiled(text);
        (self.profile.dialect(decompiled), decompiled)
    }

    /// [`Engine::parse`].
    pub fn parse_json(&self, text: &str, tokens: bool) -> String {
        let (dialect, decompiled) = self.dialect_for(text);
        let parse = gsc_syntax::parse(text, dialect);
        let root = parse.syntax();
        let pos = Positions::new(text);
        let mut errors: Vec<SyntaxError> = parse.errors().to_vec();
        errors.extend(check_dialect(&root, dialect));
        let mut j = Json::new();
        j.object(|j| {
            j.bool("decompiled", decompiled);
            j.key("errors");
            j.array(|j| {
                for e in &errors {
                    j.item_object(|j| {
                        j.str("code", e.code);
                        j.str("message", &e.message);
                        j.str("severity", if e.is_syntax() { "error" } else { "warning" });
                        pos.range(j, e.range.start(), e.range.end());
                    });
                }
            });
            j.key("tree");
            tree(j, &pos, &root, tokens);
        });
        j.finish()
    }

    /// [`Engine::check`] without JavaScript errors.
    pub fn check_json(
        &self,
        text: &str,
        path: Option<&str>,
        select: Option<&str>,
        preview: bool,
    ) -> Result<String, String> {
        let path = path.unwrap_or("main.gsc").replace('\\', "/");
        let select = select
            .map(|s| {
                let items: Vec<&str> = s
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect();
                select::parse_list(&items)
            })
            .transpose()?;
        let selection = Selection {
            layers: vec![Layer {
                select,
                ..Layer::default()
            }],
            preview,
        };
        let opts = Options {
            selection: &selection,
            per_file_ignores: &[],
            root: None,
        };
        // A fresh workspace per call: re-checking in place would free the old caches on a
        // thread, which wasm32-unknown-unknown lacks.
        let mut ws = Workspace::new(self.profile.clone())
            .with_jobs(1)
            .with_decompiled(Decompiled::Auto);
        let root = ws.add_root(Root::new("web"));
        ws.add_file(root, PathBuf::from("web").join(&path), &path, text);
        let results = gsc_check::check(&ws, &opts);
        let pos = Positions::new(text);
        let mut j = Json::new();
        j.array(|j| {
            for d in results.iter().flat_map(|r| &r.diagnostics) {
                j.item_object(|j| diagnostic(j, &pos, d));
            }
        });
        Ok(j.finish())
    }

    /// [`Engine::format`].
    pub fn format_json(&self, text: &str, style: Option<&str>) -> String {
        let (dialect, _) = self.dialect_for(text);
        let mut opts = gsc_fmt::Options::for_game(&self.profile.dialect.id);
        if let Some(s) = style.and_then(gsc_fmt::Style::from_name) {
            opts = opts.with_style(s);
        }
        let mut j = Json::new();
        j.object(|j| match gsc_fmt::format(text, dialect, &opts) {
            Ok(out) => {
                j.bool("ok", true);
                j.bool("changed", out != text);
                j.str("text", &out);
            }
            Err(e) => {
                j.bool("ok", false);
                j.str("error", &e.to_string());
            }
        });
        j.finish()
    }
}

/// The embedded profile, builtins and events of `game`.
fn embedded_texts(game: Option<&str>) -> Option<(&'static str, &'static str, &'static str)> {
    #[cfg(gsc_embedded_t7)]
    if matches!(game, Some("t7" | "bo3")) {
        return Some((embedded::PROFILE, embedded::BUILTINS, embedded::EVENTS));
    }
    let _ = game;
    None
}

fn diagnostic(j: &mut Json, pos: &Positions, d: &Diagnostic) {
    j.str("code", &d.code);
    j.str("name", d.name());
    j.str("message", &d.message);
    pos.range(j, d.range.start(), d.range.end());
    if let Some(fix) = &d.fix {
        j.key("fix");
        j.object(|j| {
            j.str("message", &fix.message);
            j.bool("safe", fix.applicability == Applicability::Safe);
            j.key("edits");
            j.array(|j| {
                for e in &fix.edits {
                    j.item_object(|j| {
                        pos.range(j, e.range.start(), e.range.end());
                        j.str("content", &e.content);
                    });
                }
            });
        });
    }
}

fn tree(j: &mut Json, pos: &Positions, node: &SyntaxNode, tokens: bool) {
    let r = node.text_range();
    j.object(|j| {
        j.str("kind", &format!("{:?}", node.kind()));
        j.num("start", u32::from(r.start()).into());
        j.num("end", u32::from(r.end()).into());
        j.num("start16", pos.utf16(r.start()).into());
        j.num("end16", pos.utf16(r.end()).into());
        j.key("children");
        j.array(|j| {
            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Node(n) => {
                        j.comma();
                        tree(j, pos, &n, tokens);
                    }
                    NodeOrToken::Token(t) if tokens => {
                        let r = t.text_range();
                        j.item_object(|j| {
                            j.str("kind", &format!("{:?}", t.kind()));
                            j.num("start", u32::from(r.start()).into());
                            j.num("end", u32::from(r.end()).into());
                            j.str("text", t.text());
                        });
                    }
                    NodeOrToken::Token(_) => {}
                }
            }
        });
    });
}

#[cfg(test)]
mod tests;
