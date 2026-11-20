use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

macro_rules! lower_enum {
    ($(#[$m:meta])* $name:ident { $($var:ident = $s:literal),+ } default $def:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
        #[serde(rename_all = "lowercase")]
        pub enum $name { $(#[allow(missing_docs)] $var),+ }
        impl Default for $name { fn default() -> Self { Self::$def } }
        #[allow(dead_code)]
        impl $name {
            pub(crate) fn as_str(self) -> &'static str { match self { $(Self::$var => $s),+ } }
            pub(crate) fn parse(s: &str) -> Option<Self> { match s { $($s => Some(Self::$var),)+ _ => None } }
        }
    };
}

lower_enum!(StageKind { Stage = "stage", Milestone = "milestone", Gate = "gate" } default Stage);
lower_enum!(Priority { Low = "low", Med = "med", High = "high" } default Med);
lower_enum!(AgentChanges { Auto = "auto", Review = "review" } default Auto);
lower_enum!(TaskKind { Approval = "approval", Decision = "decision" } default Approval);
lower_enum!(ApprovalState { Pending = "pending", Approved = "approved", Rejected = "rejected" } default Pending);
lower_enum!(InboxKind { Approval = "approval", Decision = "decision", Owner = "owner" } default Owner);
lower_enum!(Status { Done = "done", Active = "active", Ready = "ready", Blocked = "blocked" } default Ready);

/// Who does a stage: `me`, `ai`, `both`, `ai:<name>` (a named agent, `[a-z0-9._-]{1,32}`) or
/// `human:<name>` (letters, digits, space, `._-`, up to 32). Serialized as that plain string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Who(String);

const WHO_NAME_MAX: usize = 32;

impl Who {
    pub fn me() -> Self {
        Who("me".into())
    }

    pub fn ai() -> Self {
        Who("ai".into())
    }

    pub fn both() -> Self {
        Who("both".into())
    }

    /// Validate and wrap `s` (no trimming).
    pub fn parse(s: &str) -> Result<Who, String> {
        let bad = |what: &str| format!("bad who `{s}`: {what} (use me | ai | both | ai:<name> | human:<name>)");
        let len_ok = |n: &str| (1..=WHO_NAME_MAX).contains(&n.chars().count());
        match s {
            "me" | "ai" | "both" => {}
            _ => match s.split_once(':') {
                Some(("ai", n)) if len_ok(n) && n.chars().all(|c| matches!(c, 'a'..='z' | '0'..='9' | '.' | '_' | '-')) => {}
                Some(("ai", _)) => return Err(bad("agent name must be 1-32 chars of [a-z0-9._-]")),
                Some(("human", n))
                    if len_ok(n)
                        && n.trim() == n
                        && n.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-')) => {}
                Some(("human", _)) => {
                    return Err(bad("human name must be 1-32 letters, digits, spaces or ._- without outer spaces"));
                }
                _ => return Err(bad("unknown value")),
            },
        }
        Ok(Who(s.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `ai` or `ai:<name>`.
    pub fn is_agent(&self) -> bool {
        self.0 == "ai" || self.0.starts_with("ai:")
    }

    /// `claude` for `ai:claude`; `None` for every other value.
    pub fn agent_name(&self) -> Option<&str> {
        self.0.strip_prefix("ai:")
    }

    /// Label for the UI: `ai:claude` -> `Claude`, `human:olia` -> `Olia`; plain values unchanged.
    pub fn display(&self) -> String {
        match self.0.split_once(':') {
            Some((_, n)) => {
                let mut c = n.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
            }
            None => self.0.clone(),
        }
    }
}

impl Default for Who {
    fn default() -> Self {
        Who::ai()
    }
}

impl std::fmt::Display for Who {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::str::FromStr for Who {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        Who::parse(s)
    }
}

impl<'de> Deserialize<'de> for Who {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Who::parse(&s).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for Who {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Who".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "default": "ai",
            "description": "me | ai | both | ai:<agent name> | human:<name>",
            "pattern": "^(me|ai|both|ai:[a-z0-9._-]{1,32}|human:[\\p{L}\\p{N} ._-]{1,32})$"
        })
    }
}

impl Stage {
    /// Ids of all tasks and subtasks.
    pub fn task_ids(&self) -> std::collections::HashSet<&str> {
        self.tasks.iter().flat_map(|t| std::iter::once(t).chain(t.sub.iter())).map(|t| t.id.as_str()).collect()
    }

    /// Ids of the done tasks and subtasks.
    pub fn done_task_ids(&self) -> std::collections::HashSet<&str> {
        self.tasks
            .iter()
            .flat_map(|t| std::iter::once(t).chain(t.sub.iter()))
            .filter(|t| t.is_done())
            .map(|t| t.id.as_str())
            .collect()
    }

    /// Drop evidence of tasks that are no longer done or no longer exist (a gate keeps its own, keyed by the stage key).
    pub(crate) fn prune_evidence(&mut self) {
        let done: std::collections::HashSet<String> = self.done_task_ids().into_iter().map(String::from).collect();
        let key = self.key.clone();
        let review: std::collections::HashSet<String> =
            self.task_info.keys().filter(|k| self.in_review(k)).cloned().collect();
        self.evidence.retain(|k, _| done.contains(k) || *k == key || review.contains(k));
    }

    fn find_task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().flat_map(|t| std::iter::once(t).chain(t.sub.iter())).find(|t| t.id == id)
    }

    /// Kind of a task: `task_info.<ID>.kind`, else by the stage key (`APPR` -> approval, `DECIDE` -> decision).
    pub fn task_kind(&self, id: &str) -> Option<TaskKind> {
        match self.task_info.get(id).and_then(|i| i.kind) {
            Some(k) => Some(k),
            None => match self.key.as_str() {
                "APPR" => Some(TaskKind::Approval),
                "DECIDE" => Some(TaskKind::Decision),
                _ => None,
            },
        }
    }

    /// The task waits for the owner: approval pending and the task is not done (computed, not stored).
    pub fn in_review(&self, id: &str) -> bool {
        self.task_info.get(id).and_then(|i| i.approval.as_ref()).is_some_and(|a| a.state == ApprovalState::Pending)
            && self.find_task(id).is_some_and(|t| !t.is_done())
    }

    /// Some task of this stage is in review.
    pub fn has_review(&self) -> bool {
        self.task_info.keys().any(|k| self.in_review(k))
    }

    /// Gate result: the passing check stored under the stage key.
    pub fn gate_passed(&self) -> bool {
        self.kind == StageKind::Gate && self.evidence.get(&self.key).is_some_and(crate::evidence::Evidence::passed)
    }

    /// New empty stage (kind=stage, who=ai, priority=med).
    pub fn new(key: &str, title: &str) -> Self {
        Stage {
            key: key.into(),
            title: title.into(),
            kind: StageKind::Stage,
            who: Who::ai(),
            due: None,
            priority: Priority::Med,
            branch: None,
            pr: None,
            tags: vec![],
            depends: vec![],
            description: String::new(),
            tasks: vec![],
            comments: vec![],
            sessions: vec![],
            evidence: std::collections::BTreeMap::new(),
            artifacts: vec![],
            check: None,
            task_info: std::collections::BTreeMap::new(),
            extra: Map::new(),
            layout: Layout::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Dep {
    pub id: String,
    pub soft: bool,
}

/// Extra details of one task, kept in the stage frontmatter (`task_info`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TaskInfo {
    /// `approval` | `decision`; otherwise derived from the stage key (`APPR`, `DECIDE`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<TaskKind>,
    /// Free text (several lines allowed), up to 4000 characters.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// Attached files: project-relative paths or http(s) links, up to 20.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// Owner's approval of the work: pending / approved / rejected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<Approval>,
    /// A question to the owner with options and, once answered, the choice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<TaskDecision>,
    /// Delivery status (`set_task_status`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<crate::taskstatus::TaskStatus>,
}

impl TaskInfo {
    pub fn is_empty(&self) -> bool {
        self.note.trim().is_empty()
            && self.files.is_empty()
            && self.kind.is_none()
            && self.approval.is_none()
            && self.decision.is_none()
            && self.status.is_none()
    }
}

/// Approval state of a task (`task_info.<ID>.approval`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Approval {
    pub state: ApprovalState,
    /// Up to 10 short reasons of a rejection (`off-theme`, `duplicate`, `quality`, `scale`, `wrong-place`, or your own).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// When and by whom the owner decided.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub by: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub requested_by: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub requested_at: String,
}

/// One option of a [`TaskDecision`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DecisionOption {
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// A question with options for the owner (`task_info.<ID>.decision`); `choice` is the answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TaskDecision {
    pub question: String,
    pub options: Vec<DecisionOption>,
    /// Index into `options`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended: Option<usize>,
    /// The owner's answer: an option label or free text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub by: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub asked_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub done: bool,
    pub sub: Vec<Task>,
}

impl Task {
    /// A task with subtasks is done when all of them are; otherwise its own flag.
    pub fn is_done(&self) -> bool {
        if self.sub.is_empty() { self.done } else { self.sub.iter().all(|s| s.done) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Comment {
    pub at: String,
    pub author: String,
    pub text: String,
}

/// Raw pieces of the markdown body that Pathloom does not manage.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Layout {
    /// Blank lines that followed the description (kept to stay byte-identical).
    pub desc_tail: String,
    /// Everything after the first managed section, in file order.
    pub segments: Vec<Seg>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Seg {
    Tasks { tail: String },
    Comments { tail: String },
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Stage {
    pub key: String,
    pub title: String,
    pub kind: StageKind,
    pub who: Who,
    pub due: Option<String>,
    pub priority: Priority,
    pub branch: Option<String>,
    pub pr: Option<String>,
    pub tags: Vec<String>,
    pub depends: Vec<Dep>,
    pub description: String,
    pub tasks: Vec<Task>,
    pub comments: Vec<Comment>,
    /// External client sessions that worked on this stage (oldest first, at most 20).
    #[serde(default)]
    pub sessions: Vec<crate::session::SessionRef>,
    /// Proof per done task id (the check that was run and its result).
    #[serde(default)]
    pub evidence: std::collections::BTreeMap<String, crate::evidence::Evidence>,
    /// Hand-off files of this stage (spec, plan, review): project-relative paths or http(s) links.
    #[serde(default)]
    pub artifacts: Vec<String>,
    /// `kind: gate`: the command that must pass (exit code 0) for the gate to be done, e.g. `npm test`.
    #[serde(default)]
    pub check: Option<String>,
    /// Per task id: a free note and attached files (project-relative paths or links).
    #[serde(default)]
    pub task_info: std::collections::BTreeMap<String, TaskInfo>,
    /// Unknown frontmatter keys (not exposed to the UI).
    #[serde(skip)]
    pub(crate) extra: Map<String, Value>,
    /// Unmanaged body text (not exposed to the UI).
    #[serde(skip)]
    pub(crate) layout: Layout,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageView {
    #[serde(flatten)]
    pub stage: Stage,
    pub status: Status,
    pub progress: (u32, u32),
    /// Live task claims of this stage (filled by `Project::stage_views_live`, empty from `stage_views`).
    #[serde(default)]
    pub claims: Vec<crate::claims::ClaimView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    pub format: String,
    pub title: String,
    pub style: String,
    /// Re-arrange the whole board after every agent change (cards with `"pinned": true` stay).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_layout: bool,
    pub elements: Vec<Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProjectConfig {
    pub format: u32,
    pub name: String,
    pub plan_dir: String,
    pub agent_changes: AgentChanges,
    /// Agents (MCP) must attach passing evidence to mark tasks of `kind: stage` done.
    #[serde(default)]
    pub require_evidence: bool,
    /// Optional git settings (`[git]` table); absent in old files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<GitConfig>,
    /// Optional Telegram approval bridge (`[telegram]` table); the bot token comes from the environment only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telegram: Option<TelegramConfig>,
    /// Agents (MCP) cannot close tasks: `done` puts them in review until the owner approves.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub require_approval: bool,
    /// Clients (MCP) may relay a human's answer to approvals and decisions (`respond_approval`, `answer_decision`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_client_answers: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TelegramConfig {
    /// Chat that receives approval requests and may answer them.
    pub chat_id: String,
    /// Local time `HH:MM` to send the daily digest at; absent = no digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

fn default_fetch_interval() -> u64 {
    120
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GitConfig {
    /// Ref the plan is read from instead of the working tree (e.g. `origin/main`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_ref: Option<String>,
    #[serde(default = "default_fetch_interval")]
    pub fetch_interval_secs: u64,
}

/// Partial update of a stage's frontmatter. `None` leaves a field unchanged;
/// for `due` and `branch` an empty string clears the value.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StageMeta {
    pub title: Option<String>,
    pub kind: Option<StageKind>,
    pub who: Option<Who>,
    pub priority: Option<Priority>,
    pub due: Option<String>,
    pub branch: Option<String>,
    /// Gate command; empty string clears.
    pub check: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_git_optional_and_roundtrips() {
        let old = "format = 1\nname = \"p\"\nplan_dir = \"plan\"\nagent_changes = \"auto\"\nrequire_evidence = false\n";
        let c: ProjectConfig = toml::from_str(old).unwrap();
        assert!(c.git.is_none());
        assert_eq!(toml::to_string(&c).unwrap(), old);
        let c: ProjectConfig = toml::from_str(&format!("{old}[git]\nread_ref = \"origin/main\"\n")).unwrap();
        let g = c.git.unwrap();
        assert_eq!((g.read_ref.as_deref(), g.fetch_interval_secs), (Some("origin/main"), 120));
        let c: ProjectConfig = toml::from_str(&format!("{old}[telegram]
chat_id = \"123\"
")).unwrap();
        assert_eq!(c.telegram.unwrap().chat_id, "123");
    }

    #[test]
    fn who_parse_and_helpers() {
        for ok in ["me", "ai", "both", "ai:claude", "ai:gpt-4.o_1", "human:olia", "human:Olia B-2"] {
            assert_eq!(Who::parse(ok).unwrap().as_str(), ok);
        }
        for bad in ["", "AI", "bot", "ai:", "ai:Claude", "ai:a b", "human:", "human: x", "human:x ", "human:a:b", "me:x"] {
            assert!(Who::parse(bad).unwrap_err().contains("bad who"), "{bad}");
        }
        assert!(Who::parse(&format!("ai:{}", "a".repeat(33))).is_err());
        assert!(Who::parse(&format!("ai:{}", "a".repeat(32))).is_ok());
        let w = Who::parse("ai:claude").unwrap();
        assert!(w.is_agent() && Who::ai().is_agent() && !Who::me().is_agent());
        assert_eq!((w.agent_name(), Who::ai().agent_name()), (Some("claude"), None));
        assert_eq!(w.display(), "Claude");
        assert_eq!(Who::parse("human:olia").unwrap().display(), "Olia");
        assert_eq!(Who::both().display(), "both");
        assert_eq!(Who::default(), Who::ai());
    }

    #[test]
    fn who_serde_is_a_plain_string() {
        let m: StageMeta = serde_json::from_str(r#"{"who":"ai:claude"}"#).unwrap();
        assert_eq!(serde_json::to_string(&m.who).unwrap(), r#""ai:claude""#);
        assert!(serde_json::from_str::<StageMeta>(r#"{"who":"robot"}"#).unwrap_err().to_string().contains("bad who"));
        let schema = serde_json::to_value(schemars::schema_for!(Who)).unwrap();
        assert_eq!(schema["type"], "string");
        assert!(schema["pattern"].as_str().unwrap().contains("ai:"));
    }
}
