//! `pathloom://open?project=...&stage=KEY&task=ID` links (m10 §Б7). The shell only validates a link and hands the target
//! to the UI, which opens the project and selects the card.

use std::sync::Mutex;

use pathloom_core::link::{LinkTarget, make_link, parse_link};
use tauri::{AppHandle, Emitter, Manager, State};

use super::{AppState, CmdResult, with_project};

/// Startup links wait here until the UI asks (`deeplink_pending`); after that they are only emitted.
#[derive(Default)]
struct Queue {
    ui_ready: bool,
    pending: Option<LinkTarget>,
}

static QUEUE: Mutex<Queue> = Mutex::new(Queue { ui_ready: false, pending: None });

/// Validate the links (invalid ones are dropped), focus the window and pass the last valid target to the UI.
pub fn handle<S: AsRef<str>>(app: &AppHandle, urls: &[S]) {
    let Some(target) = urls.iter().rev().find_map(|u| parse_link(u.as_ref()).ok()) else { return };
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    let emit = match QUEUE.lock() {
        Ok(mut q) if !q.ui_ready => {
            q.pending = Some(target.clone());
            false
        }
        _ => true,
    };
    if emit {
        let _ = app.emit("deep-link", target);
    }
}

/// The link the app was started with, if any; marks the UI as listening.
#[tauri::command]
pub fn deeplink_pending() -> Option<LinkTarget> {
    let mut q = QUEUE.lock().ok()?;
    q.ui_ready = true;
    q.pending.take()
}

/// `pathloom://` link to the open project, a stage key or a task id.
#[tauri::command]
pub fn link_make(state: State<'_, AppState>, id: Option<String>) -> CmdResult<String> {
    with_project(&state, |p| make_link(&p.root, id.as_deref()))
}
