//! Plan templates: built-in apply specs (`templates/<id>.<lang>.json`) and the user's own, saved from a plan
//! (`<user_dir>/<name>.json`, envelope `{title, description?, spec}`; a bare apply spec is accepted too).

use crate::apply::{ApplyOptions, ApplyReport, ApplySpec, DepSpec, FrameSpec, StageSpec};
use crate::error::{Error, Result};
use crate::fsutil::write_atomic;
use crate::keygen::generate_key;
use crate::layout::{card_ref, el_id, el_type};
use crate::model::{Priority, StageKind, Who};
use crate::project::Project;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// (id, title en, description en, title uk, description uk, en json, uk json)
type Builtin = (&'static str, &'static str, &'static str, &'static str, &'static str, &'static str, &'static str);

const BUILTIN: [Builtin; 4] = [
    (
        "product-launch", "Product launch", "From positioning and beta to launch day.",
        "Запуск продукту", "Від позиціонування та бети до дня запуску.",
        include_str!("../templates/product-launch.en.json"), include_str!("../templates/product-launch.uk.json"),
    ),
    (
        "book", "Write a book", "From the concept to a published book.",
        "Написати книгу", "Від концепції до виданої книги.",
        include_str!("../templates/book.en.json"), include_str!("../templates/book.uk.json"),
    ),
    (
        "renovation", "Renovation", "From budget and design to moving in.",
        "Ремонт", "Від бюджету й проєкту до заселення.",
        include_str!("../templates/renovation.en.json"), include_str!("../templates/renovation.uk.json"),
    ),
    (
        "course", "Online course", "From audience and syllabus to open enrollment.",
        "Навчальний курс", "Від аудиторії та програми до відкриття набору.",
        include_str!("../templates/course.en.json"), include_str!("../templates/course.uk.json"),
    ),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TemplateInfo {
    pub id: String,
    pub title: String,
    pub description: String,
    pub builtin: bool,
    /// Languages with their own text (user templates have a single, unmarked language: empty).
    pub langs: Vec<String>,
}

fn check_name(name: &str) -> Result<()> {
    let ok = (1..=40).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok { Ok(()) } else { Err(Error::InvalidName(format!("template name `{name}` must match [a-z0-9-]{{1,40}}"))) }
}

fn user_file(user_dir: Option<&Path>, id: &str) -> Option<PathBuf> {
    user_dir.map(|d| d.join(format!("{id}.json")))
}

fn parse_user(text: &str) -> Option<(String, String, ApplySpec)> {
    let v: Value = serde_json::from_str(text).ok()?;
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let spec = match v.get("spec") {
        Some(sp) => serde_json::from_value(sp.clone()).ok()?,
        None => serde_json::from_value(v.clone()).ok()?,
    };
    Some((s("title"), s("description"), spec))
}

/// Built-in templates (title and description in `lang`, `uk` or else English) and the `*.json` files of `user_dir`.
pub fn list_templates(user_dir: Option<&Path>, lang: &str) -> Vec<TemplateInfo> {
    let uk = lang == "uk";
    let mut out: Vec<TemplateInfo> = BUILTIN
        .iter()
        .map(|b| TemplateInfo {
            id: b.0.into(),
            title: (if uk { b.3 } else { b.1 }).into(),
            description: (if uk { b.4 } else { b.2 }).into(),
            builtin: true,
            langs: vec!["en".into(), "uk".into()],
        })
        .collect();
    let mut mine = Vec::new();
    for e in user_dir.and_then(|d| std::fs::read_dir(d).ok()).into_iter().flatten().flatten() {
        let p = e.path();
        let Some(id) = p.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".json")) else { continue };
        if check_name(id).is_err() || out.iter().any(|t| t.id == id) {
            continue;
        }
        let Some((title, description, spec)) = std::fs::read_to_string(&p).ok().and_then(|t| parse_user(&t)) else { continue };
        let title = if title.is_empty() { id.to_string() } else { title };
        let description = if description.is_empty() { format!("{} stages", spec.stages.len()) } else { description };
        mine.push(TemplateInfo { id: id.into(), title, description, builtin: false, langs: vec![] });
    }
    mine.sort_by(|a, b| a.id.cmp(&b.id));
    out.extend(mine);
    out
}

/// The spec of a template. Built-in: `lang` `uk`, otherwise English. Unknown id -> `NotFound`.
pub fn load_template(id: &str, lang: &str, user_dir: Option<&Path>) -> Result<ApplySpec> {
    check_name(id)?;
    if let Some(b) = BUILTIN.iter().find(|b| b.0 == id) {
        let text = if lang == "uk" { b.6 } else { b.5 };
        return serde_json::from_str(text).map_err(|e| Error::InvalidSpec(format!("template {id}: {e}")));
    }
    let text = user_file(user_dir, id).and_then(|p| std::fs::read_to_string(p).ok());
    let (_, _, spec) = text.as_deref().and_then(parse_user).ok_or_else(|| Error::NotFound(format!("template {id}")))?;
    Ok(spec)
}

impl Project {
    /// Save the plan as a user template `<user_dir>/<name>.json`: stages, descriptions, tasks (undone), dependencies,
    /// kind/who/priority and the frames of the `roadmap` board. No log, claims, evidence, notes, sessions, comments or dates.
    pub fn save_as_template(&self, name: &str, title: &str, user_dir: &Path) -> Result<PathBuf> {
        check_name(name)?;
        let stages = self.load_stages()?;
        let mut spec = ApplySpec::default();
        for s in &stages {
            spec.stages.push(StageSpec {
                id: Some(s.key.clone()),
                title: Some(s.title.clone()),
                description: (!s.description.trim().is_empty()).then(|| s.description.clone()),
                kind: (s.kind != StageKind::Stage).then_some(s.kind),
                who: (s.who != Who::ai()).then(|| s.who.clone()),
                priority: (s.priority != Priority::Med).then_some(s.priority),
                depends: (!s.depends.is_empty()).then(|| {
                    s.depends.iter().map(|d| if d.soft { DepSpec::Full { id: d.id.clone(), soft: true } } else { DepSpec::Key(d.id.clone()) }).collect()
                }),
                // shortcut: subtasks are not exported, only top-level tasks.
                tasks: s.tasks.iter().map(|t| t.title.clone()).collect(),
                check: s.check.clone(),
                ..Default::default()
            });
        }
        if let Ok(b) = self.load_board("roadmap") {
            for f in b.elements.iter().filter(|e| el_type(e) == "frame") {
                let name = f.get("name").and_then(Value::as_str).unwrap_or_default();
                let members: Vec<String> = b
                    .elements
                    .iter()
                    .filter(|e| e.get("frame").and_then(Value::as_str) == Some(el_id(f)))
                    .filter_map(|e| card_ref(e).map(String::from))
                    .collect();
                if !name.is_empty() && !members.is_empty() {
                    spec.frames.push(FrameSpec { name: name.into(), members });
                }
            }
        }
        let t = title.trim();
        let doc = serde_json::json!({"title": if t.is_empty() { name } else { t }, "spec": spec});
        let text = serde_json::to_string_pretty(&doc).map_err(|e| Error::InvalidSpec(e.to_string()))?;
        let path = user_dir.join(format!("{name}.json"));
        write_atomic(&path, format!("{text}\n").as_bytes())?;
        Ok(path)
    }

    /// Give the template's stages fresh keys (generated from titles, never clashing with this project's) and
    /// rewrite depends, links and frame members. Without this a template would update stages that share its keys.
    pub fn instantiate_template(&self, mut spec: ApplySpec) -> Result<ApplySpec> {
        let mut taken: HashSet<String> = self.load_stages()?.into_iter().map(|s| s.key).collect();
        let mut map: HashMap<String, String> = HashMap::new();
        for s in &mut spec.stages {
            let Some(title) = s.title.clone() else { continue };
            let key = generate_key(&title, &|k| taken.contains(k));
            taken.insert(key.clone());
            if let Some(old) = s.id.replace(key.clone()) {
                map.insert(old, key);
            }
        }
        let re = |k: &str| map.get(k).cloned().unwrap_or_else(|| k.to_string());
        for s in &mut spec.stages {
            for d in s.depends.iter_mut().flatten() {
                match d {
                    DepSpec::Key(k) => *k = re(k),
                    DepSpec::Full { id, .. } => *id = re(id),
                }
            }
        }
        for l in &mut spec.links {
            (l.from, l.to) = (re(&l.from), re(&l.to));
        }
        for f in &mut spec.frames {
            f.members.iter_mut().for_each(|m| *m = re(m));
        }
        Ok(spec)
    }

    /// Apply a template directly (author `template`), then lay the board out so cards do not overlap.
    pub fn apply_template(&self, spec: ApplySpec) -> Result<ApplyReport> {
        let spec = self.instantiate_template(spec)?;
        let r = self.apply(&spec, &ApplyOptions { dry_run: false, author: "template".into() })?;
        self.layout_board(&r.board, &[])?;
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> (tempfile::TempDir, Project) {
        let d = tempfile::tempdir().unwrap();
        let p = Project::init(d.path(), "t").unwrap();
        (d, p)
    }

    #[test]
    fn builtins_apply_cleanly() {
        for t in list_templates(None, "en") {
            let mut counts = Vec::new();
            for lang in ["en", "uk"] {
                let spec = load_template(&t.id, lang, None).unwrap();
                assert!((5..=9).contains(&spec.stages.len()), "{}", t.id);
                assert!(spec.stages.iter().any(|s| s.kind == Some(StageKind::Milestone)));
                assert!(spec.stages.iter().any(|s| s.who == Some(Who::me())));
                assert!((1..=3).contains(&spec.frames.len()));
                counts.push(spec.stages.len());
                let (_d, p) = empty();
                let r = p.apply_template(spec).unwrap();
                assert_eq!(r.created.len(), counts[counts.len() - 1]);
                assert!(p.validate().unwrap().is_empty(), "{} {lang}: {:?}", t.id, p.validate());
            }
            assert_eq!(counts[0], counts[1], "{}", t.id);
        }
    }

    #[test]
    fn save_round_trip() {
        let (_d, p) = empty();
        p.apply_template(load_template("book", "en", None).unwrap()).unwrap();
        let keys: Vec<String> = p.load_stages().unwrap().into_iter().map(|s| s.key).collect();
        let first = p.load_stage(&keys[0]).unwrap();
        p.set_task_done(&first.tasks[0].id, true).unwrap();
        p.update_meta(&keys[0], crate::model::StageMeta { due: Some("2030-01-01".into()), ..Default::default() }).unwrap();
        let ud = tempfile::tempdir().unwrap();
        let path = p.save_as_template("my-book", "My book", ud.path()).unwrap();
        assert!(std::fs::read_to_string(path).unwrap().contains("My book"));
        let list = list_templates(Some(ud.path()), "en");
        assert!(list.iter().any(|t| t.id == "my-book" && !t.builtin && t.title == "My book"));

        let (_d2, q) = empty();
        q.apply_template(load_template("my-book", "en", Some(ud.path())).unwrap()).unwrap();
        let (a, b) = (p.load_stages().unwrap(), q.load_stages().unwrap());
        assert_eq!(a.len(), b.len());
        for s in &b {
            let o = a.iter().find(|o| o.title == s.title).unwrap();
            assert_eq!(s.tasks.iter().map(|t| &t.title).collect::<Vec<_>>(), o.tasks.iter().map(|t| &t.title).collect::<Vec<_>>());
            assert!(s.tasks.iter().all(|t| !t.done));
            assert!(s.due.is_none());
            assert_eq!((s.kind, &s.who), (o.kind, &o.who));
            let titles = |st: &[crate::model::Stage], x: &crate::model::Stage| {
                let mut v: Vec<String> = x.depends.iter().map(|d| st.iter().find(|y| y.key == d.id).unwrap().title.clone()).collect();
                v.sort();
                v
            };
            assert_eq!(titles(&b, s), titles(&a, o));
        }
        assert_eq!(q.load_board("roadmap").unwrap().elements.iter().filter(|e| el_type(e) == "frame").count(), 3);
    }

    #[test]
    fn bad_names_rejected() {
        let (_d, p) = empty();
        let ud = tempfile::tempdir().unwrap();
        for n in ["", "A", "a/b", "..", "a_b", &"x".repeat(41)] {
            assert!(matches!(p.save_as_template(n, "", ud.path()), Err(Error::InvalidName(_))), "{n}");
        }
        assert!(load_template("nope", "en", None).is_err());
    }

    #[test]
    fn applies_twice_without_clash() {
        let (_d, p) = empty();
        for _ in 0..2 {
            p.apply_template(load_template("course", "en", None).unwrap()).unwrap();
        }
        assert_eq!(p.load_stages().unwrap().len(), 16);
        assert!(p.validate().unwrap().is_empty());
    }
}
