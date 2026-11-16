use std::path::PathBuf;
use std::sync::OnceLock;

/// Startup overrides read once from the environment (used to run isolated test instances).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeConfig {
    /// `GE_BRIDGE_PORT`: port for this run only (never persisted).
    pub port: Option<u16>,
    /// `GE_BRIDGE_DATA_DIR`: prefs + `token` file live here; skips Credential Manager and the singleton mutex.
    pub data_dir: Option<PathBuf>,
    /// `GE_BRIDGE_NO_TRAY`: run headless.
    pub no_tray: bool,
}

impl RuntimeConfig {
    pub fn from_env() -> Self {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let port = get("GE_BRIDGE_PORT").and_then(|v| v.trim().parse::<u16>().ok()).filter(|p| *p != 0);
        let data_dir = get("GE_BRIDGE_DATA_DIR")
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        let no_tray = get("GE_BRIDGE_NO_TRAY")
            .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);
        Self { port, data_dir, no_tray }
    }
}

static ACTIVE: OnceLock<RuntimeConfig> = OnceLock::new();

/// Install the process-wide config (first call wins).
pub fn init(cfg: RuntimeConfig) -> &'static RuntimeConfig {
    ACTIVE.get_or_init(|| cfg)
}

/// Process-wide config; defaults (no overrides) if `init` was never called.
pub fn active() -> &'static RuntimeConfig {
    ACTIVE.get_or_init(RuntimeConfig::default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn parse(pairs: &[(&str, &str)]) -> RuntimeConfig {
        let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        RuntimeConfig::from_lookup(|k| m.get(k).cloned())
    }

    #[test]
    fn nothing_set_means_defaults() {
        assert_eq!(parse(&[]), RuntimeConfig::default());
        assert_eq!(parse(&[]).port, None);
        assert!(!parse(&[]).no_tray);
    }

    #[test]
    fn port_parsing() {
        assert_eq!(parse(&[("GE_BRIDGE_PORT", "18787")]).port, Some(18787));
        assert_eq!(parse(&[("GE_BRIDGE_PORT", " 18787 ")]).port, Some(18787));
        for bad in ["", "abc", "0", "70000", "-1", "80.5"] {
            assert_eq!(parse(&[("GE_BRIDGE_PORT", bad)]).port, None, "{bad:?}");
        }
    }

    #[test]
    fn data_dir_parsing() {
        assert_eq!(parse(&[("GE_BRIDGE_DATA_DIR", "/tmp/x")]).data_dir, Some(PathBuf::from("/tmp/x")));
        assert_eq!(parse(&[("GE_BRIDGE_DATA_DIR", "  ")]).data_dir, None);
    }

    #[test]
    fn no_tray_parsing() {
        for on in ["1", "true", "TRUE", " 1 "] {
            assert!(parse(&[("GE_BRIDGE_NO_TRAY", on)]).no_tray, "{on:?}");
        }
        for off in ["0", "", "no", "false", "2"] {
            assert!(!parse(&[("GE_BRIDGE_NO_TRAY", off)]).no_tray, "{off:?}");
        }
    }
}
