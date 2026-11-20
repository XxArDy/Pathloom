//! Spawns `pathloom mcp` and talks newline-delimited JSON-RPC over stdio.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

struct Client {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Value>,
    next_id: u64,
}

impl Client {
    fn start(dir: &std::path::Path, extra: &[&str]) -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_pathloom"))
            .arg("--project")
            .arg(dir)
            .arg("mcp")
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let out = child.stdout.take().unwrap();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    let _ = tx.send(v);
                }
            }
        });
        Client { child, stdin, rx, next_id: 1 }
    }

    fn send(&mut self, v: &Value) {
        writeln!(self.stdin, "{v}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let v = self.rx.recv_timeout(Duration::from_secs(20)).expect("no response from server");
            if v["id"] == json!(id) {
                return v;
            }
        }
    }

    fn handshake(&mut self) -> Value {
        let r = self.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test-client", "version": "1"}}),
        );
        self.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        r
    }

    fn call(&mut self, name: &str, args: Value) -> (bool, Value) {
        let r = self.request("tools/call", json!({"name": name, "arguments": args}));
        let res = &r["result"];
        let text = res["content"][0]["text"].as_str().unwrap_or("null");
        (res["isError"] == json!(true), serde_json::from_str(text).unwrap_or(Value::Null))
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn project() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    pathloom_core_init(d.path());
    d
}

fn pathloom_core_init(p: &std::path::Path) {
    assert!(Command::new(env!("CARGO_BIN_EXE_pathloom")).arg("--project").arg(p).args(["init", "--name", "demo"]).status().unwrap().success());
}

#[test]
fn initialize_list_tools_and_call() {
    let d = project();
    let mut c = Client::start(d.path(), &[]);
    let init = c.handshake();
    assert_eq!(init["result"]["serverInfo"]["name"], "pathloom");
    assert!(init["result"]["capabilities"]["tools"].is_object());

    let tools = c.request("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for n in ["project_info", "list_stages", "get_stage", "next", "search", "apply", "set_task", "add_comment", "layout_board", "validate"] {
        assert!(names.contains(&n), "missing tool {n}: {names:?}");
    }

    let (err, v) = c.call("project_info", json!({}));
    assert!(!err);
    assert_eq!(v["name"], "demo");

    let spec = json!({"stages": [{"id": "A", "title": "Alpha", "tasks": ["one"]}, {"id": "B", "title": "Beta", "depends": ["A"]}]});
    let (err, v) = c.call("apply", json!({"spec": spec, "dry_run": true}));
    assert!(!err, "{v}");
    assert_eq!(v["mode"], "dry_run");
    assert!(!d.path().join("plan/stages/A.md").exists());
    let (err, v) = c.call("apply", json!({"spec": "stages: [{id: A, title: Alpha, tasks: [one]}]"}));
    assert!(!err, "{v}");
    assert_eq!(v["report"]["created"][0], "A");

    let (_, v) = c.call("get_stage", json!({"key": "A"}));
    assert_eq!(v["hash"].as_str().unwrap().len(), 64);
    let (err, v) = c.call("set_task", json!({"task_id": "A-01", "done": true}));
    assert!(!err);
    assert_eq!(v["stage"]["status"], "done");
    let (err, v) = c.call("add_comment", json!({"stage": "A", "text": "hi"}));
    assert!(!err);
    assert_eq!(v["author"], "agent:test-client");
    let (_, v) = c.call("search", json!({"query": "alp"}));
    assert_eq!(v["results"][0]["stage"], "A");
    let (_, v) = c.call("list_stages", json!({"status": "done"}));
    assert_eq!(v["total"], 1);
    let (_, v) = c.call("next", json!({}));
    assert_eq!(v["tasks"].as_array().unwrap().len(), 0);
    let (_, v) = c.call("validate", json!({}));
    assert_eq!(v["ok"], true);

    // structured errors
    let (err, v) = c.call("get_stage", json!({"key": "NOPE"}));
    assert!(err);
    assert_eq!(v["code"], "NOT_FOUND");
    let (err, v) = c.call("apply", json!({"spec": {"stages": [{"id": "A", "depends": ["A"]}]}}));
    assert!(err);
    assert_eq!(v["code"], "CYCLE");
    let (err, v) = c.call("apply", json!({"spec": {"bogus": 1}}));
    assert!(err);
    assert_eq!(v["code"], "INVALID_SPEC");

    // resources
    let r = c.request("resources/list", json!({}));
    assert_eq!(r["result"]["resources"].as_array().unwrap().len(), 4);
    let r = c.request("resources/read", json!({"uri": "pathloom://guide/workflow"}));
    assert!(r["result"]["contents"][0]["text"].as_str().unwrap().contains("Recommended workflow"));
    let r = c.request("resources/read", json!({"uri": "pathloom://schema/spec"}));
    assert!(r["result"]["contents"][0]["text"].as_str().unwrap().contains("tasks_done"));
}

#[test]
fn read_only_blocks_writes() {
    let d = project();
    let mut c = Client::start(d.path(), &["--read-only"]);
    c.handshake();
    let (err, v) = c.call("apply", json!({"spec": {"stages": [{"id": "A", "title": "A"}]}}));
    assert!(err);
    assert_eq!(v["code"], "READ_ONLY");
    let (err, v) = c.call("apply", json!({"spec": {"stages": [{"id": "A", "title": "A"}]}, "dry_run": true}));
    assert!(!err, "{v}");
    let (err, v) = c.call("set_task", json!({"task_id": "A-01", "done": true}));
    assert!(err);
    assert_eq!(v["code"], "READ_ONLY");
    let (err, _) = c.call("layout_board", json!({}));
    assert!(err);
    let (err, _) = c.call("project_info", json!({}));
    assert!(!err);
}

impl Client {
    fn handshake_as(&mut self, name: &str) {
        self.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": name, "version": "1"}}),
        );
        self.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    }
}

#[test]
fn two_agents_claim_conflict_and_activity() {
    let d = project();
    let mut a = Client::start(d.path(), &[]);
    a.handshake_as("Claude Code");
    let mut b = Client::start(d.path(), &[]);
    b.handshake_as("codex");

    let tools = a.request("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for n in ["claim_task", "release_task", "activity"] {
        assert!(names.contains(&n), "missing tool {n}");
    }

    let spec = json!({"stages": [{"id": "A", "title": "Alpha", "who": "ai:claude-code", "tasks": ["one", "two"]}]});
    let (err, v) = a.call("apply", json!({"spec": spec}));
    assert!(!err, "{v}");

    let (err, v) = a.call("claim_task", json!({"task_id": "A-01", "ttl_minutes": 5, "note": "mine"}));
    assert!(!err, "{v}");
    assert_eq!(v["agent"], "claude-code");

    // second agent: CLAIMED with details, and the task is hidden from `next`
    let (err, v) = b.call("claim_task", json!({"task_id": "A-01"}));
    assert!(err);
    assert_eq!(v["code"], "CLAIMED");
    assert_eq!(v["details"]["task_id"], "A-01");
    assert_eq!(v["details"]["by"], "claude-code");
    assert!(v["details"]["expires_at"].as_str().unwrap().ends_with('Z'));
    let (err, v) = b.call("release_task", json!({"task_id": "A-01"}));
    assert!(err);
    assert_eq!(v["code"], "CLAIMED");
    let (_, v) = b.call("next", json!({}));
    let ids: Vec<&str> = v["tasks"].as_array().unwrap().iter().map(|t| t["task_id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["A-02"]);
    // first agent sees its own claim first
    let (_, v) = a.call("next", json!({}));
    assert_eq!(v["tasks"][0]["task_id"], "A-01");
    assert_eq!(v["tasks"][0]["claimed_by"], "claude-code");
    let (_, v) = a.call("get_stage", json!({"key": "A"}));
    assert_eq!(v["claims"][0]["task_id"], "A-01");

    // done releases the claim
    let (err, _) = a.call("set_task", json!({"task_id": "A-01", "done": true}));
    assert!(!err);
    let (_, v) = a.call("get_stage", json!({"key": "A"}));
    assert_eq!(v["claims"].as_array().unwrap().len(), 0);
    let (err, v) = b.call("claim_task", json!({"task_id": "A-02"}));
    assert!(!err, "{v}");

    // activity shows both agents, newest first
    let (_, v) = b.call("activity", json!({}));
    let e = v["entries"].as_array().unwrap();
    let rows: Vec<(&str, &str, &str)> =
        e.iter().map(|x| (x["author"].as_str().unwrap(), x["source"].as_str().unwrap(), x["action"].as_str().unwrap())).collect();
    assert_eq!(
        rows,
        [("codex", "mcp", "claim_task"), ("claude-code", "mcp", "set_task"), ("claude-code", "mcp", "claim_task"), ("claude-code", "mcp", "apply")]
    );
    let (_, v) = b.call("activity", json!({"limit": 1, "stage": "A"}));
    assert_eq!(v["entries"].as_array().unwrap().len(), 1);
    let (_, v) = b.call("activity", json!({"stage": "NOPE"}));
    assert_eq!(v["entries"].as_array().unwrap().len(), 0);

    // guide explains the protocol
    let r = a.request("resources/read", json!({"uri": "pathloom://guide/workflow"}));
    let text = r["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(text.contains("claim_task") && text.contains("ai:<name>"));
}

#[test]
fn read_only_blocks_claims() {
    let d = project();
    let mut c = Client::start(d.path(), &["--read-only"]);
    c.handshake();
    let (err, v) = c.call("claim_task", json!({"task_id": "A-01"}));
    assert!(err);
    assert_eq!(v["code"], "READ_ONLY");
    let (err, _) = c.call("activity", json!({}));
    assert!(!err);
}

#[test]
fn project_info_protocol_capabilities_and_read_only() {
    let d = project();
    let mut c = Client::start(d.path(), &[]);
    c.handshake();
    let (_, v) = c.call("project_info", json!({}));
    assert_eq!(v["protocol"], "pathloom/1");
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(v["agent_changes"], "auto");
    assert_eq!(v["agents"], json!([]));
    let caps: Vec<&str> = v["capabilities"].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
    assert!(caps.contains(&"sessions") && caps.contains(&"agents") && caps.contains(&"events"), "{caps:?}");
    let mut ro = Client::start(d.path(), &["--read-only"]);
    ro.handshake();
    let (_, v) = ro.call("project_info", json!({}));
    assert_eq!(v["capabilities"], json!(["read", "activity", "events", "validate", "commits", "agenda", "digest", "export"]));
    let (err, v) = ro.call("list_commits", json!({}));
    assert!(!err, "{v}");
    assert!(v["commits"].is_array());
    let (err, v) = ro.call("set_task_status", json!({"task_id": "A-01", "status": "pushed"}));
    assert!(err && v["code"] == "READ_ONLY");
    let (err, v) = ro.call("register_agent", json!({"name": "X"}));
    assert!(err);
    assert_eq!(v["code"], "READ_ONLY");
    let (err, _) = ro.call("list_agents", json!({}));
    assert!(!err);
}

#[test]
fn register_agent_self_only_with_two_identities() {
    let d = project();
    let mut a = Client::start(d.path(), &[]);
    a.handshake_as("Claude Code");
    let mut b = Client::start(d.path(), &[]);
    b.handshake_as("maestro");
    let (err, v) = a.call("register_agent", json!({"name": "Claude", "color": "#d97757", "avatar": "🤖"}));
    assert!(!err, "{v}");
    assert_eq!(v["agent"]["id"], "claude-code");
    let (err, v) = b.call("register_agent", json!({"name": "Maestro", "color": "#3fb6f2", "kind": "orchestrator"}));
    assert!(!err, "{v}");
    assert_eq!(v["agent"]["id"], "maestro");
    // a second call merges: only the given fields change
    let (_, v) = a.call("register_agent", json!({"color": "#fff"}));
    let agent = &v["agent"];
    assert_eq!((agent["name"].as_str(), agent["color"].as_str(), agent["avatar"].as_str()), (Some("Claude"), Some("#fff"), Some("🤖")));
    // invalid input is rejected and changes nothing
    let (err, v) = a.call("register_agent", json!({"color": "red"}));
    assert!(err);
    assert_eq!(v["code"], "INVALID_SPEC");
    let (_, v) = b.call("list_agents", json!({}));
    let ids: Vec<&str> = v["agents"].as_array().unwrap().iter().map(|x| x["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["claude-code", "maestro"]);
    assert_eq!(v["agents"][0]["color"], "#fff");
    let (_, v) = b.call("project_info", json!({}));
    assert_eq!(v["agents"].as_array().unwrap().len(), 2);
    let (_, v) = b.call("activity", json!({}));
    assert_eq!(v["entries"][0]["action"], "register_agent");
}

#[test]
fn claim_with_session_is_logged_and_linked() {
    let d = project();
    let mut c = Client::start(d.path(), &[]);
    c.handshake_as("maestro");
    c.call("apply", json!({"spec": "stages: [{id: A, title: Alpha, tasks: [one]}]"}));
    let session = json!({"client": "maestro", "id": "sess_8f2", "url": "maestro://session/sess_8f2", "title": "Implement auth"});
    let (err, v) = c.call("claim_task", json!({"task_id": "A-01", "session": session}));
    assert!(!err, "{v}");
    assert_eq!(v["session"]["id"], "sess_8f2");
    let (_, v) = c.call("get_stage", json!({"key": "A"}));
    assert_eq!(v["sessions"][0]["client"], "maestro");
    assert_eq!(v["claims"][0]["session"]["id"], "sess_8f2");
    let (_, v) = c.call("activity", json!({"limit": 1}));
    assert_eq!(v["entries"][0]["session"]["url"], "maestro://session/sess_8f2");
    // another mutating tool accepts a session too; a bad one is rejected before anything changes
    let (err, _) = c.call("set_task", json!({"task_id": "A-01", "done": true, "session": {"client": "maestro", "id": "s2"}}));
    assert!(!err);
    let (_, v) = c.call("activity", json!({"limit": 1}));
    assert_eq!(v["entries"][0]["session"]["id"], "s2");
    let (err, v) = c.call("add_comment", json!({"stage": "A", "text": "x", "session": {"client": "m", "id": "1", "url": "javascript://x"}}));
    assert!(err);
    assert_eq!(v["code"], "INVALID_SPEC");
}

#[test]
fn apply_propose_then_list_and_decide() {
    let d = project();
    let mut c = Client::start(d.path(), &[]);
    c.handshake();
    let spec = json!({"stages": [{"id": "A", "title": "Alpha", "tasks": ["one"]}]});
    let (err, v) = c.call("apply", json!({"spec": spec, "propose": true}));
    assert!(!err, "{v}");
    assert_eq!(v["mode"], "proposed");
    let id = v["proposal_id"].as_str().unwrap().to_string();
    assert!(!d.path().join("plan/stages/A.md").exists());

    let (_, v) = c.call("list_proposals", json!({}));
    assert_eq!(v["proposals"][0]["id"], id.as_str());
    assert_eq!(v["proposals"][0]["changes"][0]["path"], "plan/stages/A.md");
    assert!(v["proposals"][0]["decision"].is_null());

    let (err, v) = c.call("decide_proposal", json!({"id": id, "approve": true, "comment": "go"}));
    assert!(!err, "{v}");
    assert_eq!(v["state"], "approved");
    assert!(d.path().join("plan/stages/A.md").exists());
    let (_, v) = c.call("list_proposals", json!({"state": "approved"}));
    assert_eq!(v["proposals"][0]["decision"]["comment"], "go");
    let (_, v) = c.call("list_proposals", json!({}));
    assert!(v["proposals"].as_array().unwrap().is_empty());
    let (err, _) = c.call("list_proposals", json!({"state": "bogus"}));
    assert!(err);
}

fn cli(dir: &std::path::Path, args: &[&str]) {
    let o = Command::new(env!("CARGO_BIN_EXE_pathloom")).arg("--project").arg(dir).args(args).output().unwrap();
    assert!(o.status.success(), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn require_approval_review_and_approval_tools() {
    let d = project();
    cli(d.path(), &["approval", "require", "on"]);
    let mut c = Client::start(d.path(), &[]);
    c.handshake();
    let tools = c.request("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for n in ["request_approval", "get_approvals", "ask_decision", "list_decisions"] {
        assert!(names.contains(&n), "missing tool {n}");
    }
    assert!(!names.contains(&"decide_approval"), "agents cannot approve themselves");
    let spec = json!({"stages": [
        {"id": "APPR", "title": "Approvals", "tasks": ["Icon"]},
        {"id": "DECIDE", "title": "Decisions", "tasks": ["DB"]},
        {"id": "DEV", "title": "Dev", "tasks": ["Build", "Ship"]}]});
    assert!(!c.call("apply", json!({"spec": spec})).0);

    // set_task done -> review, evidence kept, not done
    let ev = json!({"command": "cargo test", "exit_code": 0});
    let (err, v) = c.call("set_task", json!({"task_id": "DEV-01", "done": true, "evidence": ev}));
    assert!(!err, "{v}");
    assert_eq!((v["state"].as_str(), v["done"].clone()), (Some("review"), json!(false)));
    let (_, st) = c.call("get_stage", json!({"key": "DEV"}));
    assert_eq!(st["tasks"][0]["done"], false);
    assert_eq!(st["evidence"]["DEV-01"]["command"], "cargo test");
    assert_eq!(st["taskInfo"]["DEV-01"]["approval"]["state"], "pending");
    assert_eq!(st["status"], "active");

    // apply tasks_done by an agent -> review as well
    let (err, v) = c.call("apply", json!({"spec": {"stages": [{"id": "DEV", "tasks_done": ["DEV-02"]}]}}));
    assert!(!err, "{v}");
    assert_eq!((v["state"].as_str(), v["review"][0].as_str()), (Some("review"), Some("DEV-02")));
    assert_eq!(c.call("get_stage", json!({"key": "DEV"})).1["tasks"][1]["done"], false);

    // request_approval, the owner rejects in the CLI, the agent reads reasons and comment
    let (err, v) = c.call("request_approval", json!({"task_id": "APPR-01", "files": ["shots/a.png"], "note": "icon"}));
    assert!(!err, "{v}");
    let (_, v) = c.call("get_approvals", json!({"state": "pending"}));
    assert_eq!(v["approvals"].as_array().unwrap().len(), 3);
    cli(d.path(), &["approval", "reject", "APPR-01", "--reason", "scale", "--comment", "Too big"]);
    let (_, v) = c.call("get_approvals", json!({"state": "rejected"}));
    let a = &v["approvals"][0];
    assert_eq!((a["task_id"].as_str(), a["reasons"][0].as_str(), a["comment"].as_str()), (Some("APPR-01"), Some("scale"), Some("Too big")));
    assert_eq!(a["files"][0], "shots/a.png");

    // decisions
    let q = json!({"task_id": "DECIDE-01", "question": "Which DB?", "options": [{"label": "SQLite"}, {"label": "Postgres", "description": "server"}], "recommended": 0});
    let (err, v) = c.call("ask_decision", q);
    assert!(!err, "{v}");
    assert_eq!(c.call("list_decisions", json!({"open": true})).1["decisions"].as_array().unwrap().len(), 1);
    cli(d.path(), &["decision", "answer", "DECIDE-01", "Postgres"]);
    let (_, v) = c.call("list_decisions", json!({}));
    assert_eq!(v["decisions"][0]["choice"], "Postgres");
    assert_eq!(c.call("list_decisions", json!({"open": true})).1["decisions"].as_array().unwrap().len(), 0);
    assert_eq!(c.call("get_stage", json!({"key": "DECIDE"})).1["taskInfo"]["DECIDE-01"]["decision"]["choice"], "Postgres");

    // read-only server: write tools refuse, reads work
    let mut r = Client::start(d.path(), &["--read-only"]);
    r.handshake();
    assert_eq!(r.call("request_approval", json!({"task_id": "DEV-01"})).1["code"], "READ_ONLY");
    assert!(!r.call("get_approvals", json!({})).0);
}

#[test]
fn delegation_status_and_usage_tools() {
    let d = project();
    let mut c = Client::start(d.path(), &[]);
    c.handshake_as("worker");
    c.call("apply", json!({"spec": "stages: [{id: A, title: Alpha, tasks: [one]}]"}));
    let (err, v) = c.call("claim_task", json!({"task_id": "A-01", "by": "opus", "parent": "lead", "branch": "feat/x"}));
    assert!(!err, "{v}");
    assert_eq!((v["by"].as_str(), v["parent"].as_str(), v["branch"].as_str()), (Some("opus"), Some("lead"), Some("feat/x")));
    let (_, v) = c.call("get_stage", json!({"key": "A"}));
    assert_eq!(v["claims"][0]["by"], "opus");
    let (err, v) = c.call(
        "set_task_status",
        json!({"task_id": "A-01", "status": "pushed", "branch": "feat/x", "sha": "3f2a9c1", "evidence": ["shots/x.png"]}),
    );
    assert!(!err, "{v}");
    assert_eq!(v["status"]["step"], "pushed");
    let (_, v) = c.call("get_stage", json!({"key": "A"}));
    assert_eq!(v["taskInfo"]["A-01"]["status"]["sha"], "3f2a9c1");
    let (err, v) = c.call("set_task_status", json!({"task_id": "A-01", "status": "pushed", "sha": "nothex1"}));
    assert!(err && v["code"] == "INVALID_SPEC");
    let (err, _) = c.call("set_task_status", json!({"task_id": "A-01", "status": "flying"}));
    assert!(err);
    let (err, _) = c.call("set_task", json!({"task_id": "A-01", "done": true, "evidence": {"command": "x", "exit_code": 0, "commit": "zz"}}));
    assert!(err);
    let (err, v) = c.call("set_task", json!({"task_id": "A-01", "done": true, "evidence": {"command": "x", "exit_code": 0, "commit": "3F2A9C1"}}));
    assert!(!err, "{v}");
    let (_, v) = c.call("get_stage", json!({"key": "A"}));
    assert_eq!(v["evidence"]["A-01"]["commit"], "3f2a9c1");
    let (err, v) = c.call("report_usage", json!({"tokens_in": 10, "tokens_out": 5, "task_id": "A-01", "model": "opus"}));
    assert!(!err, "{v}");
    let (_, v) = c.call("activity", json!({"limit": 1}));
    assert_eq!((v["entries"][0]["tasks"][0].as_str(), v["entries"][0]["stages"][0].as_str()), (Some("A-01"), Some("A")));
    let (err, _) = c.call("report_usage", json!({"percent_weekly": 42.5, "resets_at": "2026-11-27T00:00:00Z"}));
    assert!(!err);
    let (err, _) = c.call("report_usage", json!({"percent_weekly": 101}));
    assert!(err);
    let (err, _) = c.call("report_usage", json!({"task_id": "A-99"}));
    assert!(err);
}

#[test]
fn export_import_and_revert_tools() {
    let d = project();
    let mut c = Client::start(d.path(), &[]);
    c.handshake();
    let (err, v) = c.call("import_tasks", json!({"format": "csv", "text": "Name,Status\nWrite docs,Done\nShip,Todo\n"}));
    assert!(!err, "{v}");
    let (_, v) = c.call("export_plan", json!({"format": "md"}));
    assert!(v["text"].as_str().unwrap().contains("- [x] Write docs"), "{v}");
    assert!(c.call("export_plan", json!({"format": "pdf"})).0);
    let (_, h) = c.call("list_history", json!({}));
    let id = h["entries"][0]["id"].as_str().unwrap().to_string();
    let (err, v) = c.call("revert_change", json!({"id": id}));
    assert!(!err, "{v}");
    let (_, v) = c.call("export_plan", json!({"format": "csv"}));
    assert_eq!(v["text"].as_str().unwrap().lines().count(), 1); // header only
    let (err, v) = c.call("revert_change", json!({"id": id}));
    assert!(err && v["code"] == "CONFLICT", "{v}");
    let mut ro = Client::start(d.path(), &["--read-only"]);
    ro.handshake();
    assert!(!ro.call("list_history", json!({})).0);
    assert_eq!(ro.call("revert_change", json!({"id": id})).1["code"], "READ_ONLY");
}
