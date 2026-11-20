//! Git mode: the board is read from a git ref (e.g. `origin/main`); owner actions are committed
//! to the local checkout or queued in `.pathloom/owner-queue.jsonl` until the tree is clean.
//! Pathloom never pushes.

use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use pathloom_core::{Error, Project, gitsrc};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::watch::PlanChanged;

const QUEUE_FILE: &str = ".pathloom/owner-queue.jsonl";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct QueueItem {
    /// `approve`, `reject`, `decide` or `proposal`.
    pub action: String,
    pub task_id: String,
    #[serde(default)]
    pub args: serde_json::Value,
    pub at: String,
}

fn queue_path(root: &Path) -> PathBuf {
    root.join(QUEUE_FILE)
}

/// Queued items in file order; unreadable lines are skipped.
pub fn read_queue(root: &Path) -> Vec<QueueItem> {
    let text = std::fs::read_to_string(queue_path(root)).unwrap_or_default();
    text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

pub fn write_queue(root: &Path, items: &[QueueItem]) -> std::io::Result<()> {
    let mut text = String::new();
    for i in items {
        text.push_str(&serde_json::to_string(i).unwrap_or_default());
        text.push('\n');
    }
    pathloom_core::write_file_atomic(&queue_path(root), text.as_bytes())
}

pub fn append_queue(root: &Path, item: QueueItem) -> std::io::Result<()> {
    let mut items = read_queue(root);
    items.push(item);
    write_queue(root, &items)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    Direct,
    Queue,
}

/// Write now when git mode is off or the tree is clean; otherwise queue.
pub fn route(git_mode: bool, clean: bool) -> Route {
    if !git_mode || clean { Route::Direct } else { Route::Queue }
}

/// Applies one owner action to the local checkout. Returns the changed files (relative to the root).
pub fn perform(p: &Project, item: &QueueItem) -> pathloom_core::Result<(Vec<String>, Option<pathloom_core::Proposal>)> {
    let str_arg = |k: &str| item.args.get(k).and_then(|v| v.as_str()).map(str::to_string);
    let stage_file = || vec![p.stage_rel_path(pathloom_core::stage_key_of(&item.task_id))];
    match item.action.as_str() {
        "approve" | "reject" => {
            let reasons: Vec<String> = item.args.get("reasons").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
            p.decide_approval(&item.task_id, item.action == "approve", &reasons, str_arg("comment").as_deref(), "me")?;
            Ok((stage_file(), None))
        }
        "decide" => {
            p.answer_decision(&item.task_id, &str_arg("choice").unwrap_or_default(), "me")?;
            Ok((stage_file(), None))
        }
        "message" => {
            let text = str_arg("text").unwrap_or_default();
            p.add_comment(pathloom_core::stage_key_of(&item.task_id), "me", &format!("{}: {text}", item.task_id))?;
            Ok((stage_file(), None))
        }
        "proposal" => {
            let approve = item.args.get("approve").and_then(|v| v.as_bool()).unwrap_or(false);
            let d = p.decide_proposal(&item.task_id, approve, str_arg("comment").as_deref(), "me")?;
            Ok((if approve { d.preview.files.clone() } else { vec![] }, Some(d)))
        }
        other => Err(Error::InvalidSpec(format!("unknown queued action {other}"))),
    }
}

/// Commits the files an owner action changed ("owner: approve A-01"). Best effort: the write already happened.
pub fn commit_owner(root: &Path, item: &QueueItem, files: &[String]) {
    let msg = format!("owner: {} {}", item.action, item.task_id);
    for f in files {
        if let Err(e) = gitsrc::commit_file(root, f, &msg) {
            eprintln!("pathloom: commit of {f} failed: {e}");
        }
    }
}

// ---- source state ----

#[derive(Default)]
struct GitState {
    src: Option<Arc<tempfile::TempDir>>,
    sha: Option<String>,
    error: Option<String>,
}

static GIT: LazyLock<Mutex<GitState>> = LazyLock::new(Default::default);
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// The project to read the plan (stages, boards) from: the materialized git ref, or the local one.
pub struct Source {
    project: Project,
    pub from_git: bool,
    _keep: Option<Arc<tempfile::TempDir>>,
}

impl Deref for Source {
    type Target = Project;
    fn deref(&self) -> &Project {
        &self.project
    }
}

pub fn source(local: &Project) -> Source {
    let keep = GIT.lock().ok().and_then(|g| g.src.clone());
    if let Some(dir) = &keep
        && let Ok(project) = Project::open(dir.path())
    {
        return Source { project, from_git: true, _keep: keep };
    }
    Source { project: local.clone(), from_git: false, _keep: None }
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub read_ref: Option<String>,
    pub sha: Option<String>,
    pub error: Option<String>,
    pub queued: usize,
}

pub fn read_ref(p: &Project) -> Option<String> {
    p.config.git.as_ref().and_then(|g| g.read_ref.clone()).filter(|r| !r.trim().is_empty())
}

pub fn status(p: &Project) -> GitStatus {
    let g = GIT.lock().ok();
    GitStatus {
        read_ref: read_ref(p),
        sha: g.as_ref().and_then(|g| g.sha.clone()),
        error: g.as_ref().and_then(|g| g.error.clone()),
        queued: read_queue(&p.root).len(),
    }
}

pub fn emit_status(app: &AppHandle, p: &Project) {
    let _ = app.emit("git-status", status(p));
}

pub fn emit_all(app: &AppHandle, p: &Project) {
    let s = source(p);
    let stages = s.load_stages().map(|v| v.into_iter().map(|s| s.key).collect()).unwrap_or_default();
    let boards = s.list_boards().unwrap_or_default();
    let _ = app.emit("plan-changed", PlanChanged { stages, boards, proposals: false, group: None });
}

/// (Re)starts the background fetch loop for the project; stops any previous one.
pub fn restart(app: &AppHandle, p: &Project) {
    let gen_id = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    if let Ok(mut g) = GIT.lock() {
        *g = GitState::default();
    }
    let Some(git_ref) = read_ref(p) else { return };
    let interval = p.config.git.as_ref().map_or(120, |g| g.fetch_interval_secs.max(10));
    let (app, root) = (app.clone(), p.root.clone());
    std::thread::spawn(move || {
        loop {
            if GENERATION.load(Ordering::SeqCst) != gen_id {
                return;
            }
            if let Ok(p) = Project::open(&root) {
                tick(&app, &p, &git_ref, gen_id);
            }
            for _ in 0..interval {
                if GENERATION.load(Ordering::SeqCst) != gen_id {
                    return;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    });
}

fn tick(app: &AppHandle, p: &Project, git_ref: &str, gen_id: u64) {
    let fetch_err = gitsrc::fetch(&p.root).err().map(|e| e.to_string());
    let mut changed = false;
    let mut fresh = None;
    let mut ref_err = None;
    match gitsrc::ref_sha(&p.root, git_ref) {
        Ok(sha) => {
            let known = GIT.lock().ok().and_then(|g| g.sha.clone());
            if known.as_deref() != Some(&sha) {
                match gitsrc::materialize(&p.root, git_ref, &p.config.plan_dir) {
                    Ok(dir) => fresh = Some((sha, Arc::new(dir))),
                    Err(e) => ref_err = Some(e.to_string()),
                }
            }
        }
        Err(e) => {
            // Bad ref: fall back to the working copy.
            if let Ok(mut g) = GIT.lock() {
                changed = g.src.take().is_some();
                g.sha = None;
            }
            ref_err = Some(e.to_string());
        }
    }
    if GENERATION.load(Ordering::SeqCst) != gen_id {
        return;
    }
    if let Ok(mut g) = GIT.lock() {
        if let Some((sha, dir)) = fresh {
            (g.src, g.sha, changed) = (Some(dir), Some(sha), true);
        }
        g.error = ref_err.or(fetch_err);
    }
    retry_queue(&p.root, git_ref);
    if changed {
        emit_all(app, p);
    }
    emit_status(app, p);
}

/// Replays queued owner actions in order while the tree is clean.
fn retry_queue(root: &Path, git_ref: &str) {
    let Ok(p) = Project::open(root) else { return };
    let mut items = read_queue(root);
    if items.is_empty() || !gitsrc::is_clean(root).unwrap_or(false) {
        return;
    }
    let up_to_date = gitsrc::ff_to(root, git_ref).unwrap_or(false);
    let before = items.len();
    while let Some(item) = items.first() {
        match perform(&p, item) {
            Ok((files, _)) => commit_owner(root, item, &files),
            Err(Error::Io(_)) => break,
            // Diverged branch: the task may still arrive later; keep it.
            Err(Error::NotFound(_)) if !up_to_date => break,
            Err(e) => eprintln!("pathloom: dropped queued {} {}: {e}", item.action, item.task_id),
        }
        items.remove(0);
        if !gitsrc::is_clean(root).unwrap_or(false) {
            break;
        }
    }
    if items.len() != before {
        let _ = write_queue(root, &items);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(n: &str) -> QueueItem {
        QueueItem { action: "approve".into(), task_id: n.into(), args: serde_json::json!({}), at: "t".into() }
    }

    #[test]
    fn queue_append_read_order_and_bad_lines() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(".pathloom")).unwrap();
        assert!(read_queue(d.path()).is_empty());
        append_queue(d.path(), item("A-01")).unwrap();
        append_queue(d.path(), item("A-02")).unwrap();
        let path = queue_path(d.path());
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("not json\n");
        std::fs::write(&path, text).unwrap();
        append_queue(d.path(), item("A-03")).unwrap();
        let ids: Vec<_> = read_queue(d.path()).into_iter().map(|i| i.task_id).collect();
        assert_eq!(ids, ["A-01", "A-02", "A-03"]);
        write_queue(d.path(), &read_queue(d.path())[1..]).unwrap();
        assert_eq!(read_queue(d.path()).len(), 2);
    }

    #[test]
    fn route_decision() {
        assert_eq!(route(false, false), Route::Direct);
        assert_eq!(route(true, true), Route::Direct);
        assert_eq!(route(true, false), Route::Queue);
    }

    #[test]
    fn perform_rejects_unknown_and_missing() {
        let d = tempfile::tempdir().unwrap();
        let p = Project::init(d.path(), "t").unwrap();
        let mut bad = item("A-01");
        bad.action = "nope".into();
        assert!(perform(&p, &bad).is_err());
        assert!(matches!(perform(&p, &item("A-01")), Err(Error::NotFound(_))));
    }

    #[test]
    fn perform_message_adds_stage_comment() {
        let d = tempfile::tempdir().unwrap();
        let p = Project::init(d.path(), "t").unwrap();
        p.apply(&Project::parse_spec("stages: [{id: A, title: A, tasks: [One]}]").unwrap(), &pathloom_core::ApplyOptions::default()).unwrap();
        let msg = QueueItem { action: "message".into(), task_id: "A-01".into(), args: serde_json::json!({ "text": "use PKCE" }), at: "t".into() };
        let (files, prop) = perform(&p, &msg).unwrap();
        assert_eq!((files.len(), prop.is_none()), (1, true));
        let c = p.load_stage("A").unwrap().comments.pop().unwrap();
        assert_eq!((c.author.as_str(), c.text.as_str()), ("me", "A-01: use PKCE"));
    }
}
