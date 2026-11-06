//! The loaded workspace plus the server's own file bookkeeping: path keys that ignore spelling,
//! the URIs the client uses for open files, and roots for files outside every folder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gsc_sema::{FileId, Root, RootId, Workspace};
use lsp_types::Uri;

use crate::convert::{path_key, path_to_uri};

pub struct Analysis {
    pub ws: Workspace,
    by_key: HashMap<String, FileId>,
    /// The URI the client opened a file with.
    uris: HashMap<FileId, Uri>,
}

fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

impl Analysis {
    pub fn new(ws: Workspace) -> Analysis {
        let by_key = ws
            .files()
            .map(|f| (path_key(&ws.file(f).meta.path), f))
            .collect();
        Analysis {
            ws,
            by_key,
            uris: HashMap::new(),
        }
    }

    pub fn file_id(&self, path: &Path) -> Option<FileId> {
        self.by_key.get(&path_key(path)).copied()
    }

    pub fn uri(&self, f: FileId) -> Uri {
        self.uris
            .get(&f)
            .cloned()
            .unwrap_or_else(|| path_to_uri(&self.ws.file(f).meta.path))
    }

    pub fn is_library(&self, f: FileId) -> bool {
        self.ws.is_library(f)
    }

    /// The root a new file at `path` belongs to and its path relative to it: the deepest root
    /// directory that contains it, else a new root of its own.
    fn root_for(&mut self, path: &Path) -> (RootId, String) {
        let full = slash(&std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()));
        let mut best: Option<(usize, RootId, String)> = None;
        for (i, r) in self.ws.roots().iter().enumerate() {
            if !r.path.is_dir() {
                continue;
            }
            let rp = slash(&std::path::absolute(&r.path).unwrap_or_else(|_| r.path.clone()));
            let rp = rp.trim_end_matches('/');
            let n = rp.len();
            let inside = full.len() > n
                && full.is_char_boundary(n)
                && full.as_bytes()[n] == b'/'
                && if cfg!(windows) {
                    full[..n].eq_ignore_ascii_case(rp)
                } else {
                    &full[..n] == rp
                };
            if inside && best.as_ref().is_none_or(|b| n > b.0) {
                best = Some((n, i, full[n + 1..].to_owned()));
            }
        }
        match best {
            Some((_, r, rel)) => (r, rel),
            None => {
                let r = self.ws.add_root(Root::new(path.to_path_buf()));
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                (r, name)
            }
        }
    }

    /// Make the workspace hold `text` for `path`, adding the file when it is new. Returns the
    /// file and whether anything changed.
    pub fn set_file(&mut self, path: &Path, text: &str) -> (FileId, bool) {
        if let Some(f) = self.file_id(path) {
            if self.ws.file(f).text() == text {
                return (f, false);
            }
            self.ws.set_text(f, text);
            return (f, true);
        }
        let (root, rel) = self.root_for(path);
        let f = self.ws.add_file(root, PathBuf::from(path), &rel, text);
        self.by_key.insert(path_key(path), f);
        (f, true)
    }

    /// The client opened `path` as `uri`.
    pub fn open(&mut self, uri: &Uri, path: &Path, text: &str) -> (FileId, bool) {
        let r = self.set_file(path, text);
        self.uris.insert(r.0, uri.clone());
        r
    }

    pub fn close(&mut self, f: FileId) {
        self.uris.remove(&f);
    }

    pub fn remove(&mut self, path: &Path) -> Option<FileId> {
        let f = self.by_key.remove(&path_key(path))?;
        self.uris.remove(&f);
        self.ws.remove_file(f);
        Some(f)
    }
}
