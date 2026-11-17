//! Everything specific to OpenMausBot (OMB): its message conventions, tool bridge, bot folders and config.

pub mod disk;
pub mod link;

use crate::stream::{tool_description, tool_name};
use serde_json::Value;

/// Notes OpenMausBot writes into user messages that are not the user's words: the context block it puts
/// in front of a message (`withContextNote` in its prompt-split driver) and its automatic retry notes.
pub(crate) const OMB_CONTEXT_PREFIXES: &[&str] = &[
    "Context from OpenMausBot updated since this conversation started",
    "The OpenMausBot context notes from earlier in this conversation have been cleared",
];
const OMB_META_PREFIX: &str = "[From OpenMausBot";
pub(crate) const OMB_TIME_LINE: &str = "Current time when this message was sent:";

const OMB_RECALL_END: &str = "[end of recalled passages";

/// The part of an OpenMausBot message after its recalled-passages block (the whole text when it has none).
pub fn after_recalled_passages(text: &str) -> &str {
    match text.rfind(OMB_RECALL_END) {
        Some(at) => text[at..].find('\n').map_or("", |i| &text[at + i..]),
        None => text,
    }
}

/// Whether a message's own text (after any recalled-passages block) mentions an attached image.
pub fn mentions_attached_image(text: &str) -> bool {
    after_recalled_passages(text).contains("<attached-image")
}

pub fn is_omb_meta(text: &str) -> bool {
    text.trim_start().starts_with(OMB_META_PREFIX)
}


/// OpenMausBot's tool bridge in one request: the find/use tool names and the hidden tools' names as listed.
pub struct Bridge<'a> {
    pub find_name: &'a str,
    pub use_name: String,
    /// "trigger, generate_image, … and 12 more"
    pub names: &'a str,
}

pub fn tool_bridge(tools: &[Value]) -> Option<Bridge<'_>> {
    let find = tools.iter().find(|t| tool_name(t).is_some_and(|n| n.ends_with("find_tool")))?;
    let find_name = tool_name(find)?;
    let use_name = format!("{}use_tool", find_name.strip_suffix("find_tool")?);
    if !tools.iter().any(|t| tool_name(t) == Some(use_name.as_str())) {
        return None;
    }
    const MARKER: &str = "Tools reached this way: ";
    let desc = tool_description(find)?;
    let names = desc[desc.find(MARKER)? + MARKER.len()..].trim().trim_end_matches('.').trim();
    if names.is_empty() || names == "none" {
        return None;
    }
    Some(Bridge { find_name, use_name, names })
}

/// OpenMausBot's tool bridge (switched on for models it thinks run locally, which includes ge-bridge on
/// 127.0.0.1) lists only a few tools plus `<prefix>find_tool` and `<prefix>use_tool`; the rest, including
/// `generate_image`, are only named in find_tool's description. Gemini tends to answer in text instead of
/// searching, so spell out that the hidden tools exist and can be run through use_tool right away.
pub fn hidden_tools_hint(tools: &[Value]) -> Option<String> {
    let Bridge { find_name, use_name, names } = tool_bridge(tools)?;
    let mut hint = format!(
        "HIDDEN TOOLS: these tools are not in the schema above but you can run them: {names}. \
         Run one directly with <tool_call>{{\"name\":\"{use_name}\",\"arguments\":{{\"name\":\"<tool name>\",\"arguments\":{{...}}}}}}</tool_call>; \
         call {find_name} first only when you need its exact input schema. \
         Never tell the user you cannot do something one of these tools does."
    );
    if names.split(", ").any(|n| n == "generate_image") {
        hint.push_str(&format!(
            " When the user asks for an image, picture, photo, drawing or illustration, do not reply with a prompt or a description: \
             call <tool_call>{{\"name\":\"{use_name}\",\"arguments\":{{\"name\":\"generate_image\",\"arguments\":{{\"prompt\":\"<concrete visual description in English>\",\"aspect\":\"1:1\"}}}}}}</tool_call> \
             (aspect is 1:1, 16:9 or 9:16); the image is attached to your reply automatically."
        ));
    }
    Some(hint)
}

