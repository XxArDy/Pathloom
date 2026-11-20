//! Agenda buckets and daily digest (m10 A3, A4).

use pathloom_core::{ApplyOptions, LogEntry, Project, Status};

fn setup() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let p = Project::init(dir.path(), "demo").unwrap();
    let spec = "
stages:
  - {id: LATE, title: Late, tasks: [a], due: '2026-11-18'}
  - {id: TODAY, title: Today, tasks: [a], due: '2026-11-20'}
  - {id: WEEK, title: Week, tasks: [a], due: '2026-11-26'}
  - {id: FAR, title: Far, tasks: [a], due: '2026-11-27'}
  - {id: DONE, title: Done, tasks: [a], due: '2026-11-01'}
  - {id: BLK, title: Blocked, tasks: [a], depends: [FAR]}
  - {id: MINE, title: Mine, tasks: [a], who: me}
";
    p.apply(&Project::parse_spec(spec).unwrap(), &ApplyOptions::default()).unwrap();
    p.set_task_done("DONE-01", true).unwrap();
    (dir, p)
}

/// Set every claim's `at` / `heartbeat_at` far in the past (expiry stays in the future).
fn age(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(m) => {
            for (k, x) in m.iter_mut() {
                if k == "at" || k == "heartbeat_at" {
                    *x = "2020-01-01T00:00:00Z".into();
                } else {
                    age(x);
                }
            }
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(age),
        _ => {}
    }
}

fn keys(v: &[pathloom_core::AgendaItem]) -> Vec<&str> {
    v.iter().map(|i| i.key.as_str()).collect()
}

#[test]
fn buckets() {
    let (_d, p) = setup();
    let a = p.agenda("2026-11-20", 1).unwrap();
    assert_eq!(keys(&a.overdue), ["LATE"]);
    assert_eq!(keys(&a.due), ["TODAY"]);
    assert_eq!(keys(&a.blocked), ["BLK"]);
    assert_eq!(a.blocked[0].status, Status::Blocked);
    assert!(a.waiting.iter().any(|i| i.task_id == "MINE-01"));

    let w = p.agenda("2026-11-20", 7).unwrap(); // [20, 27): FAR (27th) stays out
    assert_eq!(keys(&w.due), ["TODAY", "WEEK"]);
    assert!(!keys(&w.overdue).contains(&"DONE") && !keys(&w.due).contains(&"DONE"), "done stages are excluded");
    assert!(p.agenda("2026-02-30", 1).is_err());
}

#[test]
fn digest_contents() {
    let (_d, p) = setup();
    let e = |at: &str, summary: &str, action: &str| {
        p.log_action(&LogEntry { at: at.into(), author: "claude".into(), action: action.into(), summary: summary.into(), ..Default::default() })
            .unwrap()
    };
    e("2026-11-18T10:00:00Z", "OLD-1 done", "task done");
    e("2026-11-20T08:00:00Z", "TODAY-1 done", "task done");
    e("2026-11-20T08:30:00Z", "comment on LATE", "add_comment");
    e("2026-11-20T09:00:00Z", "approved X-1", "approval approve");
    let d = p.digest("2026-11-19T00:00:00Z", "2026-11-20").unwrap();
    let done: Vec<&str> = d.done.iter().map(|e| e.summary.as_str()).collect();
    assert_eq!(done, ["TODAY-1 done", "approved X-1"]);
    assert_eq!(keys(&d.stuck.overdue), ["LATE"]);
    assert_eq!(keys(&d.stuck.blocked), ["BLK"]);
    assert!(d.stuck.silent_claims.is_empty());
    let md = d.to_markdown();
    assert!(md.contains("### Done") && md.contains("### Overdue") && md.contains("LATE"), "{md}");

    // A silent claim shows up as stuck.
    p.claim_task("LATE-01", "claude", 3600, "").unwrap();
    let path = p.root.join(".pathloom/claims.json");
    let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    age(&mut v);
    std::fs::write(&path, v.to_string()).unwrap();
    let d = p.digest("2026-11-19T00:00:00Z", "2026-11-20").unwrap();
    assert_eq!(d.stuck.silent_claims.len(), 1, "{:?}", d.stuck);
}

#[test]
fn empty_digest_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let p = Project::init(dir.path(), "x").unwrap();
    assert!(p.digest("2026-01-01T00:00:00Z", "2026-11-20").unwrap().to_markdown().contains("Nothing to report"));
}

#[test]
fn telegram_config_roundtrip() {
    let old = "chat_id = \"1\"";
    let c: pathloom_core::TelegramConfig = toml::from_str(old).unwrap();
    assert_eq!(c.digest, None);
    assert!(!toml::to_string(&c).unwrap().contains("digest"));
    let c: pathloom_core::TelegramConfig = toml::from_str("chat_id = \"1\"\ndigest = \"09:00\"").unwrap();
    assert_eq!(c.digest.as_deref(), Some("09:00"));
}
