//! MCP server (`pathloom mcp`): tools from spec §7.2 plus guide/schema resources. stdio transport.
//!
//! Kept thin: every tool calls `ops` (shared with the CLI). rmcp is pinned to an exact version
//! because its API changed between majors.

use crate::cli::SchemaKind;
use crate::ops::{self, ApiError, ApiResult, StageFilter};
use crate::schema::schema;
use pathloom_core::{Project, SessionRef};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, ListResourcesResult, PaginatedRequestParams,
    ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Resource, ResourceContents,
    ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::transport::stdio;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;

const GUIDE: &str = include_str!("guide.md");

#[derive(Debug, Deserialize, JsonSchema)]
struct ListStagesParams {
    /// Filter by computed status: done | active | ready | blocked
    status: Option<String>,
    /// Filter by assignee: me | ai | both | ai:<name> | human:<name>
    who: Option<String>,
    /// Only stages that have a card on this board
    board: Option<String>,
    /// Case-insensitive substring of key or title
    query: Option<String>,
    /// Page size (default 50, max 500)
    limit: Option<usize>,
    /// `next_cursor` of the previous page
    cursor: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct KeyParams {
    /// Stage key, e.g. M0
    key: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct NextParams {
    /// Max tasks (default 10)
    limit: Option<usize>,
    /// Agent name to plan for (default: you). Your claimed tasks come first; tasks claimed by others are hidden.
    agent: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ClaimParams {
    /// Task or subtask id, e.g. AUTH-02 or AUTH-02a
    task_id: String,
    /// How long the claim lasts, in minutes (default 30, max 480). Claim again to renew.
    ttl_minutes: Option<u64>,
    /// Short note about what you are doing
    note: Option<String>,
    /// Who really does the work (model or agent name), up to 64 characters
    by: Option<String>,
    /// Id of the agent that delegated this task to you
    parent: Option<String>,
    /// Git branch of the work
    branch: Option<String>,
    /// Worktree folder of the work
    worktree: Option<String>,
    /// External client session you act from (client, id, url?, title?). Logged with the action.
    session: Option<SessionRef>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TaskStatusParams {
    /// Task or subtask id
    task_id: String,
    /// claimed | pushed | merged | verified | accepted
    status: String,
    branch: Option<String>,
    /// Commit, 7-40 hex characters
    sha: Option<String>,
    /// Up to 10 project-relative paths or links (screenshots, logs)
    evidence: Option<Vec<String>>,
    session: Option<SessionRef>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CommitsParams {
    /// Only commits of this stage key
    stage: Option<String>,
    /// Max commits to scan (default 50, max 500)
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ReleaseParams {
    /// Task or subtask id
    task_id: String,
    /// Release a claim held by another agent. Only when the user asked you to.
    force: Option<bool>,
    /// External client session you act from (client, id, url?, title?). Logged with the action.
    session: Option<SessionRef>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ActivityParams {
    /// Max entries (default 20, max 200)
    limit: Option<usize>,
    /// Only entries that touch this stage key
    stage: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AgendaParams {
    /// Window in days from today (default 1, max 366)
    days: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct MakeLinkParams {
    /// Stage key (AUTH) or task id (AUTH-01); omit for a link to the project
    id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct QuickAddParams {
    /// Plain words, English or Ukrainian: `add task "Fix login" to stage AUTH by friday`
    text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchParams {
    /// Case-insensitive text to find in keys, titles, descriptions and task titles
    query: String,
    /// Max results (default 20)
    limit: Option<usize>,
}

fn spec_schema(g: &mut SchemaGenerator) -> Schema {
    schemars::json_schema!({
        "description": "Apply spec as a JSON object, or YAML/JSON text. See pathloom://schema/spec and pathloom://guide/workflow.",
        "anyOf": [g.subschema_for::<pathloom_core::ApplySpec>(), {"type": "string"}]
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ApplyParams {
    #[schemars(schema_with = "spec_schema")]
    spec: Value,
    /// Validate and report only; write nothing
    dry_run: Option<bool>,
    /// true = store a proposal for the user to approve instead of writing (the plan stays unchanged until then)
    propose: Option<bool>,
    /// External client session you act from (client, id, url?, title?). Logged with the action.
    session: Option<SessionRef>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListProposalsParams {
    /// pending (default) | approved | rejected | conflict | all
    state: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DecideProposalParams {
    /// Proposal id (from list_proposals or the apply result)
    id: String,
    /// true = apply it, false = reject it
    approve: bool,
    comment: Option<String>,
    /// The person who decided, 1-64 characters. Never your own name: you only relay what a human said.
    human: String,
    /// External client session you act from (client, id, url?, title?). Logged with the action.
    session: Option<SessionRef>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SetTaskParams {
    /// Task id, e.g. AUTH-02 or AUTH-02a
    task_id: String,
    /// true = done, false = not done
    done: bool,
    /// Proof for done=true: the check you ran (command, exit_code 0, duration_ms?, summary?, log?).
    /// The task shows as verified. Required when the project has require_evidence on.
    evidence: Option<pathloom_core::Evidence>,
    /// External client session you act from (client, id, url?, title?). Logged with the action.
    session: Option<SessionRef>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RequestApprovalParams {
    /// Task id, e.g. DESIGN-03
    task_id: String,
    /// Files to show the owner (screenshots, mockups): project-relative paths or http(s) links
    files: Option<Vec<String>>,
    /// What to look at (replaces the task note)
    note: Option<String>,
    /// External client session you act from (client, id, url?, title?). Logged with the action.
    session: Option<SessionRef>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct UpdateSessionParams {
    /// Your session: client, id, url?, title?, state (working | waiting | blocked | error | done)
    session: SessionRef,
    /// Task the session works on (its stage gets the session)
    task_id: Option<String>,
    /// Stage key, when there is no task
    stage: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RespondApprovalParams {
    /// Task in review
    task_id: String,
    /// true = approve, false = reject
    approve: bool,
    /// Rejection reasons (up to 10 short ones)
    reasons: Option<Vec<String>>,
    comment: Option<String>,
    /// The person who answered, 1-64 characters. Never your own name: you only relay what a human said.
    human: String,
    session: Option<SessionRef>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AnswerDecisionParams {
    /// Task with an open decision
    task_id: String,
    /// An option label or free text
    choice: String,
    /// The person who answered, 1-64 characters. Never your own name.
    human: String,
    session: Option<SessionRef>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetApprovalsParams {
    /// pending | approved | rejected | all (default all)
    state: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AskDecisionParams {
    /// Task id, e.g. DECIDE-01
    task_id: String,
    /// The question for the owner (up to 500 characters)
    question: String,
    /// 1-8 options: {label (up to 80), description? (up to 500)}
    options: Vec<pathloom_core::DecisionOption>,
    /// Index of the option you recommend
    recommended: Option<usize>,
    /// External client session you act from (client, id, url?, title?). Logged with the action.
    session: Option<SessionRef>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListDecisionsParams {
    /// true = only unanswered decisions
    open: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct InitParams {
    /// Project name (default: folder name)
    name: Option<String>,
    /// Also write .agents/skills/pathloom/SKILL.md for other agents (Codex, Gemini CLI, Cursor)
    agents_skill: Option<bool>,
    /// Start from a plan template (ids from list_templates)
    template: Option<String>,
    /// Template language: en | uk
    lang: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ApplyTemplateParams {
    /// Template id (from list_templates)
    id: String,
    /// en | uk (default en)
    lang: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct HeartbeatParams {
    /// Only this task (default: all your claims); claims it when you do not hold it yet
    task_id: Option<String>,
    /// What you are doing now, e.g. "running tests" (replaces the claim note)
    note: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GateParams {
    /// Gate stage key (kind: gate)
    key: String,
    /// Result of running the gate's `check` command: command, exit_code, duration_ms?, summary?, log?
    evidence: pathloom_core::Evidence,
    session: Option<SessionRef>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct UsageParams {
    /// Default 0 (e.g. when you only report the weekly limit)
    tokens_in: Option<u64>,
    tokens_out: Option<u64>,
    /// US dollars, if known
    cost_usd: Option<f64>,
    /// Stage the work was for
    stage: Option<String>,
    /// Task the work was for (its stage is added too)
    task_id: Option<String>,
    /// Model that did the work, e.g. opus
    model: Option<String>,
    /// Share of your weekly limit used so far, 0-100
    percent_weekly: Option<f64>,
    /// When the weekly limit resets (RFC3339)
    resets_at: Option<String>,
    /// Short note, e.g. "planning session"
    note: Option<String>,
    session: Option<SessionRef>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ImportParams {
    /// Format of `text`: spec-kit (GitHub Spec Kit tasks.md)
    format: String,
    /// File contents
    text: String,
    /// Stage key prefix, 1-5 letters (default: initials of the feature name)
    prefix: Option<String>,
    /// Board for new cards (default roadmap)
    board: Option<String>,
    /// Report only; write nothing
    dry_run: Option<bool>,
    /// Store a proposal for the user instead of writing
    propose: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ExportParams {
    /// md | csv | github (JSON array of issues {title, body, labels}) | html (self-contained page)
    format: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct HistoryParams {
    /// Entries to return (default 20, max 200)
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RevertParams {
    /// Entry id from list_history
    id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CommentParams {
    /// Stage key
    stage: String,
    /// One-line comment text
    text: String,
    /// External client session you act from (client, id, url?, title?). Logged with the action.
    session: Option<SessionRef>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct LayoutParams {
    /// Board name (default roadmap)
    board: Option<String>,
    /// Move only the cards of these stage keys; omit to lay out every card
    only_new: Option<Vec<String>>,
    /// External client session you act from (client, id, url?, title?). Logged with the action.
    session: Option<SessionRef>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RegisterParams {
    /// Display name, up to 40 characters
    name: Option<String>,
    /// #rgb or #rrggbb
    color: Option<String>,
    /// One emoji (up to 2 characters) or assets/<file>.png|jpg|webp|svg
    avatar: Option<String>,
    /// Free label: agent (default), orchestrator, reviewer, ...
    kind: Option<String>,
}

#[derive(Clone)]
pub struct PathloomServer {
    root: PathBuf,
    read_only: bool,
    author: Option<String>,
}

fn finish(r: ApiResult<Value>) -> CallToolResult {
    match r {
        Ok(v) => CallToolResult::success(vec![ContentBlock::text(v.to_string())]),
        Err(e) => CallToolResult::error(vec![ContentBlock::text(e.to_json().to_string())]),
    }
}

impl PathloomServer {
    fn project(&self) -> ApiResult<Project> {
        ops::open_project(&self.root).map_err(|mut e| {
            if e.code == "NOT_FOUND" {
                e.message = format!("{} in {}: call init_project to create one here", e.message, self.root.display());
            }
            e
        })
    }

    fn guard(&self) -> ApiResult<()> {
        if self.read_only { Err(ApiError::read_only()) } else { Ok(()) }
    }

    /// Placeholder actor for read-only tools (never logs).
    fn actor_anon(&self) -> ops::Actor {
        ops::Actor::new("agent", "mcp")
    }

    /// Identity for claims and the log: normalized client name (or `--author`), with the optional session.
    fn actor(&self, ctx: &RequestContext<RoleServer>, session: Option<SessionRef>) -> ApiResult<ops::Actor> {
        ops::Actor::new(&self.author(ctx), "mcp").with_session(session)
    }

    fn author(&self, ctx: &RequestContext<RoleServer>) -> String {
        if let Some(a) = &self.author {
            return a.clone();
        }
        match ctx.peer.peer_info() {
            Some(info) if !info.client_info.name.trim().is_empty() => format!("agent:{}", info.client_info.name.trim()),
            _ => "agent".into(),
        }
    }
}

#[tool_router]
impl PathloomServer {
    #[tool(
        description = "Create a Pathloom project (.pathloom/ and plan/) in the folder this server runs in. Only when project_info says there is none and the user wants a plan here."
    )]
    async fn init_project(&self, Parameters(a): Parameters<InitParams>) -> CallToolResult {
        finish((|| {
            self.guard()?;
            let name = a.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| {
                self.root.file_name().map_or_else(|| "project".into(), |n| n.to_string_lossy().into_owned())
            });
            if let Some(t) = &a.template {
                crate::tpl::check(t, a.lang.as_deref())?;
            }
            let p = Project::init(&self.root, name.trim())?;
            let mut v = json!({"name": name.trim(), "root": p.root.to_string_lossy(), "plan_dir": p.config.plan_dir});
            if let Some(t) = &a.template {
                v["template"] = crate::tpl::apply(&p, t, a.lang.as_deref(), true)?["report"].clone();
            }
            if a.agents_skill.unwrap_or(false) {
                let path = ops::install_skill(&p.root, std::path::Path::new(".agents/skills"), false)?;
                v["skill"] = json!(path.to_string_lossy());
            }
            Ok(v)
        })())
    }

    #[tool(
        description = "Plan templates: built-in (product-launch, book, renovation, course) and the user's own. Each has id, title, description, builtin, langs.",
        annotations(read_only_hint = true)
    )]
    async fn list_templates(&self) -> CallToolResult {
        finish(Ok(crate::tpl::list(None)))
    }

    #[tool(
        description = "Add a template's stages, tasks, dependencies and frames to the plan (new keys; existing stages are untouched). In review mode it stores a proposal the user approves instead of writing."
    )]
    async fn apply_template(&self, Parameters(a): Parameters<ApplyTemplateParams>) -> CallToolResult {
        finish((|| {
            self.guard()?;
            crate::tpl::apply(&self.project()?, &a.id, a.lang.as_deref(), false)
        })())
    }

    #[tool(
        description = "Project summary: boards, stage counts per status, top 5 next tasks, review mode. Call this first.",
        annotations(read_only_hint = true)
    )]
    async fn project_info(&self) -> CallToolResult {
        finish(self.project().and_then(|p| ops::project_info(&p, self.read_only)))
    }

    #[tool(
        description = "List stages (key, title, status, progress, who, priority, due). Filters: status, who, board, query. Paginated.",
        annotations(read_only_hint = true)
    )]
    async fn list_stages(&self, Parameters(a): Parameters<ListStagesParams>) -> CallToolResult {
        finish((|| {
            let f = StageFilter {
                status: a.status.as_deref().map(ops::parse_status).transpose()?,
                who: a.who.as_deref().map(ops::parse_who).transpose()?,
                board: a.board,
                query: a.query,
            };
            ops::list_stages(&self.project()?, &f, a.limit.unwrap_or(0), a.cursor.as_deref())
        })())
    }

    #[tool(
        description = "One stage in full (tasks, comments, depends, status, live `claims`) plus `hash`. Pass the hash in apply.expected to detect concurrent edits.",
        annotations(read_only_hint = true)
    )]
    async fn get_stage(&self, Parameters(a): Parameters<KeyParams>) -> CallToolResult {
        finish(self.project().and_then(|p| ops::get_stage(&p, &a.key)))
    }

    #[tool(
        description = "Open tasks you can work on now (stages that are ready or active), sorted by priority, due date, file order. Tasks you claimed come first (`claimed_by`); tasks claimed by other agents are hidden.",
        annotations(read_only_hint = true)
    )]
    async fn next(&self, Parameters(a): Parameters<NextParams>, ctx: RequestContext<RoleServer>) -> CallToolResult {
        finish((|| {
            let agent = match a.agent.as_deref() {
                Some(x) => ops::normalize_agent(x),
                None => self.actor(&ctx, None)?.name,
            };
            let tasks = self.project()?.next_tasks_for(a.limit.unwrap_or(10), Some(&agent))?;
            Ok(ops::next_result(json!(tasks)))
        })())
    }

    #[tool(
        description = "Call this when you START a task, before doing the work: the claim is how the user sees on the board what you are working on right now (your name on the card), and other agents skip the task. Same agent claiming again renews. Fails with CLAIMED (details: task_id, by, expires_at) when someone else holds it. Marking the task done releases the claim.",
        annotations(idempotent_hint = true)
    )]
    async fn claim_task(&self, Parameters(a): Parameters<ClaimParams>, ctx: RequestContext<RoleServer>) -> CallToolResult {
        finish((|| {
            self.guard()?;
            let secs = a.ttl_minutes.unwrap_or(0).saturating_mul(60);
            let d = pathloom_core::Delegation { by: a.by, parent: a.parent, branch: a.branch, worktree: a.worktree };
            ops::claim_task(&self.project()?, &self.actor(&ctx, a.session)?, &a.task_id, secs, a.note.as_deref().unwrap_or(""), &d)
        })())
    }

    #[tool(
        description = "Report where a task is in delivery: claimed, pushed, merged, verified or accepted, with branch, commit sha (7-40 hex) and evidence files (screenshots). The user sees the pipeline on the task card. Also counts as a heartbeat.",
        annotations(idempotent_hint = true)
    )]
    async fn set_task_status(&self, Parameters(a): Parameters<TaskStatusParams>, ctx: RequestContext<RoleServer>) -> CallToolResult {
        finish((|| {
            self.guard()?;
            let ev = a.evidence.unwrap_or_default();
            ops::set_task_status(&self.project()?, &self.actor(&ctx, a.session)?, &a.task_id, &a.status, a.branch.as_deref(), a.sha.as_deref(), &ev)
        })())
    }

    #[tool(
        description = "Git commits whose subject starts with a stage or task id (`AUTH-02: message`, `AUTH-02, 03: ...`, `AUTH: ...`), newest first. Empty without git.",
        annotations(read_only_hint = true)
    )]
    async fn list_commits(&self, Parameters(a): Parameters<CommitsParams>) -> CallToolResult {
        finish(self.project().and_then(|p| ops::commits(&p, a.stage.as_deref(), a.limit)))
    }

    #[tool(description = "Release your claim on a task without finishing it. force=true takes over another agent's claim: only when the user asks.")]
    async fn release_task(&self, Parameters(a): Parameters<ReleaseParams>, ctx: RequestContext<RoleServer>) -> CallToolResult {
        finish((|| {
            self.guard()?;
            ops::release_task(&self.project()?, &self.actor(&ctx, a.session)?, &a.task_id, a.force.unwrap_or(false))
        })())
    }

    #[tool(
        description = "Recent actions of agents and humans on this plan (who did what, newest first). Check it to see what others changed. Optional stage filter.",
        annotations(read_only_hint = true)
    )]
    async fn activity(&self, Parameters(a): Parameters<ActivityParams>) -> CallToolResult {
        finish(self.project().and_then(|p| ops::activity(&p, a.limit.unwrap_or(0), a.stage.as_deref())))
    }

    #[tool(
        description = "Parse a quick-add sentence (en/uk) into a draft {title, stage, due, who, unmatched} against this plan's stages. Read-only: nothing is created; use `apply` to add the task.",
        annotations(read_only_hint = true)
    )]
    async fn parse_quick_add(&self, Parameters(a): Parameters<QuickAddParams>) -> CallToolResult {
        finish(self.project().and_then(|p| crate::people::parse(&p, &a.text)))
    }

    #[tool(
        description = "Schedule from today over hard dependencies: start, finish, slack against `due`, float and critical path per stage (stage `estimate` in days, default 1; done stages take 0).",
        annotations(read_only_hint = true)
    )]
    async fn schedule(&self) -> CallToolResult {
        finish(self.project().and_then(|p| crate::people::schedule(&p)))
    }

    #[tool(
        description = "Deep link `pathloom://open?project=...&stage=KEY&task=ID` that opens the project, a stage or a task card in the Pathloom app. `id` is a stage key or task id; omit it for the project. Put it in messages and PRs so the user can jump to the card.",
        annotations(read_only_hint = true)
    )]
    async fn make_link(&self, Parameters(a): Parameters<MakeLinkParams>) -> CallToolResult {
        finish(self.project().and_then(|p| ops::make_link(&p, a.id.as_deref())))
    }

    #[tool(
        description = "Agenda for the owner: overdue stages, stages due within `days` (default 1 = today), blocked stages, and what waits for the owner (inbox). Done stages are left out.",
        annotations(read_only_hint = true)
    )]
    async fn agenda(&self, Parameters(a): Parameters<AgendaParams>) -> CallToolResult {
        finish(self.project().and_then(|p| crate::agenda::agenda(&p, a.days.unwrap_or(1))))
    }

    #[tool(
        description = "Case-insensitive text search over stage keys/titles/descriptions and task titles.",
        annotations(read_only_hint = true)
    )]
    async fn search(&self, Parameters(a): Parameters<SearchParams>) -> CallToolResult {
        finish(self.project().and_then(|p| ops::search(&p, &a.query, a.limit.unwrap_or(0))))
    }

    #[tool(
        description = "Batch create/update stages, tasks, links, frames, comments from a spec. All or nothing. Use dry_run first. In review mode, or with propose=true, it stores a proposal the user approves in the app instead of writing; read the outcome with list_proposals."
    )]
    async fn apply(&self, Parameters(a): Parameters<ApplyParams>, ctx: RequestContext<RoleServer>) -> CallToolResult {
        finish((|| {
            let dry = a.dry_run.unwrap_or(false);
            if !dry {
                self.guard()?;
            }
            let mut spec = ops::parse_spec_value(&a.spec)?;
            let p = self.project()?;
            let done: Vec<String> = spec.stages.iter().flat_map(|s| s.tasks_done.iter().cloned()).collect();
            ops::require_no_evidence_policy(&p, &done)?;
            let (propose, actor) = (a.propose.unwrap_or(false), self.actor(&ctx, a.session)?);
            // require_approval: an agent's direct changes do not close tasks, they send them to review.
            let direct = !dry && !propose && p.config.agent_changes == pathloom_core::AgentChanges::Auto;
            let diverted = if direct { ops::divert_done_to_review(&p, &mut spec)? } else { vec![] };
            let v = ops::submit_with(&p, &spec, &self.author(&ctx), dry, false, propose)?;
            let v = ops::review_diverted(&p, &actor, &diverted, v)?;
            Ok(ops::log_submit(&p, &actor, "apply", &spec, v))
        })())
    }

    #[tool(
        description = "Proposals made with apply (propose=true or review mode): state pending (default) | approved | rejected | conflict | all. Each has `changes` (stage files before/after) and, once the user decided, `decision` {state, comment, at, by}. conflict = a stage changed meanwhile, nothing was applied: re-read and propose again.",
        annotations(read_only_hint = true)
    )]
    async fn list_proposals(&self, Parameters(a): Parameters<ListProposalsParams>) -> CallToolResult {
        finish(self.project().and_then(|p| ops::list_proposals(&p, a.state.as_deref())))
    }

    #[tool(
        description = "Relay a human's decision on a pending proposal: approve (applies it as its author) or reject. Only when the project has allow_client_answers = true (otherwise FORBIDDEN); `human` names the person who decided and must not be you. You can never decide your own proposals (FORBIDDEN). Returns the decision; state conflict = nothing applied."
    )]
    async fn decide_proposal(&self, Parameters(a): Parameters<DecideProposalParams>, ctx: RequestContext<RoleServer>) -> CallToolResult {
        finish((|| {
            self.guard()?;
            let actor = self.actor(&ctx, a.session)?;
            ops::decide_proposal_for(&self.project()?, &actor, &a.id, a.approve, a.comment.as_deref(), &a.human)
        })())
    }

    #[tool(
        description = "Mark a task or subtask done (true) or not done (false); claim_task it first when you start the work. Attach `evidence` (the check you ran, exit_code 0) so the task shows as verified. Returns the stage's new status; with require_approval on, the result is {\"state\": \"review\"}: the task waits for the owner."
    )]
    async fn set_task(&self, Parameters(a): Parameters<SetTaskParams>, ctx: RequestContext<RoleServer>) -> CallToolResult {
        finish((|| {
            self.guard()?;
            ops::set_task(&self.project()?, &self.actor(&ctx, a.session)?, if a.done { "task done" } else { "task undo" }, &a.task_id, a.done, a.evidence, true)
        })())
    }

    #[tool(
        description = "Ask the owner to approve your work on a task (a screenshot, a design, a result): attach `files` and a `note`; the task goes to review (not done) until the owner approves in the app. Read the outcome with get_approvals: when rejected you get `reasons` and `comment` (also added to the stage comments); fix it and request again."
    )]
    async fn request_approval(&self, Parameters(a): Parameters<RequestApprovalParams>, ctx: RequestContext<RoleServer>) -> CallToolResult {
        finish((|| {
            self.guard()?;
            let actor = self.actor(&ctx, a.session)?;
            ops::request_approval(&self.project()?, &actor, &a.task_id, &a.files.unwrap_or_default(), a.note.as_deref())
        })())
    }

    #[tool(
        description = "Publish the live state of your session on the board: session {client, id, url?, title?, state: working | waiting | blocked | error | done} plus task_id (or stage). The card shows the state and a link to open your session. Logged only when the state changes; call it at every change. Any other call that carries `session` with a state updates it too.",
        annotations(idempotent_hint = true)
    )]
    async fn update_session(&self, Parameters(a): Parameters<UpdateSessionParams>, ctx: RequestContext<RoleServer>) -> CallToolResult {
        finish((|| {
            self.guard()?;
            ops::update_session(&self.project()?, &self.actor(&ctx, Some(a.session))?, a.task_id.as_deref(), a.stage.as_deref())
        })())
    }

    #[tool(
        description = "Relay a human's answer to a task in review (approve or reject). Only when the project has allow_client_answers = true (otherwise FORBIDDEN); `human` names the person who answered and must not be you. The first answer wins: a task already decided fails with ALREADY_DECIDED (details: by, at, state). Never decide for the user."
    )]
    async fn respond_approval(&self, Parameters(a): Parameters<RespondApprovalParams>, ctx: RequestContext<RoleServer>) -> CallToolResult {
        finish((|| {
            self.guard()?;
            let actor = self.actor(&ctx, a.session)?;
            ops::respond_approval(&self.project()?, &actor, &a.task_id, a.approve, &a.reasons.unwrap_or_default(), a.comment.as_deref(), &a.human)
        })())
    }

    #[tool(
        description = "Relay a human's answer to an open decision (see list_decisions): choice is an option label or free text. Only when the project has allow_client_answers = true (otherwise FORBIDDEN); `human` names the person who answered and must not be you. The first answer wins: ALREADY_DECIDED (details: by, at, choice) otherwise."
    )]
    async fn answer_decision(&self, Parameters(a): Parameters<AnswerDecisionParams>, ctx: RequestContext<RoleServer>) -> CallToolResult {
        finish((|| {
            self.guard()?;
            let actor = self.actor(&ctx, a.session)?;
            ops::answer_decision_for(&self.project()?, &actor, &a.task_id, &a.choice, &a.human)
        })())
    }

    #[tool(
        description = "Approvals of all stages: task_id, stage, title, state (pending | approved | rejected), files, note, reasons, comment, at, by, requested_by, requested_at. Filter by state. Only the owner approves or rejects (in the app or CLI); you cannot.",
        annotations(read_only_hint = true)
    )]
    async fn get_approvals(&self, Parameters(a): Parameters<GetApprovalsParams>) -> CallToolResult {
        finish(self.project().and_then(|p| ops::approvals(&p, a.state.as_deref())))
    }

    #[tool(
        description = "Ask the owner a question with options on a task (e.g. DECIDE-01): question, 1-8 options {label, description?}, recommended (index). The task stays open until the owner answers; read the answer with list_decisions or get_stage (task_info.<ID>.decision.choice)."
    )]
    async fn ask_decision(&self, Parameters(a): Parameters<AskDecisionParams>, ctx: RequestContext<RoleServer>) -> CallToolResult {
        finish((|| {
            self.guard()?;
            let actor = self.actor(&ctx, a.session)?;
            ops::ask_decision(&self.project()?, &actor, &a.task_id, &a.question, a.options, a.recommended)
        })())
    }

    #[tool(
        description = "Decisions of all stages: task_id, stage, title, question, options, recommended, choice (the owner's answer, absent while open), at, by, asked_by. open=true lists only unanswered ones.",
        annotations(read_only_hint = true)
    )]
    async fn list_decisions(&self, Parameters(a): Parameters<ListDecisionsParams>) -> CallToolResult {
        finish(self.project().and_then(|p| ops::decisions(&p, a.open.unwrap_or(false))))
    }

    #[tool(
        description = "Still working: refresh your claims so the app does not show you as silent (after 10 min without any action) and they do not expire. Call every few minutes during long steps; `note` tells the user what you are doing. With a task_id you have not claimed yet it claims that task.",
        annotations(idempotent_hint = true)
    )]
    async fn heartbeat(&self, Parameters(a): Parameters<HeartbeatParams>, ctx: RequestContext<RoleServer>) -> CallToolResult {
        finish((|| {
            self.guard()?;
            ops::heartbeat(&self.project()?, &self.actor(&ctx, None)?, a.task_id.as_deref(), a.note.as_deref())
        })())
    }

    #[tool(
        description = "Import another tool's task list as stages with tasks and dependencies. format: spec-kit (GitHub Spec Kit tasks.md), md (headings = stages, - [ ] items = tasks), csv (Pathloom export, a title column, or Notion), notion (Notion CSV), trello (board JSON). Re-import updates the same stages. Use dry_run first; propose=true stores a proposal for the user."
    )]
    async fn import_tasks(&self, Parameters(a): Parameters<ImportParams>, ctx: RequestContext<RoleServer>) -> CallToolResult {
        finish((|| {
            let dry = a.dry_run.unwrap_or(false);
            if !dry {
                self.guard()?;
            }
            let (p, actor) = (self.project()?, self.actor(&ctx, None)?);
            ops::import_as(&p, &actor, &a.format, &a.text, a.prefix.as_deref(), a.board.as_deref(), dry, false, a.propose.unwrap_or(false))
        })())
    }

    #[tool(
        description = "The plan as text: format md (Markdown checklist), csv, github (JSON array of {title, body, labels}, one per open task; no network) or html (one self-contained read-only page). Returns {format, text}.",
        annotations(read_only_hint = true)
    )]
    async fn export_plan(&self, Parameters(a): Parameters<ExportParams>) -> CallToolResult {
        finish(self.project().and_then(|p| ops::export_plan(&p, &a.format)))
    }

    #[tool(
        description = "Change history, newest first: {entries: [{id, at, action, files}]}. Every logged action that changed plan files is an entry; revert_change undoes one.",
        annotations(read_only_hint = true)
    )]
    async fn list_history(&self, Parameters(a): Parameters<HistoryParams>) -> CallToolResult {
        finish(self.project().and_then(|p| ops::history(&p, a.limit.unwrap_or(0))))
    }

    #[tool(
        description = "Undo one history entry by id. Fails with CONFLICT (details.paths) when those files changed since; nothing is written then. In review mode (agent_changes = review) agents may not revert: FORBIDDEN, ask the owner. The revert is logged and can be reverted too."
    )]
    async fn revert_change(&self, Parameters(a): Parameters<RevertParams>, ctx: RequestContext<RoleServer>) -> CallToolResult {
        finish((|| {
            self.guard()?;
            ops::revert_change(&self.project()?, &self.actor(&ctx, None)?, &a.id, true)
        })())
    }

    #[tool(
        description = "Record the result of a gate stage's check (kind: gate, its `check` command, e.g. npm test). Run the command yourself first. exit_code 0 makes the gate done; non-zero reopens it."
    )]
    async fn set_gate(&self, Parameters(a): Parameters<GateParams>, ctx: RequestContext<RoleServer>) -> CallToolResult {
        finish((|| {
            self.guard()?;
            ops::set_gate(&self.project()?, &self.actor(&ctx, a.session)?, &a.key, a.evidence)
        })())
    }

    #[tool(
        description = "Optional: report tokens/cost of your work (for a stage or task_id, with the model) so the user sees spending per stage and model; percent_weekly (0-100) + resets_at report your weekly limit. Pathloom only stores what you report."
    )]
    async fn report_usage(&self, Parameters(a): Parameters<UsageParams>, ctx: RequestContext<RoleServer>) -> CallToolResult {
        finish((|| {
            self.guard()?;
            let u = pathloom_core::Usage {
                tokens_in: a.tokens_in.unwrap_or(0),
                tokens_out: a.tokens_out.unwrap_or(0),
                cost_usd: a.cost_usd,
                model: a.model,
                percent_weekly: a.percent_weekly,
                resets_at: a.resets_at,
            };
            ops::report_usage(&self.project()?, &self.actor(&ctx, a.session)?, u, a.stage.as_deref(), a.task_id.as_deref(), a.note.as_deref().unwrap_or(""))
        })())
    }

    #[tool(description = "Add a one-line comment to a stage, authored as the connected agent.")]
    async fn add_comment(&self, Parameters(a): Parameters<CommentParams>, ctx: RequestContext<RoleServer>) -> CallToolResult {
        finish((|| {
            self.guard()?;
            ops::add_comment(&self.project()?, &self.actor(&ctx, a.session)?, &a.stage, &self.author(&ctx), &a.text)
        })())
    }

    #[tool(description = "Auto-layout cards on a board, left to right by dependencies. only_new limits which cards move.")]
    async fn layout_board(&self, Parameters(a): Parameters<LayoutParams>, ctx: RequestContext<RoleServer>) -> CallToolResult {
        finish((|| {
            self.guard()?;
            ops::layout(&self.project()?, &self.actor(&ctx, a.session)?, a.board.as_deref().unwrap_or("roadmap"), &a.only_new.unwrap_or_default())
        })())
    }

    #[tool(
        description = "Register or update your own display profile (name, color, avatar, kind) shown in the app and in logs. Your id is your caller identity. Call once at session start."
    )]
    async fn register_agent(&self, Parameters(a): Parameters<RegisterParams>, ctx: RequestContext<RoleServer>) -> CallToolResult {
        finish((|| {
            self.guard()?;
            let actor = self.actor(&ctx, None)?;
            let patch = ops::ProfilePatch { name: a.name, color: a.color, avatar: a.avatar, kind: a.kind };
            ops::set_agent(&self.project()?, &actor, "register_agent", &actor.name, patch)
        })())
    }

    #[tool(description = "All agent profiles (id, name, color, avatar, kind).", annotations(read_only_hint = true))]
    async fn list_agents(&self) -> CallToolResult {
        finish(self.project().and_then(|p| ops::list_agents(&p)))
    }

    #[tool(
        description = "Check project integrity: broken depends, cycles, bad task ids, cards of missing stages. `hints` lists readability fixes (redundant links, oversized stages, missing descriptions): apply them.",
        annotations(read_only_hint = true)
    )]
    async fn validate(&self) -> CallToolResult {
        finish(self.project().and_then(|p| ops::validate(&p, &self.actor_anon(), false)))
    }
}

fn resources() -> Vec<(&'static str, &'static str, &'static str, String)> {
    let js = |k| serde_json::to_string_pretty(&schema(k)).unwrap_or_default();
    vec![
        ("pathloom://guide/workflow", "Workflow guide", "text/markdown", GUIDE.to_string()),
        ("pathloom://schema/stage", "Stage JSON Schema", "application/json", js(SchemaKind::Stage)),
        ("pathloom://schema/board", "Board JSON Schema", "application/json", js(SchemaKind::Board)),
        ("pathloom://schema/spec", "Apply spec JSON Schema", "application/json", js(SchemaKind::Spec)),
    ]
}

#[tool_handler]
impl ServerHandler for PathloomServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().enable_resources().build())
            .with_server_info(Implementation::new("pathloom", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Pathloom project plan. Start with project_info, then next. When you start a task, call claim_task first: the user watches the board and sees what you are doing only through your claim. set_task done releases it. Read pathloom://guide/workflow for the rules."
                    .to_string(),
            )
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let items = resources()
            .into_iter()
            .map(|(uri, name, mime, _)| Resource::new(uri, name).with_mime_type(mime))
            .collect();
        Ok(ListResourcesResult::with_all_items(items))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let Some((uri, _, mime, text)) = resources().into_iter().find(|r| r.0 == request.uri) else {
            return Err(ErrorData::resource_not_found(format!("unknown resource {}", request.uri), None));
        };
        let contents = ResourceContents::TextResourceContents {
            uri: uri.to_string(),
            mime_type: Some(mime.to_string()),
            text,
            meta: None,
        };
        Ok(ReadResourceResult::new(vec![contents]).into())
    }
}

/// Serve MCP on stdio until the client disconnects. Nothing else may write to stdout.
pub fn run(root: PathBuf, read_only: bool, author: Option<String>) -> Result<(), String> {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|e| e.to_string())?;
    rt.block_on(async {
        let server = PathloomServer { root, read_only, author };
        let service = server.serve(stdio()).await.map_err(|e| e.to_string())?;
        service.waiting().await.map_err(|e| e.to_string())?;
        Ok(())
    })
}
