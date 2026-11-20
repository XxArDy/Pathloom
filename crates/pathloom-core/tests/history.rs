//! Change history and per-entry revert (m10 A7).

use pathloom_core::{ApplyOptions, Error, LogEntry, Project};

fn project() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let p = Project::init(dir.path(), "demo").unwrap();
    (dir, p)
}

fn edit(p: &Project, spec: &str, action: &str) {
    p.apply(&Project::parse_spec(spec).unwrap(), &ApplyOptions::default()).unwrap();
    p.log_action(&LogEntry { action: action.into(), source: "test".into(), ..Default::default() }).unwrap();
}

fn stage_file(dir: &tempfile::TempDir) -> std::path::PathBuf {
    dir.path().join("plan/stages/A.md")
}

#[test]
fn edit_is_recorded_and_revert_restores() {
    let (d, p) = project();
    assert!(p.history(0).unwrap().is_empty());
    edit(&p, "stages:\n  - {id: A, title: Alpha, tasks: [one]}\n", "apply");
    let h = p.history(0).unwrap();
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].action, "apply");
    let paths: Vec<&str> = h[0].files.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(&"plan/stages/A.md") && paths.contains(&"plan/boards/roadmap.board.json"), "{paths:?}");
    assert!(d.path().join(".pathloom/history").is_dir());
    assert!(std::fs::read_to_string(d.path().join(".pathloom/.gitignore")).unwrap().contains("history/"));

    edit(&p, "stages:\n  - {id: B, title: Beta, tasks: [two]}\n", "apply");
    // revert the newest (B created): B.md disappears, A stays
    let newest = p.history(1).unwrap().remove(0);
    p.revert(&newest.id, "tester", "test").unwrap();
    assert!(!d.path().join("plan/stages/B.md").exists() && stage_file(&d).exists());
    // the revert is itself an entry and can be reverted
    let h = p.history(0).unwrap();
    assert_eq!(h[0].action, "revert");
    p.revert(&h[0].id, "tester", "test").unwrap();
    assert!(d.path().join("plan/stages/B.md").exists());
}

#[test]
fn revert_after_later_edit_is_a_conflict() {
    let (d, p) = project();
    edit(&p, "stages:\n  - {id: A, title: Alpha, tasks: [one]}\n", "apply");
    let first = p.history(1).unwrap().remove(0);
    edit(&p, "stages:\n  - {id: A, tasks: [two]}\n", "apply");
    let before = std::fs::read_to_string(stage_file(&d)).unwrap();
    match p.revert(&first.id, "tester", "test") {
        Err(Error::HistoryConflict { paths, .. }) => assert!(paths.contains(&"plan/stages/A.md".to_string())),
        other => panic!("{other:?}"),
    }
    assert_eq!(std::fs::read_to_string(stage_file(&d)).unwrap(), before);
    assert!(matches!(p.revert("nope", "t", "t"), Err(Error::NotFound(_))));
    assert!(matches!(p.revert("../x", "t", "t"), Err(Error::NotFound(_))));
}

#[test]
fn old_project_without_history_works() {
    let (d, p) = project();
    // a project created before the feature: no baseline, no history folder
    let _ = std::fs::remove_dir_all(d.path().join(".pathloom/cache"));
    let _ = std::fs::remove_dir_all(d.path().join(".pathloom/history"));
    assert!(p.history(0).unwrap().is_empty());
    // the first action only creates the baseline
    edit(&p, "stages:\n  - {id: A, title: Alpha, tasks: [one]}\n", "apply");
    assert!(p.history(0).unwrap().is_empty());
    edit(&p, "stages:\n  - {id: A, tasks: [two]}\n", "apply");
    assert_eq!(p.history(0).unwrap().len(), 1);
}

#[test]
fn history_is_trimmed_by_size() {
    let (_d, p) = project();
    for i in 0..3 {
        edit(&p, &format!("stages:\n  - {{id: A, title: Alpha, tasks: [t{i}]}}\n"), "apply");
    }
    assert_eq!(p.history(0).unwrap().len(), 3);
    p.trim_history(1);
    assert_eq!(p.history(0).unwrap().len(), 1); // the newest always stays
}
