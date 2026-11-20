//! Pathloom core: project files, boards, stages, dependency graph and computed statuses.
//! See `README.md` for the file-format notes.

mod agenda;
mod agents;
mod apply;
mod approvals;
mod board;
mod check;
mod claims;
mod error;
mod evidence;
mod import;
pub mod events;
pub mod commits;
mod fsutil;
pub mod gitsrc;
mod hash;
mod indexkey;
mod keygen;
mod layout;
mod log;
mod model;
mod next;
mod project;
mod proposals;
mod rename;
mod protocol;
mod session;
mod stagefile;
pub mod url;
mod status;
mod taskstatus;
mod templates;
mod validate;
pub mod writes;

pub use agents::{AgentProfile, is_valid_agent_id};
pub use apply::{
    ApplyOptions, ApplyReport, ApplySpec, CardPlacement, CommentSpec, DepSpec, FrameSpec, LinkSpec, Place, StageSpec,
    UnlinkSpec,
};
pub use board::BOARD_FORMAT;
pub use check::run_check;
pub use taskstatus::{TaskStatus, TaskStep};
pub use templates::{TemplateInfo, list_templates, load_template};
pub use claims::{Claim, ClaimView, Delegation, STALE_AFTER_SECS, DEFAULT_CLAIM_TTL_SECS, MAX_CLAIM_TTL_SECS, SILENT_AFTER_SECS};
pub use log::{LogEntry, Quota, Usage, UsageSummary};
pub use fsutil::{now_rfc3339, now_secs, write_file_atomic};
pub use hash::hash_bytes;
pub use next::NextTask;
pub use proposals::{Decision, DecisionState, FileChange, Proposal, ProposalFilter};
pub use protocol::{PROTOCOL, capabilities};
pub use session::{MAX_STAGE_SESSIONS, SessionRef};
pub use validate::Issue;
pub use error::{Error, Result};
pub use evidence::Evidence;
pub use import::ImportReport;
pub use keygen::generate_key;
pub use model::{
    AgentChanges, Board, Comment, Dep, GitConfig, Priority, ProjectConfig, Stage, TelegramConfig, StageKind, StageMeta, StageView, Status,
    Approval, ApprovalState, DecisionOption, InboxKind, Task, TaskDecision, TaskInfo, TaskKind, Who,
};
pub use agenda::{Agenda, AgendaItem, Digest, SilentClaim, Stuck, add_days, date_to_days, days_to_date, today_utc};
pub use approvals::{ApprovalView, DecisionView, InboxItem};
pub use project::Project;
pub use stagefile::{is_valid_key, stage_key_of};
pub use status::{compute_statuses, stage_views};
