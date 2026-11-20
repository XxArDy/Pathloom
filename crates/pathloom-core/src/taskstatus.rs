//! Live delivery status of a task (`task_info.<ID>.status`): claimed -> pushed -> merged -> verified -> accepted.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// Pipeline step of a task's delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TaskStep {
    Claimed,
    Pushed,
    Merged,
    Verified,
    Accepted,
}

impl std::str::FromStr for TaskStep {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "claimed" => Ok(Self::Claimed),
            "pushed" => Ok(Self::Pushed),
            "merged" => Ok(Self::Merged),
            "verified" => Ok(Self::Verified),
            "accepted" => Ok(Self::Accepted),
            _ => Err(Error::InvalidSpec(format!("unknown status `{s}` (claimed|pushed|merged|verified|accepted)"))),
        }
    }
}

/// Where a task is in delivery. Set by Pathloom through `set_task_status`; invalid hand edits are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TaskStatus {
    pub step: TaskStep,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Commit, 7-40 hex characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
    /// Up to 10 project-relative paths or http(s) links (screenshots, logs).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub at: String,
    #[serde(default)]
    pub by: String,
}

/// 7-40 hex characters (a git sha or its abbreviation), lowercased.
pub(crate) fn check_sha(s: &str) -> Result<String> {
    let s = s.trim().to_ascii_lowercase();
    if (7..=40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(s)
    } else {
        Err(Error::InvalidSpec(format!("sha `{s}` must be 7-40 hex characters")))
    }
}

impl TaskStatus {
    /// Trim, lowercase the sha and validate every field.
    pub fn normalize(mut self) -> Result<TaskStatus> {
        self.branch = self.branch.map(|b| b.trim().to_string()).filter(|b| !b.is_empty());
        if self.branch.as_ref().is_some_and(|b| b.chars().count() > 200 || b.chars().any(char::is_control)) {
            return Err(Error::InvalidSpec("status branch must be one line, at most 200 characters".into()));
        }
        self.sha = self.sha.as_deref().filter(|s| !s.trim().is_empty()).map(check_sha).transpose()?;
        if self.evidence.len() > 10 {
            return Err(Error::InvalidSpec("status evidence has at most 10 entries".into()));
        }
        self.evidence = self.evidence.iter().map(|e| crate::project::check_artifact(e)).collect::<Result<_>>()?;
        self.at = self.at.trim().into();
        self.by = self.by.trim().into();
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha_rules() {
        assert_eq!(check_sha(" 3F2A9C1 ").unwrap(), "3f2a9c1");
        for bad in ["abc123", "xyz1234", &"a".repeat(41), ""] {
            assert!(check_sha(bad).is_err(), "{bad}");
        }
        assert!("pushed".parse::<TaskStep>().is_ok() && "nope".parse::<TaskStep>().is_err());
    }
}
