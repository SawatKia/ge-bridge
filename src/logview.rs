//! Human-readable view of the JSON Lines logs (`server.log`, `extension.log`) for `ge-bridge-cli logs`.
//!
//! `{"ts":1794652052501,"level":"info","src":"bg","tabId":900002,"msg":"RUN_TURN dispatch","data":{...}}`
//! becomes `17:27:32.501 INFO  bg    tab 900002  RUN_TURN dispatch  jobId=job_1 model=gemini-4-argon`.

use serde_json::Value;

const VALUE_MAX_CHARS: usize = 160;

/// debug < info < warn < error. Unknown levels rank as info so they are never hidden by default.
pub fn level_rank(level: &str) -> u8 {
    match level.to_ascii_lowercase().as_str() {
        "trace" | "debug" => 0,
        "warn" | "warning" => 2,
        "error" | "fatal" => 3,
        _ => 1,
    }
}

pub fn parse_level(arg: &str) -> Result<u8, String> {
    match arg.to_ascii_lowercase().as_str() {
        "debug" | "all" => Ok(0),
        "info" => Ok(1),
        "warn" | "warning" => Ok(2),
        "error" => Ok(3),
        other => Err(format!("unknown level `{other}`; use debug, info, warn or error")),
    }
}

pub struct Filter {
    pub min_level: u8,
    /// Case-insensitive substring of the raw line.
    pub grep: Option<String>,
}

impl Filter {
    pub fn keeps(&self, raw: &str, level: &str) -> bool {
        level_rank(level) >= self.min_level
            && self.grep.as_ref().map_or(true, |g| raw.to_lowercase().contains(&g.to_lowercase()))
    }
}

/// Lines of `text` that pass `filter`, last `n` of them, formatted (or raw with `json`).
pub fn render(text: &str, filter: &Filter, n: usize, json: bool, utc_offset_secs: i64) -> Vec<String> {
    let kept: Vec<String> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|raw| {
            let v: Value = serde_json::from_str(raw).ok()?;
            let level = v.get("level").and_then(Value::as_str).unwrap_or("info");
            if !filter.keeps(raw, level) {
                return None;
            }
            Some(if json { raw.to_string() } else { format_entry(&v, utc_offset_secs) })
        })
        .collect();
    kept[kept.len().saturating_sub(n)..].to_vec()
}

pub fn format_entry(v: &Value, utc_offset_secs: i64) -> String {
    let ts = v.get("ts").and_then(Value::as_u64).map(|ms| clock(ms, utc_offset_secs)).unwrap_or_else(|| "--:--:--.---".into());
    let level = v.get("level").and_then(Value::as_str).unwrap_or("info").to_ascii_uppercase();
    let mut line = format!("{ts} {level:<5}");
    if let Some(src) = v.get("src").and_then(Value::as_str) {
        line.push_str(&format!(" {src:<7}"));
    }
    if let Some(tab) = v.get("tabId").and_then(Value::as_u64) {
        line.push_str(&format!(" tab {tab:<8}"));
    }
    line.push(' ');
    line.push_str(v.get("msg").and_then(Value::as_str).unwrap_or(""));
    if let Some(data) = v.get("data").and_then(Value::as_object) {
        let pairs: Vec<String> = data
            .iter()
            .filter(|(k, val)| !val.is_null() && !(k.as_str() == "tabId" && v.get("tabId") == Some(*val)))
            .map(|(k, val)| format!("{k}={}", short_value(val)))
            .collect();
        if !pairs.is_empty() {
            line.push_str("  ");
            line.push_str(&pairs.join(" "));
        }
    }
    line
}

fn short_value(v: &Value) -> String {
    let s = match v {
        Value::String(s) => {
            let s = s.replace(['\n', '\r'], " ");
            if s.contains(' ') || s.is_empty() { format!("\"{s}\"") } else { s }
        }
        other => other.to_string(),
    };
    if s.chars().count() > VALUE_MAX_CHARS {
        format!("{}…", s.chars().take(VALUE_MAX_CHARS).collect::<String>())
    } else {
        s
    }
}

/// `HH:MM:SS.mmm` of a Unix-millis timestamp shifted by `utc_offset_secs`.
fn clock(ms: u64, utc_offset_secs: i64) -> String {
    let local = ms as i64 + utc_offset_secs * 1000;
    let day_ms = local.rem_euclid(86_400_000);
    let (h, m, s, milli) = (day_ms / 3_600_000, day_ms / 60_000 % 60, day_ms / 1000 % 60, day_ms % 1000);
    format!("{h:02}:{m:02}:{s:02}.{milli:03}")
}

/// The machine's current offset from UTC in seconds (DST included); 0 when unknown.
#[cfg(windows)]
pub fn local_utc_offset_secs() -> i64 {
    use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    // SAFETY: GetTimeZoneInformation only writes into the zeroed struct we pass.
    let mut tz: TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
    let id = unsafe { GetTimeZoneInformation(&mut tz) };
    let bias = tz.Bias + if id == 2 { tz.DaylightBias } else { tz.StandardBias };
    -(bias as i64) * 60
}

#[cfg(not(windows))]
pub fn local_utc_offset_secs() -> i64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXT: &str = r#"{"data":{"type":"PING_TAB"},"level":"debug","msg":"message received","recv":1,"src":"content","tabId":7156425,"ts":1794652015823}
{"data":{"jobId":"job_1","model":"gemini-4-argon","tabId":900002,"requestedEffort":null,"error":"line one\nline two"},"level":"info","msg":"RUN_TURN dispatch","src":"bg","tabId":900002,"ts":1794652052501}
not json
{"level":"warn","msg":"Extension WebSocket disconnected.","ts":1794643252734}"#;

    #[test]
    fn formats_one_line_per_entry_with_level_source_tab_and_data() {
        let lines = render(EXT, &Filter { min_level: 0, grep: None }, 10, false, 7 * 3600);
        assert_eq!(lines.len(), 3, "unparseable line skipped");
        assert_eq!(lines[1], "17:27:32.501 INFO  bg      tab 900002   RUN_TURN dispatch  error=\"line one line two\" jobId=job_1 model=gemini-4-argon");
        assert!(lines[2].starts_with("15:00:52.734 WARN  Extension WebSocket disconnected."));
    }

    #[test]
    fn filters_by_minimum_level_and_text_then_keeps_the_last_n() {
        let info = render(EXT, &Filter { min_level: 1, grep: None }, 10, false, 0);
        assert_eq!(info.len(), 2, "debug PING_TAB hidden at info");
        let warn = render(EXT, &Filter { min_level: parse_level("warn").unwrap(), grep: None }, 10, false, 0);
        assert_eq!(warn.len(), 1);
        let grep = render(EXT, &Filter { min_level: 0, grep: Some("ARGON".into()) }, 10, true, 0);
        assert_eq!(grep.len(), 1);
        assert!(grep[0].starts_with('{'), "--json keeps the raw line");
        assert_eq!(render(EXT, &Filter { min_level: 0, grep: None }, 1, false, 0).len(), 1);
        assert!(parse_level("verbose").is_err());
    }

    #[test]
    fn long_values_are_cut() {
        let v: Value = serde_json::json!({"ts":0,"level":"info","msg":"m","data":{"k":"x".repeat(500)}});
        assert!(format_entry(&v, 0).chars().count() < 220);
    }
}
