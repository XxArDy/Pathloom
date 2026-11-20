# Changelog

All notable changes to Pathloom. Versions follow [Semantic Versioning](https://semver.org/); the file and MCP formats are versioned
separately as `pathloom/1`.

## Unreleased

### Security
- History revert refuses entries whose paths point outside `plan/` (a tampered history file could overwrite any file).
- Agents cannot approve their own proposals, mark a task `accepted`, overwrite an answered decision, or revert changes in review mode. `decide_proposal` over MCP only relays a named human's answer when client answers are allowed.
- Telegram and client relays can only answer a pending approval request.
- `pathloom://` links: UNC and `\\?\` project paths are refused, and opening a project you never opened asks first.
- More dangerous link schemes and executable file types are blocked; exported HTML carries a strict Content-Security-Policy.
- Git: branch names can no longer be read as options, and untracked files no longer block the owner queue.
- Read-only MCP capabilities list only tools that a read-only server serves.

### Fixed
- File locks use OS locks: a crashed process no longer leaves a lock that blocks for minutes.
- The desktop app logs its own edits (tasks, stages, links, files, images, boards), so reverting an agent's change keeps the owner's earlier edits, and approvals made in the app count in the digest.
- Importing a Trello board with a non-ASCII due date no longer crashes.
- Telegram: long cards are shortened, a card Telegram refuses no longer blocks the others, and image previews go out only after their card.
- A repeated session state no longer rewrites the stage file.
- `.pathloom/.gitignore` of older projects gets the newer machine-state files.
- The desktop app can import Spec Kit `tasks.md`; app, CLI and MCP share one import/export path.
- `pathloom decisions --md` is in English.

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
