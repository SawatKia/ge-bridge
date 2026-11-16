//! MCP stdio client: spawns the servers an ACP client passes in `session/new` and offers their tools to the model.
//! Transport is newline-delimited JSON-RPC 2.0. Threads, not async: one reader thread per server routes
//! responses by `id` to the caller waiting on a channel, so concurrent calls work.

use super::types::{ToolImage, ToolOutcome, ToolSpec};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

const MAX_PAGES: usize = 10;
const MAX_NAME: usize = 64;
const STDERR_KEEP: usize = 20;

#[derive(Debug, Clone, PartialEq)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// Stdio entries from ACP `mcpServers`; returns (configs, notes about skipped entries).
pub fn parse_servers(v: &Value) -> (Vec<McpServerConfig>, Vec<String>) {
    let mut out = Vec::new();
    let mut notes = Vec::new();
    for (i, e) in v.as_array().into_iter().flatten().enumerate() {
        let name = e.get("name").and_then(Value::as_str).unwrap_or("");
        let label = if name.is_empty() { format!("#{i}") } else { name.to_string() };
        if let Some(t) = e.get("type").and_then(Value::as_str) {
            if t != "stdio" {
                notes.push(format!("MCP server {label}: transport '{t}' is not supported, skipped"));
                continue;
            }
        }
        let command = e.get("command").and_then(Value::as_str).unwrap_or("");
        if name.is_empty() || command.is_empty() {
            notes.push(format!("MCP server {label}: missing name or command, skipped"));
            continue;
        }
        out.push(McpServerConfig {
            name: name.to_string(),
            command: command.to_string(),
            args: e.get("args").and_then(Value::as_array).into_iter().flatten().filter_map(|a| a.as_str().map(String::from)).collect(),
            env: parse_env(e.get("env")),
        });
    }
    (out, notes)
}

/// ACP sends env as `[{name,value}]`; also accept a plain object map.
fn parse_env(v: Option<&Value>) -> Vec<(String, String)> {
    match v {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|p| Some((p.get("name")?.as_str()?.to_string(), p.get("value")?.as_str()?.to_string())))
            .collect(),
        Some(Value::Object(m)) => m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect(),
        _ => Vec::new(),
    }
}

#[derive(Debug)]
enum ReqError {
    Timeout,
    Dead(String),
    Rpc(String),
}

impl ReqError {
    fn text(&self, what: &str, timeout: Duration) -> String {
        match self {
            ReqError::Timeout => format!("{what} did not answer within {} s", timeout.as_secs_f32()),
            ReqError::Dead(m) => format!("{what}: MCP server is not running ({m})"),
            ReqError::Rpc(m) => format!("{what} failed: {m}"),
        }
    }
}

type Pending = Arc<Mutex<HashMap<u64, Sender<Value>>>>;

/// One running MCP server process.
struct Server {
    name: String,
    child: Mutex<Child>,
    stdin: Arc<Mutex<ChildStdin>>,
    pending: Pending,
    dead: Arc<AtomicBool>,
    next_id: AtomicU64,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
}

fn write_line(stdin: &Mutex<ChildStdin>, msg: &Value) -> std::io::Result<()> {
    let mut w = stdin.lock();
    writeln!(w, "{msg}")?;
    w.flush()
}

impl Server {
    fn spawn(cfg: &McpServerConfig, cwd: &Path) -> Result<Server, String> {
        let mut cmd = Command::new(&cfg.command);
        cmd.args(&cfg.args).envs(cfg.env.iter().map(|(k, v)| (k, v))).current_dir(cwd);
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000);
        let mut child = cmd.spawn().map_err(|e| format!("cannot start '{}': {e}", cfg.command))?;
        let stdin = Arc::new(Mutex::new(child.stdin.take().ok_or("no stdin")?));
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let stderr = child.stderr.take().ok_or("no stderr")?;
        let pending: Pending = Arc::default();
        let dead = Arc::new(AtomicBool::new(false));
        let stderr_tail = Arc::new(Mutex::new(VecDeque::new()));
        spawn_reader(stdout, stdin.clone(), pending.clone(), dead.clone());
        spawn_stderr_drain(stderr, stderr_tail.clone());
        Ok(Server { name: cfg.name.clone(), child: Mutex::new(child), stdin, pending, dead, next_id: AtomicU64::new(1), stderr_tail })
    }

    /// Send a request and wait for its response `result`.
    fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, ReqError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        {
            // Same lock the reader takes when it marks the server dead, so no response can be missed.
            let mut p = self.pending.lock();
            if self.dead.load(Ordering::SeqCst) {
                return Err(ReqError::Dead("stdout closed".into()));
            }
            p.insert(id, tx);
        }
        let msg = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        if let Err(e) = write_line(&self.stdin, &msg) {
            self.pending.lock().remove(&id);
            return Err(ReqError::Dead(format!("write failed: {e}")));
        }
        match rx.recv_timeout(timeout) {
            Ok(resp) => match resp.get("error") {
                Some(err) => Err(ReqError::Rpc(err.get("message").and_then(Value::as_str).unwrap_or("unknown error").to_string())),
                None => Ok(resp.get("result").cloned().unwrap_or(Value::Null)),
            },
            Err(RecvTimeoutError::Timeout) => {
                self.pending.lock().remove(&id);
                Err(ReqError::Timeout)
            }
            Err(RecvTimeoutError::Disconnected) => Err(ReqError::Dead("stdout closed".into())),
        }
    }

    fn notify(&self, method: &str) {
        let _ = write_line(&self.stdin, &json!({"jsonrpc":"2.0","method":method}));
    }

    /// initialize + initialized + paged tools/list, all within `timeout` in total.
    fn handshake(&self, timeout: Duration) -> Result<Vec<Value>, String> {
        let deadline = Instant::now() + timeout;
        let left = || deadline.saturating_duration_since(Instant::now());
        let step = |m: &str, p: Value| self.request(m, p, left()).map_err(|e| e.text(m, timeout));
        step(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "ge-bridge", "version": env!("CARGO_PKG_VERSION")}
            }),
        )?;
        self.notify("notifications/initialized");
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let params = cursor.as_ref().map_or(json!({}), |c| json!({"cursor": c}));
            let r = step("tools/list", params)?;
            tools.extend(r.get("tools").and_then(Value::as_array).cloned().unwrap_or_default());
            cursor = r.get("nextCursor").and_then(Value::as_str).map(String::from);
            if cursor.is_none() {
                break;
            }
        }
        Ok(tools)
    }

    fn kill(&self) {
        self.dead.store(true, Ordering::SeqCst);
        let mut c = self.child.lock();
        let _ = c.kill();
        let _ = c.wait();
    }

    fn stderr_text(&self) -> String {
        self.stderr_tail.lock().iter().cloned().collect::<Vec<_>>().join(" | ")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Route responses to waiting callers, answer server->client requests, fail everything on EOF.
fn spawn_reader(stdout: impl std::io::Read + Send + 'static, stdin: Arc<Mutex<ChildStdin>>, pending: Pending, dead: Arc<AtomicBool>) {
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
            let id = msg.get("id").filter(|i| !i.is_null());
            match (msg.get("method").is_some(), id) {
                (true, Some(id)) => {
                    let reply = if msg["method"] == "ping" {
                        json!({"jsonrpc":"2.0","id":id,"result":{}})
                    } else {
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"method not supported by client"}})
                    };
                    let _ = write_line(&stdin, &reply);
                }
                (false, Some(id)) => {
                    if let Some(tx) = id.as_u64().and_then(|n| pending.lock().remove(&n)) {
                        let _ = tx.send(msg);
                    }
                }
                _ => {}
            }
        }
        let mut p = pending.lock();
        dead.store(true, Ordering::SeqCst);
        p.clear(); // dropping the senders wakes every waiter
    });
}

fn spawn_stderr_drain(stderr: impl std::io::Read + Send + 'static, tail: Arc<Mutex<VecDeque<String>>>) {
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let mut t = tail.lock();
            if t.len() >= STDERR_KEEP {
                t.pop_front();
            }
            t.push_back(line);
        }
    });
}

/// All MCP servers of one ACP session and the tools they expose.
pub struct McpHub {
    servers: Vec<Server>,
    specs: Vec<ToolSpec>,
    /// exposed name -> (server index, original tool name)
    map: HashMap<String, (usize, String)>,
    errors: Vec<String>,
}

/// `<server>_<tool>` with chars outside `[A-Za-z0-9_-]` replaced by `_`, capped at 64 chars.
fn exposed_name(server: &str, tool: &str) -> String {
    format!("{server}_{tool}")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .take(MAX_NAME)
        .collect()
}

fn connect(cfg: &McpServerConfig, cwd: &Path, timeout: Duration) -> Result<(Server, Vec<Value>), String> {
    let server = Server::spawn(cfg, cwd).map_err(|e| format!("MCP server {}: {e}", cfg.name))?;
    match server.handshake(timeout) {
        Ok(tools) => Ok((server, tools)),
        Err(e) => {
            server.kill();
            thread::sleep(Duration::from_millis(50)); // let the stderr drain catch up
            let tail = server.stderr_text();
            let tail = if tail.is_empty() { String::new() } else { format!(" (stderr: {tail})") };
            Err(format!("MCP server {}: {e}{tail}", cfg.name))
        }
    }
}

impl McpHub {
    pub fn empty() -> McpHub {
        McpHub { servers: Vec::new(), specs: Vec::new(), map: HashMap::new(), errors: Vec::new() }
    }

    /// Start every server in parallel, handshake and list tools. Failed servers are left out and described in `errors()`.
    pub fn start(servers: &[McpServerConfig], cwd: &Path, startup_timeout: Duration) -> McpHub {
        let workers: Vec<_> = servers
            .iter()
            .cloned()
            .map(|cfg| {
                let cwd = cwd.to_path_buf();
                thread::spawn(move || connect(&cfg, &cwd, startup_timeout))
            })
            .collect();
        let mut hub = McpHub::empty();
        for (cfg, w) in servers.iter().zip(workers) {
            match w.join() {
                Ok(Ok((server, tools))) => hub.add(server, &tools),
                Ok(Err(e)) => hub.errors.push(e),
                Err(_) => hub.errors.push(format!("MCP server {}: startup thread panicked", cfg.name)),
            }
        }
        hub
    }

    fn add(&mut self, server: Server, tools: &[Value]) {
        let idx = self.servers.len();
        for t in tools {
            let Some(orig) = t.get("name").and_then(Value::as_str) else { continue };
            let name = exposed_name(&server.name, orig);
            if self.map.contains_key(&name) {
                continue;
            }
            self.map.insert(name.clone(), (idx, orig.to_string()));
            self.specs.push(ToolSpec {
                name,
                description: t.get("description").and_then(Value::as_str).unwrap_or("").to_string(),
                parameters: t.get("inputSchema").cloned().filter(Value::is_object).unwrap_or_else(|| json!({"type":"object"})),
            });
        }
        self.servers.push(server);
    }

    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Tools offered to the model, named `<server>_<tool>`.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.specs.clone()
    }

    pub fn handles(&self, name: &str) -> bool {
        self.map.contains_key(name)
    }

    /// tools/call, blocking up to `timeout`. Never panics; every failure is an error outcome.
    pub fn call(&self, name: &str, args: &Value, timeout: Duration) -> ToolOutcome {
        let Some((idx, orig)) = self.map.get(name) else {
            return ToolOutcome::err(format!("unknown MCP tool {name}"));
        };
        let params = json!({"name": orig, "arguments": if args.is_null() { json!({}) } else { args.clone() }});
        match self.servers[*idx].request("tools/call", params, timeout) {
            Ok(result) => outcome_from_result(&result),
            Err(e) => ToolOutcome::err(e.text(name, timeout)),
        }
    }

    /// Kill all children (also done in Drop). Later calls return error outcomes.
    pub fn shutdown(&mut self) {
        for s in &self.servers {
            s.kill();
        }
    }
}

/// Map an MCP `CallToolResult` to text + images.
fn outcome_from_result(result: &Value) -> ToolOutcome {
    let mut texts = Vec::new();
    let mut images = Vec::new();
    for b in result.get("content").and_then(Value::as_array).into_iter().flatten() {
        match b.get("type").and_then(Value::as_str) {
            Some("text") => texts.extend(b.get("text").and_then(Value::as_str).map(String::from)),
            Some("image") => {
                if let Some(data) = b.get("data").and_then(Value::as_str) {
                    let mime = b.get("mimeType").and_then(Value::as_str).unwrap_or("image/png");
                    images.push(ToolImage { mime: mime.to_string(), b64: data.to_string() });
                }
            }
            Some("resource") => texts.extend(b.pointer("/resource/text").and_then(Value::as_str).map(String::from)),
            _ => {}
        }
    }
    ToolOutcome {
        text: texts.join("\n"),
        is_error: result.get("isError").and_then(Value::as_bool).unwrap_or(false),
        images,
        ..Default::default()
    }
}

impl Drop for McpHub {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    const T: Duration = Duration::from_secs(10);

    fn fake(name: &str, env: &[(&str, &str)]) -> McpServerConfig {
        McpServerConfig {
            name: name.into(),
            command: "node".into(),
            args: vec![concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/fake-mcp.mjs").into(), name.into()],
            env: env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        }
    }

    fn start(cfgs: &[McpServerConfig]) -> McpHub {
        McpHub::start(cfgs, Path::new("."), T)
    }

    #[test]
    fn parses_stdio_and_skips_http() {
        let v = json!([
            {"name":"agents","command":"node","args":["a.js"],"env":[{"name":"X","value":"y"}]},
            {"type":"http","name":"web","url":"http://x","headers":[]},
            {"name":"nocmd"}
        ]);
        let (cfgs, notes) = parse_servers(&v);
        assert_eq!(
            cfgs,
            vec![McpServerConfig { name: "agents".into(), command: "node".into(), args: vec!["a.js".into()], env: vec![("X".into(), "y".into())] }]
        );
        assert_eq!(notes.len(), 2);
        assert!(notes[0].contains("web") && notes[0].contains("http"));
        assert!(parse_servers(&json!(null)).0.is_empty());
    }

    #[test]
    fn lists_prefixed_tools() {
        let hub = start(&[fake("fake", &[])]);
        assert!(hub.errors().is_empty(), "{:?}", hub.errors());
        let names: Vec<_> = hub.specs().into_iter().map(|s| s.name).collect();
        for n in ["fake_echo", "fake_image", "fake_fail", "fake_slow", "fake_use_tool"] {
            assert!(names.contains(&n.to_string()), "{n} in {names:?}");
            assert!(hub.handles(n));
        }
        assert!(!hub.handles("echo"));
        assert_eq!(hub.specs()[0].parameters["type"], "object");
    }

    #[test]
    fn echo_and_use_tool() {
        let hub = start(&[fake("fake", &[])]);
        let o = hub.call("fake_echo", &json!({"text":"hi"}), T);
        assert_eq!((o.text.as_str(), o.is_error), ("echo: hi", false));
        let o = hub.call("fake_use_tool", &json!({"name":"generate_image","arguments":{"p":1}}), T);
        assert_eq!(o.text, r#"ran generate_image with {"p":1}"#);
    }

    #[test]
    fn image_block_becomes_tool_image() {
        let hub = start(&[fake("fake", &[])]);
        let o = hub.call("fake_image", &json!({}), T);
        assert_eq!(o.text, "Image made.");
        assert_eq!(o.images.len(), 1);
        assert_eq!(o.images[0].mime, "image/png");
        let bytes = base64::engine::general_purpose::STANDARD.decode(&o.images[0].b64).unwrap();
        assert_eq!(bytes, [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', b'f', b'a', b'k', b'e']);
    }

    #[test]
    fn is_error_flag() {
        let hub = start(&[fake("fake", &[])]);
        let o = hub.call("fake_fail", &json!({}), T);
        assert!(o.is_error);
        assert_eq!(o.text, "it failed");
    }

    #[test]
    fn timeout_then_server_still_answers() {
        let hub = start(&[fake("fake", &[])]);
        let o = hub.call("fake_slow", &json!({"ms":3000}), Duration::from_millis(500));
        assert!(o.is_error && o.text.contains("did not answer within"), "{}", o.text);
        let o = hub.call("fake_echo", &json!({"text":"later"}), T);
        assert_eq!(o.text, "echo: later");
    }

    #[test]
    fn follows_pagination() {
        let hub = start(&[fake("fake", &[("FAKE_MCP_PAGES", "1")])]);
        assert_eq!(hub.specs().len(), 5);
        assert!(hub.handles("fake_use_tool"));
    }

    #[test]
    fn hang_returns_within_startup_timeout() {
        let t = Instant::now();
        let hub = McpHub::start(&[fake("fake", &[("FAKE_MCP_HANG", "1")])], Path::new("."), Duration::from_secs(1));
        assert!(t.elapsed() < Duration::from_secs(5));
        assert!(hub.specs().is_empty());
        assert_eq!(hub.errors().len(), 1);
        assert!(hub.errors()[0].contains("initialize"), "{:?}", hub.errors());
    }

    #[test]
    fn missing_command_is_an_error_not_a_panic() {
        let bad = McpServerConfig { name: "bad".into(), command: "definitely-not-a-command-xyz".into(), args: vec![], env: vec![] };
        let hub = start(&[bad, fake("fake", &[])]);
        assert_eq!(hub.errors().len(), 1);
        assert!(hub.errors()[0].contains("bad"));
        assert!(hub.handles("fake_echo"));
        assert!(!hub.call("nope", &json!({}), T).text.is_empty());
    }

    #[test]
    fn two_servers_get_distinct_prefixes() {
        let hub = start(&[fake("one", &[]), fake("two", &[])]);
        assert!(hub.handles("one_echo") && hub.handles("two_echo"));
        assert_eq!(hub.specs().len(), 10);
        assert_eq!(hub.call("two_echo", &json!({"text":"x"}), T).text, "echo: x");
    }

    #[test]
    fn exposed_names_are_sanitized_and_capped() {
        assert_eq!(exposed_name("my.srv", "do thing"), "my_srv_do_thing");
        assert_eq!(exposed_name("s", &"a".repeat(100)).len(), 64);
    }

    #[test]
    fn concurrent_calls() {
        let hub = Arc::new(start(&[fake("fake", &[])]));
        let hs: Vec<_> = (0..4)
            .map(|i| {
                let hub = hub.clone();
                thread::spawn(move || hub.call("fake_echo", &json!({"text": format!("t{i}")}), T).text)
            })
            .collect();
        for (i, h) in hs.into_iter().enumerate() {
            assert_eq!(h.join().unwrap(), format!("echo: t{i}"));
        }
    }

    #[test]
    fn shutdown_kills_children() {
        let mut hub = start(&[fake("fake", &[])]);
        hub.shutdown();
        assert!(hub.servers.iter().all(|s| s.child.lock().try_wait().unwrap().is_some()));
        let o = hub.call("fake_echo", &json!({"text":"x"}), Duration::from_secs(2));
        assert!(o.is_error);
    }
}
