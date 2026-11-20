//! clap definitions (spec §7.1).

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "pathloom", version, about = "Pathloom: plan files, roadmap and MCP server for AI agents")]
pub struct Cli {
    /// Project root (default: search `.pathloom/` upwards from the current dir)
    #[arg(long, global = true, value_name = "PATH")]
    pub project: Option<PathBuf>,
    /// Machine-readable JSON output (errors as JSON on stderr)
    #[arg(long, global = true)]
    pub json: bool,
    /// Author recorded on comments, claims and the activity log (`agent:` prefix is ignored)
    #[arg(long, global = true, default_value = "agent")]
    pub author: String,
    #[command(subcommand)]
    pub command: Cmd,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Create a Pathloom project in the project dir (or current dir)
    Init {
        #[arg(long, default_value = "plan")]
        plan_dir: String,
        /// Project name (default: folder name)
        #[arg(long)]
        name: Option<String>,
        /// Also write .agents/skills/pathloom/SKILL.md (Agent Skills format)
        #[arg(long)]
        agents_skill: bool,
        /// Start from a plan template (see `template list`)
        #[arg(long)]
        template: Option<String>,
        /// Template language: en | uk
        #[arg(long)]
        lang: Option<String>,
    },
    /// Plan templates: list, save the current plan, apply
    #[command(subcommand)]
    Template(TemplateCmd),
    /// Manage boards
    #[command(subcommand)]
    Board(BoardCmd),
    /// Manage stages
    #[command(subcommand)]
    Stage(StageCmd),
    /// Manage tasks
    #[command(subcommand)]
    Task(TaskCmd),
    /// Make FROM a prerequisite of TO
    Link {
        from: String,
        to: String,
        /// Soft dependency (does not block)
        #[arg(long)]
        soft: bool,
        /// Skip the review proposal and write directly
        #[arg(long)]
        direct: bool,
    },
    /// Remove the dependency FROM -> TO
    Unlink {
        from: String,
        to: String,
        #[arg(long)]
        direct: bool,
    },
    /// Add a comment to a stage
    Comment { key: String, text: String },
    /// Tasks claimed right now (by whom, until when)
    Claims,
    /// Recent agent activity, newest first
    Log {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Only entries that touch this stage
        #[arg(long, value_name = "KEY")]
        stage: Option<String>,
    },
    /// Open tasks you can work on now (your claimed tasks first; tasks claimed by others are hidden)
    Next {
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Apply a YAML/JSON spec (file or `-` for stdin)
    Apply {
        file: String,
        /// Validate and report, write nothing
        #[arg(long)]
        dry_run: bool,
        /// Write directly even when the project is in review mode
        #[arg(long)]
        direct: bool,
        /// Store a proposal for human approval (whatever the project mode is)
        #[arg(long, conflicts_with = "direct")]
        propose: bool,
    },
    /// Auto-layout the cards of a board (layered, left to right)
    Layout {
        board: String,
        /// Move only the cards of these stage keys
        #[arg(long, num_args = 1.., value_name = "KEY")]
        only_new: Vec<String>,
    },
    /// Check project integrity
    Validate {
        /// Safe fixes only: drop broken depends, remove cards of missing stages
        #[arg(long)]
        fix: bool,
    },
    /// Print a JSON Schema
    Schema { what: SchemaKind },
    /// Agent proposals: list, approve, reject
    #[command(subcommand, alias = "proposal")]
    Proposals(ProposalCmd),
    /// Approvals: request, approve, reject, list; `require` toggles review for agents
    #[command(subcommand)]
    Approval(ApprovalCmd),
    /// Decisions: ask the owner a question with options, or answer it
    #[command(subcommand)]
    Decision(DecisionCmd),
    /// All decisions; --md prints the answered ones as Markdown
    Decisions {
        #[arg(long)]
        md: bool,
    },
    /// What waits for the owner: pending approvals, open decisions, open owner tasks
    Inbox,
    /// Quick add in plain words: `pathloom add "add task X to stage AUTH by friday" [--yes]`
    Add {
        text: String,
        /// Apply the parsed draft (without it only the draft is printed)
        #[arg(long)]
        yes: bool,
    },
    /// Dates, slack and the critical path of the plan
    Schedule,
    /// Print a pathloom:// link that opens the project (or a stage key / task id) in the app
    Deeplink {
        /// Stage key (AUTH) or task id (AUTH-01); omit for the project
        id: Option<String>,
    },
    /// Overdue, due today (or within 7 days with --week), blocked, and what waits for you
    Today {
        #[arg(long)]
        week: bool,
    },
    /// What was done since a time, what is stuck, what waits (Markdown with --md)
    Digest {
        /// 24h, 7d, 90m or an RFC3339 UTC time (default 24h)
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        md: bool,
    },
    /// Bridge pending approvals to a Telegram chat (needs [telegram] in project.toml and PATHLOOM_TELEGRAM_TOKEN); runs until killed
    Telegram,
    /// Protocol version, capabilities, counts and agent profiles
    Info,
    /// Agent profiles: display name, color, avatar
    #[command(subcommand)]
    Agent(AgentCmd),
    /// Stream project events until killed: `hello`, then live changes (use --json for NDJSON)
    Watch {
        /// First replay activity-log entries newer than this time (RFC3339, e.g. 2026-11-05T10:00:00Z)
        #[arg(long, value_name = "RFC3339")]
        since: Option<String>,
        /// Only events of this stage key (repeatable); events without a stage are not filtered
        #[arg(long = "stage", value_name = "KEY")]
        stages: Vec<String>,
        /// Only these event types, comma-separated or repeated: stage, board, claims, activity, proposal, agents, approval, decision, task (`hello` always passes)
        #[arg(long = "type", value_name = "TYPE", value_delimiter = ',')]
        types: Vec<String>,
    },
    /// Client sessions on the board: publish a session's state
    #[command(subcommand)]
    Session(SessionCmd),
    /// Agent Skills export
    #[command(subcommand)]
    Skill(SkillCmd),
    /// Gate stages (`kind: gate`): run the check
    #[command(subcommand)]
    Gate(GateCmd),
    /// Token / cost totals reported by agents. With --tokens-in/--tokens-out it records a report instead
    Usage {
        #[arg(long)]
        tokens_in: Option<u64>,
        #[arg(long)]
        tokens_out: Option<u64>,
        /// US dollars
        #[arg(long)]
        cost: Option<f64>,
        /// Stage the work was for
        #[arg(long, value_name = "KEY")]
        stage: Option<String>,
        /// Task the work was for (adds its stage too)
        #[arg(long, value_name = "ID")]
        task: Option<String>,
        #[arg(long)]
        model: Option<String>,
        /// Share of the weekly limit used, 0-100
        #[arg(long)]
        percent_weekly: Option<f64>,
        /// When the weekly limit resets (RFC3339)
        #[arg(long)]
        resets_at: Option<String>,
        #[arg(long, default_value = "")]
        note: String,
    },
    /// Commits whose subject names a stage or task (`AUTH-02: message`)
    Commits {
        #[arg(long, value_name = "KEY")]
        stage: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Import another tool's task list as stages (Spec Kit: specs/<feature>/tasks.md). Re-import updates them
    Import {
        format: ImportFormat,
        /// File, or `-` for stdin
        file: String,
        /// Stage key prefix, 1-5 letters (default: initials of the feature name)
        #[arg(long)]
        prefix: Option<String>,
        /// Board for the new cards (default roadmap)
        #[arg(long)]
        board: Option<String>,
        #[arg(long)]
        dry_run: bool,
        /// Write directly even when the project is in review mode
        #[arg(long)]
        direct: bool,
        /// Store a proposal for human approval (whatever the project mode is)
        #[arg(long, conflicts_with = "direct")]
        propose: bool,
    },
    /// Export the plan as Markdown, CSV, GitHub-issues JSON or read-only HTML (stdout, or -o FILE)
    #[command(subcommand)]
    Export(ExportCmd),
    /// Change history: what each logged action changed in the plan files
    History {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Undo one history entry (only when its files were not changed since)
    Revert { id: String },
    /// Run the MCP server on stdio
    Mcp {
        /// Expose read-only tools only
        #[arg(long)]
        read_only: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum BoardCmd {
    Create {
        name: String,
        #[arg(long)]
        title: Option<String>,
    },
    List,
}

#[derive(Subcommand, Debug)]
pub enum StageCmd {
    /// Create a stage (and its card)
    Add {
        key: String,
        title: String,
        /// Depend on this stage and place the card right of it
        #[arg(long)]
        after: Option<String>,
        #[arg(long, default_value = "ai")]
        who: String,
        #[arg(long, default_value = "roadmap")]
        board: String,
        #[arg(long)]
        direct: bool,
    },
    Show {
        key: String,
    },
    /// Change a stage key everywhere: file name, task ids, dependencies, cards, claims
    RenameKey {
        key: String,
        new_key: String,
    },
    List {
        /// done | active | ready | blocked
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        who: Option<String>,
        #[arg(long)]
        board: Option<String>,
        #[arg(long)]
        query: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum TaskCmd {
    Add {
        key: String,
        title: String,
        #[arg(long)]
        direct: bool,
    },
    /// Mark a task done. With --run the check runs first and is stored as evidence; a failing check keeps it open
    Done {
        task_id: String,
        /// Check to run before closing, e.g. "cargo test" (through the system shell; output is shown)
        #[arg(long, value_name = "CMD")]
        run: Option<String>,
        /// Short result to store with the evidence, e.g. "94 passed"
        #[arg(long, requires = "run")]
        summary: Option<String>,
        /// Link or path to the full log
        #[arg(long, requires = "run")]
        log: Option<String>,
        /// Commit the check ran on (7-40 hex sha)
        #[arg(long, requires = "run", value_name = "SHA")]
        commit: Option<String>,
        /// Git branch the check ran on
        #[arg(long, requires = "run")]
        branch: Option<String>,
    },
    /// Set the delivery status of a task: claimed, pushed, merged, verified or accepted
    Status {
        task_id: String,
        step: String,
        #[arg(long)]
        branch: Option<String>,
        /// Commit, 7-40 hex
        #[arg(long)]
        sha: Option<String>,
        /// Screenshot or log: project-relative path or link (repeat up to 10 times)
        #[arg(long, value_name = "FILE")]
        evidence: Vec<String>,
    },
    Undo {
        task_id: String,
    },
    /// Claim a task so other agents skip it (renews when you already hold it)
    Claim {
        task_id: String,
        /// Lifetime: 90s, 30m, 2h (bare number = minutes); max 8h
        #[arg(long, default_value = "30m")]
        ttl: String,
        #[arg(long, default_value = "")]
        note: String,
        /// Who really does the work (model or agent name)
        #[arg(long)]
        by: Option<String>,
        /// Agent that delegated the task
        #[arg(long)]
        parent: Option<String>,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long)]
        worktree: Option<String>,
        /// External client of the session this claim comes from (e.g. maestro)
        #[arg(long, requires = "session_id")]
        session_client: Option<String>,
        /// Session id in that client
        #[arg(long, requires = "session_client")]
        session_id: Option<String>,
        #[arg(long, requires = "session_client")]
        session_url: Option<String>,
        #[arg(long, requires = "session_client")]
        session_title: Option<String>,
    },
    /// Still working: refresh your claims (all, or one task) so they are not shown as silent or expire
    Heartbeat {
        /// Only this task; claims it when you do not hold it yet
        task_id: Option<String>,
        /// What you are doing now (replaces the claim note)
        #[arg(long)]
        note: Option<String>,
    },
    /// Release your claim on a task
    Release {
        task_id: String,
        /// Release a claim held by another agent
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum AgentCmd {
    /// Create or update a profile (unset flags keep the current value)
    Set {
        /// Agent id `[a-z0-9._-]{1,32}`, the same as in `who: ai:<id>`
        id: String,
        #[arg(long)]
        name: Option<String>,
        /// #rgb or #rrggbb
        #[arg(long)]
        color: Option<String>,
        /// One emoji or assets/<file>.png
        #[arg(long)]
        avatar: Option<String>,
        #[arg(long)]
        kind: Option<String>,
    },
    List,
    Remove {
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum TemplateCmd {
    /// Built-in and your own templates (~/.pathloom/templates)
    List,
    /// Save the current plan (stages, tasks, dependencies, frames; no progress, dates or log) as a template
    Save {
        /// [a-z0-9-], up to 40
        name: String,
        #[arg(long)]
        title: Option<String>,
    },
    /// Add a template's stages to this project
    Apply {
        id: String,
        /// en | uk
        #[arg(long)]
        lang: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum SessionCmd {
    /// Upsert a client session on a task's (or a stage's) card and set its state
    Set {
        /// Client name, e.g. maestro
        #[arg(long)]
        client: String,
        /// Session id in that client
        #[arg(long)]
        id: String,
        #[arg(long)]
        url: Option<String>,
        #[arg(long)]
        title: Option<String>,
        /// working | waiting | blocked | error | done
        #[arg(long)]
        state: Option<String>,
        /// Task the session works on
        #[arg(long, value_name = "ID", conflicts_with = "stage")]
        task: Option<String>,
        /// Stage key, when there is no task
        #[arg(long, value_name = "KEY")]
        stage: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum SkillCmd {
    /// Write .agents/skills/pathloom/SKILL.md into the project
    Install {
        /// Skills folder, relative to the project root
        #[arg(long, default_value = ".agents/skills")]
        dir: PathBuf,
        /// Overwrite an existing file
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum GateCmd {
    /// Run the gate's `check` command (system shell, output shown) and record the result
    Run { key: String },
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum ImportFormat {
    /// GitHub Spec Kit tasks.md
    SpecKit,
    /// Markdown headings (stages) and `- [ ]` lists (tasks)
    Md,
    /// CSV: Pathloom's export, a `title` column, or a Notion export
    Csv,
    /// Trello board JSON export
    Trello,
    /// Notion CSV export
    Notion,
}

impl ImportFormat {
    pub fn name(self) -> &'static str {
        match self {
            Self::SpecKit => "spec-kit",
            Self::Md => "md",
            Self::Csv => "csv",
            Self::Trello => "trello",
            Self::Notion => "notion",
        }
    }
}

/// Shared by every `export` format.
#[derive(clap::Args, Debug)]
pub struct ExportOut {
    /// Write to this file instead of stdout
    #[arg(short, long, value_name = "FILE")]
    pub output: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum ExportCmd {
    /// Markdown: stage headings, `- [ ]` / `- [x]` tasks, Depends lines
    Md(ExportOut),
    /// CSV: stage_id,stage_title,task_id,task_title,done,who,priority,due,depends
    Csv(ExportOut),
    /// JSON array of {title, body, labels}, one per open task (no network)
    Github(ExportOut),
    /// One self-contained read-only HTML file: board, stages, progress (no scripts, no external links)
    Html(ExportOut),
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum SchemaKind {
    Stage,
    Board,
    Spec,
}

#[derive(Subcommand, Debug)]
pub enum ApprovalCmd {
    /// Ask the owner to approve a task (it goes to review)
    Request {
        task_id: String,
        /// File to show (project path or http(s) link); repeatable
        #[arg(long = "file", value_name = "FILE")]
        files: Vec<String>,
        #[arg(long)]
        note: Option<String>,
    },
    /// Approve: ticks the task
    Approve {
        task_id: String,
        #[arg(long)]
        comment: Option<String>,
    },
    /// Reject: the task stays open, reasons and comment are stored and added to the stage comments
    Reject {
        task_id: String,
        /// Reason (off-theme, duplicate, quality, scale, wrong-place, or your own); repeatable
        #[arg(long = "reason", value_name = "REASON")]
        reasons: Vec<String>,
        #[arg(long)]
        comment: Option<String>,
    },
    /// List approvals (pending|approved|rejected|all)
    List {
        #[arg(long)]
        state: Option<String>,
    },
    /// Turn `require_approval` on or off: agents then cannot close tasks, they go to review
    Require { value: OnOff },
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum OnOff {
    On,
    Off,
}

#[derive(Subcommand, Debug)]
pub enum DecisionCmd {
    /// Ask a question with options
    Ask {
        task_id: String,
        #[arg(long)]
        question: String,
        /// Option as `label` or `label|description`; repeat for 1-8 options
        #[arg(long = "option", value_name = "LABEL|DESCRIPTION", required = true)]
        options: Vec<String>,
        /// Index (from 0) of the recommended option
        #[arg(long)]
        recommended: Option<usize>,
    },
    /// Answer: an option label or your own text
    Answer { task_id: String, text: String },
}

#[derive(Subcommand, Debug)]
pub enum ProposalCmd {
    /// List proposals (default: pending)
    List {
        /// pending|approved|rejected|conflict|all
        #[arg(long)]
        state: Option<String>,
    },
    /// Apply a proposal (conflict when a stage changed since)
    #[command(alias = "accept")]
    Approve {
        id: String,
        #[arg(long)]
        comment: Option<String>,
    },
    Reject {
        id: String,
        #[arg(long)]
        comment: Option<String>,
    },
}

