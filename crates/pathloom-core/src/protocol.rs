//! Protocol version and capability list advertised to external clients (m7 contract §1).

/// Major version of the file and MCP contract. Breaking changes bump it to `pathloom/2`.
pub const PROTOCOL: &str = "pathloom/1";

/// What a read-only server still serves (its reading tools: `list_proposals`, `get_approvals`,
/// `list_decisions`, `list_history`, `list_templates`, `parse_quick_add`, ...).
const READ_ONLY: [&str; 20] = [
    "read", "activity", "events", "validate", "commits", "agenda", "digest", "approval_events", "task_events", "watch_filter",
    "schedule", "export_html", "export", "links", "approvals", "decisions", "proposals", "history", "templates", "quick_add",
];

/// What this build supports. A read-only server keeps only [`READ_ONLY`].
pub fn capabilities(read_only: bool) -> Vec<&'static str> {
    if read_only {
        READ_ONLY.to_vec()
    } else {
        vec![
            "read", "apply", "proposals", "claims", "activity", "agents", "events", "sessions", "layout", "validate",
            "evidence", "heartbeat", "import", "artifacts", "usage", "gates",
            "propose", "approvals", "review", "decisions", "inbox",
            "templates", "delegation", "task_status", "evidence_commit", "commits", "usage_tasks", "usage_quota", "agenda", "digest",
            "session_state", "approval_events", "task_events", "evidence_branch", "watch_filter",
            "schedule", "export_html",
            "export", "import_formats", "history", "links",
            "client_answers", "quick_add", "telegram",
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
        for mutating in [
            "apply", "claims", "agents", "sessions", "layout", "evidence", "heartbeat", "import", "usage", "gates", "propose", "review",
            "inbox", "delegation", "task_status", "evidence_commit", "usage_tasks", "usage_quota", "session_state", "evidence_branch",
            "import_formats", "client_answers", "telegram",
        ] {
            assert!(rw.contains(&mutating) && !ro.contains(&mutating), "{mutating}");
        }
        for c in ["approvals", "decisions", "proposals", "history", "templates", "quick_add", "approval_events", "watch_filter", "links"] {
            assert!(rw.contains(&c) && ro.contains(&c), "{c}");
        }
        assert_eq!(PROTOCOL, "pathloom/1");
    }
}
