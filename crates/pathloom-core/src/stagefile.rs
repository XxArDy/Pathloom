//! Parse / render of `plan/stages/<KEY>.md` (spec §4.2).

use crate::error::{Error, Result, parse_err};
use crate::model::*;
use serde_json::{Map, Value};
use std::path::Path;

/// `[A-Z][A-Z0-9]{0,7}`
pub fn is_valid_key(key: &str) -> bool {
    let b = key.as_bytes();
    (1..=8).contains(&b.len())
        && b[0].is_ascii_uppercase()
        && b.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// Stage key of a task id (`M0-02` and `M0-02a` -> `M0`).
pub fn stage_key_of(task_id: &str) -> &str {
    task_id.split('-').next().unwrap_or("")
}

const KNOWN: [&str; 15] = [
    "id", "title", "kind", "who", "due", "priority", "branch", "pr", "tags", "depends", "sessions", "evidence", "artifacts",
    "check", "task_info",
];

fn opt_str(m: &Map<String, Value>, k: &str) -> Option<String> {
    m.get(k).and_then(scalar_str)
}

impl Stage {
    /// Parse a stage file. `path` is used for the file-name check and error messages.
    /// CRLF input is accepted.
    pub fn from_markdown(path: &Path, text: &str) -> Result<Stage> {
        let text = text.replace("\r\n", "\n");
        let rest = text
            .strip_prefix("---\n")
            .ok_or_else(|| parse_err(path, "missing frontmatter (file must start with ---)"))?;
        let (fm, body) = split_frontmatter(rest)
            .ok_or_else(|| parse_err(path, "frontmatter is not closed with ---"))?;
        let mut map = if fm.trim().is_empty() {
            Map::new()
        } else {
            match serde_saphyr::from_str::<Value>(fm).map_err(|e| parse_err(path, e))? {
                Value::Object(m) => m,
                _ => return Err(parse_err(path, "frontmatter must be a mapping")),
            }
        };
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let key = opt_str(&map, "id").unwrap_or_else(|| stem.to_string());
        if !is_valid_key(&key) {
            return Err(Error::InvalidKey(key));
        }
        if !stem.is_empty() && stem != key {
            return Err(parse_err(path, format!("id `{key}` does not match file name")));
        }
        let mut st = Stage::new(&key, &opt_str(&map, "title").unwrap_or_default());
        if let Some(s) = opt_str(&map, "kind") {
            st.kind = StageKind::parse(&s).ok_or_else(|| parse_err(path, format!("bad kind `{s}`")))?;
        }
        if let Some(s) = opt_str(&map, "who") {
            st.who = Who::parse(&s).map_err(|m| parse_err(path, m))?;
        }
        if let Some(s) = opt_str(&map, "priority") {
            st.priority =
                Priority::parse(&s).ok_or_else(|| parse_err(path, format!("bad priority `{s}`")))?;
        }
        st.due = opt_str(&map, "due");
        st.branch = opt_str(&map, "branch");
        st.pr = opt_str(&map, "pr");
        if let Some(Value::Array(a)) = map.get("tags") {
            st.tags = a.iter().filter_map(scalar_str).collect();
        }
        if let Some(Value::Array(a)) = map.get("artifacts") {
            st.artifacts = a.iter().filter_map(scalar_str).collect();
        }
        st.check = opt_str(&map, "check");
        if let Some(Value::Array(a)) = map.get("depends") {
            for d in a {
                let dep = match d {
                    Value::Object(o) => scalar_str_opt(o.get("id")).map(|id| Dep {
                        id,
                        soft: o.get("soft").and_then(Value::as_bool).unwrap_or(false),
                    }),
                    other => scalar_str(other).map(|id| Dep { id, soft: false }),
                };
                st.depends.push(dep.ok_or_else(|| parse_err(path, "bad depends entry"))?);
            }
        }
        // A hand-edited, invalid `sessions` list must not make the whole stage unreadable:
        // keep it untouched as an unknown key (preserved on save) and expose no sessions.
        let mut keep_raw_sessions = false;
        if let Some(Value::Array(a)) = map.get("sessions") {
            let parsed: Result<Vec<_>, String> = a
                .iter()
                .map(|v| {
                    serde_json::from_value::<crate::session::SessionRef>(v.clone())
                        .map_err(|e| e.to_string())
                        .and_then(|s| s.normalize().map_err(|e| e.to_string()))
                })
                .collect();
            match parsed {
                Ok(list) => st.sessions = list,
                Err(_) => keep_raw_sessions = true,
            }
        }
        // Same for a hand-edited, invalid `evidence` map.
        let mut keep_raw_evidence = false;
        if let Some(Value::Object(o)) = map.get("evidence") {
            let parsed: Result<std::collections::BTreeMap<String, crate::evidence::Evidence>, String> = o
                .iter()
                .map(|(k, v)| {
                    serde_json::from_value::<crate::evidence::Evidence>(v.clone())
                        .map_err(|e| e.to_string())
                        .and_then(|e| e.normalize().map_err(|e| e.to_string()))
                        .map(|e| (k.clone(), e))
                })
                .collect();
            match parsed {
                Ok(m) => st.evidence = m,
                Err(_) => keep_raw_evidence = true,
            }
        }
        // And for `task_info`.
        let mut keep_raw_info = false;
        if let Some(Value::Object(o)) = map.get("task_info") {
            let parsed: Result<std::collections::BTreeMap<String, TaskInfo>, String> = o
                .iter()
                .map(|(k, v)| {
                    let mut i = serde_json::from_value::<TaskInfo>(v.clone()).map_err(|e| e.to_string())?;
                    i.status = i.status.map(|s| s.normalize()).transpose().map_err(|e| e.to_string())?;
                    Ok((k.clone(), i))
                })
                .collect();
            match parsed {
                Ok(m) => st.task_info = m,
                Err(_) => keep_raw_info = true,
            }
        }
        map.retain(|k, _| {
            !KNOWN.contains(&k.as_str())
                || (keep_raw_sessions && k == "sessions")
                || (keep_raw_evidence && k == "evidence")
                || (keep_raw_info && k == "task_info")
        });
        st.extra = map;
        parse_body(&mut st, body);
        Ok(st)
    }

    /// Deterministic markdown rendering (LF endings).
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("---\n");
        let mut line = |k: &str, v: &str| {
            out.push_str(k);
            out.push_str(": ");
            out.push_str(v);
            out.push('\n');
        };
        line("id", &scalar(&self.key, false));
        line("title", &scalar(&self.title, false));
        line("kind", self.kind.as_str());
        line("who", self.who.as_str());
        if let Some(d) = &self.due {
            line("due", &scalar(d, true));
        }
        line("priority", self.priority.as_str());
        if let Some(b) = &self.branch {
            line("branch", &scalar(b, false));
        }
        if let Some(p) = &self.pr {
            line("pr", &scalar(p, true));
        }
        if !self.tags.is_empty() {
            let t: Vec<String> = self.tags.iter().map(|t| scalar(t, false)).collect();
            line("tags", &format!("[{}]", t.join(", ")));
        }
        if !self.depends.is_empty() {
            let d: Vec<String> = self
                .depends
                .iter()
                .map(|d| {
                    if d.soft {
                        format!("{{id: {}, soft: true}}", scalar(&d.id, false))
                    } else {
                        scalar(&d.id, false)
                    }
                })
                .collect();
            line("depends", &format!("[{}]", d.join(", ")));
        }
        if let Some(c) = &self.check {
            line("check", &scalar(c, false));
        }
        if !self.artifacts.is_empty() {
            let a: Vec<String> = self.artifacts.iter().map(|x| scalar(x, false)).collect();
            line("artifacts", &format!("[{}]", a.join(", ")));
        }
        if !self.sessions.is_empty() {
            let v = serde_json::to_value(&self.sessions).unwrap_or(Value::Null);
            line("sessions", &yaml_value(&v));
        }
        // Evidence only for tasks that exist and are done: undone or removed tasks lose it on the next write.
        let done = self.done_task_ids();
        let ev: Map<String, Value> = self
            .evidence
            .iter()
            .filter(|(k, _)| done.contains(k.as_str()) || **k == self.key || self.in_review(k))
            .map(|(k, e)| (k.clone(), serde_json::to_value(e).unwrap_or(Value::Null)))
            .collect();
        if !ev.is_empty() {
            line("evidence", &yaml_value(&Value::Object(ev)));
        }
        // Task details only for tasks that still exist.
        let ids = self.task_ids();
        let info: Map<String, Value> = self
            .task_info
            .iter()
            .filter(|(k, i)| ids.contains(k.as_str()) && !i.is_empty())
            .map(|(k, i)| (k.clone(), serde_json::to_value(i).unwrap_or(Value::Null)))
            .collect();
        if !info.is_empty() {
            line("task_info", &yaml_value(&Value::Object(info)));
        }
        for (k, v) in &self.extra {
            line(&scalar(k, false), &yaml_value(v));
        }
        out.push_str("---\n");

        let mut body = format!("{}{}", self.description, self.layout.desc_tail);
        if !body.is_empty() && !body.ends_with('\n') {
            body.push('\n');
        }
        let mut segs = self.layout.segments.clone();
        let has_tasks = segs.iter().any(|s| matches!(s, Seg::Tasks { .. }));
        let has_comments = segs.iter().any(|s| matches!(s, Seg::Comments { .. }));
        // New managed sections: Tasks goes before Comments, otherwise at the end.
        if !has_tasks && !self.tasks.is_empty() {
            let at = segs.iter().position(|s| matches!(s, Seg::Comments { .. })).unwrap_or(segs.len());
            let tail = if at < segs.len() { "\n" } else { "" };
            segs.insert(at, Seg::Tasks { tail: tail.into() });
        }
        if !has_comments && !self.comments.is_empty() {
            segs.push(Seg::Comments { tail: String::new() });
        }
        for seg in &segs {
            // Newly created sections need a blank line before them.
            let created = match seg {
                Seg::Tasks { .. } => !has_tasks,
                Seg::Comments { .. } => !has_comments,
                Seg::Text(_) => false,
            };
            if created {
                ensure_blank_line(&mut body);
            }
            match seg {
                Seg::Text(t) => body.push_str(t),
                Seg::Tasks { tail } => {
                    body.push_str("## Tasks\n");
                    for t in &self.tasks {
                        push_task(&mut body, t, false);
                        for s in &t.sub {
                            push_task(&mut body, s, true);
                        }
                    }
                    body.push_str(tail);
                }
                Seg::Comments { tail } => {
                    body.push_str("## Comments\n");
                    for c in &self.comments {
                        body.push_str(&format!("- {} {} — {}\n", c.at, c.author, one_line(&c.text)));
                    }
                    body.push_str(tail);
                }
            }
        }
        out.push_str(&body);
        out
    }
}

fn ensure_blank_line(body: &mut String) {
    if !body.is_empty() && !body.ends_with("\n\n") {
        if !body.ends_with('\n') {
            body.push('\n');
        }
        body.push('\n');
    }
}

fn one_line(s: &str) -> String {
    s.replace(['\r', '\n'], " ")
}

fn push_task(out: &mut String, t: &Task, sub: bool) {
    out.push_str(if sub { "  - [" } else { "- [" });
    out.push(if t.done { 'x' } else { ' ' });
    out.push_str("] ");
    out.push_str(&t.id);
    if !t.title.is_empty() {
        out.push(' ');
        out.push_str(&one_line(&t.title));
    }
    out.push('\n');
}

fn split_frontmatter(rest: &str) -> Option<(&str, &str)> {
    let mut pos = 0;
    for l in rest.split_inclusive('\n') {
        if l.trim_end_matches('\n') == "---" {
            return Some((&rest[..pos], &rest[pos + l.len()..]));
        }
        pos += l.len();
    }
    None
}

#[derive(Clone, Copy, PartialEq)]
enum Sec {
    Tasks,
    Comments,
}

fn managed(line: &str) -> Option<Sec> {
    match line.trim_end() {
        "## Tasks" => Some(Sec::Tasks),
        "## Comments" => Some(Sec::Comments),
        _ => None,
    }
}

struct Acc<'a> {
    text: String,
    current: Option<(Sec, Vec<&'a str>)>,
    desc_done: bool,
}

impl<'a> Acc<'a> {
    fn flush(&mut self, st: &mut Stage) {
        if let Some((sec, lines)) = self.current.take() {
            let blank = lines.iter().rev().take_while(|l| l.trim().is_empty()).count();
            let keep = lines.len() - blank;
            let tail: String = lines[keep..].concat();
            match sec {
                Sec::Tasks => {
                    st.tasks = parse_tasks(&lines[..keep]);
                    st.layout.segments.push(Seg::Tasks { tail });
                }
                Sec::Comments => {
                    st.comments = parse_comments(&lines[..keep]);
                    st.layout.segments.push(Seg::Comments { tail });
                }
            }
        }
        if self.text.is_empty() {
            return;
        }
        let t = std::mem::take(&mut self.text);
        if self.desc_done {
            st.layout.segments.push(Seg::Text(t));
        } else {
            let d = t.trim_end_matches('\n');
            st.description = d.to_string();
            st.layout.desc_tail = t[d.len()..].to_string();
        }
    }
}

fn parse_body(st: &mut Stage, body: &str) {
    let (mut seen_tasks, mut seen_comments, mut fence) = (false, false, false);
    let mut acc = Acc { text: String::new(), current: None, desc_done: false };
    for line in body.split_inclusive('\n') {
        if line.starts_with("```") || line.starts_with("~~~") {
            fence = !fence;
        }
        let sec = if fence { None } else { managed(line) };
        // A repeated managed heading is kept as ordinary text.
        let new_sec = match sec {
            Some(Sec::Tasks) if !seen_tasks => Some(Sec::Tasks),
            Some(Sec::Comments) if !seen_comments => Some(Sec::Comments),
            _ => None,
        };
        if let Some(s) = new_sec {
            acc.flush(st);
            acc.desc_done = true;
            if s == Sec::Tasks { seen_tasks = true } else { seen_comments = true }
            acc.current = Some((s, Vec::new()));
            continue;
        }
        if acc.current.is_some() {
            // Section ends at a heading or at any line that is not a task / comment of that section
            // (kept as text, so hand-written list items are never dropped on save).
            let l = line.trim_end_matches(['\n', '\r']);
            let item = l.trim().is_empty()
                || match acc.current.as_ref().map(|c| c.0) {
                    Some(Sec::Tasks) => task_line(l).is_some(),
                    _ => comment_line(l).is_some(),
                };
            let heading = !fence && (line.starts_with("# ") || line.starts_with("## "));
            if item && !heading {
                if let Some((_, lines)) = acc.current.as_mut() {
                    lines.push(line);
                }
                continue;
            }
            acc.flush(st);
        }
        acc.text.push_str(line);
    }
    acc.flush(st);
}

fn check_box(line: &str) -> Option<(bool, &str)> {
    let r = line.strip_prefix('[')?;
    let mut cs = r.chars();
    let done = match cs.next()? {
        ' ' => false,
        'x' | 'X' => true,
        _ => return None,
    };
    Some((done, cs.as_str().strip_prefix(']')?))
}

/// `- [ ] ID title` (top level) or `  - [x] ID title` (subtask): `(is_sub, done, rest after the box)`.
fn task_line(l: &str) -> Option<(bool, bool, &str)> {
    let (sub, rest) = match (l.strip_prefix("- "), l.strip_prefix("  - ")) {
        (Some(r), _) => (false, r),
        (None, Some(r)) => (true, r),
        _ => return None,
    };
    check_box(rest).map(|(done, rest)| (sub, done, rest))
}

/// `- <at> <author> — <text>`.
fn comment_line(l: &str) -> Option<Comment> {
    let l = l.strip_prefix("- ")?;
    let (at, rest) = l.split_once(' ')?;
    let (author, text) = rest.split_once(" — ")?;
    Some(Comment { at: at.into(), author: author.into(), text: text.into() })
}

fn parse_tasks(lines: &[&str]) -> Vec<Task> {
    let mut out: Vec<Task> = Vec::new();
    for raw in lines {
        let Some((sub, done, rest)) = task_line(raw.trim_end_matches(['\n', '\r'])) else { continue };
        let rest = rest.trim();
        let (id, title) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let t = Task { id: id.into(), title: title.trim().into(), done, sub: vec![] };
        match (sub, out.last_mut()) {
            (true, Some(parent)) => parent.sub.push(t),
            _ => out.push(t),
        }
    }
    out
}

fn parse_comments(lines: &[&str]) -> Vec<Comment> {
    lines.iter().filter_map(|raw| comment_line(raw.trim_end_matches(['\n', '\r']))).collect()
}

fn scalar_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn scalar_str_opt(v: Option<&Value>) -> Option<String> {
    v.and_then(scalar_str)
}

/// YAML scalar: plain when unambiguous, otherwise a JSON (= YAML double-quoted) string.
/// `loose` additionally allows digits/dates (`pr: 12`, `due: 2026-11-14`).
fn scalar(s: &str, loose: bool) -> String {
    if plain_ok(s, loose) { s.to_string() } else { serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into()) }
}

fn plain_ok(s: &str, loose: bool) -> bool {
    let Some(f) = s.chars().next() else { return false };
    if s.ends_with(' ') || s.contains("  ") {
        return false;
    }
    if loose && f.is_ascii_digit() && s.chars().all(|c| c.is_ascii_digit() || "-:TZ".contains(c)) {
        return true;
    }
    if !(f.is_alphabetic() || f == '_') {
        return false;
    }
    if !s.chars().all(|c| c.is_alphanumeric() || " _./-+".contains(c)) {
        return false;
    }
    !matches!(
        s.to_ascii_lowercase().as_str(),
        "true" | "false" | "null" | "yes" | "no" | "on" | "off" | "y" | "n" | "inf" | "nan"
    )
}

fn yaml_value(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => scalar(s, true),
        Value::Array(a) => format!("[{}]", a.iter().map(yaml_value).collect::<Vec<_>>().join(", ")),
        Value::Object(o) => format!(
            "{{{}}}",
            o.iter().map(|(k, v)| format!("{}: {}", scalar(k, false), yaml_value(v))).collect::<Vec<_>>().join(", ")
        ),
    }
}
