use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub struct LinkOutcome {
    pub path: PathBuf,
    /// OpenMausBot's image provider points at ge-bridge (false: the user's own provider was kept).
    pub image_gen: bool,
    /// Bot and thread settings moved from the old instance to the ACP one; None when OpenMausBot was
    /// running, so `bots.json` was left alone (the app would write its own copy back over ours).
    pub moved_bots: Option<usize>,
}

/// The ACP agent instance OpenMausBot runs bots on (`ge-bridge-cli acp` over stdio).
pub const ACP_INSTANCE: &str = "ge-bridge-acp";
/// The openai-compat instance earlier versions wrote; removed in favour of `ACP_INSTANCE`.
pub const OLD_INSTANCE: &str = "gemini-enterprise";

/// Patches `%USERPROFILE%\.openmausbot\config.json` (backup `.bak`): adds the ACP instance, removes the old
/// openai-compat one, keeps image generation on ge-bridge, and moves bots on the old instance to the new one.
pub fn link_openmausbot(port: u16, token: &str, _default_model: &str, _enable_tools: bool) -> Result<LinkOutcome, String> {
    let home = dirs::home_dir().ok_or_else(|| "Could not locate user home directory".to_string())?;
    link_in(&home.join(".openmausbot"), &acp_command(), port, token, openmausbot_running())
}

pub fn link_in(maus_dir: &Path, cli: &str, port: u16, token: &str, omb_running: bool) -> Result<LinkOutcome, String> {
    fs::create_dir_all(maus_dir).map_err(|e| e.to_string())?;
    let config_path = maus_dir.join("config.json");

    let mut root: Value = if config_path.exists() {
        let raw = fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
        let _ = fs::write(config_path.with_extension("json.bak"), &raw);
        serde_json::from_str(&raw).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    if !root.get("instances").map(|v| v.is_object()).unwrap_or(false) {
        root["instances"] = json!({});
    }
    let instances = root["instances"].as_object_mut().expect("instances is an object");
    let old = instances.remove(OLD_INSTANCE);
    let existing = instances.get(ACP_INSTANCE).or(old.as_ref()).cloned();
    instances.insert(ACP_INSTANCE.into(), acp_instance(cli, existing.as_ref()));
    let image_gen = match openmausbot_image_gen(root.get("imageGen"), port, token) {
        Some(cfg) => {
            root["imageGen"] = cfg;
            true
        }
        None => false,
    };

    let formatted = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    fs::write(&config_path, format!("{formatted}\n")).map_err(|e| e.to_string())?;
    let moved_bots = if omb_running { None } else { Some(move_bots(&maus_dir.join("bots.json"))?) };
    Ok(LinkOutcome { path: config_path, image_gen, moved_bots })
}

/// Rewrites every `instanceId` / `lastInstanceId` naming the old instance in `bots.json` (backup `.bak`).
fn move_bots(path: &Path) -> Result<usize, String> {
    let Ok(raw) = fs::read_to_string(path) else { return Ok(0) };
    let mut bots: Value = serde_json::from_str(&raw).map_err(|e| format!("bots.json is not valid JSON: {e}"))?;
    let moved = rename_instance(&mut bots, OLD_INSTANCE, ACP_INSTANCE);
    if moved > 0 {
        fs::write(path.with_extension("json.bak"), &raw).map_err(|e| e.to_string())?;
        fs::write(path, serde_json::to_string(&bots).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    }
    Ok(moved)
}

fn rename_instance(v: &mut Value, from: &str, to: &str) -> usize {
    match v {
        Value::Object(map) => map
            .iter_mut()
            .map(|(k, val)| {
                if k.ends_with("nstanceId") && val.as_str() == Some(from) {
                    *val = json!(to);
                    1
                } else {
                    rename_instance(val, from, to)
                }
            })
            .sum(),
        Value::Array(items) => items.iter_mut().map(|i| rename_instance(i, from, to)).sum(),
        _ => 0,
    }
}

/// `"<dir>\ge-bridge-cli.exe" acp` next to the running exe; `ge-bridge.exe --acp` when the CLI is missing.
fn acp_command() -> String {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ge-bridge.exe"));
    let cli = exe.with_file_name("ge-bridge-cli.exe");
    if cli.exists() {
        format!("\"{}\" acp", cli.display())
    } else {
        format!("\"{}\" --acp", exe.display())
    }
}

#[cfg(windows)]
fn openmausbot_running() -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq OpenMausBot.exe", "/NH"])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW: the tray app has no console to borrow
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("OpenMausBot.exe"))
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn openmausbot_running() -> bool {
    false
}

const GE_BRIDGE_ICON: &[u8] = include_bytes!("../../extension/icons/icon128.png");

fn ge_bridge_icon() -> Value {
    use base64::{engine::general_purpose::STANDARD, Engine};
    json!({ "kind": "custom", "dataUrl": format!("data:image/png;base64,{}", STANDARD.encode(GE_BRIDGE_ICON)) })
}

pub const IMAGE_MODEL_ID: &str = "gemini-enterprise-image";

/// OpenMausBot's image provider set to ge-bridge's `/v1/images/generations` ("custom" provider).
/// Returns None (leave it alone) when the user already chose another image provider; a ge-bridge
/// entry is refreshed so a new token or port takes effect.
fn openmausbot_image_gen(current: Option<&Value>, port: u16, token: &str) -> Option<Value> {
    let url = format!("http://127.0.0.1:{port}/v1");
    let mut cfg = current.filter(|v| v.is_object()).cloned().unwrap_or_else(|| json!({}));
    let primary = cfg.get("primary").and_then(|p| p.get("provider")).and_then(|v| v.as_str())
        .or_else(|| cfg.get("provider").and_then(|v| v.as_str()));
    let ours = cfg.get("customUrl").and_then(|v| v.as_str()).is_some_and(|u| u.starts_with("http://127.0.0.1:"));
    match primary {
        None => {}
        Some("custom") if ours => {}
        Some(_) => return None,
    }
    cfg["primary"] = json!({ "provider": "custom", "model": IMAGE_MODEL_ID });
    cfg["customUrl"] = json!(url);
    cfg["customApiKey"] = json!(token);
    cfg["customModel"] = json!(IMAGE_MODEL_ID);
    Some(cfg)
}

/// The ACP instance entry. Shows the GeBridge icon instead of the driver's letter avatar, unless the user
/// already picked their own icon in OpenMausBot (on this instance or the old one). The Gemini preset that
/// earlier versions wrote is replaced.
fn acp_instance(cli: &str, existing: Option<&Value>) -> Value {
    let legacy = json!({ "kind": "preset", "preset": "google" });
    let icon = existing
        .and_then(|e| e.get("icon"))
        .filter(|i| **i != legacy)
        .cloned()
        .unwrap_or_else(ge_bridge_icon);
    json!({
        "driver": "customAcp",
        "displayName": "Gemini Enterprise (GeBridge)",
        "icon": icon,
        "config": { "cli": cli }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_gen_set_when_unset_refreshed_when_ours_left_alone_otherwise() {
        let fresh = openmausbot_image_gen(Some(&json!({ "maxPerTurn": 2 })), 8787, "tok").unwrap();
        assert_eq!(fresh["primary"], json!({ "provider": "custom", "model": IMAGE_MODEL_ID }));
        assert_eq!(fresh["customUrl"], "http://127.0.0.1:8787/v1");
        assert_eq!(fresh["customApiKey"], "tok");
        assert_eq!(fresh["maxPerTurn"], 2);
        assert!(openmausbot_image_gen(None, 8787, "tok").is_some());

        let refreshed = openmausbot_image_gen(Some(&fresh), 9000, "new").unwrap();
        assert_eq!(refreshed["customUrl"], "http://127.0.0.1:9000/v1");
        assert_eq!(refreshed["customApiKey"], "new");

        let openai = json!({ "primary": { "provider": "openai", "model": "gpt-image-1" }, "key": "sk" });
        assert!(openmausbot_image_gen(Some(&openai), 8787, "tok").is_none());
        let other_custom = json!({ "primary": { "provider": "custom", "model": "m" }, "customUrl": "https://img.example.com/v1" });
        assert!(openmausbot_image_gen(Some(&other_custom), 8787, "tok").is_none());
    }

    #[test]
    fn acp_instance_uses_ge_bridge_icon_and_keeps_a_user_icon() {
        let fresh = acp_instance("\"C:\\x\\ge-bridge-cli.exe\" acp", None);
        assert_eq!(fresh["driver"], "customAcp");
        assert_eq!(fresh["config"]["cli"], "\"C:\\x\\ge-bridge-cli.exe\" acp");
        assert_eq!(fresh["icon"]["kind"], "custom");
        let url = fresh["icon"]["dataUrl"].as_str().unwrap();
        assert!(url.starts_with("data:image/png;base64,iVBORw0KGgo"));
        assert!(url.len() < 128 * 1024 * 4 / 3);

        let custom = json!({ "icon": { "kind": "custom", "dataUrl": "data:image/png;base64,AAAA" } });
        assert_eq!(acp_instance("x", Some(&custom))["icon"], custom["icon"]);
        let old = json!({ "icon": { "kind": "preset", "preset": "google" } });
        assert_eq!(acp_instance("x", Some(&old))["icon"], fresh["icon"]);
    }

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ge-link-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn sample(dir: &Path) {
        let cfg = json!({
            "instances": {
                "gemini-enterprise": { "driver": "openai-compat", "icon": { "kind": "custom", "dataUrl": "data:image/png;base64,MINE" } },
                "claude": { "driver": "claude" }
            },
            "imageGen": {}
        });
        fs::write(dir.join("config.json"), cfg.to_string()).unwrap();
        let bots = json!([
            { "id": "b1", "name": "Artist", "modelSelection": { "instanceId": "gemini-enterprise", "model": "gemini-3.8-flash" },
              "tasks": [{ "threadId": "t1", "lastInstanceId": "gemini-enterprise", "modelSelection": { "instanceId": "gemini-enterprise" } }] },
            { "id": "b2", "name": "Coder", "modelSelection": { "instanceId": "claude" } }
        ]);
        fs::write(dir.join("bots.json"), bots.to_string()).unwrap();
    }

    #[test]
    fn link_replaces_the_old_instance_and_moves_its_bots() {
        let dir = temp_dir("move");
        sample(&dir);
        let out = link_in(&dir, "\"C:\\x\\ge-bridge-cli.exe\" acp", 8787, "tok", false).unwrap();
        assert_eq!(out.moved_bots, Some(3));
        assert!(out.image_gen);
        let cfg: Value = serde_json::from_str(&fs::read_to_string(dir.join("config.json")).unwrap()).unwrap();
        assert!(cfg["instances"].get(OLD_INSTANCE).is_none());
        assert_eq!(cfg["instances"]["claude"]["driver"], "claude", "other instances kept");
        assert_eq!(cfg["instances"][ACP_INSTANCE]["driver"], "customAcp");
        assert_eq!(cfg["instances"][ACP_INSTANCE]["icon"]["dataUrl"], "data:image/png;base64,MINE", "the user's icon moves over");
        assert_eq!(cfg["imageGen"]["customUrl"], "http://127.0.0.1:8787/v1");
        let bots = fs::read_to_string(dir.join("bots.json")).unwrap();
        assert!(!bots.contains("gemini-enterprise"));
        assert!(bots.contains("\"model\":\"gemini-3.8-flash\""), "the chosen model stays");
        assert!(fs::read_to_string(dir.join("bots.json.bak")).unwrap().contains("gemini-enterprise"));
        assert_eq!(link_in(&dir, "x", 8787, "tok", false).unwrap().moved_bots, Some(0), "running again is harmless");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bots_are_left_alone_while_openmausbot_runs() {
        let dir = temp_dir("running");
        sample(&dir);
        let out = link_in(&dir, "x", 8787, "tok", true).unwrap();
        assert_eq!(out.moved_bots, None);
        assert!(fs::read_to_string(dir.join("bots.json")).unwrap().contains("gemini-enterprise"));
        let _ = fs::remove_dir_all(&dir);
    }
}
