//! Plan templates for the CLI and MCP (core: `pathloom_core::templates`).

use crate::ops::{self, ApiResult};
use pathloom_core::{Project, list_templates, load_template, user_templates_dir as user_dir};
use serde_json::{Value, json};

pub fn list(lang: Option<&str>) -> Value {
    json!({"templates": list_templates(user_dir().as_deref(), lang.unwrap_or("en"))})
}

/// Fail early (before `init` creates files) when the template does not exist.
pub fn check(id: &str, lang: Option<&str>) -> ApiResult<()> {
    load_template(id, lang.unwrap_or("en"), user_dir().as_deref())?;
    Ok(())
}

/// Apply a template as author `template`. Review mode stores a proposal unless `direct`; otherwise the board is laid out.
pub fn apply(p: &Project, id: &str, lang: Option<&str>, direct: bool) -> ApiResult<Value> {
    let spec = load_template(id, lang.unwrap_or("en"), user_dir().as_deref())?;
    let spec = p.instantiate_template(spec)?;
    let v = ops::submit(p, &spec, "template", false, direct)?;
    if v["mode"] == "applied" {
        p.layout_board(spec.board.as_deref().unwrap_or("roadmap"), &[])?;
    }
    Ok(v)
}

pub fn save(p: &Project, name: &str, title: Option<&str>) -> ApiResult<Value> {
    let dir = user_dir().ok_or_else(|| ops::ApiError::new("IO_ERROR", "cannot find the home folder (set PATHLOOM_TEMPLATES_DIR)"))?;
    let path = p.save_as_template(name, title.unwrap_or(name), &dir)?;
    Ok(json!({"template": name, "path": path.to_string_lossy()}))
}
