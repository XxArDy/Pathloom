//! Per-change history with revert: `.pathloom/history/<id>.json` = `{id, at, action, files: [{path, before, after}]}`.
//!
//! Hook: [`Project::log_action`] (every logged app / CLI / MCP action) calls `record_history`, which compares the
//! plan files (everything under `plan_dir`) with a baseline copy kept in `.pathloom/cache/history-base.json`
//! (cache is git-ignored in every project) and stores an entry for the files that differ, then refreshes the baseline.
//! No caller changes are needed. Limits of this design:
//! - Edits made outside Pathloom (an editor, `git pull`) between two logged actions are attributed to the next action.
//! - A project that never had a baseline (created before this feature) gets one at its first logged action; that
//!   first action has no entry. `init` writes the baseline, so new projects are covered from the start.
//! - Files over 2 MB or not valid UTF-8 are not tracked.
//! - The history folder is trimmed to about 20 MB by deleting the oldest entries.
//!
//! Every caller that changes plan files must log (or [`Project::checkpoint`]) right after, or its edit is
//! folded into the next entry and reverting that entry would undo it too.
//!
//! `revert` is allowed only when every file of the entry still equals its `after`; otherwise it fails with
//! `Error::HistoryConflict` and writes nothing. Entry paths outside `plan_dir` (absolute, `..`) fail with
//! `Error::OutsideProject`. A revert is logged like any action, so it can be reverted too.

use crate::error::{Error, Result};
use crate::fsutil::{FileLock, format_secs, now_secs, write_atomic};
use crate::log::LogEntry;
use crate::project::Project;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const HISTORY_DIR: &str = ".pathloom/history";
const BASELINE: &str = ".pathloom/cache/history-base.json";
const LOCK: &str = ".pathloom/cache/history.lock";
const MAX_HISTORY_BYTES: u64 = 20 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// One changed file; `None` = the file did not exist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryFile {
    /// Relative to the project root, `/` separators.
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub at: String,
    pub action: String,
    pub files: Vec<HistoryFile>,
}

type Snapshot = BTreeMap<String, String>;

fn walk(root: &Path, dir: &Path, out: &mut Snapshot) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(meta) = e.metadata() else { continue };
        if meta.is_dir() {
            walk(root, &p, out);
        } else if meta.len() <= MAX_FILE_BYTES
            && let (Ok(text), Ok(rel)) = (std::fs::read_to_string(&p), p.strip_prefix(root))
        {
            out.insert(rel.to_string_lossy().replace('\\', "/"), text);
        }
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 40 && id.chars().all(|c| c.is_ascii_alphanumeric())
}

impl Project {
    fn history_dir(&self) -> PathBuf {
        self.root.join(HISTORY_DIR)
    }

    fn plan_snapshot(&self) -> Snapshot {
        let mut s = Snapshot::new();
        walk(&self.root, &self.plan_dir(), &mut s);
        s
    }

    /// `rel` is a relative `/`-separated path inside `plan_dir`: no `..`, root, drive, `\` or `:` (Windows streams).
    fn is_plan_rel(&self, rel: &str) -> bool {
        let p = Path::new(rel);
        !rel.contains(['\\', ':', '\0'])
            && p.components().all(|c| matches!(c, std::path::Component::Normal(_)))
            && self.root.join(p).starts_with(self.plan_dir())
    }

    /// Record a history entry for plan changes made by `action` without writing an activity log line
    /// (frequent edits such as board saves). Best effort, like [`Project::log_action`].
    pub fn checkpoint(&self, action: &str) -> Result<()> {
        self.record_history(&LogEntry { action: action.into(), ..Default::default() })
    }

    /// Store the current plan files as the baseline of the next entry (used by `init`).
    pub(crate) fn history_prime(&self) -> Result<()> {
        let _l = FileLock::acquire(&self.root.join(LOCK))?;
        self.save_baseline(&self.plan_snapshot())
    }

    fn save_baseline(&self, s: &Snapshot) -> Result<()> {
        let json = serde_json::to_vec(s).map_err(|e| Error::InvalidSpec(e.to_string()))?;
        write_atomic(&self.root.join(BASELINE), &json)
    }

    /// Called by `log_action` after the log line is written.
    pub(crate) fn record_history(&self, e: &LogEntry) -> Result<()> {
        let _l = FileLock::acquire(&self.root.join(LOCK))?;
        let now = self.plan_snapshot();
        let base: Option<Snapshot> = std::fs::read(self.root.join(BASELINE)).ok().and_then(|b| serde_json::from_slice(&b).ok());
        let Some(base) = base else { return self.save_baseline(&now) };
        if base == now {
            return Ok(());
        }
        let paths: std::collections::BTreeSet<&String> = base.keys().chain(now.keys()).collect();
        let files: Vec<HistoryFile> = paths
            .into_iter()
            .filter(|p| base.get(*p) != now.get(*p))
            .map(|p| HistoryFile { path: p.clone(), before: base.get(p).cloned(), after: now.get(p).cloned() })
            .collect();
        let entry = HistoryEntry {
            id: ulid::Ulid::generate().to_string().to_lowercase(),
            at: if e.at.is_empty() { format_secs(now_secs()) } else { e.at.clone() },
            action: e.action.clone(),
            files,
        };
        let json = serde_json::to_vec(&entry).map_err(|e| Error::InvalidSpec(e.to_string()))?;
        write_atomic(&self.history_dir().join(format!("{}.json", entry.id)), &json)?;
        self.ensure_gitignore();
        self.save_baseline(&now)?;
        self.trim_history(MAX_HISTORY_BYTES);
        Ok(())
    }

    fn history_files(&self) -> Vec<(String, u64)> {
        let mut v: Vec<(String, u64)> = std::fs::read_dir(self.history_dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                (name.ends_with(".json")).then(|| (name, e.metadata().map_or(0, |m| m.len())))
            })
            .collect();
        v.sort();
        v
    }

    /// Delete the oldest entries until the folder is at most `max` bytes (the newest entry always stays).
    pub fn trim_history(&self, max: u64) {
        let files = self.history_files();
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        for (name, len) in files.iter().take(files.len().saturating_sub(1)) {
            if total <= max {
                break;
            }
            if std::fs::remove_file(self.history_dir().join(name)).is_ok() {
                total -= len;
            }
        }
    }

    /// Newest entries first, at most `limit` (0 = all). Unreadable files are skipped.
    pub fn history(&self, limit: usize) -> Result<Vec<HistoryEntry>> {
        let mut out = Vec::new();
        for (name, _) in self.history_files().into_iter().rev() {
            if limit != 0 && out.len() >= limit {
                break;
            }
            let entry = std::fs::read(self.history_dir().join(name)).ok().and_then(|b| serde_json::from_slice::<HistoryEntry>(&b).ok());
            out.extend(entry);
        }
        Ok(out)
    }

    /// Undo history entry `id`: restore each file's `before` (deleting files that did not exist). Fails with
    /// `HistoryConflict` (nothing written) when a file no longer equals its `after`. The revert is logged as
    /// action `revert` by `author` / `source`, which makes it an entry of its own.
    pub fn revert(&self, id: &str, author: &str, source: &str) -> Result<HistoryEntry> {
        if !valid_id(id) {
            return Err(Error::NotFound(format!("history entry {id}")));
        }
        let _plan = self.lock_plan()?;
        let path = self.history_dir().join(format!("{id}.json"));
        let entry: HistoryEntry = std::fs::read(&path)
            .map_err(|_| Error::NotFound(format!("history entry {id}")))
            .and_then(|b| serde_json::from_slice(&b).map_err(|e| crate::error::parse_err(&path, e)))?;
        // Entry files are plain JSON anyone can edit: only plan files may be read, written or deleted.
        if let Some(f) = entry.files.iter().find(|f| !self.is_plan_rel(&f.path)) {
            return Err(Error::OutsideProject(f.path.clone()));
        }
        let changed: Vec<String> = entry
            .files
            .iter()
            .filter(|f| std::fs::read_to_string(self.root.join(&f.path)).ok() != f.after)
            .map(|f| f.path.clone())
            .collect();
        if !changed.is_empty() {
            return Err(Error::HistoryConflict { id: id.into(), paths: changed });
        }
        for f in &entry.files {
            let target = self.root.join(&f.path);
            match &f.before {
                Some(text) => write_atomic(&target, text.as_bytes())?,
                None => match std::fs::remove_file(&target) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                    _ => {}
                },
            }
        }
        let log = LogEntry {
            author: author.into(),
            source: source.into(),
            action: "revert".into(),
            summary: format!("reverted {} ({})", entry.id, entry.action),
            files: entry.files.iter().map(|f| f.path.clone()).collect(),
            ..Default::default()
        };
        let _ = self.log_action(&log); // the revert is done; a log failure must not report it as failed
        Ok(entry)
    }
}
