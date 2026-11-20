//! Export the plan (Markdown, CSV, GitHub-issues JSON) and import other tools' lists (Markdown, CSV, Notion CSV,
//! Trello JSON) as an [`ApplySpec`], so `apply`'s dry-run / propose / review rules work unchanged.
//!
//! Imported stages get keys from [`generate_key`] that never collide with existing stages. A stage with the same
//! title as an existing one is reused (re-import appends only new tasks). Exported ids (`stage_id`, `KEY: Title`
//! headings) are kept when free. The apply spec has no subtasks: nested items and Trello checklist items become
//! plain tasks of the same stage. Dependencies that name no stage of the file or the project are dropped.

use crate::apply::{ApplySpec, DepSpec, StageSpec};
use crate::error::{Error, Result};
use crate::keygen::generate_key;
use crate::model::{Priority, Stage, Task, Who};
use crate::project::{Project, push_task};
use crate::stagefile::is_valid_key;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

fn bad(m: impl Into<String>) -> Error {
    Error::InvalidSpec(m.into())
}

// ---------- export ----------

const CSV_HEADER: [&str; 9] = ["stage_id", "stage_title", "task_id", "task_title", "done", "who", "priority", "due", "depends"];

fn csv_cell(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

fn md_line(t: &Task, depth: usize, out: &mut String) {
    let line = t.title.replace('\n', " ");
    out.push_str(&format!("{}- [{}] {line}\n", "  ".repeat(depth), if t.is_done() { 'x' } else { ' ' }));
    for s in &t.sub {
        md_line(s, depth + 1, out);
    }
}

impl Project {
    /// `# <project>`, then `## KEY: Title` per stage with `Depends:` / `Due:` lines and `- [ ]` / `- [x]` tasks.
    pub fn export_markdown(&self) -> Result<String> {
        let mut out = format!("# {}\n", self.config.name.replace('\n', " "));
        for s in self.load_stages()? {
            out.push_str(&format!("\n## {}: {}\n\n", s.key, s.title.replace('\n', " ")));
            if !s.depends.is_empty() {
                let d: Vec<&str> = s.depends.iter().map(|d| d.id.as_str()).collect();
                out.push_str(&format!("Depends: {}\n", d.join(", ")));
            }
            if let Some(d) = &s.due {
                out.push_str(&format!("Due: {d}\n"));
            }
            if !s.depends.is_empty() || s.due.is_some() {
                out.push('\n');
            }
            s.tasks.iter().for_each(|t| md_line(t, 0, &mut out));
        }
        Ok(out)
    }

    /// One row per task (a stage without tasks gets one row with empty task columns); `depends` is `A;B`.
    pub fn export_csv(&self) -> Result<String> {
        let mut out = CSV_HEADER.join(",");
        out.push('\n');
        for s in self.load_stages()? {
            let deps = s.depends.iter().map(|d| d.id.as_str()).collect::<Vec<_>>().join(";");
            let head = [s.key.clone(), s.title.clone()];
            let tail = [s.who.to_string(), s.priority.as_str().to_string(), s.due.clone().unwrap_or_default(), deps];
            let mut row = |task: Option<&Task>| {
                let (id, title, done) = task.map_or((String::new(), String::new(), String::new()), |t| {
                    (t.id.clone(), t.title.clone(), t.is_done().to_string())
                });
                let cells: Vec<String> = head.iter().cloned().chain([id, title, done]).chain(tail.iter().cloned()).collect();
                out.push_str(&cells.iter().map(|c| csv_cell(c)).collect::<Vec<_>>().join(","));
                out.push('\n');
            };
            if s.tasks.is_empty() {
                row(None);
            }
            s.tasks.iter().for_each(|t| row(Some(t)));
        }
        Ok(out)
    }

    /// JSON array `[{title, body, labels}]`, one per open task; labels = `[stage key]`. Pathloom makes no network call.
    pub fn export_github_issues(&self) -> Result<String> {
        let mut issues = Vec::new();
        for s in self.load_stages()? {
            for t in s.tasks.iter().filter(|t| !t.is_done()) {
                let mut body = format!("Stage {}: {}\nTask: {}", s.key, s.title, t.id);
                if !s.depends.is_empty() {
                    let d: Vec<&str> = s.depends.iter().map(|d| d.id.as_str()).collect();
                    body.push_str(&format!("\nDepends on stages: {}", d.join(", ")));
                }
                if let Some(d) = &s.due {
                    body.push_str(&format!("\nDue: {d}"));
                }
                issues.push(json!({"title": t.title, "body": body, "labels": [s.key]}));
            }
        }
        serde_json::to_string_pretty(&issues).map_err(|e| bad(e.to_string()))
    }
}

// ---------- import ----------

#[derive(Debug, Default)]
struct Draft {
    /// Key or title the file calls this stage (also used to resolve `depends`).
    hint: Option<String>,
    title: String,
    who: Option<String>,
    priority: Option<String>,
    due: Option<String>,
    depends: Vec<String>,
    /// (title, done)
    tasks: Vec<(String, bool)>,
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10 && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
}

fn truthy(s: &str) -> bool {
    matches!(s.trim().to_lowercase().as_str(), "true" | "yes" | "y" | "1" | "x" | "done" | "complete" | "completed" | "finished" | "closed")
}

impl Project {
    fn build_spec(&self, drafts: Vec<Draft>) -> Result<ApplySpec> {
        if drafts.is_empty() {
            return Err(bad("nothing to import: no stages with tasks found"));
        }
        let existing = self.load_stages()?;
        let mut used: HashSet<String> = HashSet::new();
        let mut names: HashMap<String, String> = HashMap::new(); // hint / lowercase title -> key
        let mut keys = Vec::new();
        for d in &drafts {
            let free = |k: &str| !used.contains(k);
            let by_hint = d.hint.as_deref().and_then(|h| existing.iter().find(|s| s.key == h && s.title == d.title)).map(|s| s.key.clone());
            let by_title = existing.iter().find(|s| s.title == d.title && free(&s.key)).map(|s| s.key.clone());
            let hint_ok = d.hint.as_deref().filter(|h| is_valid_key(h) && free(h) && !existing.iter().any(|s| s.key == *h) && !self.stage_path(h).exists());
            let key = by_hint
                .filter(|k| free(k))
                .or(by_title)
                .or_else(|| hint_ok.map(String::from))
                .unwrap_or_else(|| {
                    generate_key(&d.title, &|k| used.contains(k) || existing.iter().any(|s| s.key == k) || self.stage_path(k).exists())
                });
            used.insert(key.clone());
            if let Some(h) = &d.hint {
                names.entry(h.clone()).or_insert_with(|| key.clone());
            }
            names.entry(d.title.to_lowercase()).or_insert_with(|| key.clone());
            keys.push(key);
        }
        let mut spec = ApplySpec::default();
        for (d, key) in drafts.iter().zip(&keys) {
            let mut sim = existing.iter().find(|s| s.key == *key).cloned().unwrap_or_else(|| Stage::new(key, &d.title));
            let (mut tasks, mut done) = (Vec::new(), Vec::new());
            for (t, is_done) in &d.tasks {
                let t = t.trim();
                if t.is_empty() {
                    continue;
                }
                let id = match sim.tasks.iter().find(|x| x.title == t) {
                    Some(x) => x.id.clone(),
                    None => {
                        push_task(&mut sim, t);
                        tasks.push(t.to_string());
                        sim.tasks.last().map(|x| x.id.clone()).unwrap_or_default()
                    }
                };
                if *is_done {
                    done.push(id);
                }
            }
            let mut deps: Vec<DepSpec> = Vec::new();
            for name in &d.depends {
                let k = names
                    .get(name)
                    .or_else(|| names.get(&name.to_lowercase()))
                    .cloned()
                    .or_else(|| existing.iter().find(|s| s.key == *name).map(|s| s.key.clone()));
                if let Some(k) = k.filter(|k| k != key && deps.iter().all(|x| !matches!(x, DepSpec::Key(y) if *y == *k))) {
                    deps.push(DepSpec::Key(k));
                }
            }
            spec.stages.push(StageSpec {
                id: Some(key.clone()),
                title: Some(d.title.clone()),
                who: d.who.as_deref().and_then(|w| Who::parse(w.trim()).ok()),
                priority: d.priority.as_deref().and_then(|p| match p.trim().to_lowercase().as_str() {
                    "medium" => Some(Priority::Med),
                    p => Priority::parse(p),
                }),
                due: d.due.as_deref().map(str::trim).filter(|x| is_date(x)).map(String::from),
                depends: (!deps.is_empty()).then_some(deps),
                tasks,
                tasks_done: done,
                ..Default::default()
            });
        }
        Ok(spec)
    }

    /// `#` / `##` headings are stages (`KEY: Title` keeps the key), `Depends:` / `Due:` lines before the first
    /// item set those fields, `- [ ]` / `- [x]` / `-` / `*` items are tasks (nested items are flattened).
    /// Headings without tasks and dependencies are dropped; items before any heading go to `Imported`.
    pub fn import_markdown(&self, text: &str) -> Result<ApplySpec> {
        let mut drafts: Vec<Draft> = Vec::new();
        let mut cur: Option<Draft> = None;
        let mut loose = Draft { title: "Imported".into(), ..Default::default() };
        let mut fence = false;
        let flush = |cur: &mut Option<Draft>, drafts: &mut Vec<Draft>| {
            drafts.extend(cur.take().filter(|d| !d.tasks.is_empty() || !d.depends.is_empty()));
        };
        for line in text.lines() {
            let t = line.trim_start();
            if t.starts_with("```") {
                fence = !fence;
                continue;
            }
            if fence {
                continue;
            }
            if let Some(h) = line.strip_prefix("## ").or_else(|| line.strip_prefix("# ")) {
                flush(&mut cur, &mut drafts);
                let h = h.trim();
                let (hint, title) = match h.split_once(": ") {
                    Some((k, rest)) if is_valid_key(k) && !rest.trim().is_empty() => (Some(k.to_string()), rest.trim()),
                    _ => (None, h),
                };
                cur = Some(Draft { hint, title: title.to_string(), ..Default::default() });
            } else if let Some(item) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("+ ")) {
                let (done, title) = match item.strip_prefix("[ ] ") {
                    Some(r) => (false, r),
                    None => match item.strip_prefix("[x] ").or_else(|| item.strip_prefix("[X] ")) {
                        Some(r) => (true, r),
                        None => (false, item),
                    },
                };
                if !title.trim().is_empty() {
                    cur.as_mut().unwrap_or(&mut loose).tasks.push((title.trim().to_string(), done));
                }
            } else if let Some(d) = cur.as_mut().filter(|d| d.tasks.is_empty()) {
                if let Some(v) = t.strip_prefix("Depends:") {
                    d.depends = v.split([',', ';']).map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
                } else if let Some(v) = t.strip_prefix("Due:") {
                    d.due = Some(v.trim().to_string());
                }
            }
        }
        flush(&mut cur, &mut drafts);
        if !loose.tasks.is_empty() {
            drafts.insert(0, loose);
        }
        self.build_spec(drafts)
    }

    /// Pathloom's own CSV (see [`Project::export_csv`]), a plain `title` column, or Notion (`Name` + optional
    /// `Status`: done/complete marks done). Rows are grouped by `stage_id` / `stage_title` / `Stage` / `Group` /
    /// `List` / `Section` / `Phase` when present, else all go into one stage `Imported`.
    pub fn import_csv(&self, text: &str) -> Result<ApplySpec> {
        let rows = parse_csv(text);
        let Some((head, body)) = rows.split_first() else { return Err(bad("empty CSV")) };
        let head: Vec<String> = head.iter().map(|h| h.trim().to_lowercase()).collect();
        let col = |names: &[&str]| names.iter().find_map(|n| head.iter().position(|h| h == n));
        let (c_id, c_stage) = (col(&["stage_id"]), col(&["stage_title", "stage", "group", "list", "section", "phase", "epic"]));
        let c_title = col(&["task_title", "title", "name", "task"]);
        if c_title.is_none() && c_stage.is_none() {
            return Err(bad("CSV needs a `title` or `Name` column (or `stage_title` / `task_title`)"));
        }
        let (c_done, c_status) = (col(&["done"]), col(&["status"]));
        let (c_who, c_prio, c_due, c_deps) = (col(&["who"]), col(&["priority"]), col(&["due"]), col(&["depends"]));
        let mut drafts: Vec<Draft> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();
        for r in body {
            let cell = |c: Option<usize>| c.and_then(|i| r.get(i)).map(|s| s.trim()).unwrap_or_default();
            let (id, stage, title) = (cell(c_id), cell(c_stage), cell(c_title));
            let name = if stage.is_empty() { id } else { stage };
            let group = if id.is_empty() { name } else { id };
            let (group, name) = if group.is_empty() { ("Imported", "Imported") } else { (group, name) };
            let i = *index.entry(group.to_string()).or_insert_with(|| {
                drafts.push(Draft { hint: (!id.is_empty()).then(|| id.to_string()), title: name.to_string(), ..Default::default() });
                drafts.len() - 1
            });
            let d = &mut drafts[i];
            for (slot, v) in [(&mut d.who, cell(c_who)), (&mut d.priority, cell(c_prio)), (&mut d.due, cell(c_due))] {
                if slot.is_none() && !v.is_empty() {
                    *slot = Some(v.to_string());
                }
            }
            if d.depends.is_empty() {
                d.depends = cell(c_deps).split([';', ',', '|']).map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
            }
            if !title.is_empty() {
                let done = truthy(cell(c_done)) || truthy(cell(c_status));
                d.tasks.push((title.to_string(), done));
            }
        }
        self.build_spec(drafts)
    }

    /// Notion CSV export (same reader as [`Project::import_csv`]).
    pub fn import_notion(&self, text: &str) -> Result<ApplySpec> {
        self.import_csv(text)
    }

    /// Trello board JSON export: open lists become stages, open cards tasks (done = `dueComplete`), checklist
    /// items tasks named `Card: item`. A stage's `due` is the earliest due date of its open cards.
    pub fn import_trello(&self, json_text: &str) -> Result<ApplySpec> {
        let v: Value = serde_json::from_str(json_text).map_err(|e| bad(format!("not Trello JSON: {e}")))?;
        let arr = |k: &str| v.get(k).and_then(Value::as_array).cloned().unwrap_or_default();
        let s = |o: &Value, k: &str| o.get(k).and_then(Value::as_str).unwrap_or_default().trim().to_string();
        let closed = |o: &Value| o.get("closed").and_then(Value::as_bool).unwrap_or(false);
        let (lists, cards, checklists) = (arr("lists"), arr("cards"), arr("checklists"));
        let mut drafts = Vec::new();
        for l in lists.iter().filter(|l| !closed(l) && !s(l, "name").is_empty()) {
            let lid = s(l, "id");
            let mut d = Draft { title: s(l, "name"), ..Default::default() };
            for c in cards.iter().filter(|c| !closed(c) && s(c, "idList") == lid && !s(c, "name").is_empty()) {
                let name = s(c, "name");
                d.tasks.push((name.clone(), c.get("dueComplete").and_then(Value::as_bool).unwrap_or(false)));
                let due = s(c, "due");
                if due.len() >= 10 && is_date(&due[..10]) && d.due.as_deref().is_none_or(|x| &due[..10] < x) {
                    d.due = Some(due[..10].to_string());
                }
                let ids: Vec<String> = c.get("idChecklists").and_then(Value::as_array).into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect();
                for cl in checklists.iter().filter(|cl| ids.contains(&s(cl, "id"))) {
                    for it in cl.get("checkItems").and_then(Value::as_array).into_iter().flatten() {
                        let n = s(it, "name");
                        if !n.is_empty() {
                            d.tasks.push((format!("{name}: {n}"), s(it, "state") == "complete"));
                        }
                    }
                }
            }
            if !d.tasks.is_empty() {
                drafts.push(d);
            }
        }
        self.build_spec(drafts)
    }
}

/// RFC 4180 reader: quoted fields, `""` escapes, CRLF or LF, optional BOM. Blank rows are dropped.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let (mut row, mut field, mut quoted) = (Vec::<String>::new(), String::new(), false);
    let mut chars = text.strip_prefix('\u{feff}').unwrap_or(text).chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            (true, '"') => quoted = false,
            (true, _) => field.push(c),
            (false, '"') if field.is_empty() => quoted = true,
            (false, ',') => row.push(std::mem::take(&mut field)),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            (false, _) => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows.retain(|r| r.iter().any(|c| !c.trim().is_empty()));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_reader_handles_quotes_and_crlf() {
        let rows = parse_csv("\u{feff}a,b\r\n\"x, \"\"y\"\"\",\"l1\nl2\"\r\n\r\nlast,");
        assert_eq!(rows, vec![vec!["a", "b"], vec!["x, \"y\"", "l1\nl2"], vec!["last", ""]]);
        assert_eq!(csv_cell("a\"b"), "\"a\"\"b\"");
    }
}
