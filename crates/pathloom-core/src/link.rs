//! Deep links `pathloom://open?project=<path>&stage=KEY&task=ID` (m10 §Б7). Pure string code, no I/O.

use crate::error::{Error, Result};
use crate::stagefile::{is_valid_key, stage_key_of};
use std::path::Path;

const PREFIX: &str = "pathloom://open?";

/// What a link points at: a project and optionally one stage or task card.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LinkTarget {
    pub project: String,
    pub stage: Option<String>,
    pub task: Option<String>,
}

fn bad(msg: impl Into<String>) -> Error {
    Error::InvalidSpec(msg.into())
}

fn encode(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

fn decode(s: &str) -> Result<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok());
            out.push(hex.ok_or_else(|| bad("link has a broken %-escape"))?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| bad("link is not valid UTF-8"))
}

/// Link to a project, a stage (`KEY`) or a task (`KEY-NN`). `target` is a stage key or task id.
pub fn make_link(root: &Path, target: Option<&str>) -> Result<String> {
    let mut link = format!("{PREFIX}project={}", encode(&root.display().to_string()));
    if let Some(t) = target {
        let key = stage_key_of(t);
        if !is_valid_key(key) {
            return Err(bad(format!("`{t}` is not a stage key or task id")));
        }
        link.push_str(&format!("&stage={key}"));
        if t != key {
            link.push_str(&format!("&task={}", encode(t)));
        }
    }
    Ok(link)
}

/// Parse and validate a `pathloom://open?...` link; other schemes and hosts are rejected.
pub fn parse_link(link: &str) -> Result<LinkTarget> {
    let head = link.get(..PREFIX.len()).filter(|h| h.eq_ignore_ascii_case(PREFIX));
    let query = head.map(|_| &link[PREFIX.len()..]).ok_or_else(|| bad("not a pathloom://open link"))?;
    let (mut project, mut stage, mut task) = (None, None, None);
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let v = decode(v)?;
        match k {
            "project" => project = Some(v),
            "stage" => stage = Some(v),
            "task" => task = Some(v),
            _ => {}
        }
    }
    let project = project.filter(|p| !p.trim().is_empty() && !p.contains('\0')).ok_or_else(|| bad("link has no project"))?;
    let stage = stage.filter(|s| !s.is_empty());
    let task = task.filter(|s| !s.is_empty());
    if let Some(s) = &stage
        && !is_valid_key(s)
    {
        return Err(bad(format!("link stage `{s}` is not a valid key")));
    }
    if let Some(t) = &task {
        let k = stage_key_of(t);
        if !is_valid_key(k) || stage.as_deref().is_some_and(|s| s != k) || t.len() > 64 || t.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(bad(format!("link task `{t}` is not a valid task id")));
        }
    }
    let stage = stage.or_else(|| task.as_deref().map(|t| stage_key_of(t).to_string()));
    Ok(LinkTarget { project, stage, task })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_windows_path_with_spaces_and_cyrillic() {
        let root = Path::new(r"D:\Мої проєкти\Path loom");
        let l = make_link(root, Some("M0-02")).unwrap();
        assert!(l.starts_with("pathloom://open?project=D%3A%5C"));
        assert!(!l.contains(' ') && !l.contains('\\'));
        let t = parse_link(&l).unwrap();
        assert_eq!(t, LinkTarget { project: root.display().to_string(), stage: Some("M0".into()), task: Some("M0-02".into()) });
    }

    #[test]
    fn project_and_stage_links() {
        let l = make_link(Path::new("/a/b"), None).unwrap();
        assert_eq!(parse_link(&l).unwrap(), LinkTarget { project: "/a/b".into(), stage: None, task: None });
        let t = parse_link(&make_link(Path::new("/a"), Some("M1")).unwrap()).unwrap();
        assert_eq!((t.stage.as_deref(), t.task), (Some("M1"), None));
    }

    #[test]
    fn rejects_bad_links() {
        for l in [
            "http://open?project=%2Fa",
            "pathloom://close?project=%2Fa",
            "pathloom://open?stage=M0",
            "pathloom://open?project=%2Fa&stage=m0",
            "pathloom://open?project=%2Fa&task=M0-1&stage=M1",
            "pathloom://open?project=%ZZ",
            "pathloom://open?project=%FF",
        ] {
            assert!(parse_link(l).is_err(), "{l}");
        }
        assert!(make_link(Path::new("/a"), Some("bad key")).is_err());
    }
}
