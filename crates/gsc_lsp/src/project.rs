//! What the server analyses: client settings, `gruff.toml`, the game (set, configured or
//! detected), engine data, and the workspace built from them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gsc_check::config::Config as CheckConfig;
use gsc_sema::data::toml_get::{self, strs};
use gsc_sema::{Catalogue, FieldCatalogue, Profile, Root, Workspace};
use gsc_syntax::{Dialect, Mode};
use serde::Deserialize;

/// The `gsc.*` settings of the client.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Game id or alias (`t7`, `bo3`...); empty or `auto` to read gruff.toml or detect it.
    pub game: Option<String>,
    /// The gruff-data directory (or its `profiles` folder).
    pub data_dir: Option<String>,
    /// Library roots: the game's own scripts, read for name resolution, never reported on.
    pub library_roots: Vec<String>,
    /// `retail` or `community`.
    pub mode: Option<String>,
    /// Hash packages (files or directories) naming the game's hashed identifiers; adds to
    /// `hash_packages` in gruff.toml and `$GSC_HASH_PACKAGES`.
    pub hash_packages: Vec<String>,
}

impl Settings {
    /// Read settings from `initializationOptions` or a `didChangeConfiguration` payload: either
    /// the `gruff` section itself or an object holding it (`gsc`, the older section name, is
    /// read too).
    pub fn from_json(v: &serde_json::Value) -> Settings {
        let v = v.get("gruff").or_else(|| v.get("gsc")).unwrap_or(v);
        serde_json::from_value(v.clone()).unwrap_or_default()
    }

    fn game(&self) -> Option<&str> {
        self.game
            .as_deref()
            .map(str::trim)
            .filter(|g| !g.is_empty() && !g.eq_ignore_ascii_case("auto"))
    }
}

/// One builtin's parameter types, from the catalogue's `param_types`; where it names no type,
/// the corpus measure (`corpus_param_types`) marked `~` (`~int`).
#[derive(Clone, Debug, Default)]
pub struct ParamTypes {
    /// `(lower-case name)` -> `(vm, kind, types)` per entry.
    pub by_name: HashMap<String, Vec<(String, String, Vec<String>)>>,
}

/// Does a catalogue type name a type (`int`, `any|string`), not only `?`, `any` or a raw tag?
fn names_a_type(t: &str) -> bool {
    t.split('|')
        .any(|x| !matches!(x, "?" | "any" | "unknown" | "undefined") && !x.starts_with("tag"))
}

/// `param_types`, completed per index by `corpus_param_types`.
fn merge_types(measured: &[String], corpus: &[String]) -> Vec<String> {
    (0..measured.len().max(corpus.len()))
        .map(|i| {
            let m = measured.get(i).map_or("?", String::as_str);
            match corpus.get(i) {
                Some(c) if !names_a_type(m) && names_a_type(c) => format!("~{c}"),
                _ => m.to_owned(),
            }
        })
        .collect()
}

impl ParamTypes {
    fn load(path: &Path) -> ParamTypes {
        match std::fs::read_to_string(path) {
            Ok(text) => ParamTypes::from_toml_str(&text),
            Err(_) => ParamTypes::default(),
        }
    }

    fn from_toml_str(text: &str) -> ParamTypes {
        let mut out = ParamTypes::default();
        let Ok(doc) = text.parse::<toml::Table>() else {
            return out;
        };
        let Some(list) = doc.get("builtin").and_then(|v| v.as_array()) else {
            return out;
        };
        for b in list {
            let Some(t) = b.as_table() else { continue };
            let Some(name) = t.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            let types = merge_types(&strs(t, "param_types"), &strs(t, "corpus_param_types"));
            if types.is_empty() {
                continue;
            }
            let s = |k: &str| toml_get::str(t, k).unwrap_or("").to_owned();
            out.by_name
                .entry(name.to_ascii_lowercase())
                .or_default()
                .push((s("vm"), s("kind"), types));
        }
        out
    }

    pub fn get(&self, name: &str, vm: &str, kind: &str) -> Option<&[String]> {
        self.by_name
            .get(&name.to_ascii_lowercase())?
            .iter()
            .find(|(v, k, _)| v == vm && k == kind)
            .map(|(_, _, t)| t.as_slice())
    }
}

/// Everything resolved before loading files.
#[derive(Clone, Debug)]
pub struct Project {
    /// Workspace folders: reported on.
    pub primary: Vec<PathBuf>,
    /// Game scripts: name resolution only.
    pub library: Vec<PathBuf>,
    pub dialect: Dialect,
    /// Where the game came from: `setting`, `gruff.toml`, `detected: ...` or `default`.
    pub game_source: String,
    pub catalogue: Option<Catalogue>,
    pub fields: Option<FieldCatalogue>,
    /// The engine's event catalogue (`gruff-data/events/<game>.toml`).
    pub events: Option<gsc_sema::EventCatalogue>,
    pub params: ParamTypes,
    pub check: CheckConfig,
    pub format: gsc_fmt::Options,
    /// Names from the hash packages, for a game that hashes identifiers.
    pub names: Option<std::sync::Arc<gsc_sema::Dictionary>>,
    /// Problems worth telling the user (missing data, bad gruff.toml).
    pub warnings: Vec<String>,
}

/// The non-empty entries of a path list setting, trimmed; relative ones are under `base`.
fn setting_paths(list: &[String], base: &Path) -> Vec<PathBuf> {
    list.iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| {
            let p = PathBuf::from(s);
            if p.is_absolute() {
                p
            } else {
                base.join(p)
            }
        })
        .collect()
}

/// The hash packages of the settings, gruff.toml and `$GSC_HASH_PACKAGES`, for a game that hashes
/// identifiers.
fn load_names(
    game: &str,
    settings: &Settings,
    check: &CheckConfig,
    base: &Path,
    warnings: &mut Vec<String>,
) -> Option<std::sync::Arc<gsc_sema::Dictionary>> {
    let scheme = gsc_sema::Scheme::for_profile(game)?;
    let mut paths = setting_paths(&settings.hash_packages, base);
    paths.extend(check.hash_packages.iter().cloned());
    if let Some(env) = std::env::var_os("GSC_HASH_PACKAGES") {
        paths.extend(std::env::split_paths(&env).filter(|p| !p.as_os_str().is_empty()));
    }
    if paths.is_empty() {
        return None;
    }
    let (names, report) = gsc_sema::load_packages(scheme, &paths);
    warnings.extend(report.errors.iter().map(|e| format!("hash package {e}")));
    (!names.is_empty()).then(|| std::sync::Arc::new(names))
}

/// The `profiles` directory of gruff-data: the setting, then `$GRUFF_DATA`, then a `gruff-data` folder
/// next to a workspace folder, the working directory or the executable, or above them.
pub fn profiles_dir(setting: Option<&str>, folders: &[PathBuf]) -> Option<PathBuf> {
    let setting = setting.map(str::trim).filter(|d| !d.is_empty());
    gsc_sema::data::find_profiles_dir(setting.map(std::path::Path::new), folders)
}

/// Guess the game of a folder: a game id in its path, else the import style and VMs of a sample
/// of its scripts (`#using` means T7 or later, `#include` with client scripts T4-T6, `#include`
/// alone the IW engine).
pub fn detect_game(folder: &Path) -> Option<(String, String)> {
    const IDS: &[(&str, &str)] = &[
        ("bo3", "t7"),
        ("t7", "t7"),
        ("bo4", "t8"),
        ("t8", "t8"),
        ("bocw", "t9"),
        ("t9", "t9"),
        ("bo2", "t6"),
        ("t6", "t6"),
        ("bo1", "t5"),
        ("t5", "t5"),
        ("waw", "t4"),
        ("t4", "t4"),
        ("iw3", "iw3"),
        ("cod4", "iw3"),
        ("iw4", "iw4"),
        ("iw4x", "iw4"),
        ("mw2", "iw4"),
        ("iw5", "iw5"),
        ("mw3", "iw5"),
        ("iw6", "iw6"),
        ("s1", "s1"),
        ("h1", "h1"),
    ];
    for comp in folder.components().rev() {
        let c = comp.as_os_str().to_string_lossy().to_ascii_lowercase();
        for part in c.split(|ch: char| !ch.is_ascii_alphanumeric()) {
            if let Some((_, id)) = IDS.iter().find(|(k, _)| *k == part) {
                return Some((id.to_string(), format!("folder name `{c}`")));
            }
        }
    }
    let mut files = Vec::new();
    let _ = gsc_sema::vfs::collect_files(folder, &mut files);
    if files.is_empty() {
        return None;
    }
    let csc = files
        .iter()
        .any(|f| f.extension().is_some_and(|x| x.eq_ignore_ascii_case("csc")));
    let (mut using, mut include) = (0, 0);
    for f in files.iter().take(200) {
        let Ok(bytes) = std::fs::read(f) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).to_ascii_lowercase();
        using += text.matches("#using ").count();
        include += text.matches("#include ").count();
    }
    if using > include {
        Some(("t7".to_owned(), "`#using` imports".to_owned()))
    } else if include > 0 && csc {
        Some((
            "t6".to_owned(),
            "`#include` imports and client scripts".to_owned(),
        ))
    } else if include > 0 {
        Some(("iw5".to_owned(), "`#include` imports".to_owned()))
    } else {
        None
    }
}

/// Read a key of the top level of a gruff.toml the check configuration does not know.
fn toml_str(path: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let doc: toml::Table = text.parse().ok()?;
    let base = gsc_check::config::tool_table(&doc).unwrap_or(&doc);
    base.get(key)?.as_str().map(str::to_owned)
}

fn mode_of(s: Option<&str>) -> Mode {
    s.and_then(|m| m.parse().ok()).unwrap_or_default()
}

impl Project {
    /// Resolve settings, gruff.toml (or gsc.toml) and data for the workspace `folders`.
    pub fn resolve(settings: &Settings, folders: &[PathBuf]) -> Project {
        let mut warnings = Vec::new();
        let cfg_path = folders.first().and_then(|f| gsc_check::config::find(f));
        let check = match &cfg_path {
            Some(p) => gsc_check::config::load(p).unwrap_or_else(|e| {
                let name = p.file_name().unwrap_or_default().to_string_lossy();
                warnings.push(format!("{name} ignored: {e}"));
                CheckConfig::default()
            }),
            None => CheckConfig::default(),
        };
        let toml_data_dir = cfg_path.as_deref().and_then(|p| {
            toml_str(p, "data-dir")
                .or_else(|| toml_str(p, "data_dir"))
                .map(|d| p.parent().unwrap_or(Path::new(".")).join(d))
        });
        let data_setting = settings
            .data_dir
            .clone()
            .filter(|d| !d.trim().is_empty())
            .or_else(|| toml_data_dir.map(|d| d.display().to_string()));
        let profiles = profiles_dir(data_setting.as_deref(), folders);

        let (game, game_source) = if let Some(g) = settings.game() {
            (Some(g.to_owned()), "setting".to_owned())
        } else if let Some(g) = check.game.clone() {
            let name = cfg_path.as_deref().and_then(Path::file_name);
            let name = name.map_or("gruff.toml".into(), |n| n.to_string_lossy());
            (Some(g), name.into_owned())
        } else if let Some((g, why)) = folders.iter().find_map(|f| detect_game(f)) {
            (Some(g), format!("detected from {why}"))
        } else {
            (None, "default".to_owned())
        };
        let mode = mode_of(
            settings.mode.as_deref().or(cfg_path
                .as_deref()
                .and_then(|p| toml_str(p, "mode"))
                .as_deref()),
        );

        let game_id = game.clone().unwrap_or_else(|| "t7".to_owned());
        let dialect = match (&profiles, game_id.as_str()) {
            (_, "superset") => Dialect::superset(),
            (Some(dir), id) => Dialect::find(dir, id).unwrap_or_else(|e| {
                warnings.push(format!("game `{id}`: {e}; reading files as the superset"));
                Dialect::superset()
            }),
            (None, _) => {
                warnings.push(
                    "cannot find gruff-data (set `gruff.dataDir` or GRUFF_DATA): reading files as the \
                     superset dialect, without builtins"
                        .to_owned(),
                );
                Dialect::superset()
            }
        };
        let dialect = dialect.with_mode(mode);

        let data_file = |kind: &str| {
            profiles
                .as_deref()
                .and_then(|d| gsc_sema::data::data_file(d, kind, &dialect.id))
        };
        let catalogue_path = profiles
            .as_deref()
            .and_then(|d| gsc_sema::data::catalogue_file(d, &dialect.id, game.as_deref()));
        let catalogue = catalogue_path
            .as_deref()
            .and_then(|p| Catalogue::load(p).map_err(|e| warnings.push(e)).ok());
        let params = catalogue_path
            .as_deref()
            .map(ParamTypes::load)
            .unwrap_or_default();
        let fields = data_file("fields")
            .and_then(|p| FieldCatalogue::load(&p).map_err(|e| warnings.push(e)).ok());
        let events = data_file("events").and_then(|p| {
            gsc_sema::EventCatalogue::load(&p)
                .map_err(|e| warnings.push(e))
                .ok()
        });
        if catalogue.is_none() && profiles.is_some() {
            warnings.push(format!(
                "no builtin catalogue for {}: builtin checks and completions are off",
                dialect.id
            ));
        }

        let fmt_cfg = cfg_path
            .as_deref()
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n == "gruff.toml" || n == "gsc.toml")
            })
            .map(|p| {
                gsc_fmt::FormatConfig::load(p).unwrap_or_else(|e| {
                    warnings.push(e.to_string());
                    gsc_fmt::FormatConfig::default()
                })
            })
            .unwrap_or_default();
        let format = fmt_cfg.apply(gsc_fmt::Options::for_game(&dialect.id));

        let base = folders.first().cloned().unwrap_or_default();
        let mut library = setting_paths(&settings.library_roots, &base);
        library.extend(check.library.iter().cloned());
        library.retain(|l| {
            let ok = l.exists();
            if !ok {
                warnings.push(format!("library root not found: {}", l.display()));
            }
            ok
        });
        library.dedup();
        let names = load_names(&dialect.id, settings, &check, &base, &mut warnings);

        Project {
            primary: folders.to_vec(),
            library,
            dialect,
            game_source,
            catalogue,
            fields,
            events,
            params,
            check,
            format,
            names,
            warnings,
        }
    }

    /// An empty workspace with this project's profile and roots.
    pub fn workspace(&self) -> Workspace {
        let profile = Profile::new(
            self.dialect.clone(),
            self.catalogue.clone(),
            self.fields.clone(),
        )
        .with_events(self.events.clone())
        .with_names(self.names.clone());
        let mut ws = Workspace::new(profile);
        for p in &self.primary {
            ws.add_root(Root::new(p.clone()));
        }
        for p in &self.library {
            ws.add_root(Root::new(p.clone()).library(true));
        }
        ws
    }

    /// How the game was chosen, for the log.
    pub fn describe(&self) -> String {
        format!(
            "game {} ({}, {}), {} folder(s), {} library root(s), builtins: {}, hash names: {}",
            self.dialect.id,
            self.game_source,
            self.dialect.mode(),
            self.primary.len(),
            self.library.len(),
            self.catalogue
                .as_ref()
                .map_or("none".to_owned(), |c| format!("{} entries", c.len())),
            self.names.as_ref().map_or(0, |d| d.len()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_settings_in_both_shapes() {
        let a = Settings::from_json(&serde_json::json!({
            "gsc": { "game": "t7", "libraryRoots": ["x"], "mode": "community" }
        }));
        let b = Settings::from_json(&serde_json::json!({
            "game": "t7", "libraryRoots": ["x"], "mode": "community"
        }));
        assert_eq!(a, b);
        assert_eq!(a.library_roots, vec!["x"]);
        assert_eq!(
            Settings::from_json(&serde_json::json!(null)),
            Settings::default()
        );
        let auto = Settings {
            game: Some("auto".into()),
            ..Settings::default()
        };
        assert_eq!(auto.game(), None);
    }

    #[test]
    fn detects_games_from_folders() {
        let root = std::env::temp_dir().join(format!("gsc_lsp_detect_{}", std::process::id()));
        let t7 = root.join("mymod");
        std::fs::create_dir_all(&t7).unwrap();
        std::fs::write(
            t7.join("a.gsc"),
            "#using scripts\\shared\\util_shared;\nfunction f() {}\n",
        )
        .unwrap();
        assert_eq!(detect_game(&t7).unwrap().0, "t7");
        let iw = root.join("other");
        std::fs::create_dir_all(&iw).unwrap();
        std::fs::write(iw.join("a.gsc"), "#include maps\\mp\\_utility;\nf() {}\n").unwrap();
        assert_eq!(detect_game(&iw).unwrap().0, "iw5");
        std::fs::write(iw.join("a.csc"), "#include clientscripts\\_utility;\n").unwrap();
        assert_eq!(detect_game(&iw).unwrap().0, "t6");
        assert_eq!(detect_game(&root.join("bo2-mod")).unwrap().0, "t6");
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod param_type_tests {
    use super::*;

    #[test]
    fn corpus_types_fill_the_gaps() {
        let p = ParamTypes::from_toml_str(
            r#"
[[builtin]]
name = "spawn"
vm = "gsc"
kind = "function"
param_types = ["string", "?"]
corpus_param_types = ["int", "vector", "int"]

[[builtin]]
name = "moveto"
vm = "gsc"
kind = "method"
corpus_param_types = ["vector", "?"]
"#,
        );
        let t = p.get("spawn", "gsc", "function").unwrap();
        assert_eq!(t, ["string", "~vector", "~int"]);
        assert_eq!(p.get("moveto", "gsc", "method").unwrap(), ["~vector", "?"]);
    }
}
