//! HTTP client for the running ge-bridge server (`/v1/models`, `/v1/chat/completions`) on 127.0.0.1.
//! Raw HTTP/1.0 over a TcpStream: the server ends the body by closing the connection, so no chunk framing.

use crate::{config, state};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Longest silence from the server while a model turn is running.
const IDLE_TIMEOUT: Duration = Duration::from_secs(600);
/// Read granularity, so the cancel flag is checked about every half second.
const POLL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallOut {
    pub id: String,
    pub name: String,
    pub arguments_json: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AssistantTurn {
    pub text: String,
    pub tool_calls: Vec<ToolCallOut>,
}

pub enum ChatError {
    Cancelled,
    Failed(String),
}

impl From<String> for ChatError {
    fn from(s: String) -> Self {
        ChatError::Failed(s)
    }
}

pub struct Llm {
    port: u16,
    token: String,
}

impl Llm {
    pub fn new() -> Self {
        config::init(config::RuntimeConfig::from_env());
        Self { port: state::PersistedPrefs::load().port, token: state::load_or_create_token() }
    }

    fn connect(&self, method: &str, path: &str, body: Option<&Value>) -> Result<TcpStream, String> {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port))
            .map_err(|_| format!("ge-bridge is not running on 127.0.0.1:{}. Start ge-bridge.exe.", self.port))?;
        stream.set_read_timeout(Some(POLL)).ok();
        let payload = body.map(Value::to_string).unwrap_or_default();
        let req = format!(
            "{method} {path} HTTP/1.0\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{payload}",
            self.port,
            self.token,
            payload.len()
        );
        stream.write_all(req.as_bytes()).map_err(|e| format!("could not send the request to ge-bridge: {e}"))?;
        Ok(stream)
    }

    /// Model ids from `/v1/models`.
    pub fn models(&self) -> Result<Vec<String>, String> {
        let stream = self.connect("GET", "/v1/models", None)?;
        let mut rd = BufReader::new(stream);
        let (status, _ct) = read_head(&mut rd, None)?;
        let body = read_all(&mut rd, None)?;
        if status != 200 {
            return Err(http_error(status, &body));
        }
        let v: Value = serde_json::from_str(&body).map_err(|e| format!("bad /v1/models answer: {e}"))?;
        Ok(v["data"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str().map(String::from)).collect())
    }

    /// One model step. Text and reasoning stream through the callbacks as they arrive.
    pub fn chat(
        &self,
        messages: &[Value],
        tools: &[Value],
        model: &str,
        on_text: &mut dyn FnMut(&str),
        on_thought: &mut dyn FnMut(&str),
        cancel: &AtomicBool,
    ) -> Result<AssistantTurn, ChatError> {
        let mut body = json!({ "model": model, "stream": true, "messages": messages });
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools.to_vec());
        }
        let stream = self.connect("POST", "/v1/chat/completions", Some(&body))?;
        let mut rd = BufReader::new(stream);
        let (status, content_type) = read_head(&mut rd, Some(cancel))?;
        if status != 200 {
            let b = read_all(&mut rd, Some(cancel))?;
            return Err(http_error(status, &b).into());
        }
        if !content_type.contains("event-stream") {
            // Fallback: a plain non-stream completion.
            let b = read_all(&mut rd, Some(cancel))?;
            return parse_plain(&b, on_text).map_err(Into::into);
        }

        let mut text = String::new();
        let mut calls: Vec<(String, String, String)> = Vec::new();
        let mut line = Vec::new();
        let mut last_data = Instant::now();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(ChatError::Cancelled);
            }
            match rd.read_until(b'\n', &mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                    if last_data.elapsed() > IDLE_TIMEOUT {
                        return Err(ChatError::Failed("ge-bridge stopped answering".into()));
                    }
                    continue;
                }
                Err(e) => return Err(ChatError::Failed(format!("connection to ge-bridge failed: {e}"))),
            }
            if !line.ends_with(b"\n") {
                continue; // partial line before a read timeout; keep accumulating
            }
            last_data = Instant::now();
            let raw = String::from_utf8_lossy(&line).trim().to_string();
            line.clear();
            let Some(data) = raw.strip_prefix("data:") else { continue };
            let data = data.trim();
            if data == "[DONE]" {
                break;
            }
            let Ok(chunk) = serde_json::from_str::<Value>(data) else { continue };
            if let Some(msg) = chunk.pointer("/error/message").and_then(Value::as_str) {
                return Err(ChatError::Failed(msg.to_string()));
            }
            let delta = &chunk["choices"][0]["delta"];
            if let Some(r) = delta["reasoning_content"].as_str().filter(|s| !s.is_empty()) {
                on_thought(r);
            }
            if let Some(t) = delta["content"].as_str().filter(|s| !s.is_empty()) {
                text.push_str(t);
                on_text(t);
            }
            for tc in delta["tool_calls"].as_array().into_iter().flatten() {
                let idx = tc["index"].as_u64().unwrap_or(calls.len() as u64) as usize;
                while calls.len() <= idx {
                    calls.push(Default::default());
                }
                let c = &mut calls[idx];
                if let Some(id) = tc["id"].as_str().filter(|s| !s.is_empty()) {
                    c.0 = id.to_string();
                }
                if let Some(n) = tc["function"]["name"].as_str() {
                    c.1.push_str(n);
                }
                if let Some(a) = tc["function"]["arguments"].as_str() {
                    c.2.push_str(a);
                }
            }
        }
        let tool_calls = calls
            .into_iter()
            .enumerate()
            .filter(|(_, c)| !c.1.is_empty())
            .map(|(i, (id, name, arguments_json))| ToolCallOut {
                id: if id.is_empty() { format!("call_{i}") } else { id },
                name,
                arguments_json,
            })
            .collect();
        Ok(AssistantTurn { text, tool_calls })
    }
}

fn parse_plain(body: &str, on_text: &mut dyn FnMut(&str)) -> Result<AssistantTurn, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("bad answer from ge-bridge: {e}"))?;
    let m = &v["choices"][0]["message"];
    let text = m["content"].as_str().unwrap_or("").to_string();
    if !text.is_empty() {
        on_text(&text);
    }
    let tool_calls = m["tool_calls"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, tc)| ToolCallOut {
            id: tc["id"].as_str().filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(|| format!("call_{i}")),
            name: tc["function"]["name"].as_str().unwrap_or("").to_string(),
            arguments_json: tc["function"]["arguments"].as_str().unwrap_or("{}").to_string(),
        })
        .collect();
    Ok(AssistantTurn { text, tool_calls })
}

fn http_error(status: u16, body: &str) -> String {
    let msg = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.pointer("/error/message").and_then(Value::as_str).map(String::from));
    match (status, msg) {
        (401, _) => "ge-bridge rejected the token (401). Run ge-bridge-cli with the same Windows user and GE_BRIDGE_DATA_DIR as ge-bridge.exe.".into(),
        (_, Some(m)) => m,
        (s, None) => format!("ge-bridge answered HTTP {s}"),
    }
}

fn timed_out(e: &std::io::Error) -> bool {
    matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)
}

/// Reads the status line and headers; returns (status, content-type lowercased).
fn read_head(rd: &mut BufReader<TcpStream>, cancel: Option<&AtomicBool>) -> Result<(u16, String), String> {
    let mut status = 0u16;
    let mut ct = String::new();
    let mut line = Vec::new();
    let mut first = true;
    let started = Instant::now();
    loop {
        match rd.read_until(b'\n', &mut line) {
            Ok(0) => return Err("ge-bridge closed the connection without answering".into()),
            Ok(_) => {}
            Err(e) if timed_out(&e) => {
                if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                    return Err("cancelled".into());
                }
                if started.elapsed() > IDLE_TIMEOUT {
                    return Err("ge-bridge did not answer in time".into());
                }
                continue;
            }
            Err(e) => return Err(format!("no answer from ge-bridge: {e}")),
        }
        if !line.ends_with(b"\n") {
            continue;
        }
        let l = String::from_utf8_lossy(&line).trim().to_string();
        line.clear();
        if first {
            status = l.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            first = false;
        } else if l.is_empty() {
            return Ok((status, ct));
        } else if let Some((k, v)) = l.split_once(':') {
            if k.eq_ignore_ascii_case("content-type") {
                ct = v.trim().to_ascii_lowercase();
            }
        }
    }
}

fn read_all(rd: &mut BufReader<TcpStream>, cancel: Option<&AtomicBool>) -> Result<String, String> {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    let started = Instant::now();
    loop {
        match rd.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(e) if timed_out(&e) => {
                if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                    return Err("cancelled".into());
                }
                if started.elapsed() > IDLE_TIMEOUT {
                    return Err("ge-bridge did not answer in time".into());
                }
            }
            Err(e) => return Err(format!("no answer from ge-bridge: {e}")),
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}
