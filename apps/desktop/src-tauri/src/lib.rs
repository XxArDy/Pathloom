//! Tauri shell: thin commands over `pathloom-core`. All file access goes through the core;
//! the WebView has no filesystem permissions.

mod gitmode;
mod people;
mod recent;
mod watch;

use std::path::Path;
use std::sync::{LazyLock, Mutex};

use pathloom_core::{AgentChanges, AgentProfile, Board, Claim, LogEntry, Project, Proposal, StageMeta, StageView, Task};
use tauri_plugin_opener::OpenerExt;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

#[derive(Default)]
struct AppState {
    project: Mutex<Option<Project>>,
    watcher: Mutex<Option<watch::ProjectWatcher>>,
}

/// Last known plan file contents, for "undo agent changes". One project is open at a time.
static SNAPSHOT: LazyLock<Mutex<watch::Snapshot>> = LazyLock::new(Default::default);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectSnapshot {
    root: String,
    name: String,
    boards: Vec<String>,
    stages: Vec<StageView>,
}

type CmdResult<T> = Result<T, String>;

fn with_project<T>(
    state: &State<'_, AppState>,
    f: impl FnOnce(&Project) -> pathloom_core::Result<T>,
) -> CmdResult<T> {
    let guard = state.project.lock().map_err(|_| "internal state poisoned".to_string())?;
    let project = guard.as_ref().ok_or_else(|| "no project is open".to_string())?;
    f(project).map_err(|e| e.to_string())
}

/// Stage views from the git ref when git mode is active (claims stay local), else from the working copy.
fn views(p: &Project) -> pathloom_core::Result<Vec<StageView>> {
    let src = gitmode::source(p);
    if !src.from_git {
        return p.stage_views_live();
    }
    let mut views = src.stage_views_live()?;
    let local = p.stage_views_live().unwrap_or_default();
    for v in &mut views {
        if let Some(l) = local.iter().find(|l| l.stage.key == v.stage.key) {
            v.claims = l.claims.clone();
        }
    }
    Ok(views)
}

fn snapshot(p: &Project) -> pathloom_core::Result<ProjectSnapshot> {
    Ok(ProjectSnapshot {
        root: p.root.display().to_string(),
        name: p.config.name.clone(),
        boards: gitmode::source(p).list_boards()?,
        stages: views(p)?,
    })
}

fn activate(app: &AppHandle, state: &State<'_, AppState>, project: Project) -> CmdResult<ProjectSnapshot> {
    let snap = snapshot(&project).map_err(|e| e.to_string())?;
    // Recent list is a convenience; failing to write it must not block opening the project.
    let _ = recent::remember(app, &snap.root, &snap.name);
    let plan_dir = project.plan_dir();
    if let Ok(mut s) = SNAPSHOT.lock() {
        *s = watch::Snapshot::capture(&plan_dir);
    }
    // Live reload is best effort: the app still works without it.
    let watcher = watch::start(app.clone(), project.root.clone(), plan_dir, &SNAPSHOT).ok();
    *state.watcher.lock().map_err(|_| "internal state poisoned".to_string())? = watcher;
    gitmode::restart(app, &project);
    *state.project.lock().map_err(|_| "internal state poisoned".to_string())? = Some(project);
    Ok(snap)
}

#[tauri::command]
fn project_create(app: AppHandle, state: State<'_, AppState>, path: String, name: String) -> CmdResult<ProjectSnapshot> {
    let project = Project::init(Path::new(&path), &name).map_err(|e| e.to_string())?;
    activate(&app, &state, project)
}

#[tauri::command]
fn project_open(app: AppHandle, state: State<'_, AppState>, path: String) -> CmdResult<ProjectSnapshot> {
    let project = Project::open(Path::new(&path)).map_err(|e| e.to_string())?;
    activate(&app, &state, project)
}

/// Creates (or reopens) a sample project in the app data folder, in the UI language.
#[tauri::command]
fn demo_open(app: AppHandle, state: State<'_, AppState>, lang: String) -> CmdResult<ProjectSnapshot> {
    let uk = lang.starts_with("uk");
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join(if uk { "demo-uk" } else { "demo-en" });
    let project = match Project::open(&dir) {
        Ok(p) => p,
        Err(_) => {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let p = Project::init(&dir, if uk { "Демо: мій перший застосунок" } else { "Demo: my first app" }).map_err(|e| e.to_string())?;
            let text = if uk { include_str!("../demo/demo.uk.yaml") } else { include_str!("../demo/demo.en.yaml") };
            let spec = Project::parse_spec(text).map_err(|e| e.to_string())?;
            let opts = pathloom_core::ApplyOptions { dry_run: false, author: "agent:claude".into() };
            p.apply(&spec, &opts).map_err(|e| e.to_string())?;
            p
        }
    };
    activate(&app, &state, project)
}

/// Recent projects whose folder still holds a Pathloom project (moved or deleted ones are skipped).
#[tauri::command]
fn recent_projects(app: AppHandle) -> Vec<recent::RecentProject> {
    recent::load(&app).into_iter().filter(|r| Path::new(&r.path).join(".pathloom/project.toml").is_file()).collect()
}

/// Remove a project from the recent list only; its files stay on disk.
#[tauri::command]
fn recent_forget(app: AppHandle, path: String) -> CmdResult<()> {
    recent::forget(&app, &path).map_err(|e| e.to_string())
}

/// Turn auto-layout of a board on/off; turning it on arranges the board right away.
#[tauri::command]
fn board_auto_layout(state: State<'_, AppState>, name: String, on: bool) -> CmdResult<Board> {
    with_project(&state, |p| {
        let b = p.set_board_auto_layout(&name, on)?;
        if on {
            log_app(p, "layout_board", &format!("auto-layout on for {name}"), vec![], vec![]);
        }
        Ok(b)
    })
}

/// Arrange the whole board now (pinned cards stay).
#[tauri::command]
fn board_arrange(state: State<'_, AppState>, name: String) -> CmdResult<Board> {
    with_project(&state, |p| {
        let b = p.layout_board(&name, &[])?;
        log_app(p, "layout_board", &format!("arranged {name}"), vec![], vec![]);
        Ok(b)
    })
}

#[tauri::command]
fn board_load(state: State<'_, AppState>, name: String) -> CmdResult<Board> {
    with_project(&state, |p| gitmode::source(p).load_board(&name))
}

#[tauri::command]
fn board_save(state: State<'_, AppState>, name: String, board: Board) -> CmdResult<()> {
    with_project(&state, |p| p.save_board(&name, &board))
}

#[tauri::command]
fn board_create(state: State<'_, AppState>, name: String, title: String) -> CmdResult<Board> {
    with_project(&state, |p| p.create_board(&name, &title))
}

#[tauri::command]
fn boards_list(state: State<'_, AppState>) -> CmdResult<Vec<String>> {
    with_project(&state, |p| gitmode::source(p).list_boards())
}

#[tauri::command]
fn stages_list(state: State<'_, AppState>) -> CmdResult<Vec<StageView>> {
    with_project(&state, views)
}

#[tauri::command]
fn stage_create(state: State<'_, AppState>, title: String) -> CmdResult<StageView> {
    with_project(&state, |p| {
        let stage = p.create_stage(&title, None)?;
        let all = views(p)?;
        Ok(all.into_iter().find(|v| v.stage.key == stage.key))
    })?
    .ok_or_else(|| "created stage not found".to_string())
}

fn find_done(tasks: &[Task], id: &str) -> Option<bool> {
    tasks.iter().find_map(|t| {
        if t.id == id {
            Some(if t.sub.is_empty() { t.done } else { t.sub.iter().all(|s| s.done) })
        } else {
            find_done(&t.sub, id)
        }
    })
}

#[tauri::command]
fn task_toggle(state: State<'_, AppState>, task_id: String) -> CmdResult<Vec<StageView>> {
    with_project(&state, |p| {
        let done = p
            .load_stages()?
            .iter()
            .find_map(|s| find_done(&s.tasks, &task_id))
            .ok_or_else(|| pathloom_core::Error::NotFound(task_id.clone()))?;
        p.set_task_done(&task_id, !done)?;
        views(p)
    })
}

#[tauri::command]
fn task_add(state: State<'_, AppState>, key: String, title: String) -> CmdResult<Vec<StageView>> {
    with_project(&state, |p| {
        p.add_task(&key, &title)?;
        views(p)
    })
}

#[tauri::command]
fn stage_rename(state: State<'_, AppState>, key: String, title: String) -> CmdResult<Vec<StageView>> {
    with_project(&state, |p| {
        p.set_title(&key, &title)?;
        views(p)
    })
}

#[tauri::command]
fn stage_update(state: State<'_, AppState>, key: String, meta: StageMeta) -> CmdResult<Vec<StageView>> {
    with_project(&state, |p| {
        p.update_meta(&key, meta)?;
        views(p)
    })
}

#[tauri::command]
fn stage_source(state: State<'_, AppState>, key: String) -> CmdResult<String> {
    with_project(&state, |p| gitmode::source(p).stage_source(&key))
}

#[tauri::command]
fn stage_delete(state: State<'_, AppState>, key: String) -> CmdResult<Vec<StageView>> {
    with_project(&state, |p| {
        p.delete_stage(&key)?;
        views(p)
    })
}

#[tauri::command]
fn task_rename(state: State<'_, AppState>, task_id: String, title: String) -> CmdResult<Vec<StageView>> {
    with_project(&state, |p| {
        p.rename_task(&task_id, &title)?;
        views(p)
    })
}

#[tauri::command]
fn task_remove(state: State<'_, AppState>, task_id: String) -> CmdResult<Vec<StageView>> {
    with_project(&state, |p| {
        p.remove_task(&task_id)?;
        views(p)
    })
}

#[tauri::command]
fn subtask_add(state: State<'_, AppState>, task_id: String, title: String) -> CmdResult<Vec<StageView>> {
    with_project(&state, |p| {
        p.add_subtask(&task_id, &title)?;
        views(p)
    })
}

/// Reverts one group of external (agent) changes reported by `plan-changed`.
#[tauri::command]
fn agent_undo(state: State<'_, AppState>, group: u64) -> CmdResult<Vec<StageView>> {
    let (touched, conflicts) =
        SNAPSHOT.lock().map_err(|_| "internal state poisoned".to_string())?.revert(group).map_err(|e| e.to_string())?;
    if touched.is_empty() && !conflicts.is_empty() {
        return Err("not undone: the files changed again after the agent's change".into());
    }
    with_project(&state, |p| {
        let files = touched.iter().filter_map(|f| f.strip_prefix(&p.root).ok()).map(|f| f.to_string_lossy().replace('\\', "/")).collect();
        log_app(p, "undo", "undid external changes", vec![], files);
        views(p)
    })
}

/// Records an action done in the app in the activity log. Logging never fails the action.
fn log_app(p: &Project, action: &str, summary: &str, stages: Vec<String>, files: Vec<String>) {
    let entry = LogEntry {
        at: pathloom_core::now_rfc3339(),
        author: "me".into(),
        source: "app".into(),
        action: action.into(),
        summary: summary.into(),
        stages,
        tasks: vec![],
        files,
        ..Default::default()
    };
    let _ = p.log_action(&entry);
}

#[tauri::command]
fn activity(state: State<'_, AppState>, limit: usize, stage: Option<String>) -> CmdResult<Vec<LogEntry>> {
    with_project(&state, |p| p.read_log(limit.min(500), stage.as_deref()))
}

#[tauri::command]
fn agents_list(state: State<'_, AppState>) -> CmdResult<Vec<AgentProfile>> {
    with_project(&state, |p| p.agents())
}

/// Opens an external session link (http/https or an app's own scheme) after the UI asked the user.
/// Never opens javascript:, data:, file: or anything without a scheme.
#[tauri::command]
fn open_url(app: AppHandle, url: String) -> CmdResult<()> {
    pathloom_core::url::check_session(&url).map_err(|e| e.to_string())?;
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

/// Open a project-relative artifact file with the system's default app (paths outside the project fail).
#[tauri::command]
fn artifact_open(app: AppHandle, state: State<'_, AppState>, path: String) -> CmdResult<()> {
    let full = with_project(&state, |p| p.artifact_path(&path))?;
    if pathloom_core::url::is_executable(&full) {
        return Err(format!("{path}: programs and scripts are not opened from a plan; open it from the file manager"));
    }
    app.opener().open_path(full.display().to_string(), None::<&str>).map_err(|e| e.to_string())
}

/// Open the bundled third-party license notices (or the repo copy in development).
#[tauri::command]
fn open_licenses(app: AppHandle) -> CmdResult<()> {
    let bundled = app.path().resource_dir().ok().map(|d| d.join("licenses").join("THIRD-PARTY-LICENSES.md"));
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../THIRD-PARTY-LICENSES.md");
    let file = bundled.filter(|p| p.is_file()).unwrap_or(dev);
    app.opener().open_path(file.display().to_string(), None::<&str>).map_err(|e| e.to_string())
}

/// Change a stage key everywhere (file, task ids, dependencies, cards, claims).
#[tauri::command]
fn stage_rename_key(state: State<'_, AppState>, key: String, new_key: String) -> CmdResult<Vec<StageView>> {
    with_project(&state, |p| {
        let s = p.rename_stage_key(&key, &new_key)?;
        log_app(p, "rename_key", &format!("{key} renamed to {}", s.key), vec![s.key.clone()], vec![]);
        views(p)
    })
}

#[tauri::command]
fn task_note_set(state: State<'_, AppState>, task_id: String, note: String) -> CmdResult<Vec<StageView>> {
    with_project(&state, |p| {
        p.set_task_note(&task_id, &note)?;
        views(p)
    })
}

/// Attach a file to a task. `path` is a link, a project-relative path, or an absolute path inside the project
/// (from the file picker); files outside the project are refused.
#[tauri::command]
fn task_file_add(state: State<'_, AppState>, task_id: String, path: String) -> CmdResult<Vec<StageView>> {
    with_project(&state, |p| {
        let abs = Path::new(&path);
        let rel = if abs.is_absolute() {
            let root = p.root.canonicalize()?;
            let full = abs.canonicalize()?;
            full.strip_prefix(&root)
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .map_err(|_| pathloom_core::Error::InvalidSpec("the file must be inside the project folder".into()))?
        } else {
            path.clone()
        };
        p.add_task_file(&task_id, &rel)?;
        views(p)
    })
}

#[tauri::command]
fn task_file_remove(state: State<'_, AppState>, task_id: String, path: String) -> CmdResult<Vec<StageView>> {
    with_project(&state, |p| {
        p.remove_task_file(&task_id, &path)?;
        views(p)
    })
}

#[tauri::command]
fn git_head(state: State<'_, AppState>) -> CmdResult<Option<String>> {
    with_project(&state, |p| Ok(pathloom_core::commits::head(&p.root)))
}

/// Recent commits that mention the stage (by key or one of its tasks).
#[tauri::command]
async fn stage_commits(state: State<'_, AppState>, key: String) -> CmdResult<Vec<serde_json::Value>> {
    let (root, keys) = with_project(&state, |p| {
        let keys: std::collections::HashSet<String> = views(p)?.into_iter().map(|s| s.stage.key).collect();
        Ok((p.root.clone(), keys))
    })?;
    tauri::async_runtime::spawn_blocking(move || {
        pathloom_core::commits::scan(&root, 500, &keys)
            .into_iter()
            .filter(|c| c.stages.contains(&key))
            .map(|c| serde_json::json!({ "sha": c.sha, "subject": c.subject, "author": c.author, "at": c.at, "tasks": c.tasks }))
            .collect()
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn usage_summary(state: State<'_, AppState>) -> CmdResult<pathloom_core::UsageSummary> {
    with_project(&state, |p| p.usage_summary())
}

/// Run a shell command in `root` off the UI thread; returns the evidence (exit code, duration) or the error.
async fn run_shell(root: std::path::PathBuf, cmd: String) -> CmdResult<Result<pathloom_core::Evidence, String>> {
    tauri::async_runtime::spawn_blocking(move || pathloom_core::run_check(&cmd, Some(&root), true).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())
}

/// Run a gate's `check` in the project folder (after the user confirmed it in the UI) and record the result.
#[tauri::command]
async fn gate_run(state: State<'_, AppState>, key: String) -> CmdResult<Vec<StageView>> {
    let p = with_project(&state, |p| Ok(p.clone()))?;
    let check = p.load_stage(&key).map_err(|e| e.to_string())?.check.ok_or("this gate has no check command")?;
    let (root, cmd) = (p.root.clone(), check.clone());
    let mut ev = run_shell(root, cmd).await??;
    let code = ev.exit_code;
    ev.by = "me".into();
    let (_, passed) = p.set_gate_result(&key, ev).map_err(|e| e.to_string())?;
    let summary = if passed { format!("gate {key} passed: {check}") } else { format!("gate {key} failed ({code}): {check}") };
    log_app(&p, "set_gate", &summary, vec![key], vec![]);
    if !passed {
        return Err(format!("`{check}` exited with {code}: the gate stays open"));
    }
    views(&p).map_err(|e| e.to_string())
}

#[tauri::command]
fn claims_list(state: State<'_, AppState>) -> CmdResult<Vec<Claim>> {
    with_project(&state, |p| p.claims())
}

/// The owner frees a task an agent has claimed (always forced: the human decides).
#[tauri::command]
fn claim_release(state: State<'_, AppState>, task_id: String) -> CmdResult<Vec<StageView>> {
    with_project(&state, |p| {
        p.release_task(&task_id, "me", true)?;
        let key = pathloom_core::stage_key_of(&task_id).to_string();
        log_app(p, "release_task", &format!("released {task_id}"), vec![key], vec![]);
        views(p)
    })
}

/// Max size of one pasted/dropped image.
const MAX_ASSET_BYTES: usize = 25 * 1024 * 1024;

/// Stores an image in plan/assets/<hash>.<ext> and returns "assets/<file>" (path relative to plan/).
#[tauri::command]
fn asset_save(state: State<'_, AppState>, name: String, bytes: Vec<u8>) -> CmdResult<String> {
    if bytes.len() > MAX_ASSET_BYTES {
        return Err("image is larger than 25 MB".into());
    }
    let ext = Path::new(&name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp"))
        .unwrap_or_else(|| "png".into());
    let hash = pathloom_core::hash_bytes(&bytes);
    let file = format!("{}.{ext}", &hash[..16]);
    with_project(&state, |p| {
        let dir = p.plan_dir().join("assets");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(&file);
        if !path.exists() {
            pathloom_core::write_file_atomic(&path, &bytes)?;
        }
        Ok(format!("assets/{file}"))
    })
}

/// Serves plan/assets/* of the open project for <img> (custom `plasset` scheme). Nothing outside it.
fn serve_asset(app: &AppHandle, uri_path: &str) -> tauri::http::Response<Vec<u8>> {
    let not_found = || tauri::http::Response::builder().status(404).body(Vec::new()).unwrap_or_default();
    let state = app.state::<AppState>();
    let Ok(guard) = state.project.lock() else { return not_found() };
    let Some(project) = guard.as_ref() else { return not_found() };
    let rel: String = uri_path.trim_start_matches('/').split('/').map(percent_decode).collect::<Vec<_>>().join("/");
    let full = if let Some(rel) = rel.strip_prefix("files/") {
        // Task attachments (images only): project-relative, checked to stay inside the project.
        let is_image = matches!(
            Path::new(rel).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(),
            Some("png" | "jpg" | "jpeg" | "gif" | "webp")
        );
        match project.artifact_path(rel) {
            Ok(f) if is_image => f,
            _ => return not_found(),
        }
    } else {
        let assets = project.plan_dir().join("assets");
        let Some(rel) = rel.strip_prefix("assets/") else { return not_found() };
        let (Ok(base), Ok(full)) = (assets.canonicalize(), assets.join(rel).canonicalize()) else { return not_found() };
        if !full.starts_with(&base) {
            return not_found();
        }
        full
    };
    let Ok(body) = std::fs::read(&full) else { return not_found() };
    let mime = match full.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    };
    tauri::http::Response::builder()
        .header("Content-Type", mime)
        .header("Cache-Control", if uri_path.trim_start_matches('/').starts_with("files/") { "no-cache" } else { "max-age=31536000, immutable" })
        .body(body)
        .unwrap_or_default()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len()
            && let Some(v) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Custom themes: every *.json in <project>/.pathloom/themes and <app config>/themes (raw JSON, validated in the UI).
#[tauri::command]
fn themes_list(app: AppHandle, state: State<'_, AppState>) -> Vec<serde_json::Value> {
    let mut dirs = Vec::new();
    if let Ok(guard) = state.project.lock()
        && let Some(p) = guard.as_ref() {
            dirs.push(p.root.join(".pathloom").join("themes"));
        }
    if let Ok(dir) = app.path().app_config_dir() {
        dirs.push(dir.join("themes"));
    }
    let mut out = Vec::new();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            if path.extension().is_some_and(|x| x == "json") {
                // A theme file is small; ignore anything large or unreadable.
                let ok_size = e.metadata().map(|m| m.len() < 64 * 1024).unwrap_or(false);
                if let (true, Ok(text)) = (ok_size, std::fs::read_to_string(&path))
                    && let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                        out.push(v);
                    }
            }
        }
    }
    out
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentSetup {
    /// Absolute path of the bundled `pathloom` CLI (next to the app executable), if present.
    cli_path: Option<String>,
    /// "auto" or "review" for the open project.
    agent_changes: String,
    /// Agents must attach passing evidence to close tasks.
    require_evidence: bool,
    /// `[git] read_ref` ("" = off) and its fetch interval.
    read_ref: String,
    fetch_interval_secs: u64,
}

#[tauri::command]
fn agent_setup(state: State<'_, AppState>) -> AgentSetup {
    let exe = if cfg!(windows) { "pathloom.exe" } else { "pathloom" };
    let cli_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(exe)))
        .filter(|p| p.is_file())
        .map(|p| p.display().to_string());
    let agent_changes = state
        .project
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|p| p.config.agent_changes))
        .map_or("auto", |m| if m == AgentChanges::Review { "review" } else { "auto" })
        .to_string();
    let require_evidence =
        state.project.lock().ok().and_then(|g| g.as_ref().map(|p| p.config.require_evidence)).unwrap_or(false);
    let git = state.project.lock().ok().and_then(|g| g.as_ref().and_then(|p| p.config.git.clone()));
    AgentSetup {
        cli_path,
        agent_changes,
        require_evidence,
        read_ref: git.as_ref().and_then(|g| g.read_ref.clone()).unwrap_or_default(),
        fetch_interval_secs: git.map_or(120, |g| g.fetch_interval_secs),
    }
}

#[tauri::command]
fn set_agent_changes(state: State<'_, AppState>, review: bool) -> CmdResult<()> {
    let mut guard = state.project.lock().map_err(|_| "internal state poisoned".to_string())?;
    let project = guard.as_mut().ok_or_else(|| "no project is open".to_string())?;
    project
        .set_agent_changes(if review { AgentChanges::Review } else { AgentChanges::Auto })
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_require_evidence(state: State<'_, AppState>, on: bool) -> CmdResult<()> {
    let mut guard = state.project.lock().map_err(|_| "internal state poisoned".to_string())?;
    let project = guard.as_mut().ok_or_else(|| "no project is open".to_string())?;
    project.set_require_evidence(on).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_git_config(app: AppHandle, state: State<'_, AppState>, read_ref: String, interval: u64) -> CmdResult<()> {
    let mut guard = state.project.lock().map_err(|_| "internal state poisoned".to_string())?;
    let project = guard.as_mut().ok_or_else(|| "no project is open".to_string())?;
    project.set_git_config(&read_ref, interval).map_err(|e| e.to_string())?;
    gitmode::restart(&app, project);
    gitmode::emit_all(&app, project);
    gitmode::emit_status(&app, project);
    Ok(())
}

#[tauri::command]
fn git_status(state: State<'_, AppState>) -> CmdResult<gitmode::GitStatus> {
    with_project(&state, |p| Ok(gitmode::status(p)))
}

/// Runs an owner action: in git mode commits it to the local checkout, or queues it while the tree is dirty.
/// Returns `None` when it was queued, else the proposal the action decided (if it was one).
fn owner_action(app: &AppHandle, p: &Project, action: &str, task_id: &str, args: serde_json::Value) -> pathloom_core::Result<Option<Option<Proposal>>> {
    let item = gitmode::QueueItem { action: action.into(), task_id: task_id.into(), args, at: pathloom_core::now_rfc3339() };
    let git = gitmode::read_ref(p);
    let clean = git.is_some() && pathloom_core::gitsrc::is_clean(&p.root).unwrap_or(false);
    if gitmode::route(git.is_some(), clean) == gitmode::Route::Direct {
        if let Some(r) = &git {
            let _ = pathloom_core::gitsrc::ff_to(&p.root, r);
        }
        match gitmode::perform(p, &item) {
            Ok((files, prop)) => {
                if git.is_some() {
                    gitmode::commit_owner(&p.root, &item, &files);
                }
                return Ok(Some(prop));
            }
            // The task is not in the local copy yet (the board shows a newer ref): queue it.
            Err(pathloom_core::Error::NotFound(_)) if git.is_some() => {}
            Err(e) => return Err(e),
        }
    }
    gitmode::append_queue(&p.root, item)?;
    gitmode::emit_status(app, p);
    Ok(None)
}

#[tauri::command]
fn proposals_list(state: State<'_, AppState>) -> CmdResult<Vec<Proposal>> {
    with_project(&state, |p| p.list_proposals())
}

#[tauri::command]
fn proposal_decide(app: AppHandle, state: State<'_, AppState>, id: String, approve: bool, comment: Option<String>) -> CmdResult<Proposal> {
    with_project(&state, |p| {
        let args = serde_json::json!({ "approve": approve, "comment": comment });
        let Some(Some(d)) = owner_action(&app, p, "proposal", &id, args)? else {
            // Queued: the proposal stays pending until the tree is clean.
            return p.list_proposals()?.into_iter().find(|x| x.id == id).ok_or_else(|| pathloom_core::Error::NotFound(id.clone()));
        };
        let st = d.decision.as_ref().map(|x| x.state.as_str()).unwrap_or("decided");
        let (stages, files) = if st == "approved" {
            (d.preview.created.iter().chain(&d.preview.updated).cloned().collect(), d.preview.files.clone())
        } else {
            (vec![], vec![])
        };
        log_app(p, &format!("proposal_{st}"), &format!("{st} proposal {id}"), stages, files);
        Ok(d)
    })
}

/// The owner approves or rejects a task in review (reasons are stored ids, e.g. `off-theme`).
#[tauri::command]
fn approvals_decide(app: AppHandle, state: State<'_, AppState>, task_id: String, approve: bool, reasons: Vec<String>, comment: Option<String>) -> CmdResult<Vec<StageView>> {
    with_project(&state, |p| {
        let action = if approve { "approve" } else { "reject" };
        if owner_action(&app, p, action, &task_id, serde_json::json!({ "reasons": reasons, "comment": comment }))?.is_none() {
            return views(p);
        }
        let key = pathloom_core::stage_key_of(&task_id).to_string();
        let what = if approve { "approved" } else { "rejected" };
        log_app(p, "decide_approval", &format!("{what} {task_id}"), vec![key], vec![]);
        views(p)
    })
}

/// The owner answers a decision with an option label or free text.
#[tauri::command]
fn decision_answer(app: AppHandle, state: State<'_, AppState>, task_id: String, choice: String) -> CmdResult<Vec<StageView>> {
    with_project(&state, |p| {
        if owner_action(&app, p, "decide", &task_id, serde_json::json!({ "choice": choice }))?.is_none() {
            return views(p);
        }
        let key = pathloom_core::stage_key_of(&task_id).to_string();
        log_app(p, "answer_decision", &format!("answered {task_id}"), vec![key], vec![]);
        views(p)
    })
}

#[tauri::command]
fn inbox_list(state: State<'_, AppState>) -> CmdResult<Vec<pathloom_core::InboxItem>> {
    with_project(&state, |p| gitmode::source(p).inbox())
}

/// Re-run the command of a task's evidence (after the user confirmed it); returns the exit code, writes nothing.
#[tauri::command]
async fn evidence_recheck(state: State<'_, AppState>, task_id: String) -> CmdResult<i32> {
    let p = with_project(&state, |p| Ok(p.clone()))?;
    let key = pathloom_core::stage_key_of(&task_id).to_string();
    let cmd = p.load_stage(&key).map_err(|e| e.to_string())?.evidence.get(&task_id).map(|e| e.command.clone()).ok_or("this task has no evidence command")?;
    Ok(run_shell(p.root.clone(), cmd).await??.exit_code)
}

#[tauri::command]
fn stage_link(state: State<'_, AppState>, from: String, to: String) -> CmdResult<Vec<StageView>> {
    with_project(&state, |p| {
        p.link(&from, &to, false)?;
        views(p)
    })
}

#[tauri::command]
fn stage_unlink(state: State<'_, AppState>, from: String, to: String) -> CmdResult<Vec<StageView>> {
    with_project(&state, |p| {
        p.unlink(&from, &to)?;
        views(p)
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[tauri::command]
fn agenda(state: State<'_, AppState>, today: String, days: u32) -> CmdResult<pathloom_core::Agenda> {
    with_project(&state, |p| p.agenda(&today, days))
}

#[tauri::command]
fn digest(state: State<'_, AppState>, since: String, today: String) -> CmdResult<pathloom_core::Digest> {
    with_project(&state, |p| p.digest(&since, &today))
}

/// `PATHLOOM_TEMPLATES_DIR`, else `~/.pathloom/templates` (same rule as the CLI).
fn templates_dir() -> Option<std::path::PathBuf> {
    if let Some(d) = std::env::var_os("PATHLOOM_TEMPLATES_DIR") {
        return Some(d.into());
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(std::path::PathBuf::from(home).join(".pathloom").join("templates"))
}

#[tauri::command]
fn templates_list(lang: String) -> Vec<pathloom_core::TemplateInfo> {
    pathloom_core::list_templates(templates_dir().as_deref(), &lang)
}

/// New project in an (empty) folder, filled from a template.
#[tauri::command]
fn template_new(app: AppHandle, state: State<'_, AppState>, path: String, name: String, id: String, lang: String) -> CmdResult<ProjectSnapshot> {
    let spec = pathloom_core::load_template(&id, &lang, templates_dir().as_deref()).map_err(|e| e.to_string())?;
    let project = Project::init(Path::new(&path), &name).map_err(|e| e.to_string())?;
    project.apply_template(spec).map_err(|e| e.to_string())?;
    activate(&app, &state, project)
}

/// Add a template's stages to the open project.
#[tauri::command]
fn template_apply(state: State<'_, AppState>, id: String, lang: String) -> CmdResult<Vec<StageView>> {
    with_project(&state, |p| {
        let spec = pathloom_core::load_template(&id, &lang, templates_dir().as_deref())?;
        p.apply_template(spec)?;
        log_app(p, "template", &format!("applied template {id}"), vec![], vec![]);
        views(p)
    })
}

#[tauri::command]
fn template_save(state: State<'_, AppState>, name: String, title: String) -> CmdResult<String> {
    let dir = templates_dir().ok_or("cannot find the home folder")?;
    with_project(&state, |p| Ok(p.save_as_template(&name, &title, &dir)?.display().to_string()))
}

pub fn run() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .register_uri_scheme_protocol("plasset", |ctx, request| serve_asset(ctx.app_handle(), request.uri().path()))
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            project_create,
            project_open,
            recent_projects,
            demo_open,
            board_load,
            board_save,
            board_create,
            boards_list,
            stages_list,
            stage_create,
            task_toggle,
            task_add,
            stage_rename,
            agent_undo,
            activity,
            claims_list,
            agents_list,
            open_url,
            claim_release,
            asset_save,
            themes_list,
            agent_setup,
            set_agent_changes,
            set_require_evidence,
            set_git_config,
            git_status,
            recent_forget,
            stage_rename_key,
            task_note_set,
            task_file_add,
            task_file_remove,
            open_licenses,
            board_auto_layout,
            board_arrange,
            artifact_open,
            usage_summary,
            git_head,
            stage_commits,
            gate_run,
            proposals_list,
            proposal_decide,
            stage_update,
            stage_source,
            stage_delete,
            task_rename,
            task_remove,
            subtask_add,
            stage_link,
            stage_unlink,
            approvals_decide,
            decision_answer,
            inbox_list,
            evidence_recheck,
            agenda,
            digest,
            templates_list,
            template_new,
            template_apply,
            template_save,
            people::quick_add_parse, people::quick_add_apply, people::schedule, people::export_write, people::import_preview, people::import_apply, people::history_list, people::history_revert,
        ])
        .run(tauri::generate_context!());
    if let Err(err) = result {
        eprintln!("Pathloom failed to start: {err}");
        std::process::exit(1);
    }
}
