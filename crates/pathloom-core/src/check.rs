//! Run a check command through the system shell and turn the result into [`Evidence`].

use crate::error::{Error, Result};
use crate::evidence::Evidence;
use std::path::Path;
use std::process::Command;

/// Run `cmd` with `sh -c` (`cmd /C` on Windows) in `cwd` (the current dir when `None`).
/// `quiet`: capture the output and show no console window (the app); otherwise the child inherits stdio (the CLI).
/// The returned evidence has no `at`/`by`/`summary`; a signal kill counts as exit code -1.
pub fn run_check(cmd: &str, cwd: Option<&Path>, quiet: bool) -> Result<Evidence> {
    let mut c = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", cmd]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", cmd]);
        c
    };
    #[cfg(windows)]
    if quiet {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    let start = std::time::Instant::now();
    let io = |e: std::io::Error| Error::Io(std::io::Error::new(e.kind(), format!("cannot run `{cmd}`: {e}")));
    let code = if quiet { c.output().map_err(io)?.status.code() } else { c.status().map_err(io)?.code() };
    Ok(Evidence {
        command: cmd.into(),
        exit_code: code.unwrap_or(-1),
        duration_ms: Some(u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)),
        summary: None,
        log: None,
        commit: None,
        branch: None,
        at: String::new(),
        by: String::new(),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn exit_codes() {
        assert_eq!(super::run_check("exit 0", None, true).unwrap().exit_code, 0);
        assert_eq!(super::run_check("exit 3", None, true).unwrap().exit_code, 3);
    }
}
