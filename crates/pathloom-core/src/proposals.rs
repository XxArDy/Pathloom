//! Agent proposals (`.pathloom/proposals/<id>.json`): stored by `propose`, decided by a human with
//! [`Project::decide_proposal`]. Decided ones move to `proposals/decided/` (the newest 200 are kept).

use crate::apply::{ApplyOptions, ApplyReport, ApplySpec};
use crate::error::{Error, Result};
use crate::fsutil::{now_minute, now_rfc3339, write_atomic};
use crate::project::{Project, read_text};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How many decided proposals are kept.
pub const DECIDED_KEEP: usize = 200;

/// One changed stage file: project-relative path (forward slashes) and its full text before/after.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FileChange {
    pub path: String,
    /// Absent for a new file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DecisionState {
    Approved,
    Rejected,
    Conflict,
}

impl DecisionState {
    pub fn as_str(self) -> &'static str {
        match self {
            DecisionState::Approved => "approved",
            DecisionState::Rejected => "rejected",
            DecisionState::Conflict => "conflict",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Decision {
    pub state: DecisionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    pub at: String,
    pub by: String,
}

/// Which proposals [`Project::list_proposals_by`] returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalFilter {
    Pending,
    Decided(DecisionState),
    /// Pending and decided.
    All,
}

impl ProposalFilter {
    /// `pending|approved|rejected|conflict|all`.
    pub fn parse(s: &str) -> Result<ProposalFilter> {
        Ok(match s {
            "pending" => ProposalFilter::Pending,
            "approved" => ProposalFilter::Decided(DecisionState::Approved),
            "rejected" => ProposalFilter::Decided(DecisionState::Rejected),
            "conflict" => ProposalFilter::Decided(DecisionState::Conflict),
            "all" => ProposalFilter::All,
            other => {
                return Err(Error::InvalidSpec(format!("unknown proposal state `{other}` (pending|approved|rejected|conflict|all)")));
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Proposal {
    /// Time-sortable id (ULID).
    pub id: String,
    pub created_at: String,
    pub author: String,
    pub spec: ApplySpec,
    /// Dry-run result at proposal time.
    pub preview: ApplyReport,
    /// Stage key -> file hash at proposal time, for every existing stage the proposal changes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub stage_hashes: BTreeMap<String, String>,
    /// Stage files before/after (boards are not included).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<FileChange>,
    /// Set once decided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<Decision>,
}

impl Project {
    fn proposals_dir(&self) -> PathBuf {
        self.root.join(".pathloom/proposals")
    }

    fn decided_dir(&self) -> PathBuf {
        self.proposals_dir().join("decided")
    }

    fn proposal_path(&self, id: &str) -> Result<PathBuf> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(Error::InvalidName(format!("proposal id {id}")));
        }
        Ok(self.proposals_dir().join(format!("{id}.json")))
    }

    /// Validate `spec` (dry-run) and store it as a proposal. Nothing in `plan/` changes.
    pub fn propose(&self, spec: &ApplySpec, author: &str) -> Result<Proposal> {
        let opts = ApplyOptions { dry_run: true, author: author.into() };
        let (preview, files) = self.plan_apply(spec, &opts)?;
        let mut stage_hashes = BTreeMap::new();
        for k in &preview.updated {
            stage_hashes.insert(k.clone(), self.stage_hash(k)?);
        }
        let changes = files
            .iter()
            .filter(|(p, _)| !p.to_string_lossy().ends_with(".board.json"))
            .map(|(p, text)| FileChange {
                path: p.strip_prefix(&self.root).unwrap_or(p).to_string_lossy().replace('\\', "/"),
                before: std::fs::read_to_string(p).ok(),
                after: Some(text.clone()),
            })
            .collect();
        let p = Proposal {
            id: ulid::Ulid::generate().to_string(),
            created_at: now_minute(),
            author: author.into(),
            spec: spec.clone(),
            preview,
            stage_hashes,
            changes,
            decision: None,
        };
        let text = serde_json::to_string_pretty(&p).map_err(|e| Error::InvalidSpec(e.to_string()))?;
        write_atomic(&self.proposal_path(&p.id)?, text.as_bytes())?;
        Ok(p)
    }

    fn load_proposal_file(path: &Path) -> Result<Proposal> {
        serde_json::from_str(&read_text(path)?).map_err(|e| crate::error::parse_err(path, e))
    }

    /// State recorded in a decided file (for the event stream).
    pub(crate) fn decided_state(path: &Path) -> Option<DecisionState> {
        Self::load_proposal_file(path).ok()?.decision.map(|d| d.state)
    }

    fn load_proposal(&self, id: &str) -> Result<Proposal> {
        let path = self.proposal_path(id)?;
        if !path.is_file() {
            return Err(Error::NotFound(format!("proposal {id}")));
        }
        Self::load_proposal_file(&path)
    }

    /// Readable proposals of `dir`, oldest first. Unreadable files are skipped.
    fn read_proposals(dir: &Path) -> Vec<Proposal> {
        let mut out: Vec<Proposal> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .filter_map(|p| Self::load_proposal_file(&p).ok())
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    /// Pending proposals, oldest first. Unreadable files are skipped.
    pub fn list_proposals(&self) -> Result<Vec<Proposal>> {
        self.list_proposals_by(ProposalFilter::Pending)
    }

    /// Proposals by state, oldest first (pending ones before decided ones for `All`).
    pub fn list_proposals_by(&self, filter: ProposalFilter) -> Result<Vec<Proposal>> {
        let mut out = Vec::new();
        if matches!(filter, ProposalFilter::Pending | ProposalFilter::All) {
            out.extend(Self::read_proposals(&self.proposals_dir()));
        }
        match filter {
            ProposalFilter::Pending => {}
            ProposalFilter::All => out.extend(Self::read_proposals(&self.decided_dir())),
            ProposalFilter::Decided(s) => out.extend(
                Self::read_proposals(&self.decided_dir()).into_iter().filter(|p| p.decision.as_ref().is_some_and(|d| d.state == s)),
            ),
        }
        Ok(out)
    }

    /// First stage whose file differs from the hash recorded at proposal time (or that is gone).
    fn stale_stage(&self, p: &Proposal) -> Option<String> {
        p.stage_hashes.iter().find(|(k, h)| self.stage_hash(k).ok().as_ref() != Some(*h)).map(|(k, _)| k.clone())
    }

    /// Store the decision, move the file to `decided/` and prune old ones.
    fn finish_proposal(&self, mut p: Proposal, state: DecisionState, comment: Option<&str>, by: &str) -> Result<Proposal> {
        let comment = comment.map(str::trim).filter(|c| !c.is_empty()).map(String::from);
        p.decision = Some(Decision { state, comment, at: now_rfc3339(), by: by.trim().to_string() });
        let text = serde_json::to_string_pretty(&p).map_err(|e| Error::InvalidSpec(e.to_string()))?;
        let dir = self.decided_dir();
        write_atomic(&dir.join(format!("{}.json", p.id)), text.as_bytes())?;
        std::fs::remove_file(self.proposal_path(&p.id)?)?;
        let mut files: Vec<PathBuf> =
            std::fs::read_dir(&dir)?.flatten().map(|e| e.path()).filter(|f| f.extension().is_some_and(|x| x == "json")).collect();
        files.sort();
        for f in files.iter().take(files.len().saturating_sub(DECIDED_KEEP)) {
            let _ = std::fs::remove_file(f);
        }
        Ok(p)
    }

    /// Approve (apply as the author) or reject a pending proposal and move it to `decided/`.
    /// Approving when a stage changed since the proposal (or vanished) records `conflict` and applies nothing.
    /// An apply error keeps the proposal pending.
    pub fn decide_proposal(&self, id: &str, approve: bool, comment: Option<&str>, by: &str) -> Result<Proposal> {
        self.decide_inner(id, approve, comment, by).map(|(p, _)| p)
    }

    fn decide_inner(&self, id: &str, approve: bool, comment: Option<&str>, by: &str) -> Result<(Proposal, Option<ApplyReport>)> {
        let p = self.load_proposal(id)?;
        if !approve {
            return Ok((self.finish_proposal(p, DecisionState::Rejected, comment, by)?, None));
        }
        if self.stale_stage(&p).is_some() {
            return Ok((self.finish_proposal(p, DecisionState::Conflict, comment, by)?, None));
        }
        let report = self.apply(&p.spec, &ApplyOptions { dry_run: false, author: p.author.clone() })?;
        Ok((self.finish_proposal(p, DecisionState::Approved, comment, by)?, Some(report)))
    }

    /// Apply the proposal for real (as its author). A stage changed since the proposal gives `Error::Conflict`.
    pub fn accept_proposal(&self, id: &str) -> Result<ApplyReport> {
        let stale = self.stale_stage(&self.load_proposal(id)?);
        let (_, report) = self.decide_inner(id, true, None, "owner")?;
        report.ok_or(Error::Conflict { key: stale.unwrap_or_default() })
    }

    pub fn reject_proposal(&self, id: &str) -> Result<()> {
        self.decide_inner(id, false, None, "owner").map(|_| ())
    }
}
