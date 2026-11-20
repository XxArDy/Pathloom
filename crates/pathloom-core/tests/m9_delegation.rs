//! m9 §4-6: task status, evidence commit, usage per stage/model and weekly quota.

use pathloom_core::{Error, Evidence, LogEntry, Project, TaskStep, Usage};

fn setup() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let p = Project::init(dir.path(), "demo").unwrap();
    let spec = Project::parse_spec("stages: [{id: A, title: Alpha, tasks: [one, two]}]").unwrap();
    p.apply(&spec, &Default::default()).unwrap();
    (dir, p)
}

#[test]
fn task_status_round_trip_and_validation() {
    let (_d, p) = setup();
    let ev = vec!["shots/x.png".to_string()];
    p.set_task_status("A-01", TaskStep::Pushed, Some("feat/x"), Some("3F2A9C1"), &ev, "claude").unwrap();
    let s = p.load_stage("A").unwrap();
    let st = s.task_info["A-01"].status.as_ref().unwrap();
    assert_eq!((st.step, st.sha.as_deref(), st.by.as_str()), (TaskStep::Pushed, Some("3f2a9c1"), "claude"));
    assert!(!st.at.is_empty());
    assert!(p.stage_source("A").unwrap().contains("status: {step: pushed"));
    // replaced by the next step
    p.set_task_status("A-01", TaskStep::Merged, None, None, &[], "claude").unwrap();
    assert_eq!(p.load_stage("A").unwrap().task_info["A-01"].status.as_ref().unwrap().step, TaskStep::Merged);
    // invalid input: nothing written
    let before = p.stage_source("A").unwrap();
    for sha in ["abc", "xyz1234", &"a".repeat(41)] {
        assert!(matches!(p.set_task_status("A-01", TaskStep::Pushed, None, Some(sha), &[], "c"), Err(Error::InvalidSpec(_))), "{sha}");
    }
    let many: Vec<String> = (0..11).map(|i| format!("f{i}.png")).collect();
    assert!(p.set_task_status("A-01", TaskStep::Pushed, None, None, &many, "c").is_err());
    assert!(p.set_task_status("A-01", TaskStep::Pushed, None, None, &["../x".to_string()], "c").is_err());
    assert!(matches!(p.set_task_status("A-99", TaskStep::Pushed, None, None, &[], "c"), Err(Error::NotFound(_))));
    assert_eq!(p.stage_source("A").unwrap(), before);
}

#[test]
fn stages_without_status_stay_byte_identical_and_bad_status_is_ignored() {
    let (d, p) = setup();
    p.set_task_note("A-01", "hello").unwrap();
    let before = p.stage_source("A").unwrap();
    assert!(!before.contains("status"));
    p.set_task_note("A-01", "hello").unwrap();
    p.add_task_file("A-01", "a.png").unwrap();
    p.remove_task_file("A-01", "a.png").unwrap();
    assert_eq!(p.stage_source("A").unwrap(), before);
    // a hand-edited status with a bad sha does not break the stage and is kept as written
    let path = d.path().join("plan/stages/A.md");
    let bad = before.replace("note: hello", "note: hello, status: {step: pushed, sha: nothex}");
    assert_ne!(bad, before, "{before}");
    std::fs::write(&path, &bad).unwrap();
    assert!(p.load_stage("A").is_ok());
    p.add_task("A", "three").unwrap();
    assert!(p.stage_source("A").unwrap().contains("sha: nothex"));
}

#[test]
fn evidence_commit_is_validated_and_stored() {
    let (_d, p) = setup();
    let ev = |commit: &str| Evidence {
        command: "cargo test".into(),
        exit_code: 0,
        duration_ms: None,
        summary: None,
        log: None,
        commit: Some(commit.into()),
        branch: None,
        at: String::new(),
        by: "c".into(),
    };
    assert!(p.set_task_done_with("A-01", true, Some(ev("nothex!"))).is_err());
    assert!(p.set_task_done_with("A-01", true, Some(ev("abc12"))).is_err());
    p.set_task_done_with("A-01", true, Some(ev("ABCDEF1234"))).unwrap();
    assert_eq!(p.load_stage("A").unwrap().evidence["A-01"].commit.as_deref(), Some("abcdef1234"));
    assert!(p.stage_source("A").unwrap().contains("commit: abcdef1234"));
}

fn usage_entry(p: &Project, author: &str, at: &str, stages: &[&str], u: Usage) {
    p.log_action(&LogEntry {
        at: at.into(),
        author: author.into(),
        action: "usage".into(),
        stages: stages.iter().map(|s| s.to_string()).collect(),
        usage: Some(u),
        ..Default::default()
    })
    .unwrap();
}

#[test]
fn usage_by_stage_model_and_latest_quota() {
    let (_d, p) = setup();
    let u = |tin: u64, model: Option<&str>| Usage { tokens_in: tin, model: model.map(String::from), ..Default::default() };
    usage_entry(&p, "claude", "2026-11-01T00:00:00Z", &["A"], u(100, Some("opus")));
    usage_entry(&p, "claude", "2026-11-01T00:01:00Z", &["A"], u(50, Some("opus")));
    usage_entry(&p, "claude", "2026-11-01T00:02:00Z", &["A"], u(7, None));
    usage_entry(&p, "codex", "2026-11-01T00:03:00Z", &["A", "B"], u(20, Some("gpt")));
    let quota = |pct: f64, reset: &str| Usage { percent_weekly: Some(pct), resets_at: Some(reset.into()), ..Default::default() };
    usage_entry(&p, "claude", "2026-11-02T00:00:00Z", &[], quota(40.0, "2026-11-08T00:00:00Z"));
    usage_entry(&p, "claude", "2026-11-03T00:00:00Z", &[], quota(55.5, "2026-11-08T00:00:00Z"));
    usage_entry(&p, "codex", "2026-11-02T12:00:00Z", &[], quota(10.0, "2026-11-09T00:00:00Z"));
    let s = p.usage_summary().unwrap();
    assert_eq!(s.by_stage_model["A"]["opus"].tokens_in, 150);
    assert_eq!(s.by_stage_model["A"]["?"].tokens_in, 7);
    assert_eq!(s.by_stage_model["A"]["gpt"].tokens_in, 10);
    assert_eq!(s.by_stage_model["B"]["gpt"].tokens_in, 10);
    assert_eq!(s.quota["claude"].percent_weekly, 55.5);
    assert_eq!(s.quota["claude"].at, "2026-11-03T00:00:00Z");
    assert_eq!(s.quota["codex"].percent_weekly, 10.0);
    // existing totals unchanged
    assert_eq!(s.total.tokens_in, 177);
    assert_eq!(s.by_stage["A"].tokens_in, 167);

    for bad in [-1.0, 100.5, f64::NAN] {
        assert!(Usage { percent_weekly: Some(bad), ..Default::default() }.validate().is_err(), "{bad}");
    }
    assert!(Usage { percent_weekly: Some(100.0), ..Default::default() }.validate().is_ok());
    assert!(Usage { model: Some(" ".into()), ..Default::default() }.validate().is_err());
}
