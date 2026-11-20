//! `pathloom add`, `schedule`, `export html` and the MCP `parse_quick_add` / `schedule` tools (m10 contract A2, A5, A8).

use crate::agenda::local_today;
use crate::ops::{self, ApiError, ApiResult};
use pathloom_core::Project;
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
    let stages: Vec<(String, String)> = p.load_stages()?.into_iter().map(|s| (s.key, s.title)).collect();
    let draft = pathloom_core::parse_quick_add(text, &stages, &local_today());
    let (spec, note) = p.quick_add_spec(&draft)?;
    if let Some(n) = note {
        v["note"] = json!(n);
    }
    let r = ops::submit(p, &spec, author, false, false)?;
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
