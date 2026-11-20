//! Protocol version and capability list advertised to external clients (m7 contract §1).

/// Major version of the file and MCP contract. Breaking changes bump it to `pathloom/2`.
pub const PROTOCOL: &str = "pathloom/1";

/// What this build supports. A read-only server keeps only the non-mutating capabilities.
pub fn capabilities(read_only: bool) -> Vec<&'static str> {
    if read_only {
        vec!["read", "activity", "events", "validate", "commits", "agenda", "digest", "approval_events", "task_events", "watch_filter"]
    } else {
        vec![
            "read", "apply", "proposals", "claims", "activity", "agents", "events", "sessions", "layout", "validate",
            "evidence", "heartbeat", "import", "artifacts", "usage", "gates",
            "propose", "approvals", "review", "decisions", "inbox",
            "templates", "delegation", "task_status", "evidence_commit", "commits", "usage_tasks", "usage_quota", "agenda", "digest",
            "session_state", "approval_events", "task_events", "evidence_branch", "watch_filter",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_has_no_mutating_capabilities() {
        let rw = capabilities(false);
        let ro = capabilities(true);
        assert!(ro.iter().all(|c| rw.contains(c)));
        for mutating in ["templates", "apply", "proposals", "claims", "agents", "sessions", "layout", "evidence", "heartbeat", "import", "usage", "gates", "propose", "approvals", "review", "decisions", "inbox"] {
            assert!(rw.contains(&mutating) && !ro.contains(&mutating), "{mutating}");
        }
        for c in ["templates", "delegation", "task_status", "evidence_commit", "usage_tasks", "usage_quota"] {            assert!(rw.contains(&c) && !ro.contains(&c), "{c}");
        }
        assert!(rw.contains(&"session_state") && !ro.contains(&"session_state"));
        assert!(rw.contains(&"evidence_branch") && !ro.contains(&"evidence_branch"));
        for c in ["approval_events", "task_events", "watch_filter"] {
            assert!(rw.contains(&c) && ro.contains(&c), "{c}");
        }
        assert!(ro.contains(&"commits") && ro.contains(&"agenda") && ro.contains(&"digest"));
        assert_eq!(PROTOCOL, "pathloom/1");
    }
}
