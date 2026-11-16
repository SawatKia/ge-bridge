//! ACP sessions and the tool loop: ask the model for the next step, run its tool calls, feed results back.

use super::llm::{AssistantTurn, ChatError, Llm, ToolCallOut};
use super::mcp::{self, McpHub};
use super::rpc::{CallError, Rpc, RpcError};
use super::tools::LocalTools;
use super::types::{Risk, ToolOutcome};
use crate::bot_images::{self, RecentImage};
use parking_lot::{Condvar, Mutex};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, Hasher};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_MODEL_CALLS: usize = 30;
const MCP_CALL_TIMEOUT: Duration = Duration::from_secs(300);
const MCP_STARTUP_TIMEOUT: Duration = Duration::from_secs(20);
const RESULT_MAX_CHARS: usize = 30_000;
const DEFAULT_MODEL: &str = "default";

enum HubSlot {
    Starting,
    Ready(Option<Arc<McpHub>>),
}

struct Session {
    tools: LocalTools,
    model: Mutex<String>,
    models: Vec<String>,
    hub: Mutex<HubSlot>,
    hub_ready: Condvar,
    history: Mutex<Vec<Value>>,
    cancel: AtomicBool,
    busy: AtomicBool,
    allowed: Mutex<HashSet<Risk>>,
    /// The bot persona and standing instructions OpenMausBot sends in front of a prompt; kept for the system message.
    bot_context: Mutex<Option<String>>,
    notes: Mutex<Vec<String>>,
}

pub struct Agent {
    rpc: Arc<Rpc>,
    llm: Llm,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}

fn new_session_id() -> String {
    let s = std::collections::hash_map::RandomState::new();
    let a = s.build_hasher().finish();
    let b = std::collections::hash_map::RandomState::new().build_hasher().finish();
    format!("geb-{a:016x}{:08x}", b as u32)
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Keeps the start and end of a long result.
pub fn truncate_result(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= RESULT_MAX_CHARS {
        return text.to_string();
    }
    let head: String = chars[..20_000].iter().collect();
    let tail: String = chars[chars.len() - 6_000..].iter().collect();
    format!("{head}\n... [{} characters cut] ...\n{tail}", chars.len() - 26_000)
}

/// The line OpenMausBot ends its bot context with; the user's own words follow it.
const OMB_TIME_LINE: &str = "Current time when this message was sent:";

/// OpenMausBot has no system prompt in ACP: it sends the bot's persona, standing instructions and the current
/// time in front of the user's words in one text block. Returns (context, request); no split when the marker
/// is missing or nothing follows it.
pub fn split_bot_context(text: &str) -> (Option<String>, String) {
    let Some(at) = text.rfind(OMB_TIME_LINE) else { return (None, text.to_string()) };
    let line_end = text[at..].find('\n').map(|i| at + i).unwrap_or(text.len());
    let request = text[line_end..].trim();
    if request.is_empty() {
        return (None, text.to_string());
    }
    (Some(text[..line_end].trim().to_string()), request.to_string())
}

/// ACP content blocks to one user message.
pub fn blocks_to_text(blocks: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    for b in blocks.as_array().into_iter().flatten() {
        match b["type"].as_str().unwrap_or("") {
            "text" => parts.push(b["text"].as_str().unwrap_or("").to_string()),
            "resource_link" => {
                let uri = b["uri"].as_str().unwrap_or("");
                match b["name"].as_str().filter(|n| !n.is_empty()) {
                    Some(n) => parts.push(format!("[file: {uri}] ({n})")),
                    None => parts.push(format!("[file: {uri}]")),
                }
            }
            "resource" => {
                let r = &b["resource"];
                let uri = r["uri"].as_str().unwrap_or("");
                match r["text"].as_str() {
                    Some(t) => parts.push(format!("File {uri}:\n```\n{t}\n```")),
                    None => parts.push(format!("[file: {uri}]")),
                }
            }
            "image" => parts.push("(image omitted)".into()),
            "audio" => parts.push("(audio omitted)".into()),
            _ => {}
        }
    }
    parts.join("\n\n")
}

fn system_prompt(root: &Path) -> String {
    format!(
        "Coding agent working in the folder {}. The machine runs Windows; run_command uses PowerShell syntax. \
         Tools are available (read_file, write_file, edit_file, list_dir, grep, run_command, plus any MCP tools): \
         use them to act instead of describing what you would do. Read a file before editing it, use edit_file for \
         small changes and write_file for new files or full rewrites. Never claim a file changed, a command ran or \
         something exists unless a tool result confirmed it. If a call is rejected or fails, adapt or say so. \
         Keep answers short.",
        root.display()
    )
}

fn err_param(msg: impl Into<String>) -> RpcError {
    RpcError::invalid_params(msg)
}

impl Agent {
    pub fn new(rpc: Arc<Rpc>) -> Arc<Agent> {
        Arc::new(Agent { rpc, llm: Llm::new(), sessions: Mutex::new(HashMap::new()) })
    }

    pub fn handle(self: &Arc<Self>, method: &str, params: Value) -> Result<Value, RpcError> {
        match method {
            "initialize" => Ok(json!({
                "protocolVersion": 1,
                "agentCapabilities": {
                    "loadSession": false,
                    "promptCapabilities": { "image": false, "audio": false, "embeddedContext": true },
                    "mcpCapabilities": { "http": false, "sse": false }
                },
                "authMethods": [],
                "agentInfo": { "name": "ge-bridge", "title": "Gemini Enterprise (ge-bridge)", "version": env!("CARGO_PKG_VERSION") }
            })),
            "authenticate" => Ok(json!({})),
            "session/new" => self.new_session(&params),
            "session/set_config_option" => {
                let s = self.session(&params)?;
                match params["configId"].as_str() {
                    Some("model") => {
                        let v = params["value"].as_str().filter(|v| !v.is_empty()).ok_or_else(|| err_param("value must be a model id"))?;
                        *s.model.lock() = v.to_string();
                        Ok(json!({ "configOptions": config_options(&s) }))
                    }
                    other => Err(err_param(format!("unknown config option {}", other.unwrap_or("(none)")))),
                }
            }
            "session/set_model" => {
                let s = self.session(&params)?;
                let v = params["modelId"].as_str().filter(|v| !v.is_empty()).ok_or_else(|| err_param("modelId is required"))?;
                *s.model.lock() = v.to_string();
                Ok(json!({}))
            }
            "session/prompt" => self.prompt(&params),
            "session/cancel" => {
                if let Ok(s) = self.session(&params) {
                    s.cancel.store(true, Ordering::Relaxed);
                }
                Ok(json!({}))
            }
            _ => Err(RpcError::new(-32601, format!("method not found: {method}"))),
        }
    }

    fn session(&self, params: &Value) -> Result<Arc<Session>, RpcError> {
        let id = params["sessionId"].as_str().ok_or_else(|| err_param("sessionId is required"))?;
        self.sessions.lock().get(id).cloned().ok_or_else(|| err_param(format!("unknown session {id}")))
    }

    fn new_session(&self, params: &Value) -> Result<Value, RpcError> {
        let cwd = params["cwd"].as_str().ok_or_else(|| err_param("cwd is required"))?;
        let tools = LocalTools::new(Path::new(cwd)).map_err(err_param)?;
        let root = tools.root().to_path_buf();
        let (servers, notes) = mcp::parse_servers(&params["mcpServers"]);
        for n in &notes {
            eprintln!("acp: {n}");
        }
        let models = match self.llm.models() {
            Ok(m) if !m.is_empty() => m,
            Ok(_) => vec![DEFAULT_MODEL.to_string()],
            Err(e) => {
                eprintln!("acp: could not list models: {e}");
                vec![DEFAULT_MODEL.to_string()]
            }
        };
        let session = Arc::new(Session {
            tools,
            model: Mutex::new(DEFAULT_MODEL.to_string()),
            models,
            hub: Mutex::new(HubSlot::Starting),
            hub_ready: Condvar::new(),
            history: Mutex::new(Vec::new()),
            cancel: AtomicBool::new(false),
            busy: AtomicBool::new(false),
            allowed: Mutex::new(HashSet::new()),
            bot_context: Mutex::new(None),
            notes: Mutex::new(notes),
        });
        {
            let s = session.clone();
            std::thread::spawn(move || {
                let hub = if servers.is_empty() { McpHub::empty() } else { McpHub::start(&servers, &root, MCP_STARTUP_TIMEOUT) };
                for e in hub.errors() {
                    eprintln!("acp: {e}");
                    s.notes.lock().push(e.clone());
                }
                *s.hub.lock() = HubSlot::Ready(Some(Arc::new(hub)));
                s.hub_ready.notify_all();
            });
        }
        let id = new_session_id();
        let options = config_options(&session);
        self.sessions.lock().insert(id.clone(), session);
        Ok(json!({ "sessionId": id, "configOptions": options }))
    }

    fn prompt(self: &Arc<Self>, params: &Value) -> Result<Value, RpcError> {
        let s = self.session(params)?;
        let sid = params["sessionId"].as_str().unwrap_or("").to_string();
        if s.busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return Err(RpcError::new(-32000, "a prompt is already running in this session"));
        }
        s.cancel.store(false, Ordering::Relaxed);
        let (context, text) = split_bot_context(&blocks_to_text(&params["prompt"]));
        if context.is_some() {
            *s.bot_context.lock() = context;
        }
        let start_len = s.history.lock().len();
        s.history.lock().push(json!({ "role": "user", "content": text }));
        let res = self.run_loop(&s, &sid);
        s.busy.store(false, Ordering::Release);
        match res {
            Ok(reason) => Ok(json!({ "stopReason": reason })),
            Err(msg) => {
                s.history.lock().truncate(start_len);
                Err(RpcError::internal(msg))
            }
        }
    }

    fn update(&self, sid: &str, update: Value) {
        self.rpc.notify("session/update", json!({ "sessionId": sid, "update": update }));
    }

    fn wait_hub(&self, s: &Session) -> Option<Arc<McpHub>> {
        let mut slot = s.hub.lock();
        while matches!(*slot, HubSlot::Starting) {
            s.hub_ready.wait_for(&mut slot, Duration::from_millis(200));
        }
        match &*slot {
            HubSlot::Ready(h) => h.clone(),
            HubSlot::Starting => None,
        }
    }

    fn run_loop(&self, s: &Arc<Session>, sid: &str) -> Result<&'static str, String> {
        let hub = self.wait_hub(s);
        for note in std::mem::take(&mut *s.notes.lock()) {
            self.update(sid, json!({ "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": format!("{note}\n") } }));
        }
        let mut specs: Vec<Value> = LocalTools::specs().iter().map(|t| t.to_openai()).collect();
        if let Some(h) = &hub {
            specs.extend(h.specs().iter().map(|t| t.to_openai()));
        }
        let system = match s.bot_context.lock().as_deref() {
            Some(ctx) => format!("{}\n\n{ctx}", system_prompt(s.tools.root())),
            None => system_prompt(s.tools.root()),
        };
        let system = json!({ "role": "system", "content": system });

        for _ in 0..MAX_MODEL_CALLS {
            if s.cancel.load(Ordering::Relaxed) {
                return Ok("cancelled");
            }
            let mut messages = vec![system.clone()];
            messages.extend(s.history.lock().iter().cloned());
            let model = s.model.lock().clone();
            let mut on_text = |t: &str| {
                self.update(sid, json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": t } }));
            };
            let mut on_thought = |t: &str| {
                self.update(sid, json!({ "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": t } }));
            };
            let turn: AssistantTurn = match self.llm.chat(&messages, &specs, &model, &mut on_text, &mut on_thought, &s.cancel) {
                Ok(t) => t,
                Err(ChatError::Cancelled) => return Ok("cancelled"),
                Err(ChatError::Failed(_)) if s.cancel.load(Ordering::Relaxed) => return Ok("cancelled"),
                Err(ChatError::Failed(m)) => return Err(m),
            };
            if s.cancel.load(Ordering::Relaxed) {
                return Ok("cancelled");
            }
            let mut am = json!({ "role": "assistant", "content": turn.text });
            if !turn.tool_calls.is_empty() {
                am["tool_calls"] = Value::Array(
                    turn.tool_calls
                        .iter()
                        .map(|c| json!({ "id": c.id, "type": "function", "function": { "name": c.name, "arguments": c.arguments_json } }))
                        .collect(),
                );
            }
            s.history.lock().push(am);
            if turn.tool_calls.is_empty() {
                return Ok("end_turn");
            }
            for tc in &turn.tool_calls {
                let text = if s.cancel.load(Ordering::Relaxed) {
                    "Cancelled by the user.".to_string()
                } else {
                    self.run_call(s, sid, &hub, tc)
                };
                s.history.lock().push(json!({ "role": "tool", "tool_call_id": tc.id, "content": text }));
            }
            if s.cancel.load(Ordering::Relaxed) {
                return Ok("cancelled");
            }
        }
        Ok("max_turn_requests")
    }

    fn finish(&self, sid: &str, id: &str, failed: bool, text: &str, extra: Vec<Value>, locations: &[Value]) {
        let mut content = vec![json!({ "type": "content", "content": { "type": "text", "text": text } })];
        content.extend(extra);
        self.update(
            sid,
            json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": id,
                "status": if failed { "failed" } else { "completed" },
                "content": content,
                "locations": locations,
                "rawOutput": { "text": text }
            }),
        );
    }

    /// Runs one tool call end to end (updates, approval, execution); returns the text for the model.
    fn run_call(&self, s: &Arc<Session>, sid: &str, hub: &Option<Arc<McpHub>>, tc: &ToolCallOut) -> String {
        let id = tc.id.as_str();
        let name = tc.name.as_str();
        let parsed: Result<Value, String> = if tc.arguments_json.trim().is_empty() {
            Ok(json!({}))
        } else {
            serde_json::from_str(&tc.arguments_json).map_err(|e| e.to_string())
        };
        let local = LocalTools::handles(name);
        let on_hub = hub.as_ref().filter(|h| h.handles(name)).cloned();
        let args_for_meta = parsed.clone().unwrap_or_else(|_| json!({}));
        let (title, kind) = if local {
            (LocalTools::title(name, &args_for_meta), LocalTools::kind(name))
        } else {
            (name.to_string(), "other")
        };
        let raw_input = match &parsed {
            Ok(v) => v.clone(),
            Err(_) => Value::String(tc.arguments_json.clone()),
        };
        self.update(
            sid,
            json!({ "sessionUpdate": "tool_call", "toolCallId": id, "title": title, "kind": kind, "status": "pending", "rawInput": raw_input }),
        );
        let args = match parsed {
            Ok(a) => a,
            Err(e) => {
                let t = format!("Invalid JSON arguments for {name}: {e}");
                self.finish(sid, id, true, &t, vec![], &[]);
                return t;
            }
        };
        if !local && on_hub.is_none() {
            let t = format!("Unknown tool '{name}'.");
            self.finish(sid, id, true, &t, vec![], &[]);
            return t;
        }
        if local {
            let risk = LocalTools::risk(name);
            if risk != Risk::Read && !s.allowed.lock().contains(&risk) {
                let params = json!({
                    "sessionId": sid,
                    "toolCall": { "toolCallId": id, "title": title, "kind": kind, "status": "pending", "rawInput": args },
                    "options": [
                        { "optionId": "allow_once", "name": "Allow", "kind": "allow_once" },
                        { "optionId": "allow_always", "name": "Always allow in this session", "kind": "allow_always" },
                        { "optionId": "reject_once", "name": "Reject", "kind": "reject_once" }
                    ]
                });
                let choice = match self.rpc.request("session/request_permission", params, Some(&s.cancel)) {
                    Ok(v) => {
                        if v["outcome"]["outcome"].as_str() == Some("selected") {
                            v["outcome"]["optionId"].as_str().unwrap_or("reject_once").to_string()
                        } else {
                            s.cancel.store(true, Ordering::Relaxed);
                            "cancelled".to_string()
                        }
                    }
                    Err(CallError::Remote(_)) => "reject_once".to_string(),
                    Err(CallError::Cancelled) | Err(CallError::Closed) => {
                        s.cancel.store(true, Ordering::Relaxed);
                        "cancelled".to_string()
                    }
                };
                match choice.as_str() {
                    "allow_once" => {}
                    "allow_always" => {
                        s.allowed.lock().insert(risk);
                    }
                    "cancelled" => {
                        let t = "Cancelled by the user.".to_string();
                        self.finish(sid, id, true, &t, vec![], &[]);
                        return t;
                    }
                    _ => {
                        let t = format!("The user rejected this {name} call.");
                        self.finish(sid, id, true, &t, vec![], &[]);
                        return t;
                    }
                }
            }
        }
        self.update(sid, json!({ "sessionUpdate": "tool_call_update", "toolCallId": id, "status": "in_progress" }));

        let mut outcome: ToolOutcome = match on_hub {
            None => s.tools.call(name, &args, &s.cancel),
            Some(h) => {
                let (tx, rx) = mpsc::channel();
                let (n, a) = (name.to_string(), args.clone());
                std::thread::spawn(move || {
                    let _ = tx.send(h.call(&n, &a, MCP_CALL_TIMEOUT));
                });
                loop {
                    match rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(o) => break o,
                        Err(RecvTimeoutError::Timeout) if s.cancel.load(Ordering::Relaxed) => break ToolOutcome::err("Cancelled by the user."),
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break ToolOutcome::err(format!("{name} stopped unexpectedly.")),
                    }
                }
            }
        };

        let mut extra = Vec::new();
        if !outcome.images.is_empty() {
            let prompt = args["prompt"].as_str().or_else(|| args["arguments"]["prompt"].as_str()).unwrap_or("image").to_string();
            let root = s.tools.root();
            for img in &outcome.images {
                let saved = bot_images::save(root, &RecentImage { prompt: prompt.clone(), mime: img.mime.clone(), b64: img.b64.clone(), at: now_secs() });
                match saved {
                    Ok(p) => {
                        let rel = p.strip_prefix(root).unwrap_or(&p).display().to_string();
                        outcome.text.push_str(&format!("\nSaved to {rel}"));
                    }
                    Err(e) => outcome.text.push_str(&format!("\nCould not save the image: {e}")),
                }
                extra.push(json!({ "type": "content", "content": { "type": "image", "mimeType": img.mime, "data": img.b64 } }));
            }
        }
        let text = truncate_result(&outcome.text);
        let locations: Vec<Value> = outcome.locations.iter().map(|p| json!({ "path": p.display().to_string() })).collect();
        self.finish(sid, id, outcome.is_error, &text, extra, &locations);
        text
    }

    /// Cancels running prompts and stops the MCP servers; called when stdin closes.
    pub fn shutdown(&self) {
        let sessions: Vec<Arc<Session>> = self.sessions.lock().drain().map(|(_, s)| s).collect();
        for s in &sessions {
            s.cancel.store(true, Ordering::Relaxed);
        }
        for s in sessions {
            let hub = {
                let mut slot = s.hub.lock();
                let deadline = std::time::Instant::now() + Duration::from_secs(25);
                while matches!(*slot, HubSlot::Starting) && std::time::Instant::now() < deadline {
                    s.hub_ready.wait_for(&mut slot, Duration::from_millis(200));
                }
                match &mut *slot {
                    HubSlot::Ready(h) => h.take(),
                    HubSlot::Starting => None,
                }
            };
            if let Some(h) = hub {
                // A call thread may still hold a clone for a moment after the cancel; give it a short grace.
                let mut h = h;
                for _ in 0..20 {
                    match Arc::try_unwrap(h) {
                        Ok(mut hub) => {
                            hub.shutdown();
                            break;
                        }
                        Err(back) => {
                            h = back;
                            std::thread::sleep(Duration::from_millis(100));
                        }
                    }
                }
            }
        }
    }
}

fn config_options(s: &Session) -> Value {
    let current = s.model.lock().clone();
    let mut ids = s.models.clone();
    if !ids.contains(&current) {
        ids.insert(0, current.clone());
    }
    json!([{
        "id": "model",
        "name": "Model",
        "category": "model",
        "type": "select",
        "currentValue": current,
        "options": ids.iter().map(|m| json!({ "value": m, "name": m })).collect::<Vec<_>>()
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_become_text() {
        let b = json!([
            { "type": "text", "text": "hi" },
            { "type": "resource_link", "uri": "file:///a.txt", "name": "a.txt" },
            { "type": "resource", "resource": { "uri": "file:///b.txt", "text": "body" } },
            { "type": "image", "data": "x", "mimeType": "image/png" }
        ]);
        let t = blocks_to_text(&b);
        assert!(t.contains("hi") && t.contains("[file: file:///a.txt] (a.txt)"));
        assert!(t.contains("File file:///b.txt:\n```\nbody\n```") && t.contains("(image omitted)"));
    }

    #[test]
    fn long_results_keep_head_and_tail() {
        let long = format!("{}{}", "a".repeat(40_000), "z".repeat(10));
        let t = truncate_result(&long);
        assert!(t.len() < 30_200 && t.starts_with("aaa") && t.ends_with("zzzzzzzzzz") && t.contains("characters cut"));
        assert_eq!(truncate_result("short"), "short");
    }

    #[test]
    fn system_prompt_has_no_bot_name_marker() {
        assert!(!system_prompt(Path::new("C:/x")).contains("You are "));
        assert!(system_prompt(Path::new("C:/x")).len() < 1200);
    }

    #[test]
    fn openmausbot_bot_context_is_split_from_the_request() {
        // The shape OpenMausBot sent on 2026-11-16: persona (which talks about sticker images) + time + request.
        let sent = "You are Artist, a personal bot in OpenMausBot. Role: Visual & Asset Creator. About: Generates AI sticker images.\n\n\
                    --- BEGIN STANDING INSTRUCTIONS ---\nGenerate sticker illustrations.\n\n\
                    Current time when this message was sent: Monday, 2026-11-16 15:26 Asia/Bangkok (Thailand time, UTC+07:00).\n\n\
                    สร้างไฟล์ hello.txt";
        let (ctx, req) = split_bot_context(sent);
        assert_eq!(req, "สร้างไฟล์ hello.txt");
        let ctx = ctx.unwrap();
        assert!(ctx.starts_with("You are Artist, a personal bot"));
        assert!(ctx.ends_with("UTC+07:00)."));
        assert_eq!(split_bot_context("just a request"), (None, "just a request".to_string()));
        let only_ctx = "You are X, a bot.\nCurrent time when this message was sent: now.";
        assert_eq!(split_bot_context(only_ctx), (None, only_ctx.to_string()), "nothing after the marker: no split");
    }
}
