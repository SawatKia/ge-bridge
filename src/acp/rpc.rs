//! Newline-delimited JSON-RPC 2.0 over stdio. Stdout carries protocol JSON only (one object per line);
//! diagnostics go to stderr. Incoming requests run on their own threads so a long `session/prompt`
//! never blocks `session/cancel` or the answer to a permission request.

use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Duration;

/// JSON-RPC error a handler returns.
#[derive(Debug, Clone)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(-32602, message)
    }
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(-32603, message)
    }
}

/// Why an outgoing request produced no result.
#[derive(Debug)]
pub enum CallError {
    /// The client answered with a JSON-RPC error.
    Remote(String),
    /// The caller's cancel flag was set while waiting.
    Cancelled,
    /// stdin closed.
    Closed,
}

type Pending = Sender<Result<Value, String>>;

pub struct Rpc {
    pending: Mutex<HashMap<u64, Pending>>,
    next_id: AtomicU64,
}

pub type Handler = dyn Fn(&str, Value) -> Result<Value, RpcError> + Send + Sync;

impl Rpc {
    pub fn new() -> Arc<Rpc> {
        Arc::new(Rpc { pending: Mutex::new(HashMap::new()), next_id: AtomicU64::new(1) })
    }

    fn write(&self, v: &Value) {
        let mut line = v.to_string();
        line.push('\n');
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        // A closed stdout means the client is gone; the reader loop notices EOF and shuts down.
        let _ = out.write_all(line.as_bytes());
        let _ = out.flush();
    }

    pub fn notify(&self, method: &str, params: Value) {
        self.write(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    fn respond(&self, id: &Value, result: Result<Value, RpcError>) {
        let msg = match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": e.code, "message": e.message } }),
        };
        self.write(&msg);
    }

    /// Sends a request to the client and waits for its answer. `cancel` is polled while waiting.
    pub fn request(&self, method: &str, params: Value, cancel: Option<&AtomicBool>) -> Result<Value, CallError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().insert(id, tx);
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Ok(v)) => return Ok(v),
                Ok(Err(e)) => return Err(CallError::Remote(e)),
                Err(RecvTimeoutError::Disconnected) => {
                    self.pending.lock().remove(&id);
                    return Err(CallError::Closed);
                }
                Err(RecvTimeoutError::Timeout) => {
                    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                        self.pending.lock().remove(&id);
                        return Err(CallError::Cancelled);
                    }
                }
            }
        }
    }

    /// Reads stdin until EOF, routing responses to waiting requests and everything else to `handler`.
    /// Requests run on their own threads; notifications run inline so their order is kept.
    pub fn serve(self: &Arc<Self>, handler: Arc<Handler>) {
        let stdin = std::io::stdin();
        let mut input = stdin.lock();
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match input.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let text = String::from_utf8_lossy(&buf);
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            let msg: Value = match serde_json::from_str(text) {
                Ok(v) => v,
                Err(e) => {
                    self.respond(&Value::Null, Err(RpcError::new(-32700, format!("parse error: {e}"))));
                    continue;
                }
            };
            self.route(msg, &handler);
        }
        // Wake every waiting request: their senders drop, so they see Closed.
        self.pending.lock().clear();
    }

    fn route(self: &Arc<Self>, msg: Value, handler: &Arc<Handler>) {
        let id = msg.get("id").filter(|v| !v.is_null()).cloned();
        let Some(method) = msg.get("method").and_then(Value::as_str).map(String::from) else {
            // A response to one of our requests.
            if let Some(n) = id.as_ref().and_then(Value::as_u64) {
                if let Some(tx) = self.pending.lock().remove(&n) {
                    let r = match msg.get("error") {
                        Some(e) => Err(e.get("message").and_then(Value::as_str).unwrap_or("error").to_string()),
                        None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                    };
                    let _ = tx.send(r);
                }
            }
            return;
        };
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let run = {
            let handler = handler.clone();
            move || catch_unwind(AssertUnwindSafe(|| handler(&method, params)))
        };
        match id {
            Some(id) => {
                let me = self.clone();
                std::thread::spawn(move || {
                    let res = run().unwrap_or_else(|_| Err(RpcError::internal("internal error")));
                    me.respond(&id, res);
                });
            }
            None => {
                let _ = run();
            }
        }
    }
}
