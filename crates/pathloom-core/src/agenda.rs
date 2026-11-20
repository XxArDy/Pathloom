//! "Today / week" agenda and the daily digest (m10 contract A3, A4).

use crate::approvals::InboxItem;
use crate::error::{Error, Result};
use crate::fsutil::now_secs;
use crate::log::LogEntry;
use crate::model::Status;
use crate::project::Project;
use crate::status::stage_views;
use serde::Serialize;

/// Log actions that close work: a task ticked (CLI, MCP `set_task`, the app) or approved by the owner.
pub const DONE_ACTIONS: [&str; 2] = ["task done", "approval approve"];

/// Days since 1970-01-01 of a `YYYY-MM-DD` date (H. Hinnant); `None` for a malformed or impossible date.
pub fn date_to_days(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' || !b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit()) {
        return None;
    }
    let (y, m, d): (i64, i64, i64) = (s[..4].parse().ok()?, s[5..7].parse().ok()?, s[8..].parse().ok()?);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let dim = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if !(1..=12).contains(&m) || d < 1 || d > dim[(m - 1) as usize] {
        return None;
    }
    let y = y - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    Some(era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468)
}

/// Inverse of [`date_to_days`].
pub fn days_to_date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{m:02}-{d:02}", yoe + era * 400 + i64::from(m <= 2))
}

/// `date` plus `n` days (`n` may be negative).
pub fn add_days(date: &str, n: i64) -> Option<String> {
    date_to_days(date).map(|d| days_to_date(d + n))
}

/// Today's date in UTC (`YYYY-MM-DD`). Callers that know the local date should pass it instead.
pub fn today_utc() -> String {
    days_to_date(now_secs().div_euclid(86_400))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgendaItem {
    pub key: String,
    pub title: String,
    pub due: Option<String>,
    pub status: Status,
    pub who: String,
    pub progress: (u32, u32),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Agenda {
    pub today: String,
    pub days: u32,
    pub overdue: Vec<AgendaItem>,
    pub due: Vec<AgendaItem>,
    pub blocked: Vec<AgendaItem>,
    pub waiting: Vec<InboxItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SilentClaim {
    pub task_id: String,
    pub agent: String,
    pub last_seen: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Stuck {
    pub blocked: Vec<AgendaItem>,
    pub overdue: Vec<AgendaItem>,
    pub silent_claims: Vec<SilentClaim>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Digest {
    pub since: String,
    pub today: String,
    /// Log entries since `since` that closed a task or approved one, oldest first.
    pub done: Vec<LogEntry>,
    pub stuck: Stuck,
    pub waiting: Vec<InboxItem>,
}

fn check_today(today: &str) -> Result<i64> {
    date_to_days(today).ok_or_else(|| Error::InvalidSpec(format!("bad date `{today}` (use YYYY-MM-DD)")))
}

impl Project {
    /// Open stages that are overdue (`due` < `today`), due in `[today, today + days)`, blocked, and what waits
    /// for the owner. Done stages never appear in the first three lists. Overdue/due are sorted by `due`, then key.
    pub fn agenda(&self, today: &str, days: u32) -> Result<Agenda> {
        let t = check_today(today)?;
        let end = days_to_date(t + i64::from(days));
        let mut a = Agenda { today: today.into(), days, overdue: vec![], due: vec![], blocked: vec![], waiting: self.inbox()? };
        for v in stage_views(&self.load_stages()?).into_iter().filter(|v| v.status != Status::Done) {
            let item = AgendaItem {
                key: v.stage.key.clone(),
                title: v.stage.title.clone(),
                due: v.stage.due.clone(),
                status: v.status,
                who: v.stage.who.to_string(),
                progress: v.progress,
            };
            // Dates are fixed-width ISO, so string order is date order; a time suffix is ignored.
            let due = v.stage.due.as_deref().and_then(|d| d.get(..10)).filter(|d| date_to_days(d).is_some());
            match due {
                Some(d) if d < today => a.overdue.push(item.clone()),
                Some(d) if d < end.as_str() => a.due.push(item.clone()),
                _ => {}
            }
            if v.status == Status::Blocked {
                a.blocked.push(item);
            }
        }
        for l in [&mut a.overdue, &mut a.due] {
            l.sort_by(|x, y| (&x.due, &x.key).cmp(&(&y.due, &y.key)));
        }
        Ok(a)
    }

    /// What happened since `since` (RFC3339 UTC, `YYYY-MM-DDTHH:MM:SSZ`) and what is stuck now.
    /// "Done" = log entries whose action is in [`DONE_ACTIONS`] (never decided from the summary text).
    pub fn digest(&self, since: &str, today: &str) -> Result<Digest> {
        let ag = self.agenda(today, 1)?;
        let mut done: Vec<LogEntry> = self
            .read_log(0, None)?
            .into_iter()
            .filter(|e| e.at.as_str() > since && DONE_ACTIONS.contains(&e.action.as_str()))
            .collect();
        done.reverse();
        let now = now_secs();
        let silent_claims = self
            .claims()?
            .into_iter()
            .filter(|c| c.is_silent(now))
            .map(|c| SilentClaim { last_seen: c.last_seen().to_string(), task_id: c.task_id, agent: c.agent })
            .collect();
        Ok(Digest {
            since: since.into(),
            today: today.into(),
            done,
            stuck: Stuck { blocked: ag.blocked, overdue: ag.overdue, silent_claims },
            waiting: ag.waiting,
        })
    }
}

fn section(out: &mut String, heading: &str, rows: Vec<String>) {
    if !rows.is_empty() {
        out.push_str(&format!("\n### {heading}\n"));
        rows.iter().for_each(|r| out.push_str(&format!("- {r}\n")));
    }
}

fn stage_row(i: &AgendaItem) -> String {
    let due = i.due.as_deref().map(|d| format!(", due {d}")).unwrap_or_default();
    format!("{} {} ({}/{}{due})", i.key, i.title, i.progress.0, i.progress.1)
}

fn waiting_rows(w: &[InboxItem]) -> Vec<String> {
    w.iter().map(|i| format!("{} {} [{}]", i.task_id, i.title, i.kind.as_str())).collect()
}

impl Agenda {
    /// Markdown text (English headings; empty sections are omitted).
    pub fn to_markdown(&self) -> String {
        let mut out = format!("## Agenda {}\n", self.today);
        section(&mut out, "Overdue", self.overdue.iter().map(stage_row).collect());
        section(&mut out, "Due", self.due.iter().map(stage_row).collect());
        section(&mut out, "Blocked", self.blocked.iter().map(stage_row).collect());
        section(&mut out, "Waiting for you", waiting_rows(&self.waiting));
        if out.lines().count() == 1 {
            out.push_str("\nNothing to report.\n");
        }
        out
    }
}

impl Digest {
    /// Markdown text sent by the Telegram bridge (English headings; empty sections are omitted).
    pub fn to_markdown(&self) -> String {
        let mut out = format!("## Digest {}\n", self.today);
        section(&mut out, "Done", self.done.iter().map(|e| format!("{} ({})", e.summary, e.author)).collect());
        section(&mut out, "Overdue", self.stuck.overdue.iter().map(stage_row).collect());
        section(&mut out, "Blocked", self.stuck.blocked.iter().map(stage_row).collect());
        section(
            &mut out,
            "Silent agents",
            self.stuck.silent_claims.iter().map(|c| format!("{} {} (last seen {})", c.task_id, c.agent, c.last_seen)).collect(),
        );
        section(&mut out, "Waiting for you", waiting_rows(&self.waiting));
        if out.lines().count() == 1 {
            out.push_str("\nNothing to report.\n");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_helpers_edge_cases() {
        assert_eq!(date_to_days("1970-01-01"), Some(0));
        assert_eq!(add_days("2026-01-31", 1).unwrap(), "2026-02-01");
        assert_eq!(add_days("2026-12-31", 1).unwrap(), "2027-01-01");
        assert_eq!(add_days("2024-02-28", 1).unwrap(), "2024-02-29"); // leap
        assert_eq!(add_days("2023-02-28", 1).unwrap(), "2023-03-01");
        assert_eq!(add_days("2100-02-28", 1).unwrap(), "2100-03-01"); // not leap
        assert_eq!(add_days("2000-02-28", 2).unwrap(), "2000-03-01"); // leap (400)
        assert_eq!(add_days("2026-03-01", -1).unwrap(), "2026-02-28");
        assert_eq!(add_days("2026-01-01", -1).unwrap(), "2025-12-31");
        for bad in ["", "2026-13-01", "2026-02-29", "2026-1-01", "26-01-01", "2026/01/01", "2026-00-10", "2026-04-31"] {
            assert_eq!(date_to_days(bad), None, "{bad}");
        }
        for d in [-1000, 0, 19_000, 40_000] {
            assert_eq!(date_to_days(&days_to_date(d)), Some(d));
        }
    }
}
