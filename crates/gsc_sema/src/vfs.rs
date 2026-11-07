//! Roots, file discovery and the names a file goes by.
//!
//! A workspace is a list of roots: the mod being edited plus, optionally, the game's own script
//! trees (the T7 mod-tools sources, a decompiled dump). Every file has a key, its path relative to
//! its root without extension, which `#using`/`#include` targets and IW path qualifiers name.

use std::path::{Path, PathBuf};

/// Script file extensions.
pub const EXTENSIONS: &[&str] = &["gsc", "csc", "gsh"];

/// Server or client VM.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Vm {
    Gsc,
    Csc,
}

impl Vm {
    pub fn label(self) -> &'static str {
        match self {
            Vm::Gsc => "GSC",
            Vm::Csc => "CSC",
        }
    }
}

/// The VM a file runs in, from its extension; `None` for `.gsh` headers, which both VMs insert.
pub fn vm_of(path: &str) -> Option<Vm> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".csc") {
        Some(Vm::Csc)
    } else if lower.ends_with(".gsh") {
        None
    } else {
        Some(Vm::Gsc)
    }
}

/// Can code of one VM reach code of the other? Headers go with both.
pub fn vm_compatible(a: Option<Vm>, b: Option<Vm>) -> bool {
    a.is_none() || b.is_none() || a == b
}

/// Does `path` have a script extension?
pub fn is_script(path: &Path) -> bool {
    path.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| EXTENSIONS.iter().any(|y| x.eq_ignore_ascii_case(y)))
}

/// Collect the script files under `path` (sorted, depth first), or `path` itself when it is a
/// file. A missing path is an error.
pub fn collect_files(path: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if path.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(path)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        entries.sort();
        for e in entries {
            if e.is_dir() {
                collect_files(&e, out)?;
            } else if is_script(&e) {
                out.push(e);
            }
        }
    } else if path.exists() {
        out.push(path.to_path_buf());
    } else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such file or directory",
        ));
    }
    Ok(())
}

/// The components of `p` joined with `/`.
pub fn slash(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// The lookup key of a root-relative path: no extension, lower case in case-insensitive games.
pub fn key_of(rel: &str, case_insensitive: bool) -> String {
    let mut key = rel.to_owned();
    if let Some(dot) = key.rfind('.') {
        if !key[dot..].contains('/') {
            key.truncate(dot);
        }
    }
    if case_insensitive {
        key.make_ascii_lowercase();
    }
    key
}

/// Normalise an import target or path qualifier: `/` separators, no quotes, lower case in
/// case-insensitive games. The extension stays (see [`strip_script_ext`]).
pub fn normalize_target(s: &str, case_insensitive: bool) -> String {
    let s = s.trim().trim_matches('"').replace('\\', "/");
    if case_insensitive {
        s.to_ascii_lowercase()
    } else {
        s
    }
}

/// `t` without a leading `/` and without a script extension, as a key.
pub fn strip_script_ext(target: &str) -> &str {
    let mut t = target.trim_start_matches('/');
    for ext in [".gsc", ".csc", ".gsh"] {
        if t.len() > ext.len() && t[t.len() - ext.len()..].eq_ignore_ascii_case(ext) {
            t = &t[..t.len() - ext.len()];
        }
    }
    t
}

/// A glob over `/`-separated paths: `*` and `?` stay inside one component, `**` spans any number
/// of them. A pattern without `/` matches the file name anywhere.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let pattern = pattern.replace('\\', "/");
    let path = path.replace('\\', "/");
    if !pattern.contains('/') {
        let name = path.rsplit('/').next().unwrap_or(&path);
        return match_component(pattern.as_bytes(), name.as_bytes());
    }
    let pat: Vec<&str> = pattern.trim_start_matches("./").split('/').collect();
    let comps: Vec<&str> = path.trim_start_matches("./").split('/').collect();
    match_parts(&pat, &comps)
}

fn match_parts(pat: &[&str], comps: &[&str]) -> bool {
    match pat.split_first() {
        None => comps.is_empty(),
        Some((&"**", rest)) => (0..=comps.len()).any(|i| match_parts(rest, &comps[i..])),
        Some((p, rest)) => comps.split_first().is_some_and(|(c, crest)| {
            match_component(p.as_bytes(), c.as_bytes()) && match_parts(rest, crest)
        }),
    }
}

fn match_component(p: &[u8], s: &[u8]) -> bool {
    match p.split_first() {
        None => s.is_empty(),
        Some((b'*', rest)) => (0..=s.len()).any(|i| match_component(rest, &s[i..])),
        Some((b'?', rest)) => !s.is_empty() && match_component(rest, &s[1..]),
        Some((c, rest)) => s
            .split_first()
            .is_some_and(|(d, srest)| c.eq_ignore_ascii_case(d) && match_component(rest, srest)),
    }
}

/// Index of a root in its workspace.
pub type RootId = usize;

/// A directory (or single file) of scripts.
#[derive(Clone, Debug)]
pub struct Root {
    pub path: PathBuf,
    /// Short name, prefixed to paths when a workspace has several roots.
    pub label: String,
    /// Library roots (game scripts) take part in name resolution but are not linted.
    pub library: bool,
    /// Index into the workspace's profiles.
    pub profile: usize,
}

impl Root {
    /// A root named after its last path component.
    pub fn new(path: impl Into<PathBuf>) -> Root {
        let path = path.into();
        let label = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "root".to_owned());
        Root {
            path,
            label,
            library: false,
            profile: 0,
        }
    }

    pub fn library(mut self, library: bool) -> Root {
        self.library = library;
        self
    }

    /// The path of `file` relative to this root, `/`-separated (the file name when the root is
    /// itself a file).
    pub fn relative(&self, file: &Path) -> String {
        if self.path.is_dir() {
            slash(file.strip_prefix(&self.path).unwrap_or(file))
        } else {
            file.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        }
    }
}

/// The names of one file.
#[derive(Clone, Debug)]
pub struct FileMeta {
    /// Where it is read from.
    pub path: PathBuf,
    /// The path as printed by `gruff check`: as collected, `/` separators.
    pub display: String,
    /// Relative to its root, `/` separators.
    pub rel: String,
    /// `rel` without extension (lower case in case-insensitive games).
    pub key: String,
    pub root: RootId,
    /// `None` for headers.
    pub vm: Option<Vm>,
}

impl FileMeta {
    pub fn new(path: PathBuf, rel: String, root: RootId, case_insensitive: bool) -> FileMeta {
        FileMeta {
            display: path.display().to_string().replace('\\', "/"),
            key: key_of(&rel, case_insensitive),
            vm: vm_of(&rel),
            path,
            rel,
            root,
        }
    }

    /// The last component of the key: the default namespace of a T7 file.
    pub fn stem(&self) -> &str {
        self.key.rsplit('/').next().unwrap_or(&self.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_targets() {
        assert_eq!(
            key_of("scripts/Shared/util.gsc", true),
            "scripts/shared/util"
        );
        assert_eq!(key_of("a.b/c", false), "a.b/c");
        assert_eq!(
            normalize_target("\"maps\\mp\\_utility\"", true),
            "maps/mp/_utility"
        );
        assert_eq!(strip_script_ext("/scripts/a.GSH"), "scripts/a");
        assert_eq!(vm_of("x.csc"), Some(Vm::Csc));
        assert_eq!(vm_of("x.gsh"), None);
        assert!(vm_compatible(None, Some(Vm::Csc)));
        assert!(!vm_compatible(Some(Vm::Gsc), Some(Vm::Csc)));
    }
}
