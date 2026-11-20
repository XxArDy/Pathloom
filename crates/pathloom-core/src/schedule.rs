//! Timeline with the critical path (m10 contract A5).
//!
//! Forward pass from today over the hard dependencies (soft ones are ignored), backward pass from the project
//! finish. A stage is critical when its total float is zero. A done stage takes no time and is never critical.

use crate::agenda::{date_to_days, days_to_date};
use crate::error::{Error, Result};
use crate::model::Status;
use crate::project::Project;
use crate::status::stage_views;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScheduleItem {
    pub key: String,
    pub title: String,
    pub start: String,
    pub finish: String,
    /// Days between `finish` and `due` (negative = late); `None` without a due date.
    pub slack: Option<i64>,
    /// Days the stage may slip without moving the project finish.
    pub float: i64,
    pub critical: bool,
    /// Days used: the stage's `estimate`, 1 by default, 0 when done.
    pub estimate: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Schedule {
    pub today: String,
    /// Sorted by start, then key.
    pub items: Vec<ScheduleItem>,
    pub finish: String,
}

impl Project {
    pub fn schedule(&self, today: &str) -> Result<Schedule> {
        let t0 = date_to_days(today).ok_or_else(|| Error::InvalidSpec(format!("bad date `{today}` (use YYYY-MM-DD)")))?;
        let views = stage_views(&self.load_stages()?);
        let idx: HashMap<&str, usize> = views.iter().enumerate().map(|(i, v)| (v.stage.key.as_str(), i)).collect();
        let n = views.len();
        let dur: Vec<i64> = views
            .iter()
            .map(|v| if v.status == Status::Done { 0 } else { i64::from(v.stage.estimate.unwrap_or(1)) })
            .collect();
        // preds[i]: hard prerequisites of i (known keys only).
        let preds: Vec<Vec<usize>> = views
            .iter()
            .map(|v| v.stage.depends.iter().filter(|d| !d.soft).filter_map(|d| idx.get(d.id.as_str()).copied()).collect())
            .collect();
        let mut succs = vec![Vec::new(); n];
        for (i, ps) in preds.iter().enumerate() {
            for &p in ps {
                succs[p].push(i);
            }
        }
        // Topological order (Kahn); stages left in a cycle keep the start `today`.
        let mut indeg: Vec<usize> = preds.iter().map(Vec::len).collect();
        let mut order: Vec<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
        let mut h = 0;
        while h < order.len() {
            let i = order[h];
            h += 1;
            for &s in &succs[i] {
                indeg[s] -= 1;
                if indeg[s] == 0 {
                    order.push(s);
                }
            }
        }
        let mut es = vec![t0; n];
        let mut ef = vec![t0; n];
        for &i in &order {
            if dur[i] > 0 {
                es[i] = preds[i].iter().map(|&p| ef[p]).fold(t0, i64::max);
            }
            ef[i] = es[i] + dur[i];
        }
        let end = ef.iter().copied().fold(t0, i64::max);
        let mut lf = vec![end; n];
        for &i in order.iter().rev() {
            lf[i] = succs[i].iter().map(|&s| lf[s] - dur[s]).fold(end, i64::min);
        }
        let mut items: Vec<ScheduleItem> = (0..n)
            .map(|i| {
                let float = lf[i] - dur[i] - es[i];
                ScheduleItem {
                    key: views[i].stage.key.clone(),
                    title: views[i].stage.title.clone(),
                    start: days_to_date(es[i]),
                    finish: days_to_date(ef[i]),
                    slack: views[i].stage.due.as_deref().and_then(|d| d.get(..10)).and_then(date_to_days).map(|d| d - ef[i]),
                    float,
                    critical: float == 0 && views[i].status != Status::Done,
                    estimate: dur[i] as u32,
                }
            })
            .collect();
        items.sort_by(|a, b| (&a.start, &a.key).cmp(&(&b.start, &b.key)));
        Ok(Schedule { today: today.into(), items, finish: days_to_date(end) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply::{ApplySpec, DepSpec, StageSpec};

    const T: &str = "2026-11-20";

    /// `(key, estimate, deps, due)`; `done` keys get one finished task.
    fn project(spec: &[(&str, u32, &[&str], Option<&str>)], done: &[&str]) -> (tempfile::TempDir, Project) {
        let dir = tempfile::tempdir().unwrap();
        let p = Project::init(dir.path(), "t").unwrap();
        let stages = spec
            .iter()
            .map(|(k, e, deps, due)| StageSpec {
                id: Some((*k).into()),
                title: Some((*k).into()),
                estimate: Some(*e),
                due: due.map(String::from),
                depends: Some(deps.iter().map(|d| DepSpec::Key((*d).into())).collect()),
                tasks: vec!["t".into()],
                ..Default::default()
            })
            .collect();
        p.apply(&ApplySpec { stages, ..Default::default() }, &Default::default()).unwrap();
        for k in done {
            let s = p.load_stage(k).unwrap();
            p.set_task_done(&s.tasks[0].id, true).unwrap();
        }
        (dir, p)
    }

    fn row(s: &Schedule, k: &str) -> (String, String, i64, bool) {
        let i = s.items.iter().find(|i| i.key == k).unwrap();
        (i.start.clone(), i.finish.clone(), i.float, i.critical)
    }

    #[test]
    fn chain_is_all_critical() {
        let (_d, p) = project(&[("A", 2, &[], None), ("B", 3, &["A"], None)], &[]);
        let s = p.schedule(T).unwrap();
        assert_eq!(row(&s, "A"), ("2026-11-20".into(), "2026-11-22".into(), 0, true));
        assert_eq!(row(&s, "B"), ("2026-11-22".into(), "2026-11-25".into(), 0, true));
        assert_eq!(s.finish, "2026-11-25");
        assert_eq!(s.items.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["A", "B"]);
    }

    #[test]
    fn diamond_short_branch_has_float() {
        let (_d, p) = project(&[("A", 1, &[], None), ("B", 3, &["A"], None), ("C", 1, &["A"], None), ("D", 2, &["B", "C"], None)], &[]);
        let s = p.schedule(T).unwrap();
        assert_eq!(s.finish, "2026-11-26");
        assert_eq!(row(&s, "C"), ("2026-11-21".into(), "2026-11-22".into(), 2, false));
        for k in ["A", "B", "D"] {
            assert!(row(&s, k).3, "{k}");
        }
    }

    #[test]
    fn parallel_branches() {
        let (_d, p) = project(&[("A", 5, &[], None), ("B", 2, &[], None)], &[]);
        let s = p.schedule(T).unwrap();
        assert_eq!(s.finish, "2026-11-25");
        assert_eq!(row(&s, "A"), ("2026-11-20".into(), "2026-11-25".into(), 0, true));
        assert_eq!(row(&s, "B"), ("2026-11-20".into(), "2026-11-22".into(), 3, false));
    }

    #[test]
    fn done_stages_take_no_time_and_default_is_one_day() {
        let (_d, p) = project(&[("A", 4, &[], None), ("B", 0, &["A"], None)], &["A"]);
        let s = p.schedule(T).unwrap();
        assert_eq!(row(&s, "A"), ("2026-11-20".into(), "2026-11-20".into(), 0, false));
        // estimate 0 clears the field, so B takes the default day, starting today.
        assert_eq!(row(&s, "B"), ("2026-11-20".into(), "2026-11-21".into(), 0, true));
    }

    #[test]
    fn soft_dependencies_are_ignored() {
        let (_d, p) = project(&[("A", 3, &[], None), ("B", 1, &[], None)], &[]);
        p.link("A", "B", true).unwrap();
        let s = p.schedule(T).unwrap();
        assert_eq!(row(&s, "B").0, "2026-11-20");
    }

    #[test]
    fn due_slack_can_be_negative() {
        let (_d, p) = project(&[("A", 3, &[], Some("2026-11-22")), ("B", 1, &["A"], Some("2026-12-01")), ("C", 1, &[], None)], &[]);
        let s = p.schedule(T).unwrap();
        let slack = |k: &str| s.items.iter().find(|i| i.key == k).unwrap().slack;
        assert_eq!((slack("A"), slack("B"), slack("C")), (Some(-1), Some(7), None));
    }

    #[test]
    fn bad_today_is_an_error() {
        let (_d, p) = project(&[("A", 1, &[], None)], &[]);
        assert!(p.schedule("tomorrow").is_err());
    }
}
