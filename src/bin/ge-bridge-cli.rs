//! Console CLI for testing and diagnosing a running ge-bridge (`ge-bridge-cli.exe doctor`).
//! It talks to the local server over HTTP with the stored pairing token; it never starts the server.

use base64::Engine;
use ge_bridge::http_client::{self, SendError};
use ge_bridge::{config, logview, omb, state};
use serde_json::{json, Value};
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const HELP: &str = "ge-bridge-cli: test and diagnose a running ge-bridge

USAGE: ge-bridge-cli <command> [options]

COMMANDS
  doctor                     Check server, token, extension, Gemini tab, models, OpenMausBot link
  status                     One-screen summary of the running server
  models [--refresh]         List models (cached 24 h); --refresh rescans the Gemini model picker now
  tabs                       List the Gemini tabs and API lanes the server can use
  chat <prompt> [--model M]  Send one prompt through /v1/chat/completions and print the answer
  image <prompt> [--size WxH] [--out FILE]
                             Generate one image through /v1/images/generations (default out: image-<time>.png)
  logs [--ext] [-n N] [--level L] [--grep TEXT] [--json]
                             Show the last N entries (default 30) of server.log, or extension.log with --ext,
                             one readable line each in local time. --level debug|info|warn|error is the
                             lowest level shown (default info; debug shows everything), --grep keeps
                             entries containing TEXT, --json prints the raw JSON lines
  acp                        Run as an ACP agent on stdio (for OpenMausBot customAcp / Zed)
  link-openmausbot           Write or refresh the OpenMausBot instance and image provider
  token                      Print the pairing token (keep it private)
  help                       Show this help

Port and data folder follow GE_BRIDGE_PORT and GE_BRIDGE_DATA_DIR, like the server.";

struct Client {
    port: u16,
    token: String,
}

struct Reply {
    status: u16,
    body: Value,
}

impl Client {
    fn new() -> Self {
        let (port, token) = http_client::local_endpoint();
        Self { port, token }
    }

    /// Minimal HTTP/1.0 client: the server answers with a plain body and closes, so no chunking.
    fn call(&self, method: &str, path: &str, body: Option<&Value>, timeout_secs: u64) -> Result<Reply, String> {
        let mut stream = http_client::send(self.port, &self.token, method, path, body, Duration::from_secs(timeout_secs))
            .map_err(|e| match e {
                SendError::Connect(e) => format!("ge-bridge is not running on 127.0.0.1:{} ({e})", self.port),
                SendError::Write(e) => e.to_string(),
            })?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).map_err(|e| format!("no answer from ge-bridge: {e}"))?;
        let text = String::from_utf8_lossy(&raw);
        let (head, rest) = text.split_once("\r\n\r\n").ok_or("malformed HTTP answer")?;
        let status = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        Ok(Reply { status, body: serde_json::from_str(rest).unwrap_or(Value::String(rest.to_string())) })
    }

    fn get(&self, path: &str) -> Result<Reply, String> {
        self.call("GET", path, None, 15)
    }
}

fn error_message(r: &Reply) -> String {
    r.body.pointer("/error/message").and_then(Value::as_str).map(String::from)
        .unwrap_or_else(|| format!("HTTP {}", r.status))
}

fn age(secs: Option<u64>) -> String {
    match secs {
        None => "never".into(),
        Some(s) if s < 3600 => format!("{} min ago", s / 60),
        Some(s) => format!("{:.1} h ago", s as f64 / 3600.0),
    }
}

fn status(c: &Client) -> Result<Value, String> {
    let r = c.get("/api/status")?;
    match r.status {
        200 => Ok(r.body),
        401 => Err("the stored token was rejected (401). Is this the same Windows user and data folder as the server?".into()),
        _ => Err(error_message(&r)),
    }
}

fn cmd_doctor(c: &Client) -> bool {
    let mut ok = true;
    let mut check = |pass: bool, good: String, bad: String| {
        if pass {
            println!("  ✔ {good}");
        } else {
            println!("  ✘ {bad}");
            ok = false;
        }
    };
    println!("ge-bridge doctor (127.0.0.1:{})", c.port);

    let health = c.get("/api/health");
    check(
        matches!(&health, Ok(r) if r.status == 200),
        match &health { Ok(r) => format!("server answers (version {})", r.body["version"].as_str().unwrap_or("?")), Err(_) => String::new() },
        match &health { Err(e) => format!("{e}. Start ge-bridge.exe."), Ok(r) => format!("health check returned HTTP {}", r.status) },
    );
    if health.is_err() {
        return false;
    }
    let st = match status(c) {
        Ok(s) => {
            check(true, "pairing token accepted".into(), String::new());
            s
        }
        Err(e) => {
            check(false, String::new(), e);
            return false;
        }
    };

    check(st["extensionConnected"] == true, "browser extension connected".into(),
        "browser extension not connected: load extension/ in Chrome or Edge and open its popup".into());
    let browser = st["browserTabs"].as_u64().unwrap_or(0);
    check(browser > 0, format!("{browser} Gemini browser tab(s) open"),
        "no Gemini tab open: open your Gemini Enterprise page and sign in (API lanes still work without one)".into());
    check(st["geminiTargetSet"] == true, "Gemini home URL known".into(),
        "Gemini home URL unknown: open your Gemini Enterprise home page once".into());
    let models = st["models"].as_array().map(Vec::len).unwrap_or(0);
    let model_age = st["modelsAgeSecs"].as_u64();
    check(models > 0 && model_age.is_some(),
        format!("{models} models, scanned {}; default {}", age(model_age), st["defaultModel"].as_str().unwrap_or("?")),
        "models never scanned from Gemini: click 🔄 Models on the page pill, or run `ge-bridge-cli models --refresh`".into());

    let maus = dirs::home_dir().map(|h| h.join(".openmausbot").join("config.json"));
    match maus.as_ref().filter(|p| p.exists()).and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str::<Value>(&s).ok()) {
        None => println!("  · OpenMausBot not found (skip)"),
        Some(cfg) => {
            let cli = cfg["instances"][omb::link::ACP_INSTANCE]["config"]["cli"].as_str().unwrap_or("");
            let exe = cli.trim_start_matches('"').split(".exe").next().map(|p| format!("{p}.exe")).unwrap_or_default();
            check(!cli.is_empty() && std::path::Path::new(&exe).exists(),
                format!("OpenMausBot ACP engine runs {cli}"),
                "OpenMausBot ACP engine missing or its exe moved: run `ge-bridge-cli link-openmausbot`, then restart OpenMausBot".into());
            if cfg["instances"].get(omb::link::OLD_INSTANCE).is_some() {
                println!("  · the old `{}` instance is still there; `ge-bridge-cli link-openmausbot` replaces it", omb::link::OLD_INSTANCE);
            }
            let img = &cfg["imageGen"];
            let img_here = img["primary"]["provider"] == "custom"
                && img["customUrl"].as_str() == Some(&format!("http://127.0.0.1:{}/v1", c.port));
            if img_here {
                check(img["customApiKey"].as_str() == Some(&c.token), "OpenMausBot image generation uses ge-bridge".into(),
                    "OpenMausBot image provider has an old token: run `ge-bridge-cli link-openmausbot`".into());
            } else {
                println!("  · OpenMausBot image generation uses another provider (left as is)");
            }
        }
    }
    println!("{}", if ok { "All checks passed." } else { "Some checks failed; see above." });
    ok
}

fn cmd_status(c: &Client) -> Result<(), String> {
    let st = status(c)?;
    println!("server        127.0.0.1:{}", c.port);
    println!("extension     {}", if st["extensionConnected"] == true { "connected" } else { "not connected" });
    println!("tabs          {} browser, {} total", st["browserTabs"], st["tabList"].as_array().map(Vec::len).unwrap_or(0));
    println!("default model {}", st["defaultModel"].as_str().unwrap_or("?"));
    println!("models        {} (scanned {})", st["models"].as_array().map(Vec::len).unwrap_or(0), age(st["modelsAgeSecs"].as_u64()));
    println!("gemini url    {}", if st["geminiTargetSet"] == true { "known" } else { "unknown" });
    Ok(())
}

fn cmd_models(c: &Client, refresh: bool) -> Result<(), String> {
    if refresh {
        let before = status(c)?["modelsAgeSecs"].as_u64();
        let r = c.call("POST", "/api/refresh-models", Some(&json!({})), 15)?;
        if r.body["sent"] != true {
            return Err("could not reach the extension to rescan (is it connected?)".into());
        }
        println!("Rescanning the Gemini model picker...");
        let mut done = false;
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(500));
            let now = status(c)?["modelsAgeSecs"].as_u64();
            if now.is_some() && (before.is_none() || now < before || now == Some(0)) {
                done = true;
                break;
            }
        }
        if !done {
            println!("No new list within 10 s (is a Gemini tab open?). Showing the cached list.");
        }
    }
    let st = status(c)?;
    let default = st["defaultModel"].as_str().unwrap_or("");
    println!("Models (scanned {}, reused for 24 h):", age(st["modelsAgeSecs"].as_u64()));
    for m in st["models"].as_array().into_iter().flatten() {
        let id = m["id"].as_str().unwrap_or("");
        println!("  {} {:<28} {}", if id == default { "*" } else { " " }, id, m["display_name"].as_str().unwrap_or(""));
    }
    println!("* = default (used for `default`, `auto`, claude-* and gpt-* requests)");
    Ok(())
}

fn cmd_tabs(c: &Client) -> Result<(), String> {
    let st = status(c)?;
    let list = st["tabList"].as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        println!("No Gemini tabs reported by the extension.");
    }
    for t in list {
        println!("  #{:<8} {}", t["tabId"], t["label"].as_str().unwrap_or(""));
    }
    Ok(())
}

fn cmd_chat(c: &Client, prompt: &str, model: &str) -> Result<(), String> {
    let body = json!({ "model": model, "stream": false, "messages": [{ "role": "user", "content": prompt }] });
    let r = c.call("POST", "/v1/chat/completions", Some(&body), 300)?;
    if r.status != 200 {
        return Err(error_message(&r));
    }
    println!("[{}]", r.body["model"].as_str().unwrap_or(model));
    println!("{}", r.body.pointer("/choices/0/message/content").and_then(Value::as_str).unwrap_or(""));
    Ok(())
}

fn cmd_image(c: &Client, prompt: &str, size: Option<&str>, out: Option<PathBuf>) -> Result<(), String> {
    let mut body = json!({ "model": ge_bridge::omb::link::IMAGE_MODEL_ID, "prompt": prompt, "response_format": "b64_json", "n": 1 });
    if let Some(s) = size {
        body["size"] = json!(s);
    }
    println!("Generating (this can take up to two minutes)...");
    let r = c.call("POST", "/v1/images/generations", Some(&body), 130)?;
    if r.status != 200 {
        return Err(error_message(&r));
    }
    let b64 = r.body.pointer("/data/0/b64_json").and_then(Value::as_str).ok_or("no b64_json in the answer")?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|e| format!("bad base64: {e}"))?;
    let ext = if bytes.starts_with(&[0xff, 0xd8]) { "jpg" } else if bytes.starts_with(b"RIFF") { "webp" } else { "png" };
    let path = out.unwrap_or_else(|| PathBuf::from(format!("image-{}.{ext}", state::now_secs())));
    std::fs::write(&path, &bytes).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    println!("Saved {} ({} KB)", path.display(), bytes.len() / 1024);
    Ok(())
}

fn cmd_logs(ext: bool, n: usize, level: Option<&str>, grep: Option<&str>, json: bool) -> Result<(), String> {
    let filter = logview::Filter { min_level: level.map(logview::parse_level).transpose()?.unwrap_or(1), grep: grep.map(str::to_string) };
    config::init(config::RuntimeConfig::from_env());
    let path = state::logs_dir().join(if ext { "extension.log" } else { "server.log" });
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let lines = logview::render(&text, &filter, n, json, logview::local_utc_offset_secs());
    if lines.is_empty() {
        println!("(no entries match in {})", path.display());
    }
    for l in lines {
        println!("{l}");
    }
    Ok(())
}

/// `--name value` from the remaining args; the rest are positional and joined into the prompt.
fn split_args(args: &[String]) -> (Vec<String>, std::collections::HashMap<String, String>, Vec<String>) {
    let mut pos = Vec::new();
    let mut opts = std::collections::HashMap::new();
    let mut flags = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if matches!(a.as_str(), "--model" | "--size" | "--out" | "-o" | "-n" | "--level" | "--grep") && i + 1 < args.len() {
            opts.insert(a.trim_start_matches('-').to_string(), args[i + 1].clone());
            i += 2;
            continue;
        }
        if a.starts_with("--") {
            flags.push(a.clone());
        } else {
            pos.push(a.clone());
        }
        i += 1;
    }
    (pos, opts, flags)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().map(String::as_str) else {
        println!("{HELP}");
        return ExitCode::SUCCESS;
    };
    // ACP clients probe `<cli> --version` (OpenMausBot runs `ge-bridge-cli acp --version`) before spawning.
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("ge-bridge-cli {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if cmd == "acp" {
        return ge_bridge::acp::run();
    }
    let (pos, opts, flags) = split_args(&args[1..]);
    let prompt = pos.join(" ");
    let result: Result<(), String> = match cmd {
        "doctor" => return if cmd_doctor(&Client::new()) { ExitCode::SUCCESS } else { ExitCode::FAILURE },
        "status" => cmd_status(&Client::new()),
        "models" => cmd_models(&Client::new(), flags.iter().any(|f| f == "--refresh")),
        "tabs" => cmd_tabs(&Client::new()),
        "chat" if !prompt.is_empty() => cmd_chat(&Client::new(), &prompt, opts.get("model").map(String::as_str).unwrap_or("default")),
        "image" if !prompt.is_empty() => cmd_image(
            &Client::new(),
            &prompt,
            opts.get("size").map(String::as_str),
            opts.get("out").or_else(|| opts.get("o")).map(PathBuf::from),
        ),
        "logs" => cmd_logs(
            flags.iter().any(|f| f == "--ext"),
            opts.get("n").and_then(|v| v.parse().ok()).unwrap_or(30),
            opts.get("level").map(String::as_str),
            opts.get("grep").map(String::as_str),
            flags.iter().any(|f| f == "--json"),
        ),
        "link-openmausbot" => {
            let c = Client::new();
            c.call("POST", "/api/link-openmausbot", Some(&json!({})), 15).and_then(|r| {
                if r.status == 200 {
                    println!("Linked: {}. Restart OpenMausBot to load it.", r.body["path"].as_str().unwrap_or("OpenMausBot config"));
                    match r.body["movedBots"].as_u64() {
                        Some(n) => println!("{n} bot/thread settings moved from `{}` to `{}`.", omb::link::OLD_INSTANCE, omb::link::ACP_INSTANCE),
                        None => println!("OpenMausBot is running, so bots.json was left alone: quit OpenMausBot and run this again to move bots to `{}`.", omb::link::ACP_INSTANCE),
                    }
                    Ok(())
                } else {
                    Err(error_message(&r))
                }
            })
        }
        "token" => {
            println!("{}", Client::new().token);
            Ok(())
        }
        "help" | "--help" | "-h" => {
            println!("{HELP}");
            Ok(())
        }
        "chat" | "image" => Err(format!("`{cmd}` needs a prompt, e.g. ge-bridge-cli {cmd} \"a red apple\"")),
        other => Err(format!("unknown command `{other}`; run `ge-bridge-cli help`")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
