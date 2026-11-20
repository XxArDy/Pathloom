//! Links to external client sessions (Maestro, Codex, ...): `{client, id, url?, title?, state?, state_at?}`
//! (m7 contract §4, m10 §Б1).

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// Most sessions kept per stage; older ones are dropped.
pub const MAX_STAGE_SESSIONS: usize = 20;

/// Reference to a session of an external client that worked on a stage or claimed a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionRef {
    /// `[a-z0-9._-]{1,32}`
    pub client: String,
    /// Up to 128 printable characters.
    pub id: String,
    /// `http(s)://` or a custom scheme `app://`; never `javascript:`, `data:` or `file:`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Up to 120 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Live state published by the client (m10 §Б1): working | waiting | blocked | error | done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<SessionState>,
    /// When `state` was last set (RFC3339 UTC). Set by Pathloom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_at: Option<String>,
}

/// What the session is doing right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SessionState {
    Working,
    Waiting,
    Blocked,
    Error,
    Done,
}

impl std::str::FromStr for SessionState {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        serde_json::from_value(serde_json::Value::String(s.trim().to_lowercase()))
            .map_err(|_| bad(format!("session state `{s}` must be working|waiting|blocked|error|done")))
    }
}

fn bad(msg: impl Into<String>) -> Error {
    Error::InvalidSpec(msg.into())
}

fn clean(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

impl SessionRef {
    /// Trim, drop empty `url`/`title` and validate.
    pub fn normalize(self) -> Result<SessionRef> {
        let s = SessionRef {
            client: self.client.trim().to_string(),
            id: self.id.trim().to_string(),
            url: clean(self.url),
            title: clean(self.title),
            state: self.state,
            state_at: clean(self.state_at),
        };
        s.validate()?;
        Ok(s)
    }

    pub fn validate(&self) -> Result<()> {
        let c = &self.client;
        if !(1..=32).contains(&c.chars().count()) || !c.chars().all(|x| matches!(x, 'a'..='z' | '0'..='9' | '.' | '_' | '-')) {
            return Err(bad(format!("session client `{c}` must be 1-32 chars of [a-z0-9._-]")));
        }
        if !(1..=128).contains(&self.id.chars().count()) || self.id.chars().any(char::is_control) {
            return Err(bad("session id must be 1-128 printable characters"));
        }
        if let Some(t) = &self.title
            && (t.chars().count() > 120 || t.chars().any(char::is_control))
        {
            return Err(bad("session title must be at most 120 characters on one line"));
        }
        if let Some(u) = &self.url {
            crate::url::check_session(u)?;
        }
        if self.state_at.as_ref().is_some_and(|t| t.len() > 40 || t.chars().any(char::is_control)) {
            return Err(bad("session state_at must be an RFC3339 time"));
        }
        Ok(())
    }

    fn same(&self, o: &SessionRef) -> bool {
        self.client == o.client && self.id == o.id
    }
}

/// Add `s` to `list` (dedup by client+id: the old entry is replaced and moves to the end, keeping its
/// `url`/`title`/`state` when `s` has none). Keeps only the newest [`MAX_STAGE_SESSIONS`].
/// A given `state` gets `state_at` = now. Returns whether the state value changed
/// (a new state on a new entry counts as a change).
pub(crate) fn push_session(list: &mut Vec<SessionRef>, mut s: SessionRef) -> bool {
    let mut old_state = None;
    if let Some(i) = list.iter().position(|x| x.same(&s)) {
        let old = list.remove(i);
        old_state = old.state;
        s.url = s.url.or(old.url);
        s.title = s.title.or(old.title);
        if s.state.is_none() {
            (s.state, s.state_at) = (old.state, old.state_at);
        }
    }
    if s.state.is_some() {
        s.state_at = Some(crate::fsutil::now_rfc3339());
    }
    let changed = s.state.is_some() && s.state != old_state;
    list.push(s);
    if list.len() > MAX_STAGE_SESSIONS {
        list.drain(..list.len() - MAX_STAGE_SESSIONS);
    }
    changed
}

impl crate::project::Project {
    /// Upsert `session` into the `sessions` of stage `key` (m10 §Б1). Returns whether the state value changed.
    pub fn update_session(&self, key: &str, session: SessionRef) -> Result<bool> {
        let session = session.normalize()?;
        let mut changed = false;
        self.edit_stage(key, |st| {
            changed = push_session(&mut st.sessions, session);
            Ok(())
        })?;
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(client: &str, id: &str, url: Option<&str>) -> SessionRef {
        SessionRef { client: client.into(), id: id.into(), url: url.map(Into::into), title: None, state: None, state_at: None }
    }

    #[test]
    fn validation() {
        for ok in [Some("maestro://session/x"), Some("https://a.b/c"), Some("vscode-insiders://x"), None] {
            assert!(s("maestro", "sess_1", ok).normalize().is_ok(), "{ok:?}");
        }
        for url in ["javascript:alert(1)", "data:text/html,x", "file:///etc/passwd", "FILE://x", "nope", "://x", "a b://x", "http://", "1ab://x", "http://a b"] {
            assert!(s("maestro", "1", Some(url)).normalize().is_err(), "{url}");
        }
        for c in ["", "Maestro", "a b", &"a".repeat(33)] {
            assert!(s(c, "1", None).normalize().is_err(), "{c}");
        }
        assert!(s("m", "", None).normalize().is_err());
        assert!(s("m", &"x".repeat(129), None).normalize().is_err());
        assert!(s("m", "a\nb", None).normalize().is_err());
        let mut t = s("m", "1", None);
        t.title = Some("x".repeat(121));
        assert!(t.normalize().is_err());
        let n = SessionRef { client: " m ".into(), id: " 1 ".into(), url: Some(" ".into()), title: Some(" T ".into()), state: None, state_at: None }.normalize().unwrap();
        assert_eq!((n.client.as_str(), n.id.as_str(), n.url, n.title.as_deref()), ("m", "1", None, Some("T")));
    }

    #[test]
    fn state_round_trip_and_old_json() {
        let old: SessionRef = serde_json::from_str(r#"{"client":"m","id":"1"}"#).unwrap();
        assert_eq!((old.state, old.state_at.as_deref()), (None, None));
        assert_eq!(serde_json::to_string(&old).unwrap(), r#"{"client":"m","id":"1"}"#);
        let mut n = s("m", "1", None);
        n.state = Some("Blocked".parse().unwrap());
        let back: SessionRef = serde_json::from_str(&serde_json::to_string(&n).unwrap()).unwrap();
        assert_eq!(back.state, Some(SessionState::Blocked));
        assert!("busy".parse::<SessionState>().is_err());
        assert!(serde_json::from_str::<SessionRef>(r#"{"client":"m","id":"1","state":"busy"}"#).is_err());
    }

    #[test]
    fn push_keeps_old_state_and_reports_change() {
        let mut l = vec![];
        let with = |st| SessionRef { state: Some(st), ..s("m", "1", None) };
        assert!(push_session(&mut l, with(SessionState::Working)));
        assert!(l[0].state_at.is_some());
        assert!(!push_session(&mut l, with(SessionState::Working)));
        assert!(!push_session(&mut l, s("m", "1", None)), "no state given: nothing changes");
        assert_eq!(l[0].state, Some(SessionState::Working));
        assert!(push_session(&mut l, with(SessionState::Done)));
    }

    #[test]
    fn push_dedups_and_caps() {
        let mut l = vec![];
        push_session(&mut l, s("m", "1", Some("m://1")));
        push_session(&mut l, s("m", "2", None));
        push_session(&mut l, s("m", "1", None));
        assert_eq!(l.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["2", "1"]);
        assert_eq!(l[1].url.as_deref(), Some("m://1"));
        for i in 0..30 {
            push_session(&mut l, s("m", &format!("n{i}"), None));
        }
        assert_eq!(l.len(), MAX_STAGE_SESSIONS);
        assert_eq!(l.last().map(|x| x.id.as_str()), Some("n29"));
    }
}
