//! `gruff.toml` or `gsc.toml` (or `[tool.gruff]` / `[tool.gsc]` in a `pyproject.toml`): the
//! project settings `gruff check` reads. `gruff.toml` wins over `gsc.toml` in the same directory.
//! The keys (`game`, `select`, `extend-select`, `ignore`, `preview`, `per-file-ignores`,
//! `library`, `hash_packages`, `game-mode`, `game-modes`) are accepted at the top level and in a `[check]`
//! table; see `docs/rules/README.md`.

use std::path::{Path, PathBuf};

use gsc_sema::ModeOverrides;

use crate::select::{parse_list, Layer, Selector};

/// File names searched for, in order, in each directory from the start upwards. The `gsc` names
/// are the older ones, still read.
pub const CONFIG_FILES: &[&str] = &[
    "gruff.toml",
    ".gruff.toml",
    "gsc.toml",
    ".gsc.toml",
    "pyproject.toml",
];

/// The `[tool.*]` tables of a `pyproject.toml` read as the configuration, preferred first.
pub const PYPROJECT_TABLES: &[&str] = &["gruff", "gsc"];

/// Settings read from a configuration file.
#[derive(Clone, Debug, Default)]
pub struct Config {
    /// The file it came from.
    pub path: Option<PathBuf>,
    /// The directory per-file-ignore patterns are relative to.
    pub root: PathBuf,
    pub game: Option<String>,
    pub preview: Option<bool>,
    pub layer: Layer,
    /// `(glob, selectors)`, in file order.
    pub per_file_ignores: Vec<(String, Vec<Selector>)>,
    /// Library roots (the game's own scripts): read for name resolution, never reported on.
    /// Relative paths are relative to the configuration file.
    pub library: Vec<PathBuf>,
    /// Hash packages (files or directories) naming the game's hashed identifiers. Relative
    /// paths are relative to the configuration file.
    pub hash_packages: Vec<PathBuf>,
    /// The game mode of the project's files: `game-mode` (all of them) and `[game-modes]`
    /// (glob = mode),
    /// over the game's path conventions.
    pub modes: ModeOverrides,
    /// `[[hash-source]]`: where `gruff hash-package fetch` may look for name lists. Nothing
    /// reads these except that command, and only when it is run.
    pub hash_sources: Vec<HashSource>,
}

/// One place `gruff hash-package fetch` can get names from. A git repository and the files to
/// take out of it; no other transport, so the only thing that touches the network is `git`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HashSource {
    /// Short name: the clone's folder and the package's file name.
    pub name: String,
    /// Repository URL, cloned shallow and then updated in place.
    pub git: String,
    /// Paths inside the repository, relative, each a file to turn into a package.
    pub paths: Vec<String>,
    /// `game` of the packages written (profile ids, comma-separated); absent: any game.
    pub game: Option<String>,
    /// `family` of the packages written; absent: every family of the game.
    pub family: Option<String>,
    /// Licence of the names, copied into the package header. Say `local only` when they may not
    /// be redistributed, which is what names recovered from a game's files are.
    pub licence: Option<String>,
    /// What a line of those files holds: `names` (one name) or `pairs` (a value, then a name).
    pub columns: Option<String>,
    /// What separates the columns of a `pairs` file: `tab` (the default) or `comma`.
    pub separator: Option<String>,
}

/// The nearest configuration file at or above `start`. A `pyproject.toml` counts only when it has
/// a `[tool.gruff]` or `[tool.gsc]` table.
pub fn find(start: &Path) -> Option<PathBuf> {
    let start = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    let start = std::path::absolute(&start).unwrap_or(start);
    for dir in start.ancestors() {
        for name in CONFIG_FILES {
            let p = dir.join(name);
            if !p.is_file() {
                continue;
            }
            if *name == "pyproject.toml" {
                let has = std::fs::read_to_string(&p)
                    .ok()
                    .and_then(|t| t.parse::<toml::Table>().ok())
                    .is_some_and(|t| tool_table(&t).is_some());
                if !has {
                    continue;
                }
            }
            return Some(p);
        }
    }
    None
}

/// The `[tool.gruff]` (else `[tool.gsc]`) table of a `pyproject.toml`.
pub fn tool_table(t: &toml::Table) -> Option<&toml::Table> {
    let tool = t.get("tool")?.as_table()?;
    PYPROJECT_TABLES
        .iter()
        .find_map(|k| tool.get(*k).and_then(|v| v.as_table()))
}

/// Read a configuration file.
pub fn load(path: &Path) -> Result<Config, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut c = from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    c.path = Some(path.to_path_buf());
    c.root = path.parent().map(Path::to_path_buf).unwrap_or_default();
    c.library = c.library.iter().map(|l| c.root.join(l)).collect();
    c.hash_packages = c.hash_packages.iter().map(|l| c.root.join(l)).collect();
    Ok(c)
}

/// Parse configuration text (a `gruff.toml` or `gsc.toml`, or a file with a `[tool.gruff]` or
/// `[tool.gruff]` table).
pub fn from_str(text: &str) -> Result<Config, String> {
    let doc: toml::Table = text.parse().map_err(|e| format!("invalid TOML: {e}"))?;
    let base = tool_table(&doc).unwrap_or(&doc);
    let mut c = Config::default();
    apply(&mut c, base)?;
    if let Some(check) = base.get("check").and_then(|v| v.as_table()) {
        apply(&mut c, check)?;
    }
    Ok(c)
}

fn strings(t: &toml::Table, keys: &[&str]) -> Result<Option<Vec<String>>, String> {
    let Some((key, v)) = keys.iter().find_map(|k| t.get(*k).map(|v| (*k, v))) else {
        return Ok(None);
    };
    let bad = || format!("`{key}` must be a list of strings");
    let arr = v.as_array().ok_or_else(bad)?;
    arr.iter()
        .map(|x| x.as_str().map(str::to_owned).ok_or_else(bad))
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn apply(c: &mut Config, t: &toml::Table) -> Result<(), String> {
    if let Some(g) = t.get("game") {
        c.game = Some(g.as_str().ok_or("`game` must be a string")?.to_owned());
    }
    if let Some(p) = t.get("preview") {
        c.preview = Some(p.as_bool().ok_or("`preview` must be true or false")?);
    }
    if let Some(s) = strings(t, &["select"])? {
        c.layer.select = Some(parse_list(&s)?);
    }
    if let Some(s) = strings(t, &["extend-select", "extend_select"])? {
        c.layer.extend_select.extend(parse_list(&s)?);
    }
    if let Some(s) = strings(t, &["ignore"])? {
        c.layer.ignore.extend(parse_list(&s)?);
    }
    if let Some(s) = strings(t, &["library"])? {
        c.library.extend(s.into_iter().map(PathBuf::from));
    }
    if let Some(s) = strings(t, &["hash_packages", "hash-packages"])? {
        c.hash_packages.extend(s.into_iter().map(PathBuf::from));
    }
    if let Some(v) = t.get("hash-source").or_else(|| t.get("hash_source")) {
        let list = v
            .as_array()
            .ok_or("`hash-source` must be a list of tables (`[[hash-source]]`)")?;
        for item in list {
            let s = item
                .as_table()
                .ok_or("every `[[hash-source]]` must be a table")?;
            let text = |k: &str| -> Result<Option<String>, String> {
                match s.get(k) {
                    None => Ok(None),
                    Some(v) => v
                        .as_str()
                        .map(|v| Some(v.to_owned()))
                        .ok_or_else(|| format!("`hash-source.{k}` must be a string")),
                }
            };
            let name = text("name")?.ok_or("every `[[hash-source]]` needs a `name`")?;
            let git = text("git")?.ok_or_else(|| {
                format!("`[[hash-source]]` `{name}` needs a `git` repository: that is the only way it fetches")
            })?;
            let paths = strings(s, &["paths"])?.unwrap_or_default();
            if paths.is_empty() {
                return Err(format!(
                    "`[[hash-source]]` `{name}` needs `paths`: which files of the repository hold names"
                ));
            }
            c.hash_sources.push(HashSource {
                name,
                git,
                paths,
                game: text("game")?,
                family: text("family")?,
                licence: text("licence")?,
                columns: text("columns")?,
                separator: text("separator")?,
            });
        }
    }
    // `mode` (top level) is the language server's compiler mode (retail or community).
    if let Some(m) = t.get("game-mode").or_else(|| t.get("game_mode")) {
        let m = m
            .as_str()
            .ok_or("`game-mode` must be a string (sp, mp, zm or any)")?;
        c.modes.all = Some(gsc_sema::modes::parse_setting(m)?);
    }
    if let Some(v) = t.get("game-modes").or_else(|| t.get("game_modes")) {
        let table = v
            .as_table()
            .ok_or("`game-modes` must be a table of glob = mode (sp, mp, zm or any)")?;
        for (glob, m) in table {
            let m = m.as_str().ok_or("`game-modes` values must be strings")?;
            c.modes
                .globs
                .push((glob.clone(), gsc_sema::modes::parse_setting(m)?));
        }
    }
    let pfi = t
        .get("per-file-ignores")
        .or_else(|| t.get("per_file_ignores"));
    if let Some(v) = pfi {
        let table = v
            .as_table()
            .ok_or("`per-file-ignores` must be a table of glob = [codes]")?;
        for (glob, codes) in table {
            let list: Vec<String> = codes
                .as_array()
                .ok_or("`per-file-ignores` values must be lists of codes")?
                .iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect();
            c.per_file_ignores.push((glob.clone(), parse_list(&list)?));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_top_level_and_check_table() {
        let c = from_str(
            r#"
game = "t7"
ignore = ["B002"]
library = ["../game/scripts"]
hash_packages = ["names/t7.tsv"]
[check]
select = ["B", "unknown-builtin"]
preview = true
[check.per-file-ignores]
"*_dev.gsc" = ["L001"]
"#,
        )
        .unwrap();
        assert_eq!(c.game.as_deref(), Some("t7"));
        assert_eq!(c.preview, Some(true));
        assert_eq!(c.layer.select.as_ref().unwrap().len(), 2);
        assert_eq!(c.layer.ignore.len(), 1);
        assert_eq!(c.per_file_ignores[0].0, "*_dev.gsc");
        assert_eq!(c.library, vec![PathBuf::from("../game/scripts")]);
        assert_eq!(c.hash_packages, vec![PathBuf::from("names/t7.tsv")]);
    }

    #[test]
    fn reads_modes() {
        let c = from_str(
            "mode = 'retail'\ngame-mode = 'zm'\n[game-modes]\n'_mp_*' = 'mp'\n'*.csc' = 'any'\n",
        )
        .unwrap();
        let m = |p| c.modes.mode_of(p);
        assert_eq!(c.modes.all, Some(Some(gsc_sema::Mode::Zm)));
        assert_eq!(m("maps/_mp_x.gsc"), Some(Some(gsc_sema::Mode::Mp)));
        assert_eq!(m("shared/x.csc"), Some(None));
        assert_eq!(m("maps/x.gsc"), Some(Some(gsc_sema::Mode::Zm)));
        assert!(from_str("game-mode = 'coop'").is_err());
        assert!(from_str("[game-modes]\nx = 1").is_err());
        let c = from_str("[check]\ngame_mode = 'sp'\n").unwrap();
        assert_eq!(c.modes.all, Some(Some(gsc_sema::Mode::Sp)));
    }

    #[test]
    fn reads_tool_table() {
        let c =
            from_str("[tool.gsc]\ngame = \"t6\"\n[tool.gsc.check]\nignore = [\"L\"]\n").unwrap();
        assert_eq!(c.game.as_deref(), Some("t6"));
        assert_eq!(c.layer.ignore.len(), 1);
    }

    #[test]
    fn finds_the_nearest_file() {
        let root = std::env::temp_dir().join(format!("gsc_check_cfg_{}", std::process::id()));
        let deep = root.join("a").join("b");
        std::fs::create_dir_all(&deep).unwrap();
        // A pyproject.toml without [tool.gruff] does not count.
        std::fs::write(root.join("a").join("pyproject.toml"), "[tool.ruff]\n").unwrap();
        std::fs::write(root.join("gsc.toml"), "game = \"t7\"\n").unwrap();
        let found = find(&deep).unwrap();
        assert!(found.ends_with("gsc.toml"), "{}", found.display());
        std::fs::write(
            root.join("a").join("pyproject.toml"),
            "[tool.gsc]\ngame = \"t6\"\n",
        )
        .unwrap();
        let found = find(&deep).unwrap();
        assert!(found.ends_with("pyproject.toml"), "{}", found.display());
        assert_eq!(load(&found).unwrap().game.as_deref(), Some("t6"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn gruff_toml_wins_over_gsc_toml() {
        let root = std::env::temp_dir().join(format!("gsc_check_cfg_gruff_{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("gsc.toml"), "game = \"t7\"\n").unwrap();
        assert!(find(&root).unwrap().ends_with("gsc.toml"));
        std::fs::write(root.join("gruff.toml"), "game = \"t6\"\n").unwrap();
        let found = find(&root).unwrap();
        assert!(found.ends_with("gruff.toml"), "{}", found.display());
        assert_eq!(load(&found).unwrap().game.as_deref(), Some("t6"));
        let _ = std::fs::remove_dir_all(&root);
        let c = from_str("[tool.gruff]\ngame = \"t5\"\n[tool.gsc]\ngame = \"t4\"\n").unwrap();
        assert_eq!(c.game.as_deref(), Some("t5"));
    }

    #[test]
    fn rejects_bad_selectors() {
        assert!(from_str("select = [\"nope\"]").is_err());
        assert!(from_str("select = \"B\"").is_err());
    }
}
