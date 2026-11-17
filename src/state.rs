use parking_lot::{Mutex, RwLock};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, Notify};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    pub available: bool,
    pub supports_thinking: bool,
    pub efforts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedPrefs {
    pub port: u16,
    pub default_model: String,
    pub reasoning_effort: String,
    pub enable_tools_default: bool,
    #[serde(default)]
    pub gemini_home_url: Option<String>,
}

/// Returns the host if `raw` is an acceptable Gemini Enterprise home URL.
pub fn validate_gemini_home_url(raw: &str) -> Option<&str> {
    let rest = raw.strip_prefix("https://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..end];
    if host != "vertexaisearch.cloud.google" && host != "vertexaisearch.cloud.google.com" {
        return None;
    }
    if !rest[end..].starts_with('/') || raw.len() > 512 || raw.chars().any(|c| c.is_control() || c == ' ') {
        return None;
    }
    let path = rest[end..].split(['?', '#']).next().unwrap_or("");
    path.contains("/home/cid/").then_some(host)
}

impl Default for PersistedPrefs {
    fn default() -> Self {
        Self {
            port: 8787,
            default_model: "gemini-3.8-flash".to_string(),
            reasoning_effort: "auto".to_string(),
            enable_tools_default: true,
            gemini_home_url: None,
        }
    }
}

impl PersistedPrefs {
    pub fn app_dir() -> PathBuf {
        let dir = match &crate::config::active().data_dir {
            Some(d) => d.clone(),
            None => dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("ge-bridge"),
        };
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn load_from_disk() -> Self {
        let path = Self::app_dir().join("prefs.json");
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Loads prefs; `GE_BRIDGE_PORT` overrides the port in memory only.
    pub fn load() -> Self {
        let mut prefs = Self::load_from_disk();
        if let Some(p) = crate::config::active().port {
            prefs.port = p;
        }
        prefs
    }

    pub fn save(&self) {
        let path = Self::app_dir().join("prefs.json");
        let mut to_write = self.clone();
        if crate::config::active().port.is_some() {
            // The env port is per-run; keep whatever port is stored on disk.
            to_write.port = Self::load_from_disk().port;
        }
        if let Ok(raw) = serde_json::to_string_pretty(&to_write) {
            let _ = fs::write(path, raw);
        }
    }
}

pub fn load_or_create_token() -> String {
    if crate::config::active().data_dir.is_some() {
        return load_or_create_file_token(&PersistedPrefs::app_dir().join("token"));
    }
    if let Ok(entry) = keyring::Entry::new("ge-bridge-local", "pairing-token") {
        if let Ok(secret) = entry.get_password() {
            if secret.len() >= 20 {
                return secret;
            }
        }
        let fresh = generate_random_token();
        if entry.set_password(&fresh).is_ok() {
            return fresh;
        }
    }
    // Fallback file in %APPDATA%\ge-bridge\secret.token
    let fallback = PersistedPrefs::app_dir().join("secret.token");
    if let Ok(s) = fs::read_to_string(&fallback) {
        if s.trim().len() >= 20 {
            return s.trim().to_string();
        }
    }
    let fresh = generate_random_token();
    let _ = fs::write(&fallback, &fresh);
    fresh
}

fn load_or_create_file_token(path: &std::path::Path) -> String {
    if let Ok(s) = fs::read_to_string(path) {
        if s.trim().len() >= 20 {
            return s.trim().to_string();
        }
    }
    let fresh = generate_random_token();
    let _ = fs::write(path, &fresh);
    fresh
}

fn generate_random_token() -> String {
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("gemb_{}", hex::encode(bytes))
}

pub fn default_bootstrap_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "gemini-3.8-flash".into(),
            display_name: "Gemini 3.8 Flash".into(),
            available: true,
            supports_thinking: true,
            efforts: vec!["auto".into(), "low".into(), "medium".into(), "high".into()],
        },
        ModelInfo {
            id: "gemini-4-argon".into(),
            display_name: "Gemini 4 Argon".into(),
            available: true,
            supports_thinking: true,
            efforts: vec!["auto".into(), "low".into(), "medium".into(), "high".into(), "max".into()],
        },
        ModelInfo {
            id: "gemini-3.5-flash".into(),
            display_name: "Gemini 3.5 Flash".into(),
            available: true,
            supports_thinking: true,
            efforts: vec!["auto".into(), "low".into(), "medium".into()],
        },
        ModelInfo {
            id: "gemini-3.5-flash-lite".into(),
            display_name: "Gemini 3.5 Flash Lite".into(),
            available: true,
            supports_thinking: false,
            efforts: vec!["auto".into()],
        },
        ModelInfo {
            id: "gemini-3.1-pro-preview".into(),
            display_name: "Gemini 3.1 Pro Preview".into(),
            available: true,
            supports_thinking: true,
            efforts: vec!["auto".into(), "medium".into(), "high".into()],
        },
    ]
}

#[derive(Debug, Clone, Serialize)]
pub struct TabState {
    pub tab_id: i64,
    pub url: String,
    pub title: String,
    pub active_model: Option<String>,
    pub calibrated: bool,
    pub busy: bool,
}

/// The extension's virtual API lanes (`VIRTUAL_LANES` in background.js). Real Chrome tab ids are
/// arbitrary and often larger, so lanes are matched exactly.
pub const VIRTUAL_LANES: [i64; 2] = [900_001, 900_002];

impl TabState {
    pub fn is_api_lane(&self) -> bool {
        VIRTUAL_LANES.contains(&self.tab_id)
    }

    /// One line for the tray menu and dashboard, e.g. "🌐 hello · session …831156 · gemini-4-argon · busy".
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.is_api_lane() {
            parts.push(format!("⚡ {}", self.title));
        } else {
            let title = self.title.trim_end_matches(" - Gemini Enterprise").trim();
            let title = if title.is_empty() { "Gemini Enterprise" } else { title };
            let short: String = title.chars().take(40).collect();
            parts.push(format!("🌐 {short}"));
            match self.url.split("/r/session/").nth(1) {
                Some(rest) => {
                    let id = rest.split(['?', '#', '/']).next().unwrap_or("");
                    let tail: String = id.chars().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect();
                    parts.push(format!("session …{tail}"));
                }
                None => parts.push("new chat".into()),
            }
            if let Some(m) = &self.active_model {
                parts.push(m.clone());
            }
        }
        if self.busy {
            parts.push("busy".into());
        }
        parts.join(" · ")
    }
}

#[derive(Debug, Clone)]
pub struct SessionEntry {
    pub last_full_hash: String,
    pub active_model: String,
    pub tools_hash: Option<String>,
    pub preferred_tab_id: Option<i64>,
    pub updated_at: u64,
}

#[derive(Debug, Clone)]
pub enum DomStreamEvent {
    Chunk { thought_delta: Option<String>, text_delta: String },
    Done { full_text: String, active_model: Option<String> },
    Error(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub ts: u64,
    pub level: String,
    pub message: String,
}

pub enum TraySignal {
    RebuildMenu,
}

pub struct AppState {
    pub token: String,
    pub prefs: RwLock<PersistedPrefs>,
    pub models: RwLock<Vec<ModelInfo>>,
    pub tabs: RwLock<HashMap<i64, TabState>>,
    pub tab_notify: Notify,
    pub ext_link: RwLock<crate::link::ExtLink>,
    pub pending_jobs: Mutex<HashMap<String, mpsc::UnboundedSender<DomStreamEvent>>>,
    pub pending_images: Mutex<HashMap<String, tokio::sync::oneshot::Sender<Result<crate::images::GeneratedImage, String>>>>,
    /// Images made recently, waiting for the tool-result turn that saves them into the bot's folder.
    pub recent_images: Mutex<std::collections::VecDeque<crate::omb::disk::RecentImage>>,
    pub sessions: Mutex<VecDeque<SessionEntry>>,
    pub logs: Mutex<VecDeque<LogEntry>>,
    pub ui_events_tx: Mutex<Vec<mpsc::UnboundedSender<String>>>,
    pub tray_tx: Mutex<Option<mpsc::UnboundedSender<TraySignal>>>,
    pub server_log: crate::logfile::LogFile,
    pub ext_log: crate::logfile::LogFile,
}

impl AppState {
    pub fn load_or_init() -> Self {
        let prefs = PersistedPrefs::load();
        if crate::config::active().data_dir.is_some() && !PersistedPrefs::app_dir().join("prefs.json").exists() {
            prefs.save(); // isolated instances get a prefs file up front
        }
        let token = load_or_create_token();
        let models_path = PersistedPrefs::app_dir().join("models_cache.json");
        let models = fs::read_to_string(&models_path)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<ModelInfo>>(&s).ok())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(default_bootstrap_models);

        Self {
            token,
            prefs: RwLock::new(prefs),
            models: RwLock::new(models),
            tabs: RwLock::new(HashMap::new()),
            tab_notify: Notify::new(),
            ext_link: RwLock::new(crate::link::ExtLink::default()),
            pending_jobs: Mutex::new(HashMap::new()),
            pending_images: Mutex::new(HashMap::new()),
            recent_images: Mutex::new(std::collections::VecDeque::new()),
            sessions: Mutex::new(VecDeque::with_capacity(32)),
            logs: Mutex::new(VecDeque::with_capacity(100)),
            ui_events_tx: Mutex::new(Vec::new()),
            tray_tx: Mutex::new(None),
            server_log: crate::logfile::LogFile::new(logs_dir().join("server.log"), crate::logfile::MAX_LOG_BYTES),
            ext_log: crate::logfile::LogFile::new(logs_dir().join("extension.log"), crate::logfile::MAX_LOG_BYTES),
        }
    }

    pub fn update_models(&self, incoming: Vec<ModelInfo>) {
        if incoming.is_empty() {
            return;
        }
        {
            let mut lock = self.models.write();
            *lock = incoming.clone();
        }
        let path = models_cache_path();
        if let Ok(raw) = serde_json::to_string_pretty(&incoming) {
            let _ = fs::write(path, raw);
        }
        // Ensure default_model points to an available model
        {
            let mut prefs = self.prefs.write();
            let exists = incoming.iter().any(|m| m.available && m.id == prefs.default_model);
            if !exists {
                if let Some(first) = incoming.iter().find(|m| m.available) {
                    prefs.default_model = first.id.clone();
                    prefs.save();
                }
            }
        }
        self.notify_tray();
        self.broadcast_dashboard();
    }

    /// 3-tier model resolution: explicit `model:effort` -> catalog match -> System-Tray default fallback
    pub fn resolve_model_and_effort(
        &self,
        requested_model: Option<&str>,
        requested_effort: Option<&str>,
    ) -> Result<(String, Option<String>), String> {
        let prefs = self.prefs.read().clone();
        let catalog = self.models.read().clone();

        let (raw_name, inline_effort) = match requested_model.map(str::trim).filter(|s| !s.is_empty()) {
            Some(m) if m.contains(':') => {
                let mut parts = m.splitn(2, ':');
                (parts.next().unwrap_or(""), parts.next())
            }
            Some(m) => (m, None),
            None => ("default", None),
        };

        let chosen_model = if matches!(raw_name, "default" | "auto" | "gemini-enterprise")
            || raw_name.starts_with("claude-")
            || raw_name.starts_with("gpt-")
        {
            prefs.default_model.clone()
        } else if let Some(found) = catalog.iter().find(|m| m.id.eq_ignore_ascii_case(raw_name)) {
            if !found.available {
                return Err(format!("Model '{}' is locked or unavailable on your Gemini Enterprise tenant.", found.id));
            }
            found.id.clone()
        } else {
            // Allow custom/unlisted model slug if user explicitly typed it
            raw_name.to_lowercase()
        };

        let effort = requested_effort
            .or(inline_effort)
            .map(str::to_lowercase)
            .unwrap_or_else(|| prefs.reasoning_effort.clone());

        let final_effort = if effort == "auto" || effort.is_empty() {
            None
        } else {
            Some(effort)
        };

        Ok((chosen_model, final_effort))
    }

    pub async fn acquire_tab(&self, preferred_tab_id: Option<i64>) -> Result<i64, String> {
        loop {
            if !self.ext_connected() {
                return Err("Browser extension is not connected. Open Chrome/Edge and verify GeBridge extension.".into());
            }
            let notified = self.tab_notify.notified();
            {
                let mut tabs = self.tabs.write();
                if tabs.is_empty() {
                    return Err("No open Gemini Enterprise tab detected. Open your own Gemini Enterprise URL (https://vertexaisearch.cloud.google/...) in Chrome/Edge and sign in.".into());
                }
                if let Some(pref) = preferred_tab_id {
                    if let Some(t) = tabs.get_mut(&pref) {
                        if !t.busy {
                            t.busy = true;
                            return Ok(pref);
                        }
                    }
                }
                if let Some((&id, t)) = tabs.iter_mut().find(|(_, t)| !t.busy) {
                    t.busy = true;
                    return Ok(id);
                }
            }
            // All tabs busy: wait cleanly in async FIFO queue (up to 120s)
            tokio::select! {
                _ = notified => continue,
                _ = tokio::time::sleep(std::time::Duration::from_secs(120)) => {
                    return Err("Timed out waiting for an idle Gemini Enterprise browser tab.".into());
                }
            }
        }
    }

    pub fn release_tab(&self, tab_id: i64, active_model: Option<String>) {
        {
            let mut tabs = self.tabs.write();
            if let Some(t) = tabs.get_mut(&tab_id) {
                t.busy = false;
                if active_model.is_some() {
                    t.active_model = active_model;
                }
            }
        }
        self.tab_notify.notify_one();
        self.broadcast_dashboard();
    }

    pub fn ext_connected(&self) -> bool {
        self.ext_link.read().is_connected()
    }

    /// Seconds since the model list was last scanned (the cache file's write time); None if never.
    pub fn models_age_secs(&self) -> Option<u64> {
        let modified = fs::metadata(models_cache_path()).ok()?.modified().ok()?;
        Some(SystemTime::now().duration_since(modified).map(|d| d.as_secs()).unwrap_or(0))
    }

    /// Asks the extension to rescan models by opening the Gemini picker. Unless `force`
    /// (an explicit user click), this is skipped while the cached list is fresh. Returns whether it was sent.
    pub fn request_models_sync(&self, force: bool, reason: &str) -> bool {
        if !force && models_cache_fresh(self.models_age_secs()) {
            return false;
        }
        let sent = self.send_to_extension(&serde_json::json!({ "type": "REQUEST_MODELS_SYNC", "forceOpenMenu": true })).is_ok();
        self.log("info", format!("Model rescan requested ({reason}, force={force}, sent={sent})."));
        sent
    }

    pub fn send_to_extension(&self, payload: &serde_json::Value) -> Result<(), String> {
        let lock = self.ext_link.read();
        let tx = lock.sender().ok_or_else(|| "Extension WebSocket disconnected.".to_string())?;
        tx.send(payload.to_string()).map_err(|_| "Failed to write to Extension WebSocket.".into())
    }

    pub fn log(&self, level: &str, msg: impl Into<String>) {
        let entry = LogEntry {
            ts: now_secs(),
            level: level.to_string(),
            message: msg.into(),
        };
        self.server_log.append_line(&serde_json::json!({ "ts": now_millis(), "level": entry.level, "msg": entry.message }).to_string());
        {
            let mut logs = self.logs.lock();
            if logs.len() >= 100 {
                logs.pop_front();
            }
            logs.push_back(entry.clone());
        }
        self.broadcast_dashboard();
    }

    pub fn status_json(&self) -> serde_json::Value {
        let prefs = self.prefs.read().clone();
        let ext_connected = self.ext_connected();
        let mut tabs: Vec<TabState> = self.tabs.read().values().cloned().collect();
        tabs.sort_by_key(|t| (t.is_api_lane(), t.tab_id));
        let tab_list: Vec<serde_json::Value> = tabs
            .iter()
            .map(|t| serde_json::json!({ "tabId": t.tab_id, "kind": if t.is_api_lane() { "api" } else { "browser" }, "label": t.label(), "url": t.url, "busy": t.busy }))
            .collect();
        let models: Vec<ModelInfo> = self.models.read().iter().filter(|m| m.available).cloned().collect();
        serde_json::json!({
            "extensionConnected": ext_connected,
            "tabReady": ext_connected && !tabs.is_empty(),
            "tabs": tabs,
            "tabList": tab_list,
            "browserTabs": tabs.iter().filter(|t| !t.is_api_lane()).count(),
            "models": models,
            "defaultModel": prefs.default_model,
            "modelsAgeSecs": self.models_age_secs(),
            "modelsCacheTtlSecs": MODELS_CACHE_TTL_SECS,
            "reasoningEffort": prefs.reasoning_effort,
            "port": prefs.port,
            "geminiTargetSet": prefs.gemini_home_url.is_some(),
            "autostart": crate::integrations::is_autostart_enabled()
        })
    }

    pub fn broadcast_dashboard(&self) {
        let msg = serde_json::json!({
            "type": "state",
            "status": self.status_json(),
            "logs": *self.logs.lock()
        })
        .to_string();
        self.ui_events_tx.lock().retain(|tx| tx.send(msg.clone()).is_ok());
    }

    pub fn notify_tray(&self) {
        if let Some(tx) = self.tray_tx.lock().as_ref() {
            let _ = tx.send(TraySignal::RebuildMenu);
        }
    }
}

pub fn hash_json<T: Serialize + ?Sized>(val: &T) -> String {
    let mut hasher = Sha1::new();
    if let Ok(bytes) = serde_json::to_vec(val) {
        hasher.update(&bytes);
    }
    hex::encode(hasher.finalize())
}

/// `<app_dir>/logs`, where `server.log` and `extension.log` live.
/// A model scan opens the picker in the Gemini tab, so the list is reused for this long.
pub const MODELS_CACHE_TTL_SECS: u64 = 24 * 3600;

pub fn models_cache_path() -> PathBuf {
    PersistedPrefs::app_dir().join("models_cache.json")
}

/// Fresh = scanned from the Gemini tab less than MODELS_CACHE_TTL_SECS ago.
pub fn models_cache_fresh(age_secs: Option<u64>) -> bool {
    matches!(age_secs, Some(a) if a < MODELS_CACHE_TTL_SECS)
}

pub fn logs_dir() -> PathBuf {
    PersistedPrefs::app_dir().join("logs")
}

pub fn now_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state(prefs: PersistedPrefs, models: Vec<ModelInfo>) -> AppState {
        AppState {
            token: "tok".into(),
            prefs: RwLock::new(prefs),
            models: RwLock::new(models),
            tabs: RwLock::new(HashMap::new()),
            tab_notify: Notify::new(),
            ext_link: RwLock::new(crate::link::ExtLink::default()),
            pending_jobs: Mutex::new(HashMap::new()),
            pending_images: Mutex::new(HashMap::new()),
            recent_images: Mutex::new(std::collections::VecDeque::new()),
            sessions: Mutex::new(VecDeque::new()),
            logs: Mutex::new(VecDeque::new()),
            ui_events_tx: Mutex::new(Vec::new()),
            tray_tx: Mutex::new(None),
            server_log: crate::logfile::LogFile::new(std::env::temp_dir().join("ge-bridge-unit-state").join("server.log"), crate::logfile::MAX_LOG_BYTES),
            ext_log: crate::logfile::LogFile::new(std::env::temp_dir().join("ge-bridge-unit-state").join("extension.log"), crate::logfile::MAX_LOG_BYTES),
        }
    }

    fn default_state() -> AppState {
        test_state(PersistedPrefs::default(), default_bootstrap_models())
    }

    fn tab(id: i64) -> TabState {
        TabState { tab_id: id, url: String::new(), title: String::new(), active_model: None, calibrated: true, busy: false }
    }

    // ---- validate_gemini_home_url ----

    #[test]
    fn home_url_accept_table() {
        for url in [
            "https://vertexaisearch.cloud.google/home/cid/abc-123",
            "https://vertexaisearch.cloud.google.com/home/cid/abc-123",
            "https://vertexaisearch.cloud.google.com/home/cid/abc?x=1#frag",
            "https://vertexaisearch.cloud.google/u/0/home/cid/abc",
        ] {
            assert!(validate_gemini_home_url(url).is_some(), "{url}");
        }
        assert_eq!(
            validate_gemini_home_url("https://vertexaisearch.cloud.google.com/home/cid/x"),
            Some("vertexaisearch.cloud.google.com")
        );
    }

    #[test]
    fn home_url_reject_table() {
        let long = format!("https://vertexaisearch.cloud.google/home/cid/{}", "a".repeat(512));
        let at_limit = format!("https://vertexaisearch.cloud.google/home/cid/{}", "a".repeat(512 - 45));
        assert_eq!(at_limit.len(), 512);
        assert!(validate_gemini_home_url(&at_limit).is_some(), "exactly 512 chars is allowed");
        let rejects: Vec<String> = vec![
            "http://vertexaisearch.cloud.google/home/cid/abc".into(),
            "vertexaisearch.cloud.google/home/cid/abc".into(),
            "https://vertexaisearch.cloud.google.evil.io/home/cid/abc".into(),
            "https://vertexaisearch.cloud.google.com.evil.io/home/cid/abc".into(),
            "https://evilvertexaisearch.cloud.google/home/cid/abc".into(),
            "https://vertexaisearch.cloud.google@evil.io/home/cid/abc".into(),
            "https://evil.io/vertexaisearch.cloud.google/home/cid/abc".into(),
            "https://evil.io?u=vertexaisearch.cloud.google/home/cid/abc".into(),
            "https://vertexaisearch.cloud.google".into(),
            "https://vertexaisearch.cloud.google/".into(),
            "https://vertexaisearch.cloud.google/home/".into(),
            "https://vertexaisearch.cloud.google/home/cid".into(),
            "https://vertexaisearch.cloud.google/other?next=/home/cid/abc".into(),
            "https://vertexaisearch.cloud.google/home/cid/a b".into(),
            "https://vertexaisearch.cloud.google/home/cid/a\nb".into(),
            "https://vertexaisearch.cloud.google/home/cid/a\tb".into(),
            "https://vertexaisearch.cloud.google/home/cid/a\u{7f}b".into(),
            "".into(),
            long,
        ];
        for url in &rejects {
            assert!(validate_gemini_home_url(url).is_none(), "should reject {url:?}");
        }
    }

    // ---- prefs ----

    #[test]
    fn prefs_json_without_home_url_still_loads() {
        let raw = r#"{"port":8787,"default_model":"m","reasoning_effort":"auto","enable_tools_default":true}"#;
        let p: PersistedPrefs = serde_json::from_str(raw).unwrap();
        assert_eq!(p.gemini_home_url, None);
        assert_eq!(p.default_model, "m");
    }

    #[test]
    fn prefs_roundtrip() {
        let p = PersistedPrefs { gemini_home_url: Some("https://x".into()), port: 1234, ..Default::default() };
        let back: PersistedPrefs = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back.port, 1234);
        assert_eq!(back.gemini_home_url.as_deref(), Some("https://x"));
    }

    #[test]
    fn prefs_defaults() {
        let p = PersistedPrefs::default();
        assert_eq!(p.port, 8787);
        assert_eq!(p.reasoning_effort, "auto");
        assert!(p.enable_tools_default);
    }

    // ---- file token ----

    #[test]
    fn file_token_is_created_then_reused() {
        let dir = std::env::temp_dir().join(format!("ge-bridge-test-{}", generate_random_token()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("token");
        let t1 = load_or_create_file_token(&path);
        assert!(t1.starts_with("gemb_") && t1.len() >= 20);
        assert_eq!(fs::read_to_string(&path).unwrap(), t1);
        assert_eq!(load_or_create_file_token(&path), t1);
        fs::write(&path, "short").unwrap();
        let t2 = load_or_create_file_token(&path);
        assert!(t2.len() >= 20 && t2 != t1, "too-short token is regenerated");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn random_tokens_differ() {
        assert_ne!(generate_random_token(), generate_random_token());
    }

    // ---- resolve_model_and_effort ----

    fn resolve(st: &AppState, m: Option<&str>, e: Option<&str>) -> Result<(String, Option<String>), String> {
        st.resolve_model_and_effort(m, e)
    }

    #[test]
    fn resolution_table() {
        let st = default_state();
        let def = "gemini-3.8-flash".to_string();
        let ok = |m: &str, e: Option<&str>| (m.to_string(), e.map(String::from));
        // (requested model, requested effort, expected)
        let cases: Vec<(Option<&str>, Option<&str>, (String, Option<String>))> = vec![
            (None, None, ok(&def, None)),
            (Some(""), None, ok(&def, None)),
            (Some("  "), None, ok(&def, None)),
            (Some("default"), None, ok(&def, None)),
            (Some("auto"), None, ok(&def, None)),
            (Some("gemini-enterprise"), None, ok(&def, None)),
            (Some("claude-3-5-sonnet"), None, ok(&def, None)),
            (Some("claude-opus-4"), None, ok(&def, None)),
            (Some("gpt-4o"), None, ok(&def, None)),
            (Some("gemini-4-argon"), None, ok("gemini-4-argon", None)),
            (Some("GEMINI-4-ARGON"), None, ok("gemini-4-argon", None)),
            (Some("gemini-4-argon:high"), None, ok("gemini-4-argon", Some("high"))),
            (Some("gemini-4-argon:HIGH"), None, ok("gemini-4-argon", Some("high"))),
            (Some("gemini-4-argon:low"), Some("max"), ok("gemini-4-argon", Some("max"))),
            (Some("gemini-4-argon"), Some("Medium"), ok("gemini-4-argon", Some("medium"))),
            (Some("gemini-4-argon:auto"), None, ok("gemini-4-argon", None)),
            (Some("gemini-4-argon"), Some(""), ok("gemini-4-argon", None)),
            (Some("default:high"), None, ok(&def, Some("high"))),
            (Some("claude-3:low"), None, ok(&def, Some("low"))),
            (Some("Totally-Unknown-Model"), None, ok("totally-unknown-model", None)),
        ];
        for (m, e, want) in cases {
            assert_eq!(resolve(&st, m, e), Ok(want.clone()), "model={m:?} effort={e:?}");
        }
    }

    #[test]
    fn resolution_uses_tray_prefs() {
        let prefs = PersistedPrefs {
            default_model: "gemini-4-argon".into(),
            reasoning_effort: "medium".into(),
            ..Default::default()
        };
        let st = test_state(prefs, default_bootstrap_models());
        assert_eq!(resolve(&st, Some("claude-x"), None), Ok(("gemini-4-argon".into(), Some("medium".into()))));
        assert_eq!(resolve(&st, None, Some("low")), Ok(("gemini-4-argon".into(), Some("low".into()))));
        assert_eq!(resolve(&st, None, Some("auto")), Ok(("gemini-4-argon".into(), None)));
    }

    #[test]
    fn resolution_rejects_locked_model() {
        let mut models = default_bootstrap_models();
        models[1].available = false;
        let st = test_state(PersistedPrefs::default(), models);
        let err = resolve(&st, Some("gemini-4-argon"), None).unwrap_err();
        assert!(err.contains("gemini-4-argon") && err.contains("unavailable"), "{err}");
    }

    // ---- hash_json ----

    #[test]
    fn hash_json_is_deterministic_and_sensitive() {
        let a = serde_json::json!({"a": 1});
        assert_eq!(hash_json(&a), hash_json(&serde_json::json!({"a": 1})));
        assert_ne!(hash_json(&a), hash_json(&serde_json::json!({"a": 2})));
        assert_eq!(hash_json(&a).len(), 40);
    }

    // ---- tabs ----

    #[tokio::test]
    async fn acquire_tab_errors_without_extension_or_tabs() {
        let st = default_state();
        assert!(st.acquire_tab(None).await.unwrap_err().contains("not connected"));
        let (tx, _rx) = mpsc::unbounded_channel();
        st.ext_link.write().attach(tx);
        assert!(st.acquire_tab(None).await.unwrap_err().contains("No open Gemini"));
    }

    #[tokio::test]
    async fn acquire_prefers_requested_tab_and_release_frees_it() {
        let st = default_state();
        let (tx, _rx) = mpsc::unbounded_channel();
        st.ext_link.write().attach(tx);
        st.tabs.write().insert(1, tab(1));
        st.tabs.write().insert(2, tab(2));
        assert_eq!(st.acquire_tab(Some(2)).await, Ok(2));
        assert!(st.tabs.read()[&2].busy);
        assert_eq!(st.acquire_tab(Some(2)).await, Ok(1), "preferred busy -> falls back to idle tab");
        st.release_tab(2, Some("m".into()));
        assert!(!st.tabs.read()[&2].busy);
        assert_eq!(st.tabs.read()[&2].active_model.as_deref(), Some("m"));
    }

    #[tokio::test]
    async fn busy_waiter_wakes_on_release() {
        let st = std::sync::Arc::new(default_state());
        let (tx, _rx) = mpsc::unbounded_channel();
        st.ext_link.write().attach(tx);
        st.tabs.write().insert(1, tab(1));
        assert_eq!(st.acquire_tab(None).await, Ok(1));
        let st2 = st.clone();
        let waiter = tokio::spawn(async move { st2.acquire_tab(None).await });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        st.release_tab(1, None);
        assert_eq!(tokio::time::timeout(std::time::Duration::from_secs(2), waiter).await.unwrap().unwrap(), Ok(1));
    }

    #[test]
    fn send_to_extension_requires_live_link_and_survives_stale_detach() {
        let st = default_state();
        assert!(st.send_to_extension(&serde_json::json!({})).is_err());
        let (a_tx, _a_rx) = mpsc::unbounded_channel();
        let (b_tx, mut b_rx) = mpsc::unbounded_channel();
        let a = st.ext_link.write().attach(a_tx);
        st.ext_link.write().attach(b_tx);
        assert!(!st.ext_link.write().detach(a));
        assert!(st.ext_connected());
        st.send_to_extension(&serde_json::json!({"k": 1})).unwrap();
        assert_eq!(b_rx.try_recv().unwrap(), r#"{"k":1}"#);
    }

    #[test]
    fn models_cache_is_fresh_for_24_hours() {
        assert!(!models_cache_fresh(None));
        assert!(models_cache_fresh(Some(0)));
        assert!(models_cache_fresh(Some(MODELS_CACHE_TTL_SECS - 1)));
        assert!(!models_cache_fresh(Some(MODELS_CACHE_TTL_SECS)));
    }

    #[test]
    fn tab_label_describes_browser_tabs_and_api_lanes() {
        let mut t = tab(7);
        t.title = "hello - Gemini Enterprise".into();
        t.url = "https://vertexaisearch.cloud.google/us/home/cid/x/r/session/14517608353838831156?csesidx=1".into();
        t.active_model = Some("gemini-4-argon".into());
        assert_eq!(t.label(), "🌐 hello · session …831156 · gemini-4-argon");
        t.url = "https://vertexaisearch.cloud.google/us/home/cid/x?csesidx=1".into();
        t.active_model = None;
        t.busy = true;
        assert_eq!(t.label(), "🌐 hello · new chat · busy");
        let mut lane = tab(900_001);
        lane.title = "Gemini Enterprise (API lane 1)".into();
        assert!(lane.is_api_lane());
        assert_eq!(lane.label(), "⚡ Gemini Enterprise (API lane 1)");
        assert!(!tab(7_155_877).is_api_lane(), "real Chrome tab ids can exceed the lane ids");
    }

    #[test]
    fn status_json_reports_extension_and_tabs() {
        let st = default_state();
        assert_eq!(st.status_json()["extensionConnected"], false);
        assert_eq!(st.status_json()["tabReady"], false);
        let (tx, _rx) = mpsc::unbounded_channel();
        st.ext_link.write().attach(tx);
        st.tabs.write().insert(1, tab(1));
        let j = st.status_json();
        assert_eq!(j["extensionConnected"], true);
        assert_eq!(j["tabReady"], true);
        assert_eq!(j["port"], 8787);
        assert_eq!(j["geminiTargetSet"], false);
    }
}
