//! `pathloom telegram`: a long-running bridge that sends pending approvals to a Telegram chat and records the answers.
//!
//! The bot token comes only from `PATHLOOM_TELEGRAM_TOKEN`; it is never printed or logged (see [`sanitize`]).
//! Blocking HTTP, one thread: `getUpdates` long polling doubles as the loop's heartbeat.

use crate::ops::{self, ApiError, ApiResult};
use pathloom_core::{ApprovalState, ApprovalView, Project, write_file_atomic};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::Duration;

const TOKEN_ENV: &str = "PATHLOOM_TELEGRAM_TOKEN";
const POLL_SECS: u64 = 25;
/// Reason chips shown after the reject button: (callback key, label). `none` means "no reason".
const REASONS: [(&str, &str); 6] = [
    ("off-theme", "off-theme"),
    ("duplicate", "duplicate"),
    ("quality", "quality"),
    ("scale", "scale"),
    ("wrong-place", "wrong-place"),
    ("none", "no reason"),
];

/// Parsed inline-button callback.
#[derive(Debug, PartialEq, Eq)]
enum Cb {
    Approve(String),
    Reject(String),
    Reason(String, String),
}

impl Cb {
    /// `a:<id>` / `r:<id>` / `x:<id>:<reason>`; `None` when over Telegram's 64-byte limit.
    fn format(&self) -> Option<String> {
        let s = match self {
            Cb::Approve(id) => format!("a:{id}"),
            Cb::Reject(id) => format!("r:{id}"),
            Cb::Reason(id, r) => format!("x:{id}:{r}"),
        };
        (s.len() <= 64).then_some(s)
    }

    fn parse(s: &str) -> Option<Cb> {
        let mut it = s.splitn(3, ':');
        let (kind, id) = (it.next()?, it.next()?.to_string());
        if id.is_empty() {
            return None;
        }
        match (kind, it.next()) {
            ("a", None) => Some(Cb::Approve(id)),
            ("r", None) => Some(Cb::Reject(id)),
            ("x", Some(r)) if !r.is_empty() => Some(Cb::Reason(id, r.into())),
            _ => None,
        }
    }
}

/// Remove the token from text that may contain a request URL.
fn sanitize(text: &str, token: &str) -> String {
    if token.is_empty() { text.to_string() } else { text.replace(token, "***") }
}

/// `.pathloom/telegram.json`: update offset and which approvals were already sent (`task_id -> requested_at`).
#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    offset: i64,
    #[serde(default)]
    sent: BTreeMap<String, String>,
    /// Local date (`YYYY-MM-DD`) the daily digest was last sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    digest_sent: Option<String>,
}

/// Send the digest now? Once per local day, at or after `cfg` (`HH:MM`); a malformed `cfg` never sends.
fn should_send_digest(now_hhmm: &str, today: &str, last_sent: Option<&str>, cfg: Option<&str>) -> bool {
    let ok = |t: &str| {
        t.len() == 5 && t.as_bytes()[2] == b':' && matches!((t[..2].parse::<u8>(), t[3..].parse::<u8>()), (Ok(h), Ok(m)) if h < 24 && m < 60)
    };
    // Zero-padded HH:MM compares in time order.
    cfg.is_some_and(|c| ok(c) && ok(now_hhmm) && now_hhmm >= c) && last_sent != Some(today)
}

fn send_digest(bot: &Bot, p: &Project, chat: &str, st: &mut State) -> Result<(), String> {
    let now = chrono::Local::now();
    let cfg = p.config.telegram.as_ref().and_then(|t| t.digest.as_deref());
    let today = now.format("%Y-%m-%d").to_string();
    if !should_send_digest(&now.format("%H:%M").to_string(), &today, st.digest_sent.as_deref(), cfg) {
        return Ok(());
    }
    let v = crate::agenda::digest(p, None).map_err(|e| e.message)?;
    let text = v["markdown"].as_str().unwrap_or_default();
    bot.call("sendMessage", &json!({"chat_id": chat, "text": text}), 30)?;
    st.digest_sent = Some(today);
    st.save(p).map_err(|e| e.to_string())
}

impl State {
    fn path(p: &Project) -> std::path::PathBuf {
        p.root.join(".pathloom/telegram.json")
    }

    fn load(p: &Project) -> State {
        std::fs::read_to_string(Self::path(p)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    fn save(&self, p: &Project) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        write_file_atomic(&Self::path(p), text.as_bytes())
    }
}

/// Pending approvals not yet sent for their current request (a re-request has a new `requested_at`).
fn to_send<'a>(pending: &'a [ApprovalView], st: &State) -> Vec<&'a ApprovalView> {
    pending.iter().filter(|a| st.sent.get(&a.task_id) != Some(&a.requested_at)).collect()
}

fn is_image(f: &str) -> bool {
    let f = f.split(['?', '#']).next().unwrap_or(f).to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif"].iter().any(|e| f.ends_with(e))
}

fn card_text(a: &ApprovalView) -> String {
    let note = if a.note.is_empty() { String::new() } else { format!("\n{}", a.note) };
    format!("{} {}{note}", a.task_id, a.title)
}

fn multipart(boundary: &str, fields: &[(&str, &str)], file: (&str, &str, &[u8])) -> Vec<u8> {
    let mut b = Vec::new();
    for (k, v) in fields {
        b.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n").bytes());
    }
    let (field, name, bytes) = file;
    let name = name.replace(['"', '\r', '\n'], "_");
    b.extend(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .bytes(),
    );
    b.extend(bytes);
    b.extend(format!("\r\n--{boundary}--\r\n").bytes());
    b
}

struct Bot {
    token: String,
}

impl Bot {
    fn url(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{method}", self.token)
    }

    fn finish(&self, r: attohttpc::Result<attohttpc::Response>) -> Result<Value, String> {
        let body = r.and_then(attohttpc::Response::text).map_err(|e| sanitize(&e.to_string(), &self.token))?;
        let v: Value = serde_json::from_str(&body).map_err(|_| "telegram: reply is not JSON".to_string())?;
        if v["ok"].as_bool() == Some(true) {
            Ok(v["result"].clone())
        } else {
            Err(sanitize(&format!("telegram: {}", v["description"].as_str().unwrap_or("request failed")), &self.token))
        }
    }

    fn call(&self, method: &str, params: &Value, timeout: u64) -> Result<Value, String> {
        let r = attohttpc::post(self.url(method))
            .timeout(Duration::from_secs(timeout))
            .header("Content-Type", "application/json")
            .bytes(params.to_string())
            .send();
        self.finish(r)
    }

    fn upload_photo(&self, chat_id: &str, name: &str, bytes: &[u8]) -> Result<(), String> {
        let boundary = "pathloomBoundary9c1e7a";
        let body = multipart(boundary, &[("chat_id", chat_id)], ("photo", name, bytes));
        let r = attohttpc::post(self.url("sendPhoto"))
            .timeout(Duration::from_secs(60))
            .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
            .bytes(body)
            .send();
        self.finish(r).map(|_| ())
    }
}

/// Inline keyboard from rows of (label, callback data); buttons whose data is too long are dropped.
fn keyboard(rows: &[Vec<(String, Option<String>)>]) -> Value {
    let rows: Vec<Vec<Value>> = rows
        .iter()
        .map(|r| r.iter().filter_map(|(t, d)| d.as_ref().map(|d| json!({"text": t, "callback_data": d}))).collect())
        .collect();
    json!({"inline_keyboard": rows})
}

fn main_keyboard(id: &str) -> Value {
    keyboard(&[vec![("✅".into(), Cb::Approve(id.into()).format()), ("❌".into(), Cb::Reject(id.into()).format())]])
}

fn reason_keyboard(id: &str) -> Value {
    let rows: Vec<Vec<(String, Option<String>)>> = REASONS
        .chunks(2)
        .map(|c| c.iter().map(|(k, label)| ((*label).to_string(), Cb::Reason(id.into(), (*k).into()).format())).collect())
        .collect();
    keyboard(&rows)
}

fn send_previews(bot: &Bot, p: &Project, chat: &str, a: &ApprovalView) {
    for f in a.files.iter().filter(|f| is_image(f)) {
        let r = if f.starts_with("http://") || f.starts_with("https://") {
            bot.call("sendPhoto", &json!({"chat_id": chat, "photo": f}), 30).map(|_| ())
        } else {
            // artifact_path rejects anything that resolves outside the project root.
            p.artifact_path(f).map_err(|e| e.to_string()).and_then(|path| {
                let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("image");
                bot.upload_photo(chat, name, &bytes)
            })
        };
        if let Err(e) = r {
            eprintln!("telegram: preview {f} for {} skipped: {e}", a.task_id);
        }
    }
}

fn send_new(bot: &Bot, p: &Project, chat: &str, st: &mut State) -> Result<(), String> {
    let pending = p.approvals(Some(ApprovalState::Pending)).map_err(|e| e.to_string())?;
    for a in to_send(&pending, st) {
        if Cb::Reason(a.task_id.clone(), "wrong-place".into()).format().is_none() {
            eprintln!("telegram: task id {} is too long for a button, skipped", a.task_id);
            continue;
        }
        send_previews(bot, p, chat, a);
        let msg = json!({"chat_id": chat, "text": card_text(a), "reply_markup": main_keyboard(&a.task_id)});
        bot.call("sendMessage", &msg, 30)?;
        st.sent.insert(a.task_id.clone(), a.requested_at.clone());
        st.save(p).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Telegram username (or first name / id), reduced to characters safe in an author field.
fn who(from: &Value) -> String {
    let raw = from["username"].as_str().or(from["first_name"].as_str()).map_or_else(|| from["id"].to_string(), String::from);
    let s: String = raw.chars().filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c)).take(32).collect();
    format!("telegram:{}", if s.is_empty() { "user" } else { &s })
}

fn decide(p: &Project, task_id: &str, approve: bool, reasons: &[String], by: &str) -> ApiResult<()> {
    let s = p.decide_approval(task_id, approve, reasons, None, by)?;
    let state = if approve { "approved" } else { "rejected" };
    let action = if approve { "approval approve" } else { "approval reject" };
    let mut e = ops::entry(action, format!("{state} {task_id}"), vec![s.key.clone()], vec![task_id.into()], vec![]);
    e.author = by.into();
    e.source = "telegram".into();
    if let Err(err) = p.log_action(&e) {
        eprintln!("telegram: activity log not written: {err}");
    }
    Ok(())
}

fn handle_callback(bot: &Bot, p: &Project, chat: &str, cq: &Value) -> Result<(), String> {
    let (cid, data) = (cq["id"].as_str().unwrap_or(""), cq["data"].as_str().unwrap_or(""));
    if cq["message"]["chat"]["id"].as_i64().map(|i| i.to_string()).as_deref() != Some(chat) {
        return Ok(()); // not our chat: ignore silently
    }
    let mid = &cq["message"]["message_id"];
    let answer = |text: &str| bot.call("answerCallbackQuery", &json!({"callback_query_id": cid, "text": text}), 30).map(|_| ());
    let Some(cb) = Cb::parse(data) else { return answer("unknown button") };
    let (id, approve, reasons) = match &cb {
        Cb::Approve(id) => (id, true, vec![]),
        Cb::Reject(id) => (id, false, vec![]),
        Cb::Reason(id, r) => (id, false, if r == "none" { vec![] } else { vec![r.clone()] }),
    };
    let all = p.approvals(None).map_err(|e| e.to_string())?;
    let Some(view) = all.iter().find(|a| &a.task_id == id) else { return answer("task not found") };
    let edit_text = |extra: &str| {
        let text = format!("{}\n\n{extra}", card_text(view));
        bot.call("editMessageText", &json!({"chat_id": chat, "message_id": mid, "text": text}), 30).map(|_| ())
    };
    if view.state != ApprovalState::Pending {
        answer("already decided")?;
        return edit_text(&format!("already decided ({})", format!("{:?}", view.state).to_lowercase()));
    }
    if matches!(cb, Cb::Reject(_)) {
        answer("pick a reason")?;
        let markup = json!({"chat_id": chat, "message_id": mid, "reply_markup": reason_keyboard(id)});
        return bot.call("editMessageReplyMarkup", &markup, 30).map(|_| ());
    }
    let by = who(&cq["from"]);
    match decide(p, id, approve, &reasons, &by) {
        Ok(()) => {
            let result = if approve {
                "✅ approved".to_string()
            } else if reasons.is_empty() {
                "❌ rejected".to_string()
            } else {
                format!("❌ rejected [{}]", reasons.join(", "))
            };
            answer(&result)?;
            edit_text(&format!("{result} by {by}"))
        }
        Err(e) => answer(&e.message.chars().take(150).collect::<String>()),
    }
}

/// Run the bridge until killed.
pub fn run(p: &Project) -> ApiResult<()> {
    let token = std::env::var(TOKEN_ENV).unwrap_or_default().trim().to_string();
    if token.is_empty() {
        return Err(ApiError::new("INVALID_SPEC", format!("set {TOKEN_ENV} to the bot token from @BotFather")));
    }
    let Some(cfg) = p.config.telegram.as_ref() else {
        return Err(ApiError::new("INVALID_SPEC", "add `[telegram]` with `chat_id = \"...\"` to .pathloom/project.toml"));
    };
    let (chat, bot, mut st) = (cfg.chat_id.clone(), Bot { token }, State::load(p));
    eprintln!("telegram: bridge started for chat {chat}; Ctrl+C to stop");
    loop {
        if let Err(e) = send_new(&bot, p, &chat, &mut st) {
            eprintln!("telegram: {e}");
        }
        if let Err(e) = send_digest(&bot, p, &chat, &mut st) {
            eprintln!("telegram: digest: {e}");
        }
        let params = json!({"offset": st.offset, "timeout": POLL_SECS, "allowed_updates": ["callback_query"]});
        match bot.call("getUpdates", &params, POLL_SECS + 15) {
            Ok(v) => {
                for u in v.as_array().into_iter().flatten() {
                    st.offset = st.offset.max(u["update_id"].as_i64().unwrap_or(0) + 1);
                    if u["callback_query"].is_object()
                        && let Err(e) = handle_callback(&bot, p, &chat, &u["callback_query"])
                    {
                        eprintln!("telegram: {e}");
                    }
                }
                if let Err(e) = st.save(p) {
                    eprintln!("telegram: cannot save state: {e}");
                }
            }
            Err(e) => {
                eprintln!("telegram: {e}");
                std::thread::sleep(Duration::from_secs(5));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(id: &str, at: &str) -> ApprovalView {
        ApprovalView {
            task_id: id.into(),
            stage: "s".into(),
            title: "T".into(),
            state: ApprovalState::Pending,
            files: vec![],
            note: String::new(),
            reasons: vec![],
            comment: String::new(),
            at: String::new(),
            by: String::new(),
            requested_by: String::new(),
            requested_at: at.into(),
        }
    }

    #[test]
    fn callback_roundtrip() {
        for cb in [Cb::Approve("t1".into()), Cb::Reject("t1".into()), Cb::Reason("t1".into(), "off-theme".into())] {
            assert_eq!(Cb::parse(&cb.format().unwrap()), Some(cb));
        }
        assert_eq!(Cb::Reason("t1".into(), "quality".into()).format().unwrap(), "x:t1:quality");
        for bad in ["", "a", "a:", "z:t1", "x:t1", "x:t1:", "a:t1:extra"] {
            assert_eq!(Cb::parse(bad), None, "{bad}");
        }
        assert!(Cb::Approve("a".repeat(63)).format().is_none());
        assert!(REASONS.iter().all(|(k, _)| Cb::Reason("a".repeat(20), (*k).into()).format().is_some()));
    }

    #[test]
    fn sanitizer_hides_token() {
        let t = "123:SECRET";
        let s = sanitize(&format!("error sending https://api.telegram.org/bot{t}/getUpdates: timed out"), t);
        assert!(!s.contains("SECRET") && s.contains("bot***/getUpdates"));
        assert_eq!(sanitize("plain", ""), "plain");
    }

    #[test]
    fn state_roundtrip_and_selection() {
        let dir = tempfile::tempdir().unwrap();
        let p = Project::init(dir.path(), "p").unwrap();
        assert_eq!(State::load(&p), State::default());
        let mut st = State { offset: 7, ..Default::default() };
        st.sent.insert("t1".into(), "2026-01-01T00:00:00Z".into());
        st.save(&p).unwrap();
        assert_eq!(State::load(&p), st);

        let pending = vec![view("t1", "2026-01-01T00:00:00Z"), view("t2", "x"), view("t1b", "y")];
        let ids: Vec<_> = to_send(&pending, &st).iter().map(|a| a.task_id.as_str()).collect();
        assert_eq!(ids, ["t2", "t1b"]);
        // Same task requested again later is sent again.
        let again = vec![view("t1", "2026-02-02T00:00:00Z")];
        assert_eq!(to_send(&again, &st).len(), 1);
    }

    #[test]
    fn digest_schedule() {
        let s = should_send_digest;
        assert!(!s("08:59", "2026-11-20", None, Some("09:00")));
        assert!(s("09:00", "2026-11-20", None, Some("09:00")));
        assert!(s("23:30", "2026-11-20", Some("2026-11-19"), Some("09:00"))); // late start still sends
        assert!(!s("10:00", "2026-11-20", Some("2026-11-20"), Some("09:00"))); // once a day
        assert!(!s("10:00", "2026-11-20", None, None));
        for bad in ["9:00", "24:00", "09:60", "ab:cd", ""] {
            assert!(!s("10:00", "2026-11-20", None, Some(bad)), "{bad}");
        }
    }

    #[test]
    fn old_state_without_digest_field_loads() {
        let st: State = serde_json::from_str(r#"{"offset":3,"sent":{"t":"x"}}"#).unwrap();
        assert_eq!((st.offset, st.digest_sent.as_deref()), (3, None));
        assert!(!serde_json::to_string(&st).unwrap().contains("digest_sent"));
    }

    #[test]
    fn image_detection_and_multipart() {
        assert!(is_image("a/B.PNG") && is_image("https://x/y.jpg?v=1") && !is_image("doc.pdf"));
        let b = String::from_utf8(multipart("BB", &[("chat_id", "1")], ("photo", "a\"b.png", b"DATA"))).unwrap();
        assert!(b.contains("name=\"chat_id\"\r\n\r\n1\r\n") && b.contains("filename=\"a_b.png\"") && b.ends_with("--BB--\r\n"));
    }

    #[test]
    fn who_is_sanitized() {
        assert_eq!(who(&json!({"username": "Bob_1", "id": 5})), "telegram:Bob_1");
        assert_eq!(who(&json!({"first_name": "a b;c"})), "telegram:abc");
        assert_eq!(who(&json!({"id": 42})), "telegram:42");
    }
}
