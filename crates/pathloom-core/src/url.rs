//! One place that decides which links and files Pathloom may store or hand to the OS to open.

use crate::error::{Error, Result};

const MAX_URL: usize = 2048;

/// Schemes that start another program or leak data on Windows/macOS (never allowed, even as a "custom scheme").
const DENIED_SCHEMES: [&str; 26] = [
    "javascript", "data", "vbscript", "file", "blob", "ms-msdt", "search-ms", "search", "ms-officecmd", "ms-word", "ms-excel",
    "ms-powerpoint", "ms-settings", "shell", "ms-appinstaller", "mk", "its", "mhtml", "res", "hcp", "ms-cxh", "ms-cxh-full",
    "ldap", "telnet", "about", "vscode",
];

/// Extensions the OS would run (or that carry code) when "opened": never opened from a plan.
const EXECUTABLE_EXT: [&str; 34] = [
    "exe", "bat", "cmd", "com", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "msi", "msc", "scr", "lnk", "url", "reg",
    "hta", "cpl", "jar", "sh", "app", "command", "pif", "chm", "appref-ms", "msix", "appx", "py", "pyw", "scf", "library-ms",
    "settingcontent-ms",
];

fn bad(u: &str, what: &str) -> Error {
    Error::InvalidSpec(format!("link `{u}` {what}"))
}

fn clean(u: &str) -> Result<()> {
    if u.len() > MAX_URL || u.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(bad(u, "must be one line without spaces, up to 2048 characters"));
    }
    Ok(())
}

/// `http://` or `https://` links only (artifacts, evidence logs).
pub fn check_http(u: &str) -> Result<()> {
    clean(u)?;
    let lower = u.to_ascii_lowercase();
    let rest = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://"));
    if rest.is_none_or(str::is_empty) {
        return Err(bad(u, "must be an http(s) link"));
    }
    Ok(())
}

/// Session links: `http(s)://` or a client's own scheme (`app://...`), never a denied scheme.
pub fn check_session(u: &str) -> Result<()> {
    clean(u)?;
    let err = || bad(u, "must be http(s):// or a custom scheme like app://");
    let (scheme, rest) = u.split_once("://").ok_or_else(err)?;
    let mut cs = scheme.chars();
    let ok = cs.next().is_some_and(|c| c.is_ascii_alphabetic())
        && cs.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'));
    if !ok || rest.is_empty() {
        return Err(err());
    }
    if DENIED_SCHEMES.contains(&scheme.to_ascii_lowercase().as_str()) {
        return Err(bad(u, "uses a scheme that is not allowed"));
    }
    Ok(())
}

/// The scheme of `u` (lowercase, whitespace and control characters ignored, so `java\tscript:` is `javascript`),
/// or `None` when `u` has none. A single letter is a drive (`C:`), not a scheme.
pub fn scheme_of(u: &str) -> Option<String> {
    let s: String = u.chars().filter(|c| !c.is_whitespace() && !c.is_control()).collect::<String>().to_ascii_lowercase();
    let (scheme, _) = s.split_once(':')?;
    let mut cs = scheme.chars();
    let ok = cs.next().is_some_and(|c| c.is_ascii_alphabetic()) && cs.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'));
    (ok && scheme.len() > 1).then(|| scheme.to_string())
}

/// True for files the OS would execute when opened (by extension, case-insensitive).
pub fn is_executable(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| EXECUTABLE_EXT.contains(&e.trim().to_ascii_lowercase().as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn http_only() {
        assert!(check_http("https://a.b/c").is_ok() && check_http("HTTP://a.b").is_ok());
        for u in ["javascript:alert(1)", "java\tscript:alert(1)", "ftp://x", "http://", "https://a b", "file:///x", "ms-msdt://x"] {
            assert!(check_http(u).is_err(), "{u}");
        }
    }

    #[test]
    fn session_denylist() {
        for ok in ["maestro://s/1", "https://a.b", "vscode-insiders://x"] {
            assert!(check_session(ok).is_ok(), "{ok}");
        }
        for u in ["ms-msdt://x", "SEARCH-MS://x", "ms-settings://x", "file://x", "java\tscript://x", "nope", "ldap://x", "a b://x", "vscode://x", "ms-appinstaller://x"] {
            assert!(check_session(u).is_err(), "{u}");
        }
    }

    #[test]
    fn scheme_strips_control() {
        assert_eq!(scheme_of("java\tscript:alert(1)").as_deref(), Some("javascript"));
        assert_eq!(scheme_of("C:\\x"), None);
        assert_eq!(scheme_of("assets/a.png"), None);
    }

    #[test]
    fn executables() {
        for p in ["a.exe", "a.BAT", "dir/x.Ps1", "x.lnk", "x.url", "x.sh", "h.chm", "x.appref-ms", "x.msix", "x.py", "x.scf", "x.library-ms", "x.settingcontent-ms"] {
            assert!(is_executable(Path::new(p)), "{p}");
        }
        assert!(!is_executable(Path::new("notes.md")) && !is_executable(Path::new("exe")));
    }
}
