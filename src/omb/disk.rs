//! Saving generated images into the asking bot's working folder and showing them in its reply, the way
//! OpenMausBot's Antigravity engine does (file in the project, preview and link in the chat).
//!
//! The image itself is made by `/v1/images/generations`, which OpenMausBot calls without saying which bot
//! asked. The bot is known one turn later: the chat turn that carries the `generate_image` tool result.
//! That turn's system prompt names the bot ("You are Artist, a personal bot in OpenMausBot"), and
//! OpenMausBot's `bots.json` maps the name to its working folder (`cwd`), or to its workspace folder.

use crate::state::{now_secs, AppState};
use crate::stream::NormalizedMessage;
use base64::Engine;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// One image ge-bridge made recently, kept until the tool-result turn picks it up.
#[derive(Debug, Clone)]
pub struct RecentImage {
    pub prompt: String,
    pub mime: String,
    pub b64: String,
    pub at: u64,
}

pub const RECENT_IMAGES_MAX: usize = 8;
/// How long an image waits for its tool-result turn.
pub const RECENT_IMAGE_TTL_SECS: u64 = 15 * 60;
/// Without a prompt match, only an image this fresh is taken as the one the tool just made.
const UNMATCHED_IMAGE_MAX_AGE_SECS: u64 = 3 * 60;
pub const IMAGES_SUBFOLDER: &str = "generated-images";

/// The prompt of the image call the latest tool result answers, when the turn ends on that result.
pub fn image_call_prompt(messages: &[NormalizedMessage]) -> Option<String> {
    if messages.last()?.role != "tool" {
        return None;
    }
    let call = messages.iter().rev().find(|m| m.role == "assistant" && !m.tool_calls.is_empty())?;
    call.tool_calls.iter().find_map(|tc| {
        let args: Value = serde_json::from_str(&tc.arguments_json).ok()?;
        let prompt = if tc.name.ends_with("generate_image") {
            args.get("prompt")
        } else if tc.name.ends_with("use_tool") && args.get("name").and_then(Value::as_str) == Some("generate_image") {
            args.pointer("/arguments/prompt")
        } else {
            None
        }?;
        prompt.as_str().map(|p| p.trim().to_string()).filter(|p| !p.is_empty())
    })
}

/// The image for `prompt`: the newest one made from the same prompt, else the newest one made in the
/// last few minutes (a harness may trim or rewrite the prompt it sends on).
pub fn pick_image<'a>(recent: &'a [RecentImage], prompt: &str, now: u64) -> Option<&'a RecentImage> {
    let live = |i: &&RecentImage| now.saturating_sub(i.at) <= RECENT_IMAGE_TTL_SECS;
    recent
        .iter()
        .rev()
        .filter(live)
        .find(|i| i.prompt.trim() == prompt.trim())
        .or_else(|| recent.iter().rev().filter(live).find(|i| now.saturating_sub(i.at) <= UNMATCHED_IMAGE_MAX_AGE_SECS))
}

/// "Artist" from "You are Artist, a personal bot in OpenMausBot. …".
pub fn bot_name(system: &str) -> Option<&str> {
    let rest = &system[system.find("You are ")? + "You are ".len()..];
    let name = rest[..rest.find(", ")?].trim();
    (!name.is_empty() && name.chars().count() <= 80).then_some(name)
}

/// The bot's working folder (`cwd`) from OpenMausBot's `bots.json`, else its workspace folder.
pub fn bot_folder(bots_json: &Value, name: &str, omb_dir: &Path) -> Option<PathBuf> {
    let bots = bots_json.as_array().or_else(|| bots_json.get("bots").and_then(Value::as_array))?;
    let bot = bots.iter().find(|b| b.get("name").and_then(Value::as_str) == Some(name))?;
    if let Some(cwd) = bot.get("cwd").and_then(Value::as_str).filter(|c| !c.trim().is_empty()) {
        return Some(PathBuf::from(cwd));
    }
    let id = bot.get("id").and_then(Value::as_str)?;
    Some(omb_dir.join("workspaces").join(id))
}

/// `sticker-cute-japanese-girl-1794752102.png`
pub fn file_name(prompt: &str, at: u64, mime: &str) -> String {
    let words: Vec<String> = prompt
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() > 2)
        .take(5)
        .map(str::to_ascii_lowercase)
        .collect();
    let stem = if words.is_empty() { "image".to_string() } else { words.join("-") };
    let ext = match mime {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        _ => "png",
    };
    format!("{stem}-{at}.{ext}")
}

/// `file:///D:/My%20Project/x.png` (what OpenMausBot's chat turns back into a local path).
pub fn file_url(path: &Path) -> String {
    let p = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file:///");
    for b in p.trim_start_matches('/').bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The text added to the bot's reply: a preview and a link to the saved file.
pub fn reply_note(path: &Path) -> String {
    let url = file_url(path);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    format!("\n\n![{name}]({url})\n\nSaved to [`{name}`]({url})")
}

/// Writes the image to `<folder>/generated-images/<name>`.
pub fn save(folder: &Path, img: &RecentImage) -> Result<PathBuf, String> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(&img.b64).map_err(|e| format!("bad image data: {e}"))?;
    let dir = folder.join(IMAGES_SUBFOLDER);
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let path = dir.join(file_name(&img.prompt, img.at, &img.mime));
    std::fs::write(&path, bytes).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    Ok(path)
}

/// Where the bot that sent `messages` keeps its files, read from OpenMausBot's own data folder.
pub fn folder_for(messages: &[NormalizedMessage], omb_dir: &Path) -> Option<PathBuf> {
    let system = messages.iter().find(|m| m.role == "system")?;
    let name = bot_name(&system.text)?;
    let raw = std::fs::read_to_string(omb_dir.join("bots.json")).ok()?;
    let bots: Value = serde_json::from_str(&raw).ok()?;
    bot_folder(&bots, name, omb_dir)
}

/// Saves the image ge-bridge made for `prompt` into the asking bot's working folder (OpenMausBot's
/// `bots.json`), or into Pictures\GeBridge when the bot is unknown, and returns the text to add to the
/// reply (preview and link). None when no matching image is waiting.
pub(crate) fn save_image_for_bot(state: &AppState, messages: &[NormalizedMessage], prompt: &str, job_id: &str) -> Option<String> {
    let img = {
        let mut recent = state.recent_images.lock();
        let list: Vec<_> = recent.iter().cloned().collect();
        let img = pick_image(&list, prompt, now_secs())?.clone();
        recent.retain(|i| !(i.at == img.at && i.prompt == img.prompt));
        img
    };
    // An isolated run (GE_BRIDGE_DATA_DIR, as the tests use) keeps both places inside its data folder.
    let isolated = crate::config::active().data_dir.clone();
    let omb = match &isolated {
        Some(d) => d.join("openmausbot"),
        None => dirs::home_dir()?.join(".openmausbot"),
    };
    let fallback = match &isolated {
        Some(d) => d.join("Pictures").join("GeBridge"),
        None => dirs::picture_dir().or_else(dirs::home_dir)?.join("GeBridge"),
    };
    let (folder, known) = match folder_for(messages, &omb) {
        Some(f) => (f, true),
        None => (fallback, false),
    };
    match save(&folder, &img) {
        Ok(path) => {
            state.log("info", format!("{job_id}: saved the image to {}{}", path.display(), if known { "" } else { " (bot folder unknown)" }));
            Some(reply_note(&path))
        }
        Err(e) => {
            state.log("warn", format!("{job_id}: could not save the image: {e}"));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::ParsedToolCall;
    use serde_json::json;

    fn msg(role: &str, text: &str) -> NormalizedMessage {
        NormalizedMessage { role: role.into(), text: text.into(), images: vec![], tool_calls: vec![], tool_call_id: None, tool_name: None }
    }

    fn call(name: &str, args: &str) -> NormalizedMessage {
        NormalizedMessage {
            tool_calls: vec![ParsedToolCall { id: "c1".into(), name: name.into(), arguments_json: args.into() }],
            ..msg("assistant", "")
        }
    }

    #[test]
    fn the_image_prompt_comes_from_the_call_the_tool_result_answers() {
        let bridged = vec![
            msg("user", "draw"),
            call("agents_use_tool", r#"{"name":"generate_image","arguments":{"prompt":" A cat ","aspect":"1:1"}}"#),
            msg("tool", "Image made"),
        ];
        assert_eq!(image_call_prompt(&bridged).as_deref(), Some("A cat"));
        let direct = vec![msg("user", "draw"), call("generate_image", r#"{"prompt":"A dog"}"#), msg("tool", "ok")];
        assert_eq!(image_call_prompt(&direct).as_deref(), Some("A dog"));
        let other = vec![msg("user", "x"), call("agents_use_tool", r#"{"name":"trigger","arguments":{}}"#), msg("tool", "ok")];
        assert_eq!(image_call_prompt(&other), None);
        assert_eq!(image_call_prompt(&bridged[..2]), None, "not a tool-result turn");
    }

    #[test]
    fn picks_the_matching_image_else_a_fresh_one() {
        let img = |p: &str, at| RecentImage { prompt: p.into(), mime: "image/png".into(), b64: String::new(), at };
        let recent = vec![img("A cat", 1000), img("A dog", 1100)];
        assert_eq!(pick_image(&recent, "A cat", 1200).unwrap().prompt, "A cat");
        assert_eq!(pick_image(&recent, "something else", 1200).unwrap().prompt, "A dog", "newest within 3 min");
        assert!(pick_image(&recent, "something else", 1100 + 181).is_none(), "too old without a match");
        assert!(pick_image(&recent, "A cat", 1000 + 15 * 60 + 1).is_none(), "expired");
    }

    #[test]
    fn the_bot_folder_is_its_cwd_or_its_workspace() {
        assert_eq!(bot_name("You are Artist, a personal bot in OpenMausBot. Role: …"), Some("Artist"));
        assert_eq!(bot_name("You are UX/UI Designer, a personal bot"), Some("UX/UI Designer"));
        assert_eq!(bot_name("Be helpful."), None);
        let bots = json!([
            { "id": "a1", "name": "Artist", "cwd": "D:\\My Project\\sticker_lines" },
            { "id": "t9", "name": "Task Intake", "cwd": null },
        ]);
        let omb = Path::new("C:/Users/u/.openmausbot");
        assert_eq!(bot_folder(&bots, "Artist", omb), Some(PathBuf::from("D:\\My Project\\sticker_lines")));
        assert_eq!(bot_folder(&bots, "Task Intake", omb), Some(omb.join("workspaces").join("t9")));
        assert_eq!(bot_folder(&bots, "Nobody", omb), None);
    }

    #[test]
    fn names_urls_and_the_reply_note() {
        assert_eq!(file_name("Chibi sticker of a cute Japanese girl, waving", 17, "image/png"), "chibi-sticker-cute-japanese-girl-17.png");
        assert_eq!(file_name("สติกเกอร์", 5, "image/jpeg"), "image-5.jpg");
        let p = Path::new("D:\\My Project\\sticker_lines\\generated-images\\a b.png");
        assert_eq!(file_url(p), "file:///D:/My%20Project/sticker_lines/generated-images/a%20b.png");
        let note = reply_note(p);
        assert!(note.contains("![a b.png](file:///D:/My%20Project/sticker_lines/generated-images/a%20b.png)"));
        assert!(note.contains("Saved to [`a b.png`]("));
    }

    #[test]
    fn save_writes_the_decoded_bytes_into_generated_images() {
        let dir = std::env::temp_dir().join(format!("ge-bridge-botimg-{}", std::process::id()));
        let img = RecentImage { prompt: "red apple".into(), mime: "image/png".into(), b64: "iVBORw0KGgo=".into(), at: 42 };
        let path = save(&dir, &img).unwrap();
        assert_eq!(path, dir.join("generated-images").join("red-apple-42.png"));
        assert_eq!(std::fs::read(&path).unwrap(), base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgo=").unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
