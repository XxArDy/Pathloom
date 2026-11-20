//! Links git commits to stages/tasks via the subject prefix (`AUTH-02, 03: message`).

use crate::gitsrc::run;
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitRef {
    pub sha: String,
    pub subject: String,
    pub author: String,
    pub at: String,
    pub stages: Vec<String>,
    pub tasks: Vec<String>,
}

fn push_unique(v: &mut Vec<String>, s: String) {
    if !v.contains(&s) {
        v.push(s);
    }
}

/// Parses the text before the first `:`. Returns `(stages, tasks)`; both empty when the
/// prefix is not fully valid (the commit is then not linked).
pub fn parse_prefix(subject: &str) -> (Vec<String>, Vec<String>) {
    parse(subject).unwrap_or_default()
}

fn parse(subject: &str) -> Option<(Vec<String>, Vec<String>)> {
    let (prefix, _) = subject.split_once(':')?;
    let (mut stages, mut tasks) = (Vec::new(), Vec::new());
    let mut last: Option<(String, usize)> = None; // (stage key, number width)
    for part in prefix.split(',') {
        let part = part.trim();
        if !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()) {
            let (key, width) = last.clone()?;
            push_unique(&mut tasks, format!("{key}-{part:0>width$}"));
            continue;
        }
        let (key, num) = match part.split_once('-') {
            Some((k, n)) => (k, Some(n)),
            None => (part, None),
        };
        if !crate::stagefile::is_valid_key(key) {
            return None;
        }
        push_unique(&mut stages, key.to_string());
        match num {
            Some(n) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => {
                push_unique(&mut tasks, format!("{key}-{n}"));
                last = Some((key.to_string(), n.len()));
            }
            Some(_) => return None,
            None => last = Some((key.to_string(), 1)),
        }
    }
    Some((stages, tasks))
}

/// Recent commits (newest first) whose subject links to known stages. Empty without git.
pub fn scan(root: &Path, limit: usize, stage_keys: &HashSet<String>) -> Vec<CommitRef> {
    let n = limit.to_string();
    let Ok(out) = run(root, &["log", "-n", &n, "--format=%H%x1f%an%x1f%aI%x1f%s"]) else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut f = line.split('\u{1f}');
            let (sha, author, at, subject) = (f.next()?, f.next()?, f.next()?, f.next()?);
            let (mut stages, mut tasks) = parse_prefix(subject);
            stages.retain(|s| stage_keys.contains(s));
            tasks.retain(|t| t.split_once('-').is_some_and(|(k, _)| stage_keys.contains(k)));
            if stages.is_empty() && tasks.is_empty() {
                return None;
            }
            Some(CommitRef {
                sha: sha.into(),
                subject: subject.into(),
                author: author.into(),
                at: at.into(),
                stages,
                tasks,
            })
        })
        .collect()
}

/// `git rev-parse HEAD`, if any.
pub fn head(root: &Path) -> Option<String> {
    let out = run(root, &["rev-parse", "HEAD"]).ok().filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> (Vec<String>, Vec<String>) {
        parse_prefix(s)
    }

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn single_task() {
        assert_eq!(p("NATIVE-05: x"), (v(&["NATIVE"]), v(&["NATIVE-05"])));
    }

    #[test]
    fn padded_numbers() {
        assert_eq!(p("AUTH-02, 03: x"), (v(&["AUTH"]), v(&["AUTH-02", "AUTH-03"])));
        assert_eq!(p("AUTH-2,3: x"), (v(&["AUTH"]), v(&["AUTH-2", "AUTH-3"])));
        assert_eq!(p("AUTH-002, 3, 10: x"), (v(&["AUTH"]), v(&["AUTH-002", "AUTH-003", "AUTH-010"])));
    }

    #[test]
    fn stage_only_and_mixed() {
        assert_eq!(p("AUTH: x"), (v(&["AUTH"]), v(&[])));
        assert_eq!(p("AUTH-02, BILL-01: x"), (v(&["AUTH", "BILL"]), v(&["AUTH-02", "BILL-01"])));
        assert_eq!(p("  AUTH ,  BILL : x"), (v(&["AUTH", "BILL"]), v(&[])));
    }

    #[test]
    fn rejects() {
        for s in [
            "Merge branch 'x'", "fix: x", "auth-01: x", "AUTH-: x", "AUTH-0a: x", "A234567890B: x",
            "AUTH-01, : x", ": x", "03: x", "AUTH, 03, FOO bar: x", "Fix AUTH-01: x", "",
        ] {
            assert_eq!(p(s), (v(&[]), v(&[])), "{s}");
        }
    }

    #[test]
    fn dedupes_and_key_edges() {
        assert_eq!(p("A-1, 1: x"), (v(&["A"]), v(&["A-1"])));
        assert_eq!(p("A1B2C3D4-7: x").0, v(&["A1B2C3D4"]));
        assert_eq!(p("A1B2C3D4E-7: x"), (v(&[]), v(&[])), "keys are at most 8 characters");
    }

    #[test]
    fn scan_in_repo() {
        if run(Path::new("."), &["--version"]).map(|o| !o.status.success()).unwrap_or(true) {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        let g = |a: &[&str]| assert!(run(r, a).unwrap().status.success(), "{a:?}");
        assert!(scan(r, 10, &HashSet::new()).is_empty());
        assert!(head(r).is_none());
        g(&["init", "--quiet"]);
        g(&["config", "user.name", "T"]);
        g(&["config", "user.email", "t@t"]);
        g(&["config", "commit.gpgsign", "false"]);
        for m in ["AUTH-02, 03: login", "ZZZ-01: unknown", "fix: nothing", "AUTH: stage"] {
            g(&["commit", "--quiet", "--allow-empty", "-m", m]);
        }
        let keys: HashSet<String> = ["AUTH".to_string()].into();
        let c = scan(r, 10, &keys);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].stages, v(&["AUTH"]));
        assert_eq!(c[1].tasks, v(&["AUTH-02", "AUTH-03"]));
        assert_eq!(c[1].author, "T");
        assert_eq!(head(r).unwrap(), c[0].sha);
        assert_eq!(scan(r, 1, &keys).len(), 1);
    }
}
