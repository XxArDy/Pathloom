//! Approvals, review state, decisions and the owner's inbox (m9 contract §2, §3, §9).

use crate::error::{Error, Result};
use crate::evidence::Evidence;
use crate::fsutil::{now_minute, now_rfc3339};
use crate::model::*;
use crate::project::{Project, check_artifact, mark_done};
use crate::status::compute_statuses;
use serde::Serialize;

const MAX_REASONS: usize = 10;
const MAX_REASON_LEN: usize = 40;
const MAX_COMMENT: usize = 2000;
const MAX_NOTE: usize = 4000;

fn bad(msg: impl Into<String>) -> Error {
    Error::InvalidSpec(msg.into())
}

fn stage_key(task_id: &str) -> &str {
    task_id.split('-').next().unwrap_or("")
}

fn clean(s: &str) -> String {
    s.replace("\r\n", "\n").trim().to_string()
}

fn too_long(s: &str, max: usize, what: &str) -> Result<()> {
    if s.chars().count() > max { Err(bad(format!("{what} must be at most {max} characters"))) } else { Ok(()) }
}

/// One task with an approval, across all stages.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ApprovalView {
    pub task_id: String,
    pub stage: String,
    pub title: String,
    pub state: ApprovalState,
    pub files: Vec<String>,
    pub note: String,
    pub reasons: Vec<String>,
    pub comment: String,
    pub at: String,
    pub by: String,
    pub requested_by: String,
    pub requested_at: String,
}

/// One task with a decision (asked, and answered when `choice` is set).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DecisionView {
    pub task_id: String,
    pub stage: String,
    pub title: String,
    #[serde(flatten)]
    pub decision: TaskDecision,
}

/// Something that waits for the owner.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InboxItem {
    pub task_id: String,
    pub stage: String,
    pub title: String,
    pub kind: InboxKind,
}

/// Mark `id` as waiting for review: approval pending, checkbox open. Old reasons and comment are dropped
/// (they already live in the stage comments).
fn set_pending(s: &mut Stage, id: &str, by: &str) -> Result<()> {
    if !s.task_ids().contains(id) {
        return Err(Error::NotFound(format!("task {id}")));
    }
    let info = s.task_info.entry(id.to_string()).or_default();
    info.approval =
        Some(Approval { state: ApprovalState::Pending, requested_by: by.into(), requested_at: now_rfc3339(), ..Default::default() });
    mark_done(s, id, false)
}

impl Project {
    /// Ask the owner to approve a task: adds `files` (dedup, at most 20), replaces the note when given,
    /// sets the approval to pending and un-ticks the task.
    pub fn request_approval(&self, task_id: &str, files: &[String], note: Option<&str>, by: &str) -> Result<Stage> {
        let files = files.iter().map(|f| check_artifact(f)).collect::<Result<Vec<_>>>()?;
        let note = note.map(clean);
        if let Some(n) = &note {
            too_long(n, MAX_NOTE, "task note")?;
        }
        let s = self.edit_stage(stage_key(task_id), |s| {
            if !s.task_ids().contains(task_id) {
                return Err(Error::NotFound(format!("task {task_id}")));
            }
            let info = s.task_info.entry(task_id.to_string()).or_default();
            for f in files {
                if !info.files.contains(&f) {
                    if info.files.len() >= 20 {
                        return Err(bad(format!("task {task_id} has 20 files already")));
                    }
                    info.files.push(f);
                }
            }
            if let Some(n) = note {
                info.note = n;
            }
            set_pending(s, task_id, by)
        })?;
        let _ = self.release_task(task_id, by, false);
        Ok(s)
    }

    /// `require_approval` path of "done": store the passing `evidence` (if any) and put the task in review
    /// instead of ticking it.
    pub fn submit_for_review(&self, task_id: &str, evidence: Option<Evidence>, by: &str) -> Result<Stage> {
        let ev = evidence.map(Evidence::normalize).transpose()?;
        if let Some(e) = &ev
            && !e.passed()
        {
            return Err(bad(format!(
                "evidence for {task_id} has exit_code {}: the check failed, so the task stays open",
                e.exit_code
            )));
        }
        let s = self.edit_stage(stage_key(task_id), |s| {
            set_pending(s, task_id, by)?;
            if let Some(mut e) = ev {
                if e.at.is_empty() {
                    e.at = now_rfc3339();
                }
                s.evidence.insert(task_id.to_string(), e);
            }
            Ok(())
        })?;
        let _ = self.release_task(task_id, by, false);
        Ok(s)
    }

    /// The owner decides (first answer wins: a task whose approval is not pending fails with
    /// [`Error::AlreadyDecided`]): approve ticks the task; reject un-ticks it, stores `reasons` and `comment`
    /// and adds `<by>: rejected <ID> [reasons]: <comment>` to the stage comments.
    pub fn decide_approval(&self, task_id: &str, approve: bool, reasons: &[String], comment: Option<&str>, by: &str) -> Result<Stage> {
        let reasons: Vec<String> = reasons.iter().map(|r| r.trim().to_string()).filter(|r| !r.is_empty()).collect();
        if reasons.len() > MAX_REASONS || reasons.iter().any(|r| r.chars().count() > MAX_REASON_LEN) {
            return Err(bad(format!("at most {MAX_REASONS} reasons of {MAX_REASON_LEN} characters")));
        }
        let comment = comment.map(clean).unwrap_or_default();
        too_long(&comment, MAX_COMMENT, "comment")?;
        let s = self.edit_stage(stage_key(task_id), |s| {
            if !s.task_ids().contains(task_id) {
                return Err(Error::NotFound(format!("task {task_id}")));
            }
            let info = s.task_info.entry(task_id.to_string()).or_default();
            // First answer wins. No approval record at all = the owner approves directly.
            if let Some(a) = info.approval.as_ref().filter(|a| a.state != ApprovalState::Pending) {
                return Err(Error::AlreadyDecided {
                    task_id: task_id.into(),
                    what: "approval",
                    by: a.by.clone(),
                    at: a.at.clone(),
                    value: a.state.as_str().into(),
                });
            }
            let mut a = info.approval.take().unwrap_or_default();
            a.at = now_rfc3339();
            a.by = by.into();
            a.comment = comment.clone();
            if approve {
                a.state = ApprovalState::Approved;
                a.reasons = vec![];
            } else {
                a.state = ApprovalState::Rejected;
                a.reasons = reasons.clone();
            }
            info.approval = Some(a);
            if approve && let Some(st) = info.status.as_mut() {
                st.step = crate::taskstatus::TaskStep::Accepted;
            }
            mark_done(s, task_id, approve)?;
            if !approve {
                let why = if reasons.is_empty() { String::new() } else { format!(" [{}]", reasons.join(", ")) };
                let tail = if comment.is_empty() { String::new() } else { format!(": {comment}") };
                s.comments.push(Comment {
                    at: now_minute(),
                    author: by.into(),
                    text: format!("rejected {task_id}{why}{tail}").replace('\n', " "),
                });
            }
            Ok(())
        })?;
        if approve {
            self.release_done_claims(std::slice::from_ref(&s));
        }
        Ok(s)
    }

    /// Tasks that have an approval, optionally only in `state`, ordered by stage and task id.
    pub fn approvals(&self, state: Option<ApprovalState>) -> Result<Vec<ApprovalView>> {
        let mut out = Vec::new();
        for s in self.load_stages()? {
            for (id, info) in &s.task_info {
                let Some(a) = info.approval.as_ref().filter(|a| state.is_none_or(|x| x == a.state)) else { continue };
                out.push(ApprovalView {
                    task_id: id.clone(),
                    stage: s.key.clone(),
                    title: task_title(&s, id),
                    state: a.state,
                    files: info.files.clone(),
                    note: info.note.clone(),
                    reasons: a.reasons.clone(),
                    comment: a.comment.clone(),
                    at: a.at.clone(),
                    by: a.by.clone(),
                    requested_by: a.requested_by.clone(),
                    requested_at: a.requested_at.clone(),
                });
            }
        }
        Ok(out)
    }

    /// Ask the owner a question: 1-8 options (`label` <= 80, `description` <= 500), `question` <= 500.
    /// Clears an earlier choice and un-ticks the task.
    pub fn ask_decision(
        &self,
        task_id: &str,
        question: &str,
        options: Vec<DecisionOption>,
        recommended: Option<usize>,
        by: &str,
    ) -> Result<Stage> {
        let question = clean(question);
        if question.is_empty() {
            return Err(bad("question must not be empty"));
        }
        too_long(&question, 500, "question")?;
        if !(1..=8).contains(&options.len()) {
            return Err(bad("a decision needs 1-8 options"));
        }
        let mut opts = Vec::new();
        for o in options {
            let (label, description) = (clean(&o.label), clean(&o.description));
            if label.is_empty() {
                return Err(bad("option label must not be empty"));
            }
            too_long(&label, 80, "option label")?;
            too_long(&description, 500, "option description")?;
            opts.push(DecisionOption { label, description });
        }
        if recommended.is_some_and(|r| r >= opts.len()) {
            return Err(bad("recommended must be an index into options"));
        }
        self.edit_stage(stage_key(task_id), |s| {
            if !s.task_ids().contains(task_id) {
                return Err(Error::NotFound(format!("task {task_id}")));
            }
            s.task_info.entry(task_id.to_string()).or_default().decision =
                Some(TaskDecision { question, options: opts, recommended, asked_by: by.into(), ..Default::default() });
            mark_done(s, task_id, false)
        })
    }

    /// The owner answers (first answer wins: an answered decision fails with [`Error::AlreadyDecided`]) (`choice`: an option label or free text, <= 2000): records `choice, at, by` and ticks the task.
    pub fn answer_decision(&self, task_id: &str, choice: &str, by: &str) -> Result<Stage> {
        let choice = clean(choice);
        if choice.is_empty() {
            return Err(bad("choice must not be empty"));
        }
        too_long(&choice, MAX_COMMENT, "choice")?;
        let s = self.edit_stage(stage_key(task_id), |s| {
            let d = s
                .task_info
                .get_mut(task_id)
                .and_then(|i| i.decision.as_mut())
                .ok_or_else(|| Error::NotFound(format!("decision of {task_id}")))?;
            if let Some(c) = &d.choice {
                return Err(Error::AlreadyDecided {
                    task_id: task_id.into(),
                    what: "decision",
                    by: d.by.clone(),
                    at: d.at.clone(),
                    value: c.clone(),
                });
            }
            (d.choice, d.at, d.by) = (Some(choice), now_rfc3339(), by.into());
            mark_done(s, task_id, true)
        })?;
        self.release_done_claims(std::slice::from_ref(&s));
        Ok(s)
    }

    /// Decisions across all stages; `open_only` keeps the unanswered ones.
    pub fn decisions(&self, open_only: bool) -> Result<Vec<DecisionView>> {
        let mut out = Vec::new();
        for s in self.load_stages()? {
            for (id, info) in &s.task_info {
                let Some(d) = info.decision.as_ref().filter(|d| !open_only || d.choice.is_none()) else { continue };
                out.push(DecisionView { task_id: id.clone(), stage: s.key.clone(), title: task_title(&s, id), decision: d.clone() });
            }
        }
        Ok(out)
    }

    /// What waits for the owner: pending approvals, open decisions, then open tasks of stages tagged
    /// `owner` or with `who: me` that are not blocked.
    pub fn inbox(&self) -> Result<Vec<InboxItem>> {
        let stages = self.load_stages()?;
        let statuses = compute_statuses(&stages);
        let (mut approvals, mut decisions, mut owner) = (vec![], vec![], vec![]);
        for s in &stages {
            let item = |id: &str, kind| InboxItem { task_id: id.into(), stage: s.key.clone(), title: task_title(s, id), kind };
            for (id, info) in &s.task_info {
                if s.in_review(id) {
                    approvals.push(item(id, InboxKind::Approval));
                } else if info.decision.as_ref().is_some_and(|d| d.choice.is_none()) {
                    decisions.push(item(id, InboxKind::Decision));
                }
            }
            if (s.tags.iter().any(|t| t == "owner") || s.who.as_str() == "me") && statuses.get(&s.key) != Some(&Status::Blocked) {
                let open = s.tasks.iter().flat_map(|t| if t.sub.is_empty() { vec![t] } else { t.sub.iter().collect() });
                for t in open.filter(|t| !t.done) {
                    let listed = |v: &Vec<InboxItem>| v.iter().any(|i| i.task_id == t.id);
                    if !listed(&approvals) && !listed(&decisions) {
                        owner.push(item(&t.id, InboxKind::Owner));
                    }
                }
            }
        }
        approvals.extend(decisions);
        approvals.extend(owner);
        Ok(approvals)
    }
}

fn task_title(s: &Stage, id: &str) -> String {
    s.tasks
        .iter()
        .flat_map(|t| std::iter::once(t).chain(t.sub.iter()))
        .find(|t| t.id == id)
        .map(|t| t.title.clone())
        .unwrap_or_default()
}
