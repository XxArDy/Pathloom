//! `pathloom today` / `pathloom digest` and the MCP `agenda` tool (m10 contract A3, A4).

use crate::ops::{self, ApiError, ApiResult};
use chrono::{Duration, Local, Utc};
use pathloom_core::Project;
use serde_json::{Value, json};

/// Local calendar date `YYYY-MM-DD`.
pub fn local_today() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

/// Agenda for `days` days from the local today.
pub fn agenda(p: &Project, days: u32) -> ApiResult<Value> {
    let a = p.agenda(&local_today(), days.min(366))?;
    let mut v = serde_json::to_value(&a).map_err(|e| ApiError::new("INTERNAL", e.to_string()))?;
    v["markdown"] = json!(a.to_markdown());
    Ok(v)
}

/// `--since` value: `24h`, `90m`, `7d` (before now) or an RFC3339 UTC time.
pub fn parse_since(s: &str) -> ApiResult<String> {
    let t = s.trim();
    let bad = || ApiError::new("INVALID_SPEC", format!("bad --since `{s}` (use 24h, 7d, 90m or 2026-11-05T10:00:00Z)"));
    let Some((num, unit)) = t.len().checked_sub(1).filter(|&i| t.is_char_boundary(i)).map(|i| t.split_at(i)) else { return Err(bad()) };
    let Ok(n) = num.parse::<i64>() else { return ops::parse_since(t) };
    let d = match unit {
        "m" => Duration::try_minutes(n),
        "h" => Duration::try_hours(n),
        "d" => Duration::try_days(n),
        _ => return Err(bad()),
    };
    let when = d.and_then(|d| Utc::now().checked_sub_signed(d)).ok_or_else(bad)?;
    Ok(when.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

/// Digest since `since` (default 24h); JSON carries the Markdown text too.
pub fn digest(p: &Project, since: Option<&str>) -> ApiResult<Value> {
    let since = parse_since(since.unwrap_or("24h"))?;
    let d = p.digest(&since, &local_today())?;
    let mut v = serde_json::to_value(&d).map_err(|e| ApiError::new("INTERNAL", e.to_string()))?;
    v["markdown"] = json!(d.to_markdown());
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn since_forms() {
        assert_eq!(parse_since("2026-11-05T10:00:00Z").unwrap(), "2026-11-05T10:00:00Z");
        for ok in ["24h", "7d", "90m"] {
            let s = parse_since(ok).unwrap();
            assert_eq!(s.len(), 20, "{ok}");
            assert!(s.as_str() < Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string().as_str());
        }
        for bad in ["", "h", "24x", "yesterday", "-1", "99999999999999d"] {
            assert!(parse_since(bad).is_err(), "{bad}");
        }
    }
}
