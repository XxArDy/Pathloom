use crate::error::{Error, Result, parse_err};
use crate::evidence::Evidence;
use crate::fsutil::{ReentrantLock, write_atomic};
use crate::keygen::generate_key;
use crate::model::*;
use crate::stagefile::is_valid_key;
use crate::status::cycle_for_link;
use std::path::{Path, PathBuf};

const CONFIG: &str = ".pathloom/project.toml";
const BOARD_SUFFIX: &str = ".board.json";

const PLAN_LOCK: &str = ".pathloom/plan.lock";

/// Held while a plan file is read-modified-written; see [`Project::lock_plan`].
pub(crate) type PlanGuard = ReentrantLock;

#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub config: ProjectConfig,
}

/// `plan_dir` must be a relative path inside the project (no `..`, no drive, not absolute).
fn check_plan_dir(plan_dir: &str) -> Result<()> {
    use std::path::Component;
    let pd = Path::new(plan_dir);
    let bad = plan_dir.is_empty()
        || pd.is_absolute()
        || plan_dir.starts_with(['/', '\\'])
        || pd.components().any(|c| matches!(c, Component::ParentDir | Component::Prefix(_) | Component::RootDir));
    if bad { Err(Error::OutsideProject(plan_dir.into())) } else { Ok(()) }
}

pub(crate) fn read_text(path: &Path) -> Result<String> {
    Ok(std::fs::read_to_string(path)?)
}

pub(crate) fn check_board_name(name: &str) -> Result<()> {
    let bad = name.is_empty()
        || name.contains(['\\', ':'])
        || name.split('/').any(|s| s.is_empty() || s == "." || s == "..");
    if bad { Err(Error::InvalidName(name.into())) } else { Ok(()) }
}

pub(crate) fn check_key(key: &str) -> Result<()> {
    if is_valid_key(key) { Ok(()) } else { Err(Error::InvalidKey(key.into())) }
}

/// Append a task with the next free `<KEY>-NN` id.
/// Artifact reference: project-relative path (no `..`, not absolute) or an http(s) link; 1-300 chars, one line.
pub(crate) fn check_artifact(a: &str) -> Result<String> {
    let a = a.trim().replace('\\', "/");
    let bad = || Error::InvalidSpec(format!("artifact `{a}` must be a project-relative path or an http(s) link"));
    if !(1..=300).contains(&a.chars().count()) || a.chars().any(char::is_control) {
        return Err(bad());
    }
    let lower = a.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return crate::url::check_http(&a).map(|()| a.clone()).map_err(|_| bad());
    }
    if a.starts_with('/') || a.contains(':') || a.split('/').any(|c| c == "..") {
        return Err(bad());
    }
    Ok(a)
}

pub(crate) fn push_task(s: &mut Stage, title: &str) {
    let prefix = format!("{}-", s.key);
    let max = s.tasks.iter().filter_map(|t| t.id.strip_prefix(&prefix)?.parse::<u32>().ok()).max().unwrap_or(0);
    s.tasks.push(Task { id: format!("{prefix}{:02}", max + 1), title: title.trim().into(), done: false, sub: vec![] });
}

/// Set a task or subtask (by id) done/undone; a task with subtasks follows them.
pub(crate) fn mark_done(s: &mut Stage, task_id: &str, done: bool) -> Result<()> {
    let (i, j) = s
        .tasks
        .iter()
        .enumerate()
        .find_map(|(i, t)| {
            if t.id == task_id {
                Some((i, None))
            } else {
                t.sub.iter().position(|x| x.id == task_id).map(|j| (i, Some(j)))
            }
        })
        .ok_or_else(|| Error::NotFound(format!("task {task_id}")))?;
    let t = &mut s.tasks[i];
    match j {
        Some(j) => t.sub[j].done = done,
        None => t.sub.iter_mut().for_each(|x| x.done = done),
    }
    t.done = if t.sub.is_empty() { done } else { t.sub.iter().all(|x| x.done) };
    s.prune_evidence();
    Ok(())
}

impl Project {
    /// Create `.pathloom/`, `plan/boards/roadmap.board.json` and `plan/stages/`.
    pub fn init(root: &Path, name: &str) -> Result<Project> {
        Self::init_with(root, name, "plan")
    }

    /// Like [`Project::init`] with a custom plan directory (relative, inside the project).
    pub fn init_with(root: &Path, name: &str, plan_dir: &str) -> Result<Project> {
        check_board_name(plan_dir)?;
        let cfg_path = root.join(CONFIG);
        if cfg_path.exists() {
            return Err(Error::AlreadyInitialized(root.to_path_buf()));
        }
        let config = ProjectConfig {
            format: 1,
            name: name.into(),
            plan_dir: plan_dir.into(),
            agent_changes: AgentChanges::Auto,
            require_evidence: false,
            git: None,
            telegram: None,
            require_approval: false,
        };
        let text = toml::to_string(&config).map_err(|e| parse_err(&cfg_path, e))?;
        write_atomic(&cfg_path, text.as_bytes())?;
        write_atomic(&root.join(".pathloom/.gitignore"), b"cache/\nproposals/\nlog/\nclaims.json\nclaims.lock\nplan.lock\n")?;
        let p = Project { root: root.to_path_buf(), config };
        std::fs::create_dir_all(p.plan_dir().join("stages"))?;
        if !p.board_path("roadmap").exists() {
            p.save_board("roadmap", &Board::new("Roadmap"))?;
        }
        Ok(p)
    }

    /// Switches how agent changes are applied (`auto` writes files, `review` creates proposals)
    /// and saves `.pathloom/project.toml`.
    pub fn set_agent_changes(&mut self, mode: AgentChanges) -> Result<()> {
        self.update_config("agent_changes", Some(mode.as_str().into()))
    }

    /// Whether agents must attach passing evidence to mark tasks done; saves `.pathloom/project.toml`.
    pub fn set_require_evidence(&mut self, on: bool) -> Result<()> {
        self.update_config("require_evidence", Some(on.into()))
    }

    /// Whether agents cannot close tasks themselves (they go to review); saves `.pathloom/project.toml`.
    pub fn set_require_approval(&mut self, on: bool) -> Result<()> {
        // Off is the default and is not written (keeps the file as it was before the setting existed).
        self.update_config("require_approval", on.then(|| true.into()))
    }

    /// Patch one key of `project.toml` in place: comments, unknown keys and external edits survive.
    fn update_config(&mut self, key: &str, value: Option<toml_edit::Value>) -> Result<()> {
        let _lock = self.lock_plan()?;
        let path = self.root.join(CONFIG);
        let mut doc: toml_edit::DocumentMut = read_text(&path)?.parse().map_err(|e| parse_err(&path, e))?;
        match value {
            Some(v) => doc[key] = toml_edit::value(v),
            None => {
                doc.remove(key);
            }
        }
        let text = doc.to_string();
        let config: ProjectConfig = toml::from_str(&text).map_err(|e| parse_err(&path, e))?;
        write_atomic(&path, text.as_bytes())?;
        self.config = config;
        Ok(())
    }

    /// Lock for read-modify-write of plan files (`.pathloom/plan.lock`), shared by the app, CLI and MCP processes.
    /// Re-entrant on one thread, so public entry points can take it even when they call each other.
    /// Lock order: plan lock first, then the claims lock (never the other way round).
    pub(crate) fn lock_plan(&self) -> Result<PlanGuard> {
        ReentrantLock::acquire(&self.root.join(PLAN_LOCK), std::time::Duration::from_secs(8))
    }

    pub fn open(root: &Path) -> Result<Project> {
        let path = root.join(CONFIG);
        if !path.is_file() {
            return Err(Error::NotAProject);
        }
        let config: ProjectConfig = toml::from_str(&read_text(&path)?).map_err(|e| parse_err(&path, e))?;
        check_plan_dir(&config.plan_dir)?;
        Ok(Project { root: root.to_path_buf(), config })
    }

    /// Search `start` and its ancestors for a project.
    pub fn find(start: &Path) -> Result<Project> {
        start.ancestors().find(|d| d.join(CONFIG).is_file()).map_or(Err(Error::NotAProject), Project::open)
    }

    pub fn plan_dir(&self) -> PathBuf {
        self.root.join(&self.config.plan_dir)
    }

    pub(crate) fn board_path(&self, name: &str) -> PathBuf {
        self.plan_dir().join("boards").join(format!("{name}{BOARD_SUFFIX}"))
    }

    pub(crate) fn stage_path(&self, key: &str) -> PathBuf {
        self.plan_dir().join("stages").join(format!("{key}.md"))
    }

    // ---- boards ----

    pub fn list_boards(&self) -> Result<Vec<String>> {
        fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) -> Result<()> {
            let Ok(rd) = std::fs::read_dir(dir) else { return Ok(()) };
            for e in rd {
                let e = e?;
                let fname = e.file_name().to_string_lossy().into_owned();
                if e.file_type()?.is_dir() {
                    walk(&e.path(), &format!("{prefix}{fname}/"), out)?;
                } else if let Some(stem) = fname.strip_suffix(BOARD_SUFFIX) {
                    out.push(format!("{prefix}{stem}"));
                }
            }
            Ok(())
        }
        let mut out = Vec::new();
        walk(&self.plan_dir().join("boards"), "", &mut out)?;
        out.sort();
        Ok(out)
    }

    pub fn load_board(&self, name: &str) -> Result<Board> {
        check_board_name(name)?;
        let path = self.board_path(name);
        if !path.is_file() {
            return Err(Error::NotFound(format!("board {name}")));
        }
        Board::from_json(&path, &read_text(&path)?)
    }

    pub fn save_board(&self, name: &str, board: &Board) -> Result<()> {
        check_board_name(name)?;
        write_atomic(&self.board_path(name), board.to_canonical_json().as_bytes())
    }

    pub fn create_board(&self, name: &str, title: &str) -> Result<Board> {
        check_board_name(name)?;
        let _lock = self.lock_plan()?;
        if self.board_path(name).exists() {
            return Err(Error::DuplicateKey(name.into()));
        }
        let b = Board::new(title);
        self.save_board(name, &b)?;
        Ok(b)
    }

    // ---- stages ----

    /// All readable stages, sorted by key. A stage file that does not parse is skipped, so one bad file does not
    /// hide the project; [`Project::load_stages_report`] says which were skipped. Writes to such a file still fail.
    pub fn load_stages(&self) -> Result<Vec<Stage>> {
        Ok(self.load_stages_report()?.0)
    }

    /// Like [`Project::load_stages`], plus a `PARSE_ERROR` issue for each stage file that was skipped.
    pub fn load_stages_report(&self) -> Result<(Vec<Stage>, Vec<crate::Issue>)> {
        let mut issues = Vec::new();
        let stages = self.scan_stages(&mut issues)?;
        Ok((stages, issues))
    }

    pub fn load_stage(&self, key: &str) -> Result<Stage> {
        check_key(key)?;
        let path = self.stage_path(key);
        if !path.is_file() {
            return Err(Error::NotFound(format!("stage {key}")));
        }
        Stage::from_markdown(&path, &read_text(&path)?)
    }

    pub fn save_stage(&self, stage: &Stage) -> Result<()> {
        check_key(&stage.key)?;
        write_atomic(&self.stage_path(&stage.key), stage.to_markdown().as_bytes())
    }

    /// `key`: explicit (validated, must be free) or generated from the title.
    pub fn create_stage(&self, title: &str, key: Option<&str>) -> Result<Stage> {
        let _lock = self.lock_plan()?;
        let taken = |k: &str| self.stage_path(k).exists();
        let key = match key {
            Some(k) => {
                check_key(k)?;
                if taken(k) {
                    return Err(Error::DuplicateKey(k.into()));
                }
                k.to_string()
            }
            None => generate_key(title, &taken),
        };
        let stage = Stage::new(&key, title);
        self.save_stage(&stage)?;
        Ok(stage)
    }

    /// Deletes the file and removes `key` from other stages' `depends`.
    pub fn delete_stage(&self, key: &str) -> Result<()> {
        check_key(key)?;
        let _lock = self.lock_plan()?;
        let path = self.stage_path(key);
        if !path.is_file() {
            return Err(Error::NotFound(format!("stage {key}")));
        }
        std::fs::remove_file(path)?;
        for mut s in self.load_stages()? {
            let before = s.depends.len();
            s.depends.retain(|d| d.id != key);
            if s.depends.len() != before {
                self.save_stage(&s)?;
            }
        }
        Ok(())
    }

    pub(crate) fn edit_stage(&self, key: &str, f: impl FnOnce(&mut Stage) -> Result<()>) -> Result<Stage> {
        let _lock = self.lock_plan()?;
        let mut s = self.load_stage(key)?;
        f(&mut s)?;
        self.save_stage(&s)?;
        Ok(s)
    }

    pub fn add_task(&self, key: &str, title: &str) -> Result<Stage> {
        self.edit_stage(key, |s| {
            push_task(s, title);
            Ok(())
        })
    }

    /// Renames a stage. The key (and file name) stays the same so links keep working.
    pub fn set_title(&self, key: &str, title: &str) -> Result<Stage> {
        let title = title.trim();
        if title.is_empty() {
            return Err(Error::InvalidName("stage title must not be empty".into()));
        }
        self.edit_stage(key, |s| {
            s.title = title.into();
            Ok(())
        })
    }

    /// Applies a partial frontmatter update; see [`StageMeta`].
    pub fn update_meta(&self, key: &str, meta: StageMeta) -> Result<Stage> {
        if meta.title.as_ref().is_some_and(|t| t.trim().is_empty()) {
            return Err(Error::InvalidName("stage title must not be empty".into()));
        }
        let opt = |v: String| {
            let v = v.trim().to_string();
            (!v.is_empty()).then_some(v)
        };
        self.edit_stage(key, |s| {
            if let Some(t) = meta.title {
                s.title = t.trim().into();
            }
            if let Some(k) = meta.kind {
                s.kind = k;
            }
            if let Some(w) = meta.who {
                s.who = w;
            }
            if let Some(p) = meta.priority {
                s.priority = p;
            }
            if let Some(d) = meta.due {
                s.due = opt(d);
            }
            if let Some(b) = meta.branch {
                s.branch = opt(b);
            }
            if let Some(c) = meta.check {
                s.check = opt(c.replace(['\r', '\n'], " "));
            }
            Ok(())
        })
    }

    /// Absolute path of a project-relative artifact that exists inside the project (links and escapes fail).
    pub fn artifact_path(&self, artifact: &str) -> Result<PathBuf> {
        let a = check_artifact(artifact)?;
        if a.contains("://") {
            return Err(Error::InvalidSpec(format!("artifact `{a}` is a link, not a file")));
        }
        let root = self.root.canonicalize()?;
        let full = root.join(&a).canonicalize().map_err(|_| Error::NotFound(format!("file {a}")))?;
        if !full.starts_with(&root) {
            return Err(Error::InvalidSpec(format!("artifact `{a}` is outside the project")));
        }
        Ok(full)
    }

    /// Raw text of a stage file, for the "File" tab.
    pub fn stage_source(&self, key: &str) -> Result<String> {
        check_key(key)?;
        let path = self.stage_path(key);
        if !path.is_file() {
            return Err(Error::NotFound(format!("stage {key}")));
        }
        read_text(&path)
    }

    /// Adds a subtask under a top-level task: `M0-02` gets `M0-02a`, `M0-02b`, …
    pub fn add_subtask(&self, task_id: &str, title: &str) -> Result<Stage> {
        let key = crate::stage_key_of(task_id).to_string();
        self.edit_stage(&key, |s| {
            let task = s
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id)
                .ok_or_else(|| Error::NotFound(format!("task {task_id}")))?;
            let used: Vec<char> = task.sub.iter().filter_map(|x| x.id.strip_prefix(task_id)?.chars().next()).collect();
            let letter = ('a'..='z')
                .find(|c| !used.contains(c))
                .ok_or_else(|| Error::InvalidName(format!("task {task_id} already has 26 subtasks")))?;
            task.sub.push(Task { id: format!("{task_id}{letter}"), title: title.trim().into(), done: false, sub: vec![] });
            task.done = false;
            Ok(())
        })
    }

    /// Edit a task or subtask located by its id (`M0-02` or `M0-02a`).
    fn edit_task(&self, task_id: &str, f: impl FnOnce(&mut Vec<Task>, usize, Option<usize>)) -> Result<Stage> {
        let key = crate::stage_key_of(task_id);
        self.edit_stage(key, |s| {
            let pos = s.tasks.iter().enumerate().find_map(|(i, t)| {
                if t.id == task_id {
                    Some((i, None))
                } else {
                    t.sub.iter().position(|x| x.id == task_id).map(|j| (i, Some(j)))
                }
            });
            let (i, j) = pos.ok_or_else(|| Error::NotFound(format!("task {task_id}")))?;
            f(&mut s.tasks, i, j);
            Ok(())
        })
    }

    /// Marking a task done also releases the claims on it and on its subtasks.
    pub fn set_task_done(&self, task_id: &str, done: bool) -> Result<Stage> {
        self.set_task_done_with(task_id, done, None)
    }

    /// Record the result of a gate's check under the stage key. A passing result makes the gate done;
    /// a failing one removes the previous result (the gate is open again) and is returned as `Ok(false)`.
    pub fn set_gate_result(&self, key: &str, evidence: Evidence) -> Result<(Stage, bool)> {
        let mut e = evidence.normalize()?;
        if e.at.is_empty() {
            e.at = crate::fsutil::now_rfc3339();
        }
        let passed = e.passed();
        let s = self.edit_stage(key, |s| {
            if s.kind != StageKind::Gate {
                return Err(Error::InvalidSpec(format!("{key} is not a gate (kind: gate)")));
            }
            if passed {
                s.evidence.insert(key.to_string(), e);
            } else {
                s.evidence.remove(key);
            }
            Ok(())
        })?;
        Ok((s, passed))
    }

    /// Replace the note of a task or subtask (empty clears it). Up to 4000 characters.
    pub fn set_task_note(&self, task_id: &str, note: &str) -> Result<Stage> {
        let note = note.replace("\r\n", "\n").trim().to_string();
        if note.chars().count() > 4000 {
            return Err(Error::InvalidSpec("task note must be at most 4000 characters".into()));
        }
        self.edit_task_info(task_id, |i| {
            i.note = note;
            Ok(())
        })
    }

    /// Attach a file (project-relative path or http(s) link) to a task; already attached is a no-op. Up to 20.
    pub fn add_task_file(&self, task_id: &str, file: &str) -> Result<Stage> {
        let f = check_artifact(file)?;
        self.edit_task_info(task_id, |i| {
            if !i.files.contains(&f) {
                if i.files.len() >= 20 {
                    return Err(Error::InvalidSpec(format!("task {task_id} has 20 files already")));
                }
                i.files.push(f);
            }
            Ok(())
        })
    }

    /// Set the delivery status of a task (`task_info.<ID>.status`). `sha` is 7-40 hex, `evidence` up to 10
    /// paths/links (same rules as task files); `at` is now.
    pub fn set_task_status(
        &self,
        task_id: &str,
        step: crate::TaskStep,
        branch: Option<&str>,
        sha: Option<&str>,
        evidence: &[String],
        by: &str,
    ) -> Result<Stage> {
        let st = crate::TaskStatus {
            step,
            branch: branch.map(String::from),
            sha: sha.map(String::from),
            evidence: evidence.to_vec(),
            at: crate::fsutil::now_rfc3339(),
            by: by.into(),
        }
        .normalize()?;
        self.edit_task_info(task_id, |i| {
            i.status = Some(st);
            Ok(())
        })
    }

    pub fn remove_task_file(&self, task_id: &str, file: &str) -> Result<Stage> {
        self.edit_task_info(task_id, |i| {
            i.files.retain(|x| x != file);
            Ok(())
        })
    }

    pub(crate) fn edit_task_info(&self, task_id: &str, f: impl FnOnce(&mut TaskInfo) -> Result<()>) -> Result<Stage> {
        let key = crate::stage_key_of(task_id);
        self.edit_stage(key, |s| {
            if !s.task_ids().contains(task_id) {
                return Err(Error::NotFound(format!("task {task_id}")));
            }
            let info = s.task_info.entry(task_id.to_string()).or_default();
            f(info)?;
            if s.task_info.get(task_id).is_some_and(TaskInfo::is_empty) {
                s.task_info.remove(task_id);
            }
            Ok(())
        })
    }

    /// Like [`Project::set_task_done`] with proof of the check that was run. Evidence goes only with
    /// `done = true` and must have exit code 0; `at` is filled in when empty. Reopening a task drops its evidence.
    pub fn set_task_done_with(&self, task_id: &str, done: bool, evidence: Option<Evidence>) -> Result<Stage> {
        let ev = evidence.map(Evidence::normalize).transpose()?;
        if let Some(e) = &ev {
            if !done {
                return Err(Error::InvalidSpec("evidence goes only with done=true".into()));
            }
            if !e.passed() {
                return Err(Error::InvalidSpec(format!(
                    "evidence for {task_id} has exit_code {}: the check failed, so the task stays open",
                    e.exit_code
                )));
            }
        }
        let key = crate::stage_key_of(task_id);
        let s = self.edit_stage(key, |s| {
            mark_done(s, task_id, done)?;
            if let Some(mut e) = ev {
                if e.at.is_empty() {
                    e.at = crate::fsutil::now_rfc3339();
                }
                s.evidence.insert(task_id.to_string(), e);
            }
            Ok(())
        })?;
        if done {
            self.release_done_claims(std::slice::from_ref(&s));
        }
        Ok(s)
    }

    pub fn rename_task(&self, task_id: &str, title: &str) -> Result<Stage> {
        self.edit_task(task_id, |tasks, i, j| match j {
            Some(j) => tasks[i].sub[j].title = title.trim().into(),
            None => tasks[i].title = title.trim().into(),
        })
    }

    pub fn remove_task(&self, task_id: &str) -> Result<Stage> {
        self.edit_task(task_id, |tasks, i, j| match j {
            Some(j) => {
                let t = &mut tasks[i];
                t.sub.remove(j);
                if !t.sub.is_empty() {
                    t.done = t.sub.iter().all(|x| x.done);
                }
            }
            None => {
                tasks.remove(i);
            }
        })
    }

    /// Add `from` to `to`'s depends. Self-link / cycle -> `Error::Cycle`; existing link is a no-op.
    pub fn link(&self, from: &str, to: &str, soft: bool) -> Result<()> {
        check_key(from)?;
        check_key(to)?;
        let _lock = self.lock_plan()?;
        let stages = self.load_stages()?;
        for k in [from, to] {
            if !stages.iter().any(|s| s.key == k) {
                return Err(Error::NotFound(format!("stage {k}")));
            }
        }
        if let Some(cycle) = cycle_for_link(&stages, from, to) {
            return Err(Error::Cycle(cycle));
        }
        self.edit_stage(to, |s| {
            if s.depends.iter().all(|d| d.id != from) {
                s.depends.push(Dep { id: from.into(), soft });
            }
            Ok(())
        })
        .map(|_| ())
    }

    pub fn unlink(&self, from: &str, to: &str) -> Result<()> {
        self.edit_stage(to, |s| {
            s.depends.retain(|d| d.id != from);
            Ok(())
        })
        .map(|_| ())
    }
}
