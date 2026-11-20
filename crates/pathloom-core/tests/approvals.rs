//! Approvals, review, decisions and inbox (m9 contract §2, §3, §9).

use pathloom_core::{ApplyOptions, ApprovalState, DecisionOption, Evidence, InboxKind, Project, Stage, TaskKind};
use std::path::Path;

fn setup(spec: &str) -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let p = Project::init(dir.path(), "demo").unwrap();
    p.apply(&Project::parse_spec(spec).unwrap(), &ApplyOptions::default()).unwrap();
    (dir, p)
}

const PLAN: &str = "
stages:
  - {id: APPR, title: Approvals, tasks: [Icon, Logo]}
  - {id: DECIDE, title: Decisions, tasks: [Database]}
  - {id: DEV, title: Dev, tasks: [Build, Ship]}
";

fn opts(label: &str) -> Vec<DecisionOption> {
    vec![DecisionOption { label: label.into(), description: "local".into() }, DecisionOption { label: "Postgres".into(), ..Default::default() }]
}

#[test]
fn new_task_info_fields_roundtrip() {
    let text = "---
id: APPR
title: Approvals
kind: stage
who: ai
priority: med
task_info: {APPR-01: {kind: approval, note: Look, files: [shots/a.png], approval: {state: rejected, reasons: [off-theme, quality], comment: Darker, at: \"2026-11-20T10:00:00Z\", by: me, requested_by: claude, requested_at: \"2026-11-20T09:00:00Z\"}}, APPR-02: {kind: decision, decision: {question: Which DB?, options: [{label: SQLite, description: local}, {label: Postgres}], recommended: 0, choice: SQLite, at: \"2026-11-20T10:00:00Z\", by: me, asked_by: claude}}}
---

## Tasks
- [ ] APPR-01 Icon
- [x] APPR-02 DB
";
    let s = Stage::from_markdown(Path::new("APPR.md"), text).unwrap();
    let a = s.task_info["APPR-01"].approval.as_ref().unwrap();
    assert_eq!((a.state, a.reasons.len(), a.comment.as_str(), a.by.as_str()), (ApprovalState::Rejected, 2, "Darker", "me"));
    assert_eq!((a.requested_by.as_str(), a.requested_at.as_str()), ("claude", "2026-11-20T09:00:00Z"));
    let d = s.task_info["APPR-02"].decision.as_ref().unwrap();
    assert_eq!((d.choice.as_deref(), d.recommended, d.asked_by.as_str(), d.options.len()), (Some("SQLite"), Some(0), "claude", 2));
    assert_eq!(s.task_kind("APPR-02"), Some(TaskKind::Decision));
    assert_eq!(s.task_kind("APPR-01"), Some(TaskKind::Approval));
    // write and read again: same model, same text
    let md = s.to_markdown();
    let again = Stage::from_markdown(Path::new("APPR.md"), &md).unwrap();
    assert_eq!(again.task_info, s.task_info);
    assert_eq!(again.to_markdown(), md);
    for key in ["kind:", "approval:", "reasons:", "requested_by:", "decision:", "question:", "options:", "recommended:", "choice:", "asked_by:"] {
        assert!(md.contains(key), "{key} missing in\n{md}");
    }
}

#[test]
fn task_kind_by_stage_key() {
    let (_d, p) = setup(PLAN);
    assert_eq!(p.load_stage("APPR").unwrap().task_kind("APPR-01"), Some(TaskKind::Approval));
    assert_eq!(p.load_stage("DECIDE").unwrap().task_kind("DECIDE-01"), Some(TaskKind::Decision));
    assert_eq!(p.load_stage("DEV").unwrap().task_kind("DEV-01"), None);
}

#[test]
fn old_stage_stays_byte_identical() {
    let text = "---\nid: DEV\ntitle: Dev\nkind: stage\nwho: ai\npriority: med\ntask_info: {DEV-01: {note: Use PKCE, files: [docs/a.md]}}\n---\n\n## Tasks\n- [ ] DEV-01 Login\n";
    let (_d, p) = setup("stages: [{id: DEV, title: Dev, tasks: [Login]}]");
    std::fs::write(p.plan_dir().join("stages/DEV.md"), text).unwrap();
    let s = p.load_stage("DEV").unwrap();
    p.save_stage(&s).unwrap();
    assert_eq!(p.stage_source("DEV").unwrap(), text);
}

#[test]
fn invalid_hand_edited_approval_is_ignored_and_kept() {
    let (_d, p) = setup(PLAN);
    let path = p.plan_dir().join("stages/DEV.md");
    let text = std::fs::read_to_string(&path).unwrap().replacen("priority: med\n", "priority: med\ntask_info: {DEV-01: {approval: {state: maybe}}}\n", 1);
    assert!(text.contains("state: maybe"));
    std::fs::write(&path, &text).unwrap();
    let s = p.load_stage("DEV").unwrap();
    assert!(s.task_info.is_empty());
    assert!(p.approvals(None).unwrap().is_empty());
    p.add_comment("DEV", "me", "hi").unwrap();
    assert!(p.stage_source("DEV").unwrap().contains("state: maybe"), "raw value preserved");
}

#[test]
fn request_reject_request_approve() {
    let (_d, p) = setup(PLAN);
    let s = p.request_approval("APPR-01", &["shots/a.png".into(), "shots/a.png".into()], Some("New icon"), "claude").unwrap();
    assert!(s.in_review("APPR-01"));
    assert_eq!(s.task_info["APPR-01"].files, ["shots/a.png"]);
    assert_eq!(p.inbox().unwrap().iter().filter(|i| i.kind == InboxKind::Approval).count(), 1);

    let s = p.decide_approval("APPR-01", false, &["off-theme".into(), "quality".into()], Some("Darker"), "me").unwrap();
    assert!(!s.tasks[0].done && !s.in_review("APPR-01"));
    let a = s.task_info["APPR-01"].approval.as_ref().unwrap();
    assert_eq!((a.state, a.reasons.as_slice(), a.comment.as_str()), (ApprovalState::Rejected, &["off-theme".to_string(), "quality".into()][..], "Darker"));
    let c = s.comments.last().unwrap();
    assert_eq!((c.author.as_str(), c.text.as_str()), ("me", "rejected APPR-01 [off-theme, quality]: Darker"));
    assert_eq!(p.approvals(Some(ApprovalState::Rejected)).unwrap()[0].reasons, ["off-theme", "quality"]);

    let s = p.request_approval("APPR-01", &[], None, "claude").unwrap();
    let a = s.task_info["APPR-01"].approval.as_ref().unwrap();
    assert!(a.state == ApprovalState::Pending && a.reasons.is_empty() && a.comment.is_empty());
    let s = p.decide_approval("APPR-01", true, &[], None, "me").unwrap();
    assert!(s.tasks[0].done);
    assert_eq!(s.task_info["APPR-01"].approval.as_ref().unwrap().state, ApprovalState::Approved);
    assert!(p.inbox().unwrap().iter().all(|i| i.kind != InboxKind::Approval));
    assert!(p.decide_approval("APPR-01", false, &["x".repeat(41)], None, "me").is_err());
}

#[test]
fn review_keeps_evidence_and_stage_active() {
    let (_d, p) = setup(PLAN);
    let ev = Evidence { command: "cargo test".into(), exit_code: 0, duration_ms: None, summary: None, log: None, at: String::new(), by: String::new(), commit: None, branch: None };
    let s = p.submit_for_review("DEV-01", Some(ev), "claude").unwrap();
    assert!(s.in_review("DEV-01") && !s.tasks[0].done);
    assert!(s.evidence.contains_key("DEV-01"));
    assert!(p.stage_source("DEV").unwrap().contains("cargo test"), "evidence is written for a task in review");
    assert_eq!(pathloom_core::compute_statuses(&p.load_stages().unwrap())["DEV"], pathloom_core::Status::Active);
    assert!(p.next_tasks(0).unwrap().iter().all(|t| t.task_id != "DEV-01"));
    let s = p.decide_approval("DEV-01", true, &[], None, "me").unwrap();
    assert!(s.tasks[0].done && s.evidence.contains_key("DEV-01"));
}

#[test]
fn decision_ask_answer() {
    let (_d, p) = setup(PLAN);
    assert!(p.ask_decision("DECIDE-01", "Q?", vec![], None, "claude").is_err());
    assert!(p.ask_decision("DECIDE-01", "Q?", opts("SQLite"), Some(2), "claude").is_err());
    p.ask_decision("DECIDE-01", "Which DB?", opts("SQLite"), Some(0), "claude").unwrap();
    assert_eq!(p.decisions(true).unwrap().len(), 1);
    assert_eq!(p.inbox().unwrap().iter().filter(|i| i.kind == InboxKind::Decision).count(), 1);
    let s = p.answer_decision("DECIDE-01", "SQLite", "me").unwrap();
    let d = s.task_info["DECIDE-01"].decision.as_ref().unwrap();
    assert_eq!((d.choice.as_deref(), d.by.as_str()), (Some("SQLite"), "me"));
    assert!(s.tasks[0].done);
    assert!(p.decisions(true).unwrap().is_empty() && p.decisions(false).unwrap().len() == 1);
    // asking again clears the choice and reopens the task
    let s = p.ask_decision("DECIDE-01", "Again?", opts("A"), None, "claude").unwrap();
    assert!(!s.tasks[0].done && s.task_info["DECIDE-01"].decision.as_ref().unwrap().choice.is_none());
}

#[test]
fn inbox_collects_owner_work() {
    let (_d, p) = setup(
        "
stages:
  - {id: APPR, title: Approvals, tasks: [Icon]}
  - {id: DECIDE, title: Decisions, tasks: [Database]}
  - {id: OWN, title: Mine, who: me, tasks: [Pay, Sign]}
  - {id: BLK, title: Blocked, who: me, tasks: [Wait], depends: [OWN]}
  - {id: TAG, title: Tagged, who: ai, tasks: [Look]}
  - {id: AI, title: Agent work, who: ai, tasks: [Code]}
",
    );
    let path = p.plan_dir().join("stages/TAG.md");
    let t = std::fs::read_to_string(&path).unwrap().replace("who: ai\n", "who: ai\ntags: [owner]\n");
    std::fs::write(&path, t).unwrap();
    p.request_approval("APPR-01", &[], None, "claude").unwrap();
    p.ask_decision("DECIDE-01", "Q?", opts("A"), None, "claude").unwrap();
    p.set_task_done("OWN-02", true).unwrap();
    let got: Vec<(String, InboxKind)> = p.inbox().unwrap().into_iter().map(|i| (i.task_id, i.kind)).collect();
    assert_eq!(
        got,
        [
            ("APPR-01".to_string(), InboxKind::Approval),
            ("DECIDE-01".into(), InboxKind::Decision),
            ("OWN-01".into(), InboxKind::Owner),
            ("TAG-01".into(), InboxKind::Owner),
        ]
    );
}

#[test]
fn require_approval_setting_roundtrips() {
    let (_d, mut p) = setup(PLAN);
    assert!(!std::fs::read_to_string(p.root.join(".pathloom/project.toml")).unwrap().contains("require_approval"));
    p.set_require_approval(true).unwrap();
    assert!(Project::open(&p.root).unwrap().config.require_approval);
    p.set_require_approval(false).unwrap();
    assert!(!std::fs::read_to_string(p.root.join(".pathloom/project.toml")).unwrap().contains("require_approval"));
}
