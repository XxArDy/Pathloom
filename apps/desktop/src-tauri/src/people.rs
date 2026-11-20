//! Commands for quick add, timeline, export/import and change history (docs/m10 А2, А5-А8).
//! Thin wrappers over `pathloom-core`; the owner's edits from the app apply directly (no proposal).

use std::path::Path;

use pathloom_core::{ApplyOptions, ApplySpec, Draft, Project, Schedule, StageView};
use serde::Serialize;
use tauri::State;

use super::{log_app, views, with_project, AppState, CmdResult};

const MAX_IMPORT_BYTES: u64 = 5 * 1024 * 1024;

#[tauri::command]
pub fn quick_add_parse(state: State<'_, AppState>, text: String, today: String) -> CmdResult<Draft> {
    with_project(&state, |p| {
        let stages: Vec<(String, String)> = p.load_stages()?.into_iter().map(|s| (s.key, s.title)).collect();
        Ok(pathloom_core::parse_quick_add(&text, &stages, &today))
    })
}

#[derive(Serialize)]
pub struct QuickAddResult {
    note: Option<String>,
    stages: Vec<StageView>,
}

/// Adds the (possibly edited) draft as a task; the stage due date is set only when the stage has none.
#[tauri::command]
pub fn quick_add_apply(state: State<'_, AppState>, title: String, stage: String, due: Option<String>) -> CmdResult<QuickAddResult> {
    with_project(&state, |p| {
        let draft = Draft { title, stage: Some(stage.clone()), due: due.filter(|d| !d.is_empty()), ..Default::default() };
        let (spec, note) = p.quick_add_spec(&draft)?;
        let r = p.apply(&spec, &ApplyOptions { dry_run: false, author: "me".into() })?;
        log_app(p, "quick_add", &format!("added a task to {stage}"), vec![stage], r.files);
        Ok(QuickAddResult { note, stages: views(p)? })
    })
}

#[tauri::command]
pub fn schedule(state: State<'_, AppState>, today: String) -> CmdResult<Schedule> {
    with_project(&state, |p| p.schedule(&today))
}

/// Writes the export to `path` (chosen in the save dialog) atomically.
#[tauri::command]
pub fn export_write(state: State<'_, AppState>, format: String, path: String) -> CmdResult<()> {
    let text = with_project(&state, |p| p.export(&format, &pathloom_core::now_rfc3339()))?;
    pathloom_core::write_file_atomic(Path::new(&path), text.as_bytes()).map_err(|e| e.to_string())?;
    with_project(&state, |p| {
        log_app(p, "export", &format!("exported the plan as {format}"), vec![], vec![]);
        Ok(())
    })
}

#[derive(Serialize)]
pub struct ImportPreview {
    stages: usize,
    tasks: usize,
    created: Vec<String>,
    updated: Vec<String>,
    warnings: Vec<String>,
}

fn import_spec(p: &Project, format: &str, path: &str) -> pathloom_core::Result<ApplySpec> {
    let meta = std::fs::metadata(path)?;
    if meta.len() > MAX_IMPORT_BYTES {
        return Err(pathloom_core::Error::InvalidSpec("the file is larger than 5 MB".into()));
    }
    let text = std::fs::read_to_string(path)?;
    Ok(p.import(format, &text, None)?.0)
}

/// Dry run: what an import would create or change.
#[tauri::command]
pub fn import_preview(state: State<'_, AppState>, format: String, path: String) -> CmdResult<ImportPreview> {
    with_project(&state, |p| {
        let spec = import_spec(p, &format, &path)?;
        let r = p.apply(&spec, &ApplyOptions { dry_run: true, author: "me".into() })?;
        Ok(ImportPreview {
            stages: spec.stages.len(),
            tasks: spec.stages.iter().map(|s| s.tasks.len()).sum(),
            created: r.created,
            updated: r.updated,
            warnings: r.warnings,
        })
    })
}

#[tauri::command]
pub fn import_apply(state: State<'_, AppState>, format: String, path: String) -> CmdResult<Vec<StageView>> {
    with_project(&state, |p| {
        let spec = import_spec(p, &format, &path)?;
        let r = p.apply(&spec, &ApplyOptions { dry_run: false, author: "me".into() })?;
        log_app(p, "import", &format!("imported a plan ({format})"), r.created.iter().chain(&r.updated).cloned().collect(), r.files);
        views(p)
    })
}

#[derive(Serialize)]
pub struct HistoryRow {
    id: String,
    at: String,
    action: String,
    files: Vec<String>,
}

#[tauri::command]
pub fn history_list(state: State<'_, AppState>, limit: usize) -> CmdResult<Vec<HistoryRow>> {
    with_project(&state, |p| {
        Ok(p.history(limit.clamp(1, 200))?
            .into_iter()
            .map(|e| HistoryRow { id: e.id, at: e.at, action: e.action, files: e.files.into_iter().map(|f| f.path).collect() })
            .collect())
    })
}

/// Undoes one change. A file edited since fails with `CONFLICT: …` naming the paths.
#[tauri::command]
pub fn history_revert(state: State<'_, AppState>, id: String) -> CmdResult<Vec<StageView>> {
    with_project(&state, |p| {
        p.revert(&id, "me", "app")?;
        views(p)
    })
    .map_err(|e| if e.starts_with("cannot revert") { format!("CONFLICT: {e}") } else { e })
}
