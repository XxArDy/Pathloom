//! Review hardening: plan lock, safe validate --fix, parse tolerance, config patching, managed-section text.

use pathloom_core::{ApplyOptions, Error, Project, Stage};
use std::path::Path;

fn setup(spec: &str) -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let p = Project::init(dir.path(), "demo").unwrap();
    p.apply(&Project::parse_spec(spec).unwrap(), &ApplyOptions::default()).unwrap();
    (dir, p)
}

const PLAN: &str = "
stages:
  - {id: A, title: Alpha, tasks: [One]}
  - {id: B, title: Beta, depends: [A], tasks: [Two]}
";

#[test]
fn two_threads_add_tasks_without_lost_updates() {
    let (_d, p) = setup(PLAN);
    let handles: Vec<_> = (0..2)
        .map(|t| {
            let p = p.clone();
            std::thread::spawn(move || {
                for i in 0..50 {
                    p.add_task("A", &format!("t{t}-{i}")).unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(p.load_stage("A").unwrap().tasks.len(), 101); // the original task + 100
}

#[test]
fn validate_fix_refuses_with_unparsable_stage_and_keeps_links() {
    let (d, p) = setup(PLAN);
    let b = d.path().join("plan/stages/A.md");
    std::fs::write(&b, "not a stage file").unwrap();
    let err = p.validate_fix().unwrap_err();
    assert!(err.to_string().contains("A.md"), "{err}");
    // B still depends on A, and the cards survive.
    assert_eq!(p.load_stage("B").unwrap().depends.len(), 1);
    assert!(p.load_board("roadmap").is_ok());
}

#[test]
fn unparsable_stage_is_skipped_on_reads_but_not_overwritten() {
    let (d, p) = setup(PLAN);
    let f = d.path().join("plan/stages/A.md");
    std::fs::write(&f, "garbage").unwrap();
    let (stages, issues) = p.load_stages_report().unwrap();
    assert_eq!(stages.len(), 1);
    assert_eq!(issues.len(), 1);
    assert!(p.add_task("A", "x").is_err());
    // apply must not recreate the unreadable stage over the user's file
    let spec = Project::parse_spec("stages:\n  - {id: A, title: New}\n").unwrap();
    assert!(p.apply(&spec, &ApplyOptions::default()).is_err());
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "garbage");
}

#[test]
fn plan_dir_escape_is_refused_on_open() {
    for pd in ["../..", "/etc", "a/../..", "C:\\x", "\\x"] {
        let dir = tempfile::tempdir().unwrap();
        Project::init(dir.path(), "demo").unwrap();
        let cfg = dir.path().join(".pathloom/project.toml");
        let t = std::fs::read_to_string(&cfg).unwrap().replace("plan_dir = \"plan\"", &format!("plan_dir = {pd:?}"));
        std::fs::write(&cfg, t).unwrap();
        assert!(matches!(Project::open(dir.path()), Err(Error::OutsideProject(_))), "{pd}");
        assert!(Project::find(dir.path()).is_err(), "{pd}");
    }
}

#[test]
fn config_update_keeps_comments_and_unknown_keys() {
    let (d, mut p) = setup(PLAN);
    let cfg = d.path().join(".pathloom/project.toml");
    let mut t = std::fs::read_to_string(&cfg).unwrap();
    t.push_str("\n# my note\nfuture_key = \"keep\"\n");
    std::fs::write(&cfg, t).unwrap();
    p.set_require_evidence(true).unwrap();
    let after = std::fs::read_to_string(&cfg).unwrap();
    assert!(after.contains("# my note") && after.contains("future_key = \"keep\""), "{after}");
    assert!(Project::open(d.path()).unwrap().config.require_evidence);
}

#[test]
fn unmanaged_list_lines_in_sections_survive_a_save() {
    let md = "---\nid: A\ntitle: Alpha\nkind: stage\nwho: me\npriority: med\n---\nDesc.\n\n## Tasks\n- [ ] A-01 One\n- a plain note\n- another\n\n## Comments\n- 2026-11-01T10:00Z me \u{2014} hi\n- loose line\n";
    let s = Stage::from_markdown(Path::new("A.md"), md).unwrap();
    assert_eq!(s.tasks.len(), 1);
    assert_eq!(s.comments.len(), 1);
    assert_eq!(s.to_markdown(), md);
}

#[test]
fn apply_rejects_managed_heading_in_description() {
    let (_d, p) = setup(PLAN);
    let spec = Project::parse_spec("stages:\n  - id: A\n    description: \"x\\n## Tasks\\ny\"\n").unwrap();
    assert!(matches!(p.apply(&spec, &ApplyOptions::default()), Err(Error::InvalidSpec(_))));
}

#[test]
fn removing_a_subtask_recomputes_parent_checkbox() {
    let (_d, p) = setup(PLAN);
    p.add_subtask("A-01", "s1").unwrap();
    p.add_subtask("A-01", "s2").unwrap();
    p.set_task_done_with("A-01a", true, None).unwrap();
    assert!(!p.load_stage("A").unwrap().tasks[0].done);
    let s = p.remove_task("A-01b").unwrap();
    assert!(s.tasks[0].done, "the only remaining subtask is done, so the parent is done");
}
