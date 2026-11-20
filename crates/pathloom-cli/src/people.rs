//! `pathloom add`, `schedule`, `export html` and the MCP `parse_quick_add` / `schedule` tools (m10 contract A2, A5, A8).

use crate::agenda::local_today;
use crate::ops::{self, ApiError, ApiResult};
use pathloom_core::{ApplySpec, Project, StageSpec};
use serde_json::{Value, json};

fn to_json<T: serde::Serialize>(v: &T) -> ApiResult<Value> {
    serde_json::to_value(v).map_err(|e| ApiError::new("INTERNAL", e.to_string()))
}

/// Parses `text` against the project's stages (read-only).
pub fn parse(p: &Project, text: &str) -> ApiResult<Value> {
    let stages: Vec<(String, String)> = p.load_stages()?.into_iter().map(|s| (s.key, s.title)).collect();
    to_json(&pathloom_core::parse_quick_add(text, &stages, &local_today()))
}

/// `pathloom add`: with `yes`, adds the task to the stage. A due date goes to the stage only when it has none
/// (tasks have no due date); the assignee is shown but not stored (tasks have no assignee).
pub fn add(p: &Project, text: &str, yes: bool, author: &str) -> ApiResult<Value> {
    let mut v = parse(p, text)?;
    v["applied"] = json!(false);
    if !yes {
        return Ok(v);
    }
    let bad = |m: &str| ApiError::new("INVALID_SPEC", m.to_string());
    let title = v["title"].as_str().filter(|t| !t.is_empty()).ok_or_else(|| bad("no task title found"))?.to_string();
    let key = v["stage"].as_str().ok_or_else(|| bad("no stage recognized: say `to stage <name>`"))?.to_string();
    let stage = p.load_stage(&key)?;
    let mut st = StageSpec { id: Some(key.clone()), tasks: vec![title], ..Default::default() };
    if let Some(due) = v["due"].as_str() {
        if stage.due.is_none() {
            st.due = Some(due.into());
        } else {
            v["note"] = json!(format!("stage {key} already has a due date; the new one was not applied"));
        }
    }
    let r = ops::submit(p, &ApplySpec { stages: vec![st], ..Default::default() }, author, false, false)?;
    v["applied"] = json!(r["mode"] == "applied");
    v["result"] = r;
    Ok(v)
}

pub fn schedule(p: &Project) -> ApiResult<Value> {
    to_json(&p.schedule(&local_today())?)
}

pub fn schedule_table(v: &Value) -> String {
    let mut out = format!("{:<10} {:<10} {:<10} {:>6}  {}\n", "key", "start", "finish", "slack", "critical");
    for i in v["items"].as_array().into_iter().flatten() {
        let slack = i["slack"].as_i64().map_or("-".into(), |d| d.to_string());
        out += &format!("{:<10} {:<10} {:<10} {:>6}  {}\n", i["key"].as_str().unwrap_or(""), i["start"].as_str().unwrap_or(""), i["finish"].as_str().unwrap_or(""), slack, if i["critical"] == true { "*" } else { "" });
    }
    out + &format!("project finish: {}", v["finish"].as_str().unwrap_or(""))
}

pub fn export_html(p: &Project) -> ApiResult<String> {
    Ok(p.export_html(&pathloom_core::now_rfc3339())?)
}
