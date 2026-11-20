//! Read the plan from a git ref instead of the working tree, plus small git helpers.

use crate::error::{Error, Result};
use std::path::Path;
use std::process::{Command, Output};

/// Runs `git <args>` in `root` without a console window on Windows.
pub(crate) fn run(root: &Path, args: &[&str]) -> std::io::Result<Output> {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(root);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.output()
}

fn git_err(msg: impl std::fmt::Display) -> Error {
    Error::Git(msg.to_string().trim().to_string())
}

/// Runs git and returns stdout bytes; non-zero exit becomes `Error::Git(stderr)`.
fn ok(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let out = run(root, args).map_err(git_err)?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(git_err(String::from_utf8_lossy(&out.stderr)))
    }
}

fn text(root: &Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8_lossy(&ok(root, args)?).trim().to_string())
}

/// `git fetch --quiet`.
pub fn fetch(root: &Path) -> Result<()> {
    ok(root, &["fetch", "--quiet"]).map(drop)
}

/// Commit sha a ref points to.
pub fn ref_sha(root: &Path, git_ref: &str) -> Result<String> {
    text(root, &["rev-parse", "--verify", "--end-of-options", &format!("{git_ref}^{{commit}}")])
}

/// Copies `plan_dir` as committed at `git_ref` (plus the local `.pathloom/project.toml`)
/// into a temp dir, so `Project::open(tmp.path())` reads that version.
pub fn materialize(root: &Path, git_ref: &str, plan_dir: &str) -> Result<tempfile::TempDir> {
    let sha = ref_sha(root, git_ref)?;
    let list = ok(root, &["ls-tree", "-r", "--name-only", "-z", &sha, "--", plan_dir])?;
    let tmp = tempfile::tempdir()?;
    for name in list.split(|b| *b == 0).filter(|n| !n.is_empty()) {
        let name = String::from_utf8_lossy(name).into_owned();
        let bytes = ok(root, &["show", &format!("{sha}:./{name}")])?;
        let dest = tmp.path().join(&name);
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(dest, bytes)?;
    }
    std::fs::create_dir_all(tmp.path().join(".pathloom"))?;
    std::fs::copy(root.join(".pathloom/project.toml"), tmp.path().join(".pathloom/project.toml"))?;
    Ok(tmp)
}

/// True when no tracked file has changes. Untracked files do not count: they never block a commit of one
/// file or a fast-forward, and Pathloom's own state files would otherwise keep the tree "dirty" forever.
pub fn is_clean(root: &Path) -> Result<bool> {
    Ok(ok(root, &["status", "--porcelain", "--untracked-files=no"])?.is_empty())
}

/// Fast-forwards the current branch to `git_ref`; `false` when that is not possible.
pub fn ff_to(root: &Path, git_ref: &str) -> Result<bool> {
    let out = run(root, &["merge", "--ff-only", "--quiet", "--end-of-options", git_ref]).map_err(git_err)?;
    Ok(out.status.success())
}

/// Stages and commits only `path`.
pub fn commit_file(root: &Path, path: &str, message: &str) -> Result<()> {
    ok(root, &["add", "--", path])?;
    ok(root, &["commit", "--quiet", "-m", message, "--", path]).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_ok() -> bool {
        Command::new("git").arg("--version").output().is_ok_and(|o| o.status.success())
    }

    fn g(root: &Path, args: &[&str]) {
        ok(root, args).unwrap();
    }

    fn repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        g(r, &["init", "--quiet"]);
        g(r, &["config", "user.name", "T"]);
        g(r, &["config", "user.email", "t@t"]);
        g(r, &["config", "commit.gpgsign", "false"]);
        d
    }

    fn write(root: &Path, rel: &str, bytes: &[u8]) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    #[test]
    fn materialize_returns_committed_version() {
        if !git_ok() {
            return;
        }
        let d = repo();
        let r = d.path();
        write(r, ".pathloom/project.toml", b"format = 1\n");
        write(r, "plan/stages/A.md", b"committed\n");
        write(r, "plan/bin.dat", &[0, 255, 1, 2]);
        g(r, &["add", "-A"]);
        g(r, &["commit", "--quiet", "-m", "init"]);
        write(r, "plan/stages/A.md", b"changed\n");
        write(r, "plan/stages/B.md", b"untracked\n");
        let tmp = materialize(r, "HEAD", "plan").unwrap();
        assert_eq!(std::fs::read(tmp.path().join("plan/stages/A.md")).unwrap(), b"committed\n");
        assert_eq!(std::fs::read(tmp.path().join("plan/bin.dat")).unwrap(), vec![0, 255, 1, 2]);
        assert!(!tmp.path().join("plan/stages/B.md").exists());
        assert!(tmp.path().join(".pathloom/project.toml").is_file());
        assert_eq!(ref_sha(r, "HEAD").unwrap().len(), 40);
        assert!(ref_sha(r, "nope").is_err());
    }

    #[test]
    fn clean_ff_and_commit_file() {
        if !git_ok() {
            return;
        }
        let d = repo();
        let r = d.path();
        write(r, "a.txt", b"1");
        g(r, &["add", "-A"]);
        g(r, &["commit", "--quiet", "-m", "one"]);
        assert!(is_clean(r).unwrap());
        write(r, "untracked.jsonl", b"queue");
        assert!(is_clean(r).unwrap(), "untracked files do not make the tree dirty");
        write(r, "b.txt", b"b");
        g(r, &["add", "b.txt"]);
        g(r, &["commit", "--quiet", "-m", "b"]);
        write(r, "a.txt", b"2");
        write(r, "b.txt", b"other");
        assert!(!is_clean(r).unwrap());
        commit_file(r, "a.txt", "two").unwrap();
        assert!(!is_clean(r).unwrap()); // b.txt untouched
        assert!(!ff_to(r, "--help").unwrap(), "a ref is never read as an option");
        let two = ref_sha(r, "HEAD").unwrap();
        g(r, &["branch", "side", "HEAD~1"]);
        g(r, &["checkout", "--quiet", "side"]);
        assert!(ff_to(r, &two).unwrap());
        assert_eq!(ref_sha(r, "HEAD").unwrap(), two);
        g(r, &["checkout", "--quiet", "-b", "div", "HEAD~1"]);
        write(r, "c.txt", b"c");
        g(r, &["add", "c.txt"]);
        g(r, &["commit", "--quiet", "-m", "div"]);
        assert!(!ff_to(r, &two).unwrap());
    }
}
