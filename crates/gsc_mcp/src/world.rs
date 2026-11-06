//! The loaded workspace: settings and data resolved like the language server's, files kept in
//! step with the disk between tool calls, paths printed short.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use gsc_check::incremental::Session;
use gsc_check::{Options, Selection};
use gsc_lsp::project::{Project, Settings};
use gsc_sema::{FileId, Workspace};
use gsc_syntax::TextSize;

/// What `gruff mcp` serves.
#[derive(Clone, Debug)]
pub struct Config {
    /// Workspace folders (reported on). Empty means the working directory.
    pub folders: Vec<PathBuf>,
    /// Game, data directory, library roots and mode, as the language server takes them.
    pub settings: Settings,
    /// The least time between two scans of the disk for changed files.
    pub rescan: Duration,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            folders: Vec::new(),
            settings: Settings::default(),
            rescan: Duration::from_secs(2),
        }
    }
}

type Stamp = (Option<SystemTime>, u64);

fn stamp(path: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok(), m.len()))
}

pub struct World {
    pub project: Project,
    pub ws: Workspace,
    /// The first folder, absolute: paths under it print relative to it.
    pub base: PathBuf,
    pub load_ms: u128,
    pub load_errors: Vec<String>,
    /// The settings the world was loaded with.
    pub settings: Settings,
    stamps: HashMap<PathBuf, Stamp>,
    last_scan: Instant,
    rescan: Duration,
    session: Session,
}

impl World {
    pub fn load(config: &Config) -> World {
        let t0 = Instant::now();
        let folders: Vec<PathBuf> = if config.folders.is_empty() {
            vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))]
        } else {
            config.folders.clone()
        };
        let folders: Vec<PathBuf> = folders
            .iter()
            .map(|f| std::path::absolute(f).unwrap_or_else(|_| f.clone()))
            .collect();
        let project = Project::resolve(&config.settings, &folders);
        let mut ws = project.workspace();
        let report = ws.load();
        let stamps = ws
            .files()
            .filter_map(|f| {
                let p = ws.file(f).meta.path.clone();
                stamp(&p).map(|s| (p, s))
            })
            .collect();
        let mut w = World {
            project,
            ws,
            base: folders[0].clone(),
            load_ms: t0.elapsed().as_millis(),
            load_errors: report.errors,
            settings: config.settings.clone(),
            stamps,
            last_scan: Instant::now(),
            rescan: config.rescan,
            session: Session::new(),
        };
        // The first check needs every tree once; the library ones can go after it.
        w.check();
        w.ws.drop_library_trees();
        w.load_ms = t0.elapsed().as_millis();
        w
    }

    /// Pick up files changed, added or removed on disk since the last scan (at most every
    /// `rescan`). Returns how many files changed.
    pub fn refresh(&mut self) -> usize {
        if self.last_scan.elapsed() < self.rescan {
            return 0;
        }
        self.last_scan = Instant::now();
        let (found, _) = self.ws.discover();
        let mut seen = HashSet::new();
        let mut n = 0;
        for (root, path) in found {
            seen.insert(path.clone());
            let Some(st) = stamp(&path) else { continue };
            if self.stamps.get(&path) == Some(&st) {
                continue;
            }
            self.stamps.insert(path.clone(), st);
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            if gsc_syntax::is_binary(&bytes) {
                continue;
            }
            let text = gsc_syntax::SourceText::decode(&bytes).text;
            match self.ws.file_id(&path) {
                Some(f) if self.ws.file(f).text() == text => {}
                Some(f) => {
                    self.ws.set_text(f, &text);
                    n += 1;
                }
                None => {
                    let rel = self.ws.root(root).relative(&path);
                    self.ws.add_file(root, path.clone(), &rel, &text);
                    n += 1;
                }
            }
        }
        let gone: Vec<FileId> = self
            .ws
            .files()
            .filter(|&f| !seen.contains(&self.ws.file(f).meta.path))
            .collect();
        for f in gone {
            self.stamps.remove(&self.ws.file(f).meta.path.clone());
            self.ws.remove_file(f);
            n += 1;
        }
        n
    }

    /// Check the workspace (incrementally since the last call) and return the session.
    pub fn check(&mut self) -> &Session {
        let check = &self.project.check;
        let selection = Selection {
            layers: vec![check.layer.clone()],
            preview: check.preview.unwrap_or(false),
        };
        let opts = Options {
            selection: &selection,
            per_file_ignores: &check.per_file_ignores,
            root: check.path.as_ref().map(|_| check.root.as_path()),
        };
        self.session.update(&self.ws, &opts);
        &self.session
    }

    pub fn is_library(&self, f: FileId) -> bool {
        self.ws.is_library(f)
    }

    /// How a file is printed: relative to the first folder when under it, else
    /// `<root label>:<path in root>`.
    pub fn show(&self, f: FileId) -> String {
        let m = &self.ws.file(f).meta;
        if let Ok(rel) = m.path.strip_prefix(&self.base) {
            return gsc_sema::vfs::slash(rel);
        }
        format!("{}:{}", self.ws.root(m.root).label, m.rel)
    }

    /// `path:line` of an offset.
    pub fn at(&self, f: FileId, offset: TextSize) -> String {
        format!("{}:{}", self.show(f), self.line_col(f, offset).0)
    }

    /// 1-based line and column (in bytes) of an offset.
    pub fn line_col(&self, f: FileId, offset: TextSize) -> (usize, usize) {
        let text = self.ws.file(f).text();
        let o = usize::from(offset).min(text.len());
        let line = text.as_bytes()[..o].iter().filter(|&&b| b == b'\n').count() + 1;
        let start = text[..o].rfind('\n').map_or(0, |i| i + 1);
        (line, o - start + 1)
    }

    /// The trimmed source line holding `offset`, cut to `max` characters.
    pub fn snippet(&self, f: FileId, offset: TextSize, max: usize) -> String {
        let text = self.ws.file(f).text();
        let o = usize::from(offset).min(text.len());
        let start = text[..o].rfind('\n').map_or(0, |i| i + 1);
        let end = text[o..].find('\n').map_or(text.len(), |i| o + i);
        let line = text[start..end].trim();
        if line.chars().count() > max {
            let cut: String = line.chars().take(max).collect();
            format!("{cut}...")
        } else {
            line.to_owned()
        }
    }

    /// The file a path argument names: `label:rel`, a path relative to the first folder or
    /// absolute, or a unique suffix of a file's path.
    pub fn find_file(&self, arg: &str) -> Result<FileId, String> {
        let arg = arg.trim().replace('\\', "/");
        if let Some((label, rel)) = arg.split_once(':').filter(|(l, _)| l.len() > 1) {
            let hit = self.ws.files().find(|&f| {
                let m = &self.ws.file(f).meta;
                self.ws.root(m.root).label == label && m.rel.eq_ignore_ascii_case(rel)
            });
            if let Some(f) = hit {
                return Ok(f);
            }
        }
        let p = Path::new(&arg);
        let full = if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.base.join(p)
        };
        let key = gsc_lsp::convert::path_key(&full);
        if let Some(f) = self
            .ws
            .files()
            .find(|&f| gsc_lsp::convert::path_key(&self.ws.file(f).meta.path) == key)
        {
            return Ok(f);
        }
        let lower = arg.to_ascii_lowercase();
        let hits: Vec<FileId> = self
            .ws
            .files()
            .filter(|&f| {
                let d = self.ws.file(f).meta.display.to_ascii_lowercase();
                d.ends_with(&lower)
                    && (d.len() == lower.len() || d[..d.len() - lower.len()].ends_with('/'))
            })
            .collect();
        match hits.as_slice() {
            [f] => Ok(*f),
            [] => Err(format!("no file `{arg}` in the workspace")),
            many => Err(format!(
                "`{arg}` matches {} files, e.g. {} and {}: give more of the path",
                many.len(),
                self.show(many[0]),
                self.show(many[1])
            )),
        }
    }

    /// The offset of 1-based `line` and `column` (bytes; the first non-blank when `None`).
    pub fn offset(
        &self,
        f: FileId,
        line: usize,
        column: Option<usize>,
    ) -> Result<TextSize, String> {
        let text = self.ws.file(f).text();
        let mut start = 0;
        for _ in 1..line.max(1) {
            match text[start..].find('\n') {
                Some(i) => start += i + 1,
                None => return Err(format!("{} has fewer than {line} lines", self.show(f))),
            }
        }
        let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
        let col = match column {
            Some(c) => c.max(1) - 1,
            None => text[start..end]
                .find(|c: char| !c.is_whitespace())
                .unwrap_or(0),
        };
        let at = (start + col).min(end);
        Ok(TextSize::from(at as u32))
    }
}
