use parking_lot::Mutex;
use serde_json::{json, Map, Value};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

pub const MAX_LOG_BYTES: u64 = 1024 * 1024;
pub const MAX_LINE_BYTES: usize = 8 * 1024;
pub const MAX_ENTRIES_PER_FRAME: usize = 500;

/// Append-only JSON Lines file with single-backup rotation.
pub struct LogFile {
    path: PathBuf,
    max_bytes: u64,
    inner: Mutex<Inner>,
    warned: AtomicBool,
}

#[derive(Default)]
struct Inner {
    file: Option<File>,
    size: u64,
}

impl LogFile {
    pub fn new(path: PathBuf, max_bytes: u64) -> Self {
        Self { path, max_bytes, inner: Mutex::new(Inner::default()), warned: AtomicBool::new(false) }
    }

    /// Append one line (a trailing newline is added). Never panics; I/O errors are
    /// swallowed after a single stderr warning.
    pub fn append_line(&self, line: &str) {
        if let Err(e) = self.try_append(line) {
            if !self.warned.swap(true, Ordering::Relaxed) {
                eprintln!("ge-bridge: log write to {} failed: {e}", self.path.display());
            }
            self.inner.lock().file = None; // reopen next time
        }
    }

    fn try_append(&self, line: &str) -> std::io::Result<()> {
        let add = line.len() as u64 + 1;
        let mut g = self.inner.lock();
        if g.file.is_none() {
            if let Some(dir) = self.path.parent() {
                fs::create_dir_all(dir)?;
            }
            let f = OpenOptions::new().create(true).append(true).open(&self.path)?;
            g.size = f.metadata()?.len();
            g.file = Some(f);
        }
        if g.size > 0 && g.size + add > self.max_bytes {
            g.file = None; // close the handle before renaming (Windows)
            let mut bak = self.path.clone().into_os_string();
            bak.push(".1");
            let bak = PathBuf::from(bak);
            let _ = fs::remove_file(&bak);
            fs::rename(&self.path, &bak)?;
            g.file = Some(OpenOptions::new().create(true).append(true).open(&self.path)?);
            g.size = 0;
        }
        let mut buf = Vec::with_capacity(add as usize);
        buf.extend_from_slice(line.as_bytes());
        buf.push(b'\n');
        g.file.as_mut().expect("opened above").write_all(&buf)?;
        g.size += add;
        Ok(())
    }
}

fn cut(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut i = max;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    &s[..i]
}

fn short(v: &Value, max: usize) -> Option<Value> {
    v.as_str().map(|s| Value::String(cut(s, max).to_string()))
}

/// Validate an `EXT_LOG` frame and return the JSON lines to persist.
pub fn ext_log_lines(frame: &Value, recv_ms: u64) -> Vec<String> {
    let Some(entries) = frame.get("entries").and_then(|v| v.as_array()) else { return Vec::new() };
    entries.iter().take(MAX_ENTRIES_PER_FRAME).filter_map(|e| ext_entry_line(e, recv_ms)).collect()
}

fn ext_entry_line(entry: &Value, recv_ms: u64) -> Option<String> {
    let obj = entry.as_object()?;
    let msg = obj.get("msg")?.as_str()?;
    let mut out = Map::new();
    for k in ["ts", "tabId"] {
        if let Some(v) = obj.get(k).filter(|v| v.is_number()) {
            out.insert(k.into(), v.clone());
        }
    }
    for k in ["level", "src"] {
        if let Some(v) = obj.get(k).and_then(|v| short(v, 32)) {
            out.insert(k.into(), v);
        }
    }
    out.insert("msg".into(), Value::String(msg.to_string()));
    if let Some(d) = obj.get("data") {
        out.insert("data".into(), d.clone());
    }
    out.insert("recv".into(), json!(recv_ms));

    let line = Value::Object(out.clone()).to_string();
    if line.len() <= MAX_LINE_BYTES {
        return Some(line);
    }
    out.insert("truncated".into(), Value::Bool(true));
    out.insert("msg".into(), Value::String(cut(msg, 2048).to_string()));
    if let Some(d) = out.get("data") {
        out.insert("data".into(), Value::String(cut(&d.to_string(), 3072).to_string()));
    }
    Some(Value::Object(out).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn tmp() -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!("ge-bridge-logfile-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn appends_lines_and_creates_parent_dir() {
        let d = tmp();
        let f = LogFile::new(d.join("logs").join("a.log"), MAX_LOG_BYTES);
        f.append_line("one");
        f.append_line("two");
        assert_eq!(fs::read_to_string(d.join("logs").join("a.log")).unwrap(), "one\ntwo\n");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn rotates_before_exceeding_threshold() {
        let d = tmp();
        let p = d.join("a.log");
        let f = LogFile::new(p.clone(), 20);
        f.append_line("aaaaaaaaa"); // 10 bytes
        f.append_line("bbbbbbbbb"); // 20 bytes total, fits
        f.append_line("c"); // would reach 22 -> rotate
        assert_eq!(fs::read_to_string(d.join("a.log.1")).unwrap(), "aaaaaaaaa\nbbbbbbbbb\n");
        assert_eq!(fs::read_to_string(&p).unwrap(), "c\n");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn rotation_replaces_old_backup() {
        let d = tmp();
        let p = d.join("a.log");
        let f = LogFile::new(p.clone(), 10);
        f.append_line("first");
        f.append_line("second");
        f.append_line("third");
        assert_eq!(fs::read_to_string(d.join("a.log.1")).unwrap(), "second\n");
        assert_eq!(fs::read_to_string(&p).unwrap(), "third\n");
        assert!(!d.join("a.log.2").exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn resumes_size_of_existing_file() {
        let d = tmp();
        fs::create_dir_all(&d).unwrap();
        let p = d.join("a.log");
        fs::write(&p, "123456789\n").unwrap();
        let f = LogFile::new(p.clone(), 15);
        f.append_line("abcdef");
        assert_eq!(fs::read_to_string(d.join("a.log.1")).unwrap(), "123456789\n");
        assert_eq!(fs::read_to_string(&p).unwrap(), "abcdef\n");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn io_errors_are_swallowed() {
        let d = tmp();
        fs::create_dir_all(&d).unwrap();
        let blocker = d.join("blocker");
        fs::write(&blocker, "x").unwrap();
        let f = LogFile::new(blocker.join("sub").join("a.log"), MAX_LOG_BYTES);
        f.append_line("never lands");
        f.append_line("still no panic");
        let _ = fs::remove_dir_all(&d);
    }

    fn frame(entries: Value) -> Value {
        json!({"type": "EXT_LOG", "entries": entries})
    }

    #[test]
    fn valid_entries_get_recv_and_invalid_are_dropped() {
        let f = frame(json!([
            {"ts": 1, "level": "info", "src": "bg", "tabId": 7, "msg": "hello", "data": {"a": 1}},
            "not an object",
            {"ts": 2, "level": "warn", "src": "content"},
            {"ts": 3, "level": "error", "src": "popup", "msg": "bad"}
        ]));
        let lines = ext_log_lines(&f, 999);
        assert_eq!(lines.len(), 2);
        let a: Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(a["msg"], "hello");
        assert_eq!(a["tabId"], 7);
        assert_eq!(a["data"]["a"], 1);
        assert_eq!(a["recv"], 999);
        let b: Value = serde_json::from_str(&lines[1]).unwrap();
        assert_eq!(b["msg"], "bad");
        assert!(lines.iter().all(|l| !l.contains('\n')));
    }

    #[test]
    fn caps_entries_per_frame() {
        let entries: Vec<Value> = (0..600).map(|i| json!({"ts": i, "level": "info", "src": "bg", "msg": "m"})).collect();
        assert_eq!(ext_log_lines(&frame(Value::Array(entries)), 1).len(), 500);
    }

    #[test]
    fn missing_or_non_array_entries_yield_nothing() {
        assert!(ext_log_lines(&json!({"type": "EXT_LOG"}), 1).is_empty());
        assert!(ext_log_lines(&json!({"type": "EXT_LOG", "entries": "x"}), 1).is_empty());
    }

    #[test]
    fn oversized_lines_are_truncated_and_flagged() {
        let big = "é".repeat(20_000);
        let f = frame(json!([{"ts": 1, "level": "info", "src": "bg", "msg": big.clone(), "data": {"blob": big}}]));
        let lines = ext_log_lines(&f, 5);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].len() <= MAX_LINE_BYTES, "len {}", lines[0].len());
        let v: Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(v["truncated"], true);
        assert_eq!(v["recv"], 5);
        assert!(v["msg"].as_str().unwrap().starts_with("éé"));
    }

    #[test]
    fn small_entries_are_not_flagged() {
        let f = frame(json!([{"ts": 1, "level": "info", "src": "bg", "msg": "x"}]));
        let v: Value = serde_json::from_str(&ext_log_lines(&f, 5)[0]).unwrap();
        assert!(v.get("truncated").is_none());
    }
}
