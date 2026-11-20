//! Export / import of the plan (m10 A6).

use pathloom_core::{ApplyOptions, Project};

fn apply(p: &Project, spec: &pathloom_core::ApplySpec) {
    p.apply(spec, &ApplyOptions::default()).unwrap();
}

fn source() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let p = Project::init(dir.path(), "demo").unwrap();
    let spec = "
stages:
  - {id: AA, title: 'Alpha, with comma', tasks: ['one', 'say \"hi\"'], due: '2026-12-01', priority: high}
  - {id: BB, title: Beta, depends: [AA], tasks: [two], who: me}
  - {id: CC, title: Empty milestone, kind: milestone, depends: [BB]}
";
    apply(&p, &Project::parse_spec(spec).unwrap());
    p.set_task_done("AA-01", true).unwrap();
    (dir, p)
}

type Shape = Vec<(String, String, Vec<(String, bool)>, Vec<String>)>;

/// (key, title, [(task title, done)], depends) of every stage, sorted by key.
fn shape(p: &Project) -> Shape {
    let mut v: Shape = p
        .load_stages()
        .unwrap()
        .into_iter()
        .map(|s| {
            (
                s.key.clone(),
                s.title.clone(),
                s.tasks.iter().map(|t| (t.title.clone(), t.is_done())).collect(),
                s.depends.iter().map(|d| d.id.clone()).collect(),
            )
        })
        .collect();
    v.sort();
    v
}

fn fresh() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let p = Project::init(dir.path(), "other").unwrap();
    (dir, p)
}

#[test]
fn markdown_roundtrip() {
    let (_d, src) = source();
    let md = src.export_markdown().unwrap();
    assert!(md.contains("## AA: Alpha, with comma") && md.contains("- [x] one") && md.contains("Depends: AA"), "{md}");
    let (_d2, dst) = fresh();
    apply(&dst, &dst.import_markdown(&md).unwrap());
    let (want, got) = (shape(&src), shape(&dst));
    assert_eq!(got, want); // the task-less stage CC survives through its Depends line
    // second import changes nothing: same stage keys, no duplicate tasks
    apply(&dst, &dst.import_markdown(&md).unwrap());
    assert_eq!(shape(&dst), got);
}

#[test]
fn csv_roundtrip_keeps_stages_without_tasks() {
    let (_d, src) = source();
    let csv = src.export_csv().unwrap();
    assert!(csv.starts_with("stage_id,stage_title,task_id,task_title,done,who,priority,due,depends\n"));
    assert!(csv.contains("\"Alpha, with comma\"") && csv.contains("\"say \"\"hi\"\"\""), "{csv}");
    let (_d2, dst) = fresh();
    apply(&dst, &dst.import_csv(&csv).unwrap());
    assert_eq!(shape(&dst), shape(&src));
    let a = dst.load_stage("AA").unwrap();
    assert_eq!(a.due.as_deref(), Some("2026-12-01"));
    assert_eq!(a.priority, pathloom_core::Priority::High);
}

#[test]
fn imported_keys_avoid_existing_stages() {
    let (_d, p) = fresh();
    apply(&p, &Project::parse_spec("stages:\n  - {id: IMP, title: Something else, tasks: [x]}\n").unwrap());
    let spec = p.import_markdown("## Imports\n- [ ] a\n").unwrap();
    assert_ne!(spec.stages[0].id.as_deref(), Some("IMP"));
    apply(&p, &spec);
    assert_eq!(p.load_stages().unwrap().len(), 2);
}

#[test]
fn notion_csv_status_and_group() {
    let (_d, p) = fresh();
    let csv = "Name,Status,Group\nWrite docs,Done,Docs\nShip it,In progress,Release\nPublish,Not started,Release\n";
    apply(&p, &p.import_notion(csv).unwrap());
    let stages = p.load_stages().unwrap();
    let docs = stages.iter().find(|s| s.title == "Docs").unwrap();
    assert!(docs.tasks[0].done);
    let rel = stages.iter().find(|s| s.title == "Release").unwrap();
    assert_eq!(rel.tasks.len(), 2);
    assert!(rel.tasks.iter().all(|t| !t.done));
    // plain title column, no group: one stage
    assert_eq!(p.import_csv("title\nA\nB\n").unwrap().stages.len(), 1);
    assert!(p.import_csv("foo,bar\n1,2\n").is_err());
}

const TRELLO: &str = r#"{
  "lists": [{"id": "l1", "name": "Todo", "closed": false}, {"id": "l2", "name": "Old", "closed": true}, {"id": "l3", "name": "Doing"}],
  "cards": [
    {"name": "Design", "idList": "l1", "closed": false, "dueComplete": true, "due": "2026-12-05T10:00:00.000Z"},
    {"name": "Build", "idList": "l1", "closed": false, "dueComplete": false, "due": "2026-12-03T10:00:00.000Z", "idChecklists": ["c1"]},
    {"name": "Archived", "idList": "l1", "closed": true},
    {"name": "In old list", "idList": "l2"},
    {"name": "Review", "idList": "l3"}
  ],
  "checklists": [{"id": "c1", "checkItems": [{"name": "backend", "state": "complete"}, {"name": "ui", "state": "incomplete"}]}]
}"#;

#[test]
fn trello_lists_cards_and_checklists() {
    let (_d, p) = fresh();
    let spec = p.import_trello(TRELLO).unwrap();
    assert_eq!(spec.stages.len(), 2);
    apply(&p, &spec);
    let stages = p.load_stages().unwrap();
    let todo = stages.iter().find(|s| s.title == "Todo").unwrap();
    let titles: Vec<(&str, bool)> = todo.tasks.iter().map(|t| (t.title.as_str(), t.done)).collect();
    assert_eq!(titles, [("Design", true), ("Build", false), ("Build: backend", true), ("Build: ui", false)]);
    assert_eq!(todo.due.as_deref(), Some("2026-12-03"));
    assert!(p.import_trello("not json").is_err());
}

#[test]
fn trello_non_ascii_due_does_not_panic() {
    let (_d, p) = fresh();
    // Byte 10 falls inside a multi-byte character; an impossible date is ignored too.
    let json = r#"{"lists": [{"id": "l1", "name": "Todo"}],
      "cards": [{"name": "A", "idList": "l1", "due": "2026-12-0ї"}, {"name": "B", "idList": "l1", "due": "срок завтра"},
                {"name": "C", "idList": "l1", "due": "2026-02-30T00:00:00Z"}]}"#;
    let spec = p.import_trello(json).unwrap();
    assert_eq!(spec.stages[0].tasks.len(), 3);
    assert_eq!(spec.stages[0].due, None);
}

#[test]
fn github_issues_one_per_open_task() {
    let (_d, p) = source();
    let v: serde_json::Value = serde_json::from_str(&p.export_github_issues().unwrap()).unwrap();
    let titles: Vec<&str> = v.as_array().unwrap().iter().map(|i| i["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["say \"hi\"", "two"]);
    assert_eq!(v[0]["labels"][0], "AA");
    assert!(v[1]["body"].as_str().unwrap().contains("Depends on stages: AA"));
}
