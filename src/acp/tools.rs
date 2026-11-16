//! Local file and command tools confined to the session folder (the ACP `cwd`).
//!
//! Every path goes through `LocalTools::resolve`, which canonicalizes and refuses anything outside the root
//! (`..`, absolute paths elsewhere, symlinks/junctions that point out). Secret files are refused on top of that.

use super::types::{Risk, ToolOutcome, ToolSpec};
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MAX_READ_BYTES: u64 = 5 * 1024 * 1024;
const MAX_GREP_FILE_BYTES: u64 = 2 * 1024 * 1024;
const DEFAULT_READ_LIMIT: usize = 2000;
const MAX_LINE_CHARS: usize = 2000;
const MAX_GREP_LINE_CHARS: usize = 300;
const MAX_LIST_ENTRIES: usize = 500;
const MAX_GREP_MATCHES: usize = 200;
const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 600;
const OUTPUT_CAP: usize = 30_000;
const OUTPUT_HEAD: usize = 10_000;
const OUTPUT_TAIL: usize = 20_000;
const SKIP_DIRS: [&str; 4] = [".git", "node_modules", "target", "__pycache__"];
const TOOL_NAMES: [&str; 6] = ["read_file", "write_file", "edit_file", "list_dir", "grep", "run_command"];

/// The built-in tool set, bound to one canonical root folder.
pub struct LocalTools {
    root: PathBuf,
}

impl LocalTools {
    /// `cwd` must exist and be a directory; its canonical path becomes the root.
    pub fn new(cwd: &Path) -> Result<Self, String> {
        let root = std::fs::canonicalize(cwd).map_err(|e| format!("working folder {} is not usable: {e}", cwd.display()))?;
        if !root.is_dir() {
            return Err(format!("working folder {} is not a directory", cwd.display()));
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The tool list offered to the model.
    pub fn specs() -> Vec<ToolSpec> {
        fn spec(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolSpec {
            ToolSpec {
                name: name.into(),
                description: description.into(),
                parameters: json!({ "type": "object", "properties": properties, "required": required }),
            }
        }
        vec![
            spec(
                "read_file",
                "Read a text file in the working folder. Output has line numbers. Use offset/limit for big files.",
                json!({
                    "path": {"type": "string", "description": "File path, relative to the working folder"},
                    "offset": {"type": "integer", "description": "1-based first line to read (default 1)"},
                    "limit": {"type": "integer", "description": "Max lines to return (default 2000)"}
                }),
                &["path"],
            ),
            spec(
                "write_file",
                "Create or overwrite a file with the given content. Missing parent folders are created.",
                json!({
                    "path": {"type": "string", "description": "File path, relative to the working folder"},
                    "content": {"type": "string", "description": "Full new content of the file"}
                }),
                &["path", "content"],
            ),
            spec(
                "edit_file",
                "Replace an exact string in a file. old_string must match once unless replace_all is true.",
                json!({
                    "path": {"type": "string", "description": "File path, relative to the working folder"},
                    "old_string": {"type": "string", "description": "Exact text to find (include enough context to be unique)"},
                    "new_string": {"type": "string", "description": "Replacement text"},
                    "replace_all": {"type": "boolean", "description": "Replace every occurrence (default false)"}
                }),
                &["path", "old_string", "new_string"],
            ),
            spec(
                "list_dir",
                "List files and folders (folders end with /). Skips .git, node_modules, target, __pycache__.",
                json!({
                    "path": {"type": "string", "description": "Folder to list (default \".\")"},
                    "depth": {"type": "integer", "description": "Levels to descend, 1-4 (default 1)"}
                }),
                &[],
            ),
            spec(
                "grep",
                "Search text files for a literal substring (not a regex). Returns path:line: text.",
                json!({
                    "pattern": {"type": "string", "description": "Literal text to look for"},
                    "path": {"type": "string", "description": "File or folder to search (default \".\")"},
                    "glob": {"type": "string", "description": "File name filter with * and ?, e.g. \"*.rs\""},
                    "ignore_case": {"type": "boolean", "description": "Case-insensitive match (default false)"}
                }),
                &["pattern"],
            ),
            spec(
                "run_command",
                "Run a PowerShell command in the working folder. Returns exit code, stdout and stderr.",
                json!({
                    "command": {"type": "string", "description": "The command line to run"},
                    "timeout_secs": {"type": "integer", "description": "Kill after this many seconds (default 120, max 600)"}
                }),
                &["command"],
            ),
        ]
    }

    pub fn handles(name: &str) -> bool {
        TOOL_NAMES.contains(&name)
    }

    pub fn risk(name: &str) -> Risk {
        match name {
            "write_file" | "edit_file" => Risk::Write,
            "run_command" => Risk::Execute,
            _ => Risk::Read,
        }
    }

    /// ACP ToolKind.
    pub fn kind(name: &str) -> &'static str {
        match name {
            "read_file" | "list_dir" => "read",
            "write_file" | "edit_file" => "edit",
            "grep" => "search",
            "run_command" => "execute",
            _ => "other",
        }
    }

    /// Short human title for the approval card and tool card.
    pub fn title(name: &str, args: &Value) -> String {
        let s = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let n = |k: &str| args.get(k).and_then(Value::as_u64);
        match name {
            "read_file" => {
                let path = s("path");
                if n("offset").is_some() || n("limit").is_some() {
                    let start = n("offset").unwrap_or(1).max(1);
                    let end = start + n("limit").unwrap_or(DEFAULT_READ_LIMIT as u64).max(1) - 1;
                    format!("Read {path} (lines {start}-{end})")
                } else {
                    format!("Read {path}")
                }
            }
            "write_file" => format!("Write {}", s("path")),
            "edit_file" => format!("Edit {}", s("path")),
            "list_dir" => {
                let p = s("path");
                format!("List {}", if p.is_empty() { "." } else { &p })
            }
            "grep" => {
                let p = s("path");
                let scope = if p.is_empty() || p == "." { String::new() } else { format!(" in {p}") };
                format!("Search `{}`{scope}", cut_chars(&s("pattern"), 60))
            }
            "run_command" => {
                let cmd = s("command");
                let first = cmd.lines().next().unwrap_or("").trim();
                let more = if cmd.trim().lines().count() > 1 { " …" } else { "" };
                format!("Run `{}`{more}", cut_chars(first, 80))
            }
            other => other.to_string(),
        }
    }

    /// Run one call. Never panics; failures come back as `ToolOutcome::err`.
    pub fn call(&self, name: &str, args: &Value, cancel: &AtomicBool) -> ToolOutcome {
        let result = match name {
            "read_file" => self.read_file(args),
            "write_file" => self.write_file(args),
            "edit_file" => self.edit_file(args),
            "list_dir" => self.list_dir(args),
            "grep" => self.grep(args),
            "run_command" => self.run_command(args, cancel),
            other => Err(format!("unknown tool `{other}`")),
        };
        result.unwrap_or_else(ToolOutcome::err)
    }

    // ---- path safety ----

    /// Resolve a model-supplied path to an absolute path inside the root, or explain why not.
    fn resolve(&self, raw: &str) -> Result<PathBuf, String> {
        let raw = if raw.trim().is_empty() { "." } else { raw };
        let outside = || format!("{raw} is outside the working folder {}", display_abs(&self.root));
        let joined = self.root.join(raw);
        let comps: Vec<Component> = joined.components().collect();
        let mut k = comps.len();
        let (mut out, rest_from) = loop {
            if k == 0 {
                return Err(outside());
            }
            let prefix: PathBuf = comps[..k].iter().collect();
            if let Ok(canon) = std::fs::canonicalize(&prefix) {
                break (canon, k);
            }
            k -= 1;
        };
        for c in &comps[rest_from..] {
            match c {
                Component::Normal(n) => out.push(n),
                Component::CurDir => {}
                _ => return Err(outside()),
            }
        }
        if out.starts_with(&self.root) {
            Ok(out)
        } else {
            Err(outside())
        }
    }

    /// Path shown to the model: relative to the root, forward slashes.
    fn rel(&self, abs: &Path) -> String {
        let r = abs.strip_prefix(&self.root).unwrap_or(abs);
        let s = display_abs(r);
        if s.is_empty() {
            ".".into()
        } else {
            s
        }
    }

    /// Resolve and refuse secret files (and `.git`/`.ssh` internals when writing).
    fn resolve_checked(&self, raw: &str, write: bool) -> Result<PathBuf, String> {
        let abs = self.resolve(raw)?;
        let rel = self.rel(&abs);
        let name = abs.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let in_protected = abs
            .strip_prefix(&self.root)
            .map(|r| r.components().any(|c| matches!(c.as_os_str().to_str(), Some(".ssh") | Some(".git"))))
            .unwrap_or(false);
        if is_secret_name(name) || (write && in_protected) {
            return Err(format!("{rel} looks like a secret file; refusing"));
        }
        Ok(abs)
    }

    // ---- tools ----

    fn read_file(&self, args: &Value) -> Result<ToolOutcome, String> {
        let raw = arg_str(args, "path")?;
        let abs = self.resolve_checked(raw, false)?;
        let rel = self.rel(&abs);
        let text = read_text(&abs, &rel, MAX_READ_BYTES)?;
        let offset = opt_u64(args, "offset").unwrap_or(1).max(1) as usize;
        let limit = opt_u64(args, "limit").unwrap_or(DEFAULT_READ_LIMIT as u64).max(1) as usize;
        let lines: Vec<&str> = text.lines().collect();
        let total = lines.len();
        if total > 0 && offset > total {
            return Err(format!("{rel} has only {total} lines; offset {offset} is past the end"));
        }
        let start = offset - 1;
        let end = (start + limit).min(total);
        let mut out = String::new();
        for (i, line) in lines[start.min(total)..end].iter().enumerate() {
            out.push_str(&format!("{:>6}\t{}\n", start + i + 1, cut_chars(line, MAX_LINE_CHARS)));
        }
        if end < total {
            out.push_str(&format!("… {} more lines; call again with offset={}\n", total - end, end + 1));
        }
        if out.is_empty() {
            out.push_str("(empty file)\n");
        }
        Ok(with_location(ToolOutcome::ok(out), &abs))
    }

    fn write_file(&self, args: &Value) -> Result<ToolOutcome, String> {
        let abs = self.resolve_checked(arg_str(args, "path")?, true)?;
        let rel = self.rel(&abs);
        let content = arg_str(args, "content")?;
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("cannot create folders for {rel}: {e}"))?;
        }
        std::fs::write(&abs, content).map_err(|e| format!("cannot write {rel}: {e}"))?;
        Ok(with_location(ToolOutcome::ok(format!("Wrote {} bytes to {rel}", content.len())), &abs))
    }

    fn edit_file(&self, args: &Value) -> Result<ToolOutcome, String> {
        let abs = self.resolve_checked(arg_str(args, "path")?, true)?;
        let rel = self.rel(&abs);
        let old = arg_str(args, "old_string")?;
        let new = arg_str(args, "new_string")?;
        let replace_all = args.get("replace_all").and_then(Value::as_bool).unwrap_or(false);
        if old.is_empty() {
            return Err("old_string is empty; give the exact text to replace".into());
        }
        let text = read_text(&abs, &rel, MAX_READ_BYTES)?;
        let count = text.matches(old).count();
        if count == 0 {
            return Err(format!("old_string not found in {rel}; read the file again and copy the text exactly (whitespace included)"));
        }
        if count > 1 && !replace_all {
            return Err(format!(
                "old_string found {count} times in {rel}; add surrounding context to make it unique or set replace_all=true"
            ));
        }
        let updated = if replace_all { text.replace(old, new) } else { text.replacen(old, new, 1) };
        std::fs::write(&abs, updated).map_err(|e| format!("cannot write {rel}: {e}"))?;
        let done = if replace_all { count } else { 1 };
        Ok(with_location(ToolOutcome::ok(format!("Replaced {done} occurrence(s) in {rel}")), &abs))
    }

    fn list_dir(&self, args: &Value) -> Result<ToolOutcome, String> {
        let abs = self.resolve(args.get("path").and_then(Value::as_str).unwrap_or("."))?;
        let rel = self.rel(&abs);
        if !abs.is_dir() {
            return Err(format!("{rel} is not a folder"));
        }
        let depth = opt_u64(args, "depth").unwrap_or(1).clamp(1, 4) as usize;
        let mut lines: Vec<String> = Vec::new();
        let mut capped = false;
        walk(&abs, depth, &mut |p, is_dir| {
            if lines.len() >= MAX_LIST_ENTRIES {
                capped = true;
                return false;
            }
            lines.push(format!("{}{}", self.rel(p), if is_dir { "/" } else { "" }));
            true
        });
        let mut out = lines.join("\n");
        if lines.is_empty() {
            out = format!("({rel} is empty)");
        }
        if capped {
            out.push_str(&format!("\n… listing capped at {MAX_LIST_ENTRIES} entries; narrow the path or lower depth"));
        }
        Ok(ToolOutcome::ok(out))
    }

    fn grep(&self, args: &Value) -> Result<ToolOutcome, String> {
        let pattern = arg_str(args, "pattern")?;
        if pattern.is_empty() {
            return Err("pattern is empty".into());
        }
        let abs = self.resolve(args.get("path").and_then(Value::as_str).unwrap_or("."))?;
        let glob = args.get("glob").and_then(Value::as_str).filter(|g| !g.is_empty());
        let ignore_case = args.get("ignore_case").and_then(Value::as_bool).unwrap_or(false);
        let needle = if ignore_case { pattern.to_lowercase() } else { pattern.to_string() };

        let mut files: Vec<PathBuf> = Vec::new();
        if abs.is_dir() {
            walk(&abs, usize::MAX, &mut |p, is_dir| {
                if !is_dir {
                    files.push(p.to_path_buf());
                }
                true
            });
        } else {
            files.push(abs.clone());
        }

        let mut hits: Vec<String> = Vec::new();
        let mut capped = false;
        'files: for file in files {
            let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if is_secret_name(name) || glob.is_some_and(|g| !glob_match(g, name)) {
                continue;
            }
            let rel = self.rel(&file);
            let Ok(text) = read_text(&file, &rel, MAX_GREP_FILE_BYTES) else { continue };
            for (i, line) in text.lines().enumerate() {
                let found = if ignore_case { line.to_lowercase().contains(&needle) } else { line.contains(&needle) };
                if found {
                    if hits.len() >= MAX_GREP_MATCHES {
                        capped = true;
                        break 'files;
                    }
                    hits.push(format!("{rel}:{}: {}", i + 1, cut_chars(line.trim_end(), MAX_GREP_LINE_CHARS)));
                }
            }
        }
        if hits.is_empty() {
            return Ok(ToolOutcome::ok("No matches."));
        }
        let mut out = hits.join("\n");
        if capped {
            out.push_str(&format!("\n… stopped at {MAX_GREP_MATCHES} matches; narrow the pattern, path or glob"));
        }
        Ok(ToolOutcome::ok(out))
    }

    fn run_command(&self, args: &Value, cancel: &AtomicBool) -> Result<ToolOutcome, String> {
        let command = arg_str(args, "command")?;
        if command.trim().is_empty() {
            return Err("command is empty".into());
        }
        let secs = opt_u64(args, "timeout_secs").unwrap_or(DEFAULT_TIMEOUT_SECS).clamp(1, MAX_TIMEOUT_SECS);

        let mut cmd = shell_command(command);
        cmd.current_dir(&self.root).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        for (k, _) in std::env::vars_os() {
            if is_secret_env(&k.to_string_lossy()) {
                cmd.env_remove(&k);
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = cmd.spawn().map_err(|e| format!("cannot start the shell: {e}"))?;
        let out_buf = spawn_reader(child.stdout.take());
        let err_buf = spawn_reader(child.stderr.take());

        let deadline = Instant::now() + Duration::from_secs(secs);
        let mut stop: Option<String> = None;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {}
                Err(e) => return Err(format!("cannot wait for the command: {e}")),
            }
            if cancel.load(Ordering::SeqCst) {
                stop = Some("Cancelled; the process was killed.".into());
            } else if Instant::now() >= deadline {
                stop = Some(format!("Timed out after {secs}s; the process was killed."));
            }
            if stop.is_some() {
                kill_tree(&mut child);
                break None;
            }
            std::thread::sleep(Duration::from_millis(100));
        };

        let stdout = out_buf.finish();
        let stderr = err_buf.finish();
        let mut text = match (&status, &stop) {
            (Some(s), _) => format!("Exit code: {}", s.code().unwrap_or(-1)),
            (None, Some(why)) => why.clone(),
            (None, None) => String::new(),
        };
        if !stdout.trim().is_empty() {
            text.push('\n');
            text.push_str(stdout.trim_end());
        }
        if !stderr.trim().is_empty() {
            text.push_str("\n[stderr]\n");
            text.push_str(stderr.trim_end());
        }
        let text = cap_output(&text);
        let failed = stop.is_some() || status.is_some_and(|s| !s.success());
        Ok(if failed { ToolOutcome::err(text) } else { ToolOutcome::ok(text) })
    }
}

// ---- helpers ----

fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key).and_then(Value::as_str).ok_or_else(|| format!("missing or non-string argument `{key}`"))
}

fn opt_u64(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

fn with_location(mut outcome: ToolOutcome, abs: &Path) -> ToolOutcome {
    outcome.locations.push(PathBuf::from(display_abs(abs)));
    outcome
}

/// Path as text with forward slashes and no `\\?\` prefix.
fn display_abs(p: &Path) -> String {
    let s = p.to_string_lossy();
    let s = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else {
        s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
    };
    s.replace('\\', "/")
}

/// First `n` chars of `s`, with an ellipsis when cut.
fn cut_chars(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n).collect();
        t.push('…');
        t
    }
}

fn is_secret_name(name: &str) -> bool {
    let n = name.to_lowercase();
    n == ".env"
        || n.starts_with(".env.")
        || [".pem", ".key", ".pfx", ".p12"].iter().any(|e| n.ends_with(e))
        || ["id_rsa", "id_ed25519", "id_ecdsa"].iter().any(|p| n.starts_with(p))
        || matches!(n.as_str(), ".git-credentials" | ".npmrc" | ".pypirc" | "credentials.json")
}

fn is_secret_env(name: &str) -> bool {
    let u = name.to_uppercase();
    ["KEY", "TOKEN", "SECRET", "PASSWORD", "CREDENTIAL"].iter().any(|w| u.contains(w)) || u == "GE_BRIDGE_TOKEN"
}

/// Read a UTF-8-ish text file, refusing big and binary files.
fn read_text(path: &Path, rel: &str, max_bytes: u64) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("cannot read {rel}: {e}"))?;
    if meta.is_dir() {
        return Err(format!("{rel} is a folder, not a file"));
    }
    if meta.len() > max_bytes {
        return Err(format!("{rel} is {} bytes, over the {} MB limit", meta.len(), max_bytes / (1024 * 1024)));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {rel}: {e}"))?;
    if bytes.iter().take(8192).any(|&b| b == 0) {
        return Err(format!("{rel} looks like a binary file; refusing to read it as text"));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Depth-first walk, sorted by name, skipping `SKIP_DIRS` and not following links. `visit` returns false to stop.
fn walk(dir: &Path, depth: usize, visit: &mut dyn FnMut(&Path, bool) -> bool) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else { return true };
    let mut entries: Vec<(String, PathBuf, bool)> = rd
        .flatten()
        .map(|e| {
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            (e.file_name().to_string_lossy().into_owned(), e.path(), is_dir)
        })
        .filter(|(name, _, is_dir)| !(*is_dir && SKIP_DIRS.contains(&name.as_str())))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, path, is_dir) in entries {
        if !visit(&path, is_dir) {
            return false;
        }
        if is_dir && depth > 1 && !walk(&path, depth - 1, visit) {
            return false;
        }
    }
    true
}

/// `*` and `?` wildcards, matched against a whole file name.
fn glob_match(pattern: &str, name: &str) -> bool {
    fn go(p: &[char], n: &[char]) -> bool {
        match p.first() {
            None => n.is_empty(),
            Some('*') => (0..=n.len()).any(|i| go(&p[1..], &n[i..])),
            Some('?') => !n.is_empty() && go(&p[1..], &n[1..]),
            Some(c) => n.first() == Some(c) && go(&p[1..], &n[1..]),
        }
    }
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    go(&p, &n)
}

fn shell_command(command: &str) -> Command {
    if cfg!(windows) {
        let mut c = Command::new("powershell.exe");
        c.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", command]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    }
}

/// Collects a pipe on a thread so a full pipe never blocks the child.
struct Reader {
    buf: Arc<Mutex<Vec<u8>>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

fn spawn_reader<R: Read + Send + 'static>(pipe: Option<R>) -> Reader {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let shared = Arc::clone(&buf);
    let handle = pipe.map(|mut p| {
        std::thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            while let Ok(n) = p.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                if let Ok(mut b) = shared.lock() {
                    b.extend_from_slice(&chunk[..n]);
                }
            }
        })
    });
    Reader { buf, handle }
}

impl Reader {
    /// Output so far. Waits briefly for EOF; a grandchild that kept the pipe open must not hang us.
    fn finish(self) -> String {
        if let Some(h) = self.handle {
            let until = Instant::now() + Duration::from_secs(2);
            while !h.is_finished() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        let bytes = self.buf.lock().map(|b| b.clone()).unwrap_or_default();
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

/// Kill the child and everything it started.
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Keep the head and tail of oversized output.
fn cap_output(text: &str) -> String {
    let total = text.chars().count();
    if total <= OUTPUT_CAP {
        return text.to_string();
    }
    let head: String = text.chars().take(OUTPUT_HEAD).collect();
    let tail: String = text.chars().skip(total - OUTPUT_TAIL).collect();
    format!("{head}\n… {} chars cut …\n{tail}", total - OUTPUT_HEAD - OUTPUT_TAIL)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp {
        dir: PathBuf,
        tools: LocalTools,
    }

    impl Tmp {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("ge-acp-tools-{}-{}", std::process::id(), name));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let tools = LocalTools::new(&dir).unwrap();
            Self { dir, tools }
        }
        fn put(&self, rel: &str, content: &str) {
            let p = self.dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        }
        fn run(&self, name: &str, args: Value) -> ToolOutcome {
            self.tools.call(name, &args, &AtomicBool::new(false))
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir(self.dir.join("inner"));
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn numbered(n: usize) -> String {
        (1..=n).map(|i| format!("line{i}\n")).collect()
    }

    #[test]
    fn read_offset_limit_and_truncation_note() {
        let t = Tmp::new("read");
        t.put("a.txt", &numbered(10));
        let o = t.run("read_file", json!({"path": "a.txt", "offset": 3, "limit": 2}));
        assert!(!o.is_error, "{}", o.text);
        assert!(o.text.contains("     3\tline3\n     4\tline4\n"), "{}", o.text);
        assert!(o.text.contains("6 more lines; call again with offset=5"), "{}", o.text);
        assert_eq!(o.locations.len(), 1);
        assert!(!o.locations[0].to_string_lossy().contains(r"\\?\"));
        let past = t.run("read_file", json!({"path": "a.txt", "offset": 99}));
        assert!(past.is_error);
    }

    #[test]
    fn read_refuses_binary_and_missing() {
        let t = Tmp::new("bin");
        std::fs::write(t.dir.join("b.bin"), [1u8, 0, 2]).unwrap();
        let o = t.run("read_file", json!({"path": "b.bin"}));
        assert!(o.is_error && o.text.contains("binary"), "{}", o.text);
        assert!(t.run("read_file", json!({"path": "nope.txt"})).is_error);
        assert!(t.run("read_file", json!({})).is_error);
    }

    #[test]
    fn write_creates_parents() {
        let t = Tmp::new("write");
        let o = t.run("write_file", json!({"path": "x/y/z.txt", "content": "hello"}));
        assert!(!o.is_error, "{}", o.text);
        assert_eq!(o.text, "Wrote 5 bytes to x/y/z.txt");
        assert_eq!(std::fs::read_to_string(t.dir.join("x/y/z.txt")).unwrap(), "hello");
    }

    #[test]
    fn edit_cases() {
        let t = Tmp::new("edit");
        t.put("f.txt", "one two\r\nthree two\r\n");
        let missing = t.run("edit_file", json!({"path": "f.txt", "old_string": "zzz", "new_string": "q"}));
        assert!(missing.is_error && missing.text.contains("not found"));
        let empty = t.run("edit_file", json!({"path": "f.txt", "old_string": "", "new_string": "q"}));
        assert!(empty.is_error);
        let multi = t.run("edit_file", json!({"path": "f.txt", "old_string": "two", "new_string": "2"}));
        assert!(multi.is_error && multi.text.contains("2 times"), "{}", multi.text);
        let one = t.run("edit_file", json!({"path": "f.txt", "old_string": "one", "new_string": "1"}));
        assert_eq!(one.text, "Replaced 1 occurrence(s) in f.txt");
        let all = t.run("edit_file", json!({"path": "f.txt", "old_string": "two", "new_string": "2", "replace_all": true}));
        assert_eq!(all.text, "Replaced 2 occurrence(s) in f.txt");
        assert_eq!(std::fs::read_to_string(t.dir.join("f.txt")).unwrap(), "1 2\r\nthree 2\r\n");
    }

    #[test]
    fn list_dir_depth_and_skip() {
        let t = Tmp::new("list");
        t.put("src/main.rs", "");
        t.put("src/deep/mod.rs", "");
        t.put("node_modules/x.js", "");
        t.put("target/out", "");
        t.put(".git/HEAD", "");
        t.put("README.md", "");
        let d1 = t.run("list_dir", json!({}));
        assert_eq!(d1.text, "README.md\nsrc/");
        let d2 = t.run("list_dir", json!({"depth": 2}));
        assert_eq!(d2.text, "README.md\nsrc/\nsrc/deep/\nsrc/main.rs");
        let d3 = t.run("list_dir", json!({"path": "src", "depth": 9}));
        assert!(d3.text.contains("src/deep/mod.rs"), "{}", d3.text);
        assert!(!d3.text.contains("node_modules"));
    }

    #[test]
    fn grep_glob_and_ignore_case() {
        let t = Tmp::new("grep");
        t.put("a.rs", "fn Hello() {}\nlet x = 1;\n");
        t.put("b.txt", "hello world\n");
        t.put(".env", "hello=secret\n");
        t.put("node_modules/c.rs", "hello\n");
        let sens = t.run("grep", json!({"pattern": "hello"}));
        assert_eq!(sens.text, "b.txt:1: hello world");
        let ci = t.run("grep", json!({"pattern": "hello", "ignore_case": true, "glob": "*.rs"}));
        assert_eq!(ci.text, "a.rs:1: fn Hello() {}");
        let none = t.run("grep", json!({"pattern": "absent"}));
        assert_eq!(none.text, "No matches.");
        assert!(glob_match("*.r?", "a.rs") && !glob_match("*.rs", "a.rsx"));
    }

    #[test]
    fn paths_outside_root_refused() {
        let t = Tmp::new("outside");
        for p in [r"..\..\x", "../x.txt", "a/../../x"] {
            let o = t.run("write_file", json!({"path": p, "content": "x"}));
            assert!(o.is_error && o.text.contains("outside the working folder"), "{p}: {}", o.text);
        }
        let other = std::env::temp_dir().join(format!("ge-acp-tools-{}-other.txt", std::process::id()));
        std::fs::write(&other, "x").unwrap();
        let o = t.run("read_file", json!({"path": other.to_string_lossy()}));
        let _ = std::fs::remove_file(&other);
        assert!(o.is_error && o.text.contains("outside the working folder"), "{}", o.text);
        // absolute path inside the root is fine
        t.put("in.txt", "ok\n");
        let inside = t.dir.join("in.txt");
        assert!(!t.run("read_file", json!({"path": inside.to_string_lossy()})).is_error);
    }

    #[cfg(windows)]
    #[test]
    fn junction_out_of_root_refused() {
        let t = Tmp::new("junction");
        let outside = std::env::temp_dir().join(format!("ge-acp-tools-{}-junction-target", std::process::id()));
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("s.txt"), "outside").unwrap();
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(t.dir.join("inner"))
            .arg(&outside)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if made {
            let r = t.run("read_file", json!({"path": "inner/s.txt"}));
            assert!(r.is_error && r.text.contains("outside the working folder"), "{}", r.text);
            let w = t.run("write_file", json!({"path": "inner/new.txt", "content": "x"}));
            assert!(w.is_error, "{}", w.text);
            assert!(!outside.join("new.txt").exists());
        }
        let _ = std::fs::remove_dir(t.dir.join("inner"));
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn secret_files_refused() {
        let t = Tmp::new("secret");
        t.put(".env", "A=1");
        t.put("sub/server.pem", "x");
        t.put(".git/config", "[core]\n");
        for p in [".env", ".env.local", "sub/server.pem", "id_rsa", "a/credentials.json", ".npmrc"] {
            t.put(p, "x");
            assert!(t.run("read_file", json!({"path": p})).text.contains("looks like a secret file"), "{p}");
            assert!(t.run("write_file", json!({"path": p, "content": "y"})).is_error, "{p}");
            assert!(t.run("edit_file", json!({"path": p, "old_string": "x", "new_string": "y"})).is_error, "{p}");
        }
        assert!(!t.run("read_file", json!({"path": ".git/config"})).is_error);
        assert!(t.run("write_file", json!({"path": ".git/hooks/pre-commit", "content": "x"})).is_error);
        assert!(t.run("write_file", json!({"path": ".ssh/config", "content": "x"})).is_error);
    }

    #[cfg(windows)]
    #[test]
    fn run_command_exit_codes() {
        let t = Tmp::new("run");
        let ok = t.run("run_command", json!({"command": "Write-Output hi"}));
        assert!(!ok.is_error, "{}", ok.text);
        assert!(ok.text.starts_with("Exit code: 0\nhi"), "{}", ok.text);
        let bad = t.run("run_command", json!({"command": "[Console]::Error.WriteLine('oops'); exit 3"}));
        assert!(bad.is_error && bad.text.contains("Exit code: 3") && bad.text.contains("[stderr]\noops"), "{}", bad.text);
        t.put("marker.txt", "");
        let cwd = t.run("run_command", json!({"command": "Get-ChildItem -Name"}));
        assert!(cwd.text.contains("marker.txt"), "{}", cwd.text);
    }

    #[cfg(windows)]
    #[test]
    fn run_command_timeout_kills() {
        let t = Tmp::new("timeout");
        let start = Instant::now();
        let o = t.run("run_command", json!({"command": "Start-Sleep 5", "timeout_secs": 1}));
        assert!(o.is_error && o.text.contains("Timed out after 1s"), "{}", o.text);
        assert!(start.elapsed() < Duration::from_secs(4));
    }

    #[cfg(windows)]
    #[test]
    fn run_command_cancel_kills() {
        let t = Tmp::new("cancel");
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(700));
            flag.store(true, Ordering::SeqCst);
        });
        let start = Instant::now();
        let o = t.tools.call("run_command", &json!({"command": "Start-Sleep 10"}), &cancel);
        assert!(o.is_error && o.text.contains("Cancelled"), "{}", o.text);
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[cfg(windows)]
    #[test]
    fn run_command_env_scrubbed() {
        let t = Tmp::new("env");
        std::env::set_var("GEB_TEST_TOKEN", "super-secret");
        let o = t.run("run_command", json!({"command": "Write-Output \"[$env:GEB_TEST_TOKEN]\""}));
        assert!(o.text.contains("[]") && !o.text.contains("super-secret"), "{}", o.text);
    }

    #[cfg(windows)]
    #[test]
    fn run_command_output_capped() {
        let t = Tmp::new("cap");
        let o = t.run("run_command", json!({"command": "Write-Output ('x' * 40000)"}));
        assert!(o.text.contains("chars cut"), "{}", &o.text[..80.min(o.text.len())]);
        assert!(o.text.chars().count() < 30_100);
    }

    #[test]
    fn cap_output_keeps_head_and_tail() {
        let s = format!("{}{}", "a".repeat(15_000), "b".repeat(35_000));
        let c = cap_output(&s);
        assert!(c.starts_with(&"a".repeat(10_000)) && c.ends_with(&"b".repeat(20_000)));
        assert!(c.contains("… 20000 chars cut …"));
        assert_eq!(cap_output("short"), "short");
    }

    #[test]
    fn titles_kinds_risks() {
        assert_eq!(LocalTools::title("write_file", &json!({"path": "src/main.rs"})), "Write src/main.rs");
        assert_eq!(LocalTools::title("run_command", &json!({"command": "npm test"})), "Run `npm test`");
        assert_eq!(
            LocalTools::title("read_file", &json!({"path": "README.md", "offset": 10, "limit": 41})),
            "Read README.md (lines 10-50)"
        );
        assert_eq!(LocalTools::title("read_file", &json!({"path": "a"})), "Read a");
        assert_eq!(LocalTools::risk("grep"), Risk::Read);
        assert_eq!(LocalTools::risk("edit_file"), Risk::Write);
        assert_eq!(LocalTools::risk("run_command"), Risk::Execute);
        assert_eq!(LocalTools::kind("edit_file"), "edit");
        assert_eq!(LocalTools::kind("grep"), "search");
        assert_eq!(LocalTools::kind("zzz"), "other");
        assert!(LocalTools::handles("grep") && !LocalTools::handles("zzz"));
    }

    #[test]
    fn specs_are_well_formed() {
        let specs = LocalTools::specs();
        assert_eq!(specs.len(), TOOL_NAMES.len());
        for s in &specs {
            assert!(LocalTools::handles(&s.name) && s.description.len() < 400);
            assert_eq!(s.parameters["type"], "object");
        }
    }

    #[test]
    fn new_rejects_missing_folder() {
        assert!(LocalTools::new(Path::new("Z:/definitely/not/here")).is_err());
    }
}
