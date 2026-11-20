//! Orchestrator protocol (m10 §Б1 Б2 Б3 Б5 Б8): session state, first answer wins, approval/decision/task events, event filter.

use pathloom_core::events::{ApprovalChange, DecisionChange, Event, EventFilter, EventWatcher, TaskChange, passes};
use pathloom_core::*;
use std::time::{Duration, Instant};

fn setup() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let p = Project::init(dir.path(), "demo").unwrap();
    let spec = Project::parse_spec("stages: [{id: A, title: Alpha, tasks: [one, two]}, {id: B, title: Beta, tasks: [x]}]").unwrap();
    p.apply(&spec, &ApplyOptions::default()).unwrap();
    (dir, p)
}

fn sess(state: Option<SessionState>) -> SessionRef {
    SessionRef { client: "maestro".into(), id: "s1".into(), url: Some("maestro://session/s1".into()), title: None, state, state_at: None }
}

fn wait_for(w: &EventWatcher, pred: impl Fn(&Event) -> bool) -> Option<Event> {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if let Some(e) = w.recv_timeout(Duration::from_millis(100))
            && pred(&e)
        {
            return Some(e);
        }
    }
    None
}

#[test]
fn session_state_upsert_reports_change_only_on_new_value() {
    let (_d, p) = setup();
    assert!(p.update_session("A", sess(Some(SessionState::Working))).unwrap());
    assert!(!p.update_session("A", sess(Some(SessionState::Working))).unwrap());
    assert!(!p.update_session("A", sess(None)).unwrap(), "no state given: no change");
    let s = &p.load_stage("A").unwrap().sessions;
    assert_eq!(s.len(), 1);
    assert_eq!((s[0].state, s[0].url.as_deref()), (Some(SessionState::Working), Some("maestro://session/s1")));
    assert!(s[0].state_at.is_some());
    assert!(p.update_session("A", sess(Some(SessionState::Done))).unwrap());
    assert!(p.update_session("NOPE", sess(None)).is_err());
    assert_eq!(p.load_stage("A").unwrap().sessions[0].state, Some(SessionState::Done));
}

#[test]
fn old_stage_files_without_state_still_load() {
    let (d, p) = setup();
    let path = d.path().join("plan/stages/A.md");
    let t = std::fs::read_to_string(&path).unwrap();
    let line = "sessions: [{client: maestro, id: old, url: 'maestro://x'}]\n";
    let t = t.replacen("---\n", &format!("---\n{line}"), 1);
    std::fs::write(&path, t).unwrap();
    let s = p.load_stage("A").unwrap();
    assert_eq!((s.sessions.len(), s.sessions[0].state), (1, None));
}

#[test]
fn allow_client_answers_default_and_round_trip() {
    let (d, mut p) = setup();
    assert!(!p.config.allow_client_answers);
    let toml = std::fs::read_to_string(d.path().join(".pathloom/project.toml")).unwrap();
    assert!(!toml.contains("allow_client_answers"), "{toml}");
    p.set_allow_client_answers(true).unwrap();
    assert!(Project::open(d.path()).unwrap().config.allow_client_answers);
    p.set_allow_client_answers(false).unwrap();
    assert!(!std::fs::read_to_string(d.path().join(".pathloom/project.toml")).unwrap().contains("allow_client_answers"));
}

#[test]
fn first_approval_answer_wins() {
    let (_d, p) = setup();
    p.request_approval("A-01", &[], None, "claude").unwrap();
    p.decide_approval("A-01", true, &[], None, "me").unwrap();
    let e = p.decide_approval("A-01", false, &["quality".into()], None, "telegram:bob").unwrap_err();
    match e {
        Error::AlreadyDecided { what, by, value, .. } => assert_eq!((what, by.as_str(), value.as_str()), ("approval", "me", "approved")),
        other => panic!("{other:?}"),
    }
    assert!(p.load_stage("A").unwrap().tasks[0].done, "the first answer stays");
    // a new request reopens it
    p.request_approval("A-01", &[], None, "claude").unwrap();
    p.decide_approval("A-01", false, &[], None, "me").unwrap();
    // a task without an approval record can still be approved directly by the owner
    p.decide_approval("A-02", true, &[], None, "me").unwrap();
}

#[test]
fn first_decision_answer_wins() {
    let (_d, p) = setup();
    let opts = vec![DecisionOption { label: "Yes".into(), description: String::new() }];
    p.ask_decision("B-01", "Go?", opts.clone(), None, "claude").unwrap();
    p.answer_decision("B-01", "Yes", "me").unwrap();
    let e = p.answer_decision("B-01", "No", "telegram:bob").unwrap_err();
    assert!(matches!(&e, Error::AlreadyDecided { what: "decision", by, value, .. } if by == "me" && value == "Yes"), "{e:?}");
    p.ask_decision("B-01", "Again?", opts, None, "claude").unwrap();
    p.answer_decision("B-01", "No", "me").unwrap();
}

#[test]
fn watcher_emits_approval_decision_and_returned_events() {
    let (_d, p) = setup();
    let w = EventWatcher::start(&p).unwrap();

    p.request_approval("A-01", &[], Some("look"), "claude").unwrap();
    let e = wait_for(&w, |e| matches!(e, Event::Approval { .. })).expect("requested");
    assert!(
        matches!(&e, Event::Approval { task_id, change: ApprovalChange::Requested, state: ApprovalState::Pending, by, .. } if task_id == "A-01" && by == "claude"),
        "{e:?}"
    );

    p.decide_approval("A-01", false, &["quality".into()], Some("Redo it"), "me").unwrap();
    let e = wait_for(&w, |e| matches!(e, Event::Approval { .. })).expect("decided");
    assert!(matches!(&e, Event::Approval { change: ApprovalChange::Decided, state: ApprovalState::Rejected, by, .. } if by == "me"), "{e:?}");
    let e = wait_for(&w, |e| matches!(e, Event::Task { .. })).expect("returned");
    let Event::Task { task_id, change, comment, reasons, by, .. } = e else { unreachable!() };
    assert_eq!(
        (task_id.as_str(), change, comment.as_str(), reasons, by.as_str()),
        ("A-01", TaskChange::Returned, "Redo it", vec!["quality".to_string()], "me")
    );

    // an approval does not produce a `returned`
    p.request_approval("A-02", &[], None, "claude").unwrap();
    wait_for(&w, |e| matches!(e, Event::Approval { .. })).expect("requested 2");
    p.decide_approval("A-02", true, &[], None, "me").unwrap();
    wait_for(&w, |e| matches!(e, Event::Approval { change: ApprovalChange::Decided, state: ApprovalState::Approved, .. })).expect("approved");
    assert!(wait_for(&w, |e| matches!(e, Event::Task { .. })).is_none());

    let opts = vec![DecisionOption { label: "Yes".into(), description: String::new() }];
    p.ask_decision("B-01", "Go?", opts, None, "claude").unwrap();
    let e = wait_for(&w, |e| matches!(e, Event::Decision { .. })).expect("asked");
    assert!(matches!(&e, Event::Decision { change: DecisionChange::Asked, choice: None, by, .. } if by == "claude"), "{e:?}");
    p.answer_decision("B-01", "Yes", "me").unwrap();
    let e = wait_for(&w, |e| matches!(e, Event::Decision { .. })).expect("answered");
    assert!(matches!(&e, Event::Decision { change: DecisionChange::Answered, choice: Some(c), by, .. } if c == "Yes" && by == "me"), "{e:?}");

    let j: serde_json::Value = serde_json::from_str(&e.to_json_line()).unwrap();
    assert_eq!((j["type"].as_str(), j["task_id"].as_str(), j["change"].as_str()), (Some("decision"), Some("B-01"), Some("answered")));
}

#[test]
fn watcher_does_not_replay_old_approvals() {
    let (_d, p) = setup();
    p.request_approval("A-01", &[], None, "claude").unwrap();
    let w = EventWatcher::start(&p).unwrap();
    p.add_task("A", "three").unwrap();
    wait_for(&w, |e| matches!(e, Event::Stage { .. })).expect("stage updated");
    assert!(wait_for(&w, |e| matches!(e, Event::Approval { .. })).is_none());
}

fn ev_stage(key: &str) -> Event {
    Event::Stage { key: key.into(), change: pathloom_core::events::Change::Updated, at: "t".into() }
}

fn ev_approval(task: &str) -> Event {
    Event::Approval { task_id: task.into(), change: ApprovalChange::Requested, state: ApprovalState::Pending, by: "a".into(), at: "t".into() }
}

#[test]
fn filter_passes() {
    let f = |stages: &[&str], types: &[&str]| EventFilter {
        stages: stages.iter().map(ToString::to_string).collect(),
        types: types.iter().map(ToString::to_string).collect(),
    };
    let none = f(&[], &[]);
    assert!(passes(&ev_stage("A"), &none) && passes(&Event::Agents { at: "t".into() }, &none));
    assert!(passes(&Event::hello(), &f(&["A"], &["approval"])), "hello always passes");
    let t = f(&[], &["approval", "task"]);
    assert!(passes(&ev_approval("A-01"), &t) && !passes(&ev_stage("A"), &t));
    // events with a stage key or task id are filtered by stage, the others pass
    let s = f(&["A"], &[]);
    assert!(passes(&ev_stage("A"), &s) && !passes(&ev_stage("B"), &s));
    assert!(passes(&ev_approval("A-02a"), &s) && !passes(&ev_approval("B-01"), &s));
    assert!(passes(&Event::Agents { at: "t".into() }, &s) && passes(&Event::Claims { claims: vec![], at: "t".into() }, &s));
    let act = |stages: &[&str], tasks: &[&str]| Event::Activity {
        entry: LogEntry {
            stages: stages.iter().map(ToString::to_string).collect(),
            tasks: tasks.iter().map(ToString::to_string).collect(),
            ..Default::default()
        },
    };
    assert!(passes(&act(&["A"], &[]), &s) && !passes(&act(&["B"], &[]), &s));
    assert!(passes(&act(&[], &["A-01"]), &s) && !passes(&act(&[], &["B-01"]), &s));
    assert!(passes(&act(&[], &[]), &s));
    let both = f(&["A", "B"], &["stage"]);
    assert!(passes(&ev_stage("B"), &both) && !passes(&ev_stage("C"), &both) && !passes(&ev_approval("A-01"), &both));
    assert!(f(&[], &["stage", "task"]).validate().is_ok());
    assert!(f(&[], &["nope"]).validate().is_err());
}
