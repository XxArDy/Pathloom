# Changelog

All notable changes to Pathloom. Versions follow [Semantic Versioning](https://semver.org/); the file and MCP formats are versioned
separately as `pathloom/1`.

## Unreleased

### Added
- Proposals with a diff: approve or reject each one; a proposal made on an older version of a stage is flagged as a conflict. `apply` with `propose: true` sends a single change for review.
- Approvals and decisions: agents request approval of a task (with files) or ask you to choose between options; rejections carry reasons and a comment and return the task to the agent. **Waiting for me** inbox (`pathloom inbox`). `pathloom approval require on` (`require_approval`) sends agent-closed tasks to review.
- Telegram bridge `pathloom telegram` for approvals and the daily digest (token from `PATHLOOM_TELEGRAM_TOKEN`, chat id in `project.toml`).
- Delivery status per task (`pathloom task status`, `set_task_status`), evidence with commit and branch, `pathloom commits`.
- Usage per task and model, weekly-limit meter (`pathloom usage`, `report_usage`).
- Read the board from a git ref; owner actions are committed or queued.
- Today / Week view and `pathloom today`; reminders and `pathloom digest` (off by default).
- Plan templates: built-in (product launch, book, renovation, course) and your own; `pathloom init --template`, `pathloom template list|apply|save`.
- Quick add in plain words (`pathloom add`, `Ctrl+N`).
- Timeline with critical path; stage `estimate` (days); `pathloom schedule`.
- Export to Markdown, CSV, GitHub issues and read-only HTML (`pathloom export`); import from Markdown, CSV, Trello, Notion and Spec Kit (`pathloom import`).
- Change history with revert of one change (`pathloom history`, `pathloom revert`).
- For orchestrators: session state (`pathloom session set`, `update_session`), shared approval queue for clients (`respond_approval`, `answer_decision`, only with `allow_client_answers = true`), `approval`, `decision` and `task` events, `pathloom watch --json --stage --type`, `pathloom://open?...` deep links (`pathloom deeplink`, `make_link`).

### Changed
- A second answer to an already decided approval or decision now fails with `ALREADY_DECIDED` (details: who, when, and the state or choice) instead of overwriting the first answer. The first answer wins, in the app, CLI, MCP and Telegram.

## 0.1.0 (beta) - 2026-11-05

First public release.

### Board and roadmap
- Infinite canvas with shapes, arrows that stick to shapes, pencil, text, sticky notes, images, frames, groups, alignment, snapping, minimap; clean and hand-drawn styles.
- Stage cards with task checklists and subtasks, hard and soft dependencies, milestones, gate stages, computed statuses (ready, in progress, blocked, done).
- Cards size to their content: wider (up to 400 px) for long task titles, taller for more tasks; auto-arrange uses the real sizes, and a card that grows pushes its neighbours aside.
- Phases (frames) with progress, Focus mode (`E`), List and Overview views.
- Auto-arrange: phases as column blocks, fewer crossing arrows, cards you move by hand stay pinned. Hovering a card highlights its chain; implied arrows are hidden and arrows between phases grouped.

### Agents
- `pathloom` CLI and `pathloom mcp` server: read the plan, apply batch changes, tick tasks, comment, validate.
- The board shows what an agent is doing: a claimed task gets a half-filled circle in the agent's colour, the card says "working", and a single claimed task shows the agent's name and heartbeat note ("Claude · running tests") in the room the title leaves; several claimed tasks show only colour dots, so titles stay readable.
- Agents are nudged to claim: `next` asks them to, closing a task that was never claimed returns a hint, and `heartbeat <ID>` claims the task.
- Several agents at once: task claims with heartbeat, named assignees, agent profiles, activity log, links to client sessions, `pathloom watch --json` event stream.
- Proof of done: tasks closed with the check that ran (`--run`), optional rule that agents must attach it.
- Review mode: agent changes become proposals you accept or reject. Live reload with highlights and one-click undo.
- Spec Kit `tasks.md` import, artifacts on stages, usage reporting.
- Claude Code plugin and an Agent Skills file for other agents.

### App
- Open a task to see its full title, note, attached files and history; the side panel resizes by dragging its edge.
- Theme and language pickers with live preview, like VS Code (project menu or Ctrl+K).
- Rename a stage key from the panel; task ids, links and cards follow.
- Windows, macOS and Linux builds (Tauri 2). Only Windows is tested so far.
- Graphite, Light and Warm paper themes, custom themes, English and Ukrainian, rebindable shortcuts.
- Windows installers put the Pathloom folder on PATH (removed on uninstall), so AI clients start the MCP server as plain `pathloom mcp`.
- Installer in English and Ukrainian; updates keep running agent sessions alive; uninstall can keep or delete app data (your project files are never touched).

### Known limitations
- Installers are not code-signed: Windows SmartScreen and macOS Gatekeeper show a warning.
- macOS and Linux builds have not been tried on real machines yet.
- No automatic updates yet: download the new installer from Releases.
