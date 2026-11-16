use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedMessage {
    pub role: String,
    pub text: String,
    pub images: Vec<String>,
    pub tool_calls: Vec<ParsedToolCall>,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedToolCall {
    pub id: String,
    pub name: String,
    pub arguments_json: String,
}

pub fn parse_openai_messages(messages: &[Value]) -> Vec<NormalizedMessage> {
    let mut out = Vec::with_capacity(messages.len());
    for m in messages {
        let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("user").to_string();
        let mut texts = Vec::new();
        let mut images = Vec::new();

        match m.get("content") {
            Some(Value::String(s)) => texts.push(s.clone()),
            Some(Value::Array(arr)) => {
                for part in arr {
                    match part.get("type").and_then(|v| v.as_str()) {
                        Some("text") => {
                            if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                                texts.push(t.to_string());
                            }
                        }
                        Some("image_url") => {
                            if let Some(u) = part.pointer("/image_url/url").and_then(|v| v.as_str()) {
                                images.push(u.to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        let mut tool_calls = Vec::new();
        if let Some(Value::Array(tcs)) = m.get("tool_calls") {
            for tc in tcs {
                let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("call_0").to_string();
                let name = tc.pointer("/function/name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let args = tc.pointer("/function/arguments").and_then(|v| v.as_str()).unwrap_or("{}").to_string();
                if !name.is_empty() {
                    tool_calls.push(ParsedToolCall { id, name, arguments_json: args });
                }
            }
        }

        out.push(NormalizedMessage {
            role,
            text: texts.join("\n"),
            images,
            tool_calls,
            tool_call_id: m.get("tool_call_id").and_then(|v| v.as_str()).map(String::from),
            tool_name: m.get("name").and_then(|v| v.as_str()).map(String::from),
        });
    }
    out
}

/// An Anthropic `image` block as the string the extension pastes: a base64 source becomes a data URL,
/// a URL source is kept as is so the server can reject it.
fn anthropic_image(b: &Value) -> Option<String> {
    if let Some(u) = b.pointer("/source/url").and_then(|v| v.as_str()) {
        return Some(u.to_string());
    }
    let media = b.pointer("/source/media_type").and_then(|v| v.as_str())?;
    let data = b.pointer("/source/data").and_then(|v| v.as_str())?;
    Some(format!("data:{media};base64,{data}"))
}

/// True for the only image form the extension can paste: `data:image/...;base64,...`.
pub fn is_base64_image(u: &str) -> bool {
    u.strip_prefix("data:image/").is_some_and(|r| r.split_once(',').is_some_and(|(h, _)| h.ends_with(";base64")))
}

pub fn parse_anthropic_messages(system: Option<&Value>, messages: &[Value]) -> Vec<NormalizedMessage> {
    let mut out = Vec::new();
    if let Some(sys) = system {
        let sys_text = match sys {
            Value::String(s) => s.clone(),
            Value::Array(arr) => arr
                .iter()
                .filter_map(|p| p.get("text").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };
        if !sys_text.is_empty() {
            out.push(NormalizedMessage {
                role: "system".into(),
                text: sys_text,
                images: vec![],
                tool_calls: vec![],
                tool_call_id: None,
                tool_name: None,
            });
        }
    }

    for m in messages {
        let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("user").to_string();
        let mut texts = Vec::new();
        let mut images = Vec::new();
        let mut tool_calls = Vec::new();

        match m.get("content") {
            Some(Value::String(s)) => texts.push(s.clone()),
            Some(Value::Array(blocks)) => {
                for b in blocks {
                    match b.get("type").and_then(|v| v.as_str()) {
                        Some("text") => {
                            if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                                texts.push(t.to_string());
                            }
                        }
                        Some("image") => images.extend(anthropic_image(b)),
                        Some("tool_use") => {
                            let id = b.get("id").and_then(|v| v.as_str()).unwrap_or("toolu_0").to_string();
                            let name = b.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let input = b.get("input").cloned().unwrap_or(Value::Object(Default::default()));
                            tool_calls.push(ParsedToolCall {
                                id,
                                name,
                                arguments_json: input.to_string(),
                            });
                        }
                        Some("tool_result") => {
                            let id = b.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("");
                            let mut tr_images = Vec::new();
                            let content_str = match b.get("content") {
                                Some(Value::String(s)) => s.clone(),
                                Some(Value::Array(parts)) => {
                                    let mut tr_texts = Vec::new();
                                    for p in parts {
                                        match p.get("type").and_then(|v| v.as_str()) {
                                            Some("text") => {
                                                tr_texts.extend(p.get("text").and_then(|v| v.as_str()).map(String::from))
                                            }
                                            Some("image") => tr_images.extend(anthropic_image(p)),
                                            _ => {}
                                        }
                                    }
                                    tr_texts.join("\n")
                                }
                                Some(other) => other.to_string(),
                                None => String::new(),
                            };
                            out.push(NormalizedMessage {
                                role: "tool".into(),
                                text: content_str,
                                images: tr_images,
                                tool_calls: vec![],
                                tool_call_id: Some(id.to_string()),
                                tool_name: None,
                            });
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        if !texts.is_empty() || !images.is_empty() || !tool_calls.is_empty() {
            out.push(NormalizedMessage {
                role,
                text: texts.join("\n"),
                images,
                tool_calls,
                tool_call_id: None,
                tool_name: None,
            });
        }
    }
    out
}

/// Opens every prompt that carries tools. Gemini Enterprise otherwise answers as itself ("I am a chat
/// assistant and cannot make images") instead of acting as an agent's model.
pub const ROLE_PREAMBLE: &str = "[ROLE]\n\
You are the language-model backend of an AI agent app running on the user's computer. \
The app runs tools for you (files, commands, web, images, messages and more) and sends you their results. \
Never say you cannot do something because you are a chat or text-only model: when a tool can do it, call the tool. \
Follow the system instructions and reply in the user's language.\n\
[/ROLE]";

/// Tool descriptions past this are cut: an agent's 100+ tools otherwise fill most of the prompt.
pub const TOOL_DESC_MAX_CHARS: usize = 900;
/// Tool results kept whole; older ones are cut to `OLD_TOOL_RESULT_MAX_CHARS`.
pub const RECENT_TOOL_RESULTS: usize = 8;
pub const OLD_TOOL_RESULT_MAX_CHARS: usize = 300;
pub const TOOL_RESULT_MAX_CHARS: usize = 20_000;
const OBJECTIVE_MAX_CHARS: usize = 2_000;

fn cut_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    let cut = cut.rsplit_once(' ').map(|(head, _)| head.to_string()).filter(|h| h.len() > max / 2).unwrap_or(cut);
    format!("{cut}…")
}

fn head_tail(s: &str, head: usize, tail: usize) -> String {
    let n = s.chars().count();
    let h: String = s.chars().take(head).collect();
    let t: String = s.chars().skip(n - tail).collect();
    format!("{h}\n[... tool output cut: {n} chars ...]\n{t}")
}

/// Huge results keep their start and end; results older than the last `RECENT_TOOL_RESULTS` are cut short.
pub fn prune_tool_result(text: &str, recent: bool) -> String {
    let n = text.chars().count();
    if recent {
        if n > TOOL_RESULT_MAX_CHARS { head_tail(text, 12_000, 3_000) } else { text.to_string() }
    } else if n > OLD_TOOL_RESULT_MAX_CHARS {
        head_tail(text, 200, 100)
    } else {
        text.to_string()
    }
}

pub fn build_tool_protocol_block(tools: &[Value]) -> String {
    let compact: Vec<Value> = tools
        .iter()
        .map(|t| {
            let (name, desc, params) = match t.get("function") {
                Some(func) => (func.get("name"), func.get("description"), func.get("parameters")),
                None => (t.get("name"), t.get("description"), t.get("input_schema")),
            };
            let desc = desc.and_then(Value::as_str).map(|d| cut_chars(d, TOOL_DESC_MAX_CHARS));
            serde_json::json!({ "name": name, "description": desc, "parameters": params })
        })
        .collect();

    format!(
        "[AGENT_TOOL_PROTOCOL]\n\
         You have access to external tools. Whenever you want to execute a tool, output ONLY raw <tool_call> tags (never inside markdown code fences) formatted as single-line JSON:\n\
         <tool_call>{{\"name\":\"tool_name\",\"arguments\":{{\"key\":\"value\"}}}}</tool_call>\n\
         Wait for [TOOL_RESULT] before continuing.\n\
         Tools Schema: {}\n\
         [/AGENT_TOOL_PROTOCOL]",
        serde_json::to_string(&compact).unwrap_or_else(|_| "[]".into())
    )
}

/// Closes every prompt that carries tools, so the format and the hidden-tool note are the last thing
/// Gemini reads after a long system prompt and history.
pub fn output_rules(tools: &[Value]) -> String {
    let mut s = String::from(
        "[OUTPUT_RULES]\n\
         Respond with exactly one of:\n\
         1. One or more tool calls, each as raw <tool_call>{\"name\":\"tool_name\",\"arguments\":{...}}</tool_call> (not in a code fence)\n\
         2. A plain-text final answer, only when no tool applies.\n\
         Never invent tool results: real results arrive as [TOOL_RESULT] in the next turn.\n",
    );
    if let Some(h) = hidden_tools_hint(tools) {
        s.push_str(&h);
        s.push('\n');
    }
    s.push_str("[/OUTPUT_RULES]");
    s
}

fn tool_name(t: &Value) -> Option<&str> {
    t.pointer("/function/name").or_else(|| t.get("name")).and_then(Value::as_str)
}

fn tool_description(t: &Value) -> Option<&str> {
    t.pointer("/function/description").or_else(|| t.get("description")).and_then(Value::as_str)
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

/// How this request's agent makes images: a listed tool whose name ends in `generate_image`, or
/// `generate_image` hidden behind the tool bridge.
#[derive(Debug, PartialEq)]
pub enum ImageTool {
    Direct(String),
    Bridged { use_name: String },
}

pub fn image_tool(tools: &[Value]) -> Option<ImageTool> {
    if let Some(n) = tools.iter().filter_map(tool_name).find(|n| n.ends_with("generate_image")) {
        return Some(ImageTool::Direct(n.to_string()));
    }
    let b = tool_bridge(tools)?;
    b.names.split(", ").any(|n| n == "generate_image").then(|| ImageTool::Bridged { use_name: b.use_name })
}

/// A `<tool_call>` that makes one image with `tool`.
pub fn image_tool_call(tool: &ImageTool, prompt: &str, aspect: &str) -> String {
    let args = serde_json::json!({ "prompt": prompt, "aspect": aspect });
    let call = match tool {
        ImageTool::Direct(name) => serde_json::json!({ "name": name, "arguments": args }),
        ImageTool::Bridged { use_name } => {
            serde_json::json!({ "name": use_name, "arguments": { "name": "generate_image", "arguments": args } })
        }
    };
    format!("<tool_call>{call}</tool_call>")
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

fn render_tool_call(tc: &ParsedToolCall) -> String {
    format!(
        "<tool_call>{{\"name\":{},\"arguments\":{}}}</tool_call>",
        serde_json::to_string(&tc.name).unwrap_or_default(),
        tc.arguments_json
    )
}

fn render_tool_result(m: &NormalizedMessage, recent: bool) -> String {
    format!("[TOOL_RESULT id=\"{}\"]\n{}", m.tool_call_id.as_deref().unwrap_or(""), prune_tool_result(&m.text, recent))
}

/// Indexes of tool results that stay whole (the last `RECENT_TOOL_RESULTS`).
fn recent_tool_results(messages: &[NormalizedMessage]) -> std::collections::HashSet<usize> {
    let idx: Vec<usize> = messages.iter().enumerate().filter(|(_, m)| m.role == "tool").map(|(i, _)| i).collect();
    idx[idx.len().saturating_sub(RECENT_TOOL_RESULTS)..].iter().copied().collect()
}

/// When the turn ends on tool results, restate the request they serve, so the model answers it instead
/// of summarising earlier work.
fn active_objective(messages: &[NormalizedMessage]) -> Option<String> {
    if messages.last()?.role != "tool" {
        return None;
    }
    let req = messages
        .iter()
        .rev()
        .filter(|m| m.role == "user")
        .map(|m| crate::recovery::user_request_text(&m.text).0)
        .find(|t| !t.is_empty() && !t.starts_with("[From OpenMausBot"))?;
    Some(format!(
        "[ACTIVE OBJECTIVE]\nThe user request being served: \"{}\"\n\
         Use the tool results above only to serve this request. When it is done, answer it directly; do not repeat earlier reports.\n\
         [/ACTIVE OBJECTIVE]",
        cut_chars(req, OBJECTIVE_MAX_CHARS)
    ))
}

/// Index of the first message after the last assistant reply (the current turn).
fn turn_start(messages: &[NormalizedMessage]) -> usize {
    let mut start = messages.len().saturating_sub(1);
    while start > 0 && messages[start - 1].role != "assistant" {
        start -= 1;
    }
    start
}

pub fn render_turn_prompt(
    messages: &[NormalizedMessage],
    tools: Option<&[Value]>,
    new_chat: bool,
    inject_tools: bool,
) -> (String, Vec<String>) {
    let tools = tools.filter(|t| !t.is_empty());
    let mut blocks = Vec::new();
    let mut images = Vec::new();
    let recent = recent_tool_results(messages);

    if let Some(t) = tools {
        if new_chat || inject_tools {
            blocks.push(ROLE_PREAMBLE.to_string());
        }
        if inject_tools {
            blocks.push(build_tool_protocol_block(t));
        }
    }

    if new_chat {
        let single_user = messages.len() == 1 && messages[0].role == "user" && tools.is_none();
        let start = turn_start(messages);
        for (i, m) in messages.iter().enumerate() {
            if i >= start {
                images.extend(m.images.clone());
            }
            match m.role.as_str() {
                "system" => blocks.push(format!("[SYSTEM INSTRUCTIONS]\n{}", m.text)),
                "user" if single_user => blocks.push(m.text.clone()),
                "user" => blocks.push(format!("[USER]\n{}", m.text)),
                "assistant" => {
                    let mut parts = Vec::new();
                    if !m.text.is_empty() {
                        parts.push(m.text.clone());
                    }
                    parts.extend(m.tool_calls.iter().map(render_tool_call));
                    blocks.push(format!("[ASSISTANT]\n{}", parts.join("\n")));
                }
                "tool" => blocks.push(render_tool_result(m, recent.contains(&i))),
                _ => blocks.push(m.text.clone()),
            }
            if i < start && !m.images.is_empty() {
                if let Some(last) = blocks.last_mut() {
                    last.push_str("\n(earlier image not re-sent)");
                }
            }
        }
    } else {
        // Continuation delta: only send messages appended after the last assistant reply
        let start = turn_start(messages);
        for (i, m) in messages.iter().enumerate().skip(start) {
            images.extend(m.images.clone());
            if m.role == "tool" {
                blocks.push(render_tool_result(m, recent.contains(&i)));
            } else {
                blocks.push(m.text.clone());
            }
        }
    }

    if let Some(t) = tools {
        blocks.extend(active_objective(messages));
        blocks.push(output_rules(t));
    }

    (join_blocks(blocks), images)
}

fn join_blocks(blocks: Vec<String>) -> String {
    blocks.into_iter().filter(|s| !s.trim().is_empty()).collect::<Vec<_>>().join("\n\n")
}

/// System text kept in a recovery prompt.
const RECOVERY_SYSTEM_MAX_CHARS: usize = 8_000;

/// The prompt for a second try in a fresh Gemini chat after a refusal or a policy block: the role,
/// a shortened system prompt, the tools, and only the current request with the tool results that
/// follow it. Long histories are what most often trip a refusal. Card and account numbers are masked.
pub fn render_recovery_prompt(messages: &[NormalizedMessage], tools: Option<&[Value]>) -> (String, Vec<String>) {
    let tools = tools.filter(|t| !t.is_empty());
    let mut blocks = vec![ROLE_PREAMBLE.to_string()];
    let system: Vec<&str> = messages.iter().filter(|m| m.role == "system").map(|m| m.text.as_str()).collect();
    if !system.is_empty() {
        blocks.push(format!("[SYSTEM INSTRUCTIONS]\n{}", cut_chars(&system.join("\n\n"), RECOVERY_SYSTEM_MAX_CHARS)));
    }
    if let Some(t) = tools {
        blocks.push(build_tool_protocol_block(t));
    }
    let start = messages.iter().rposition(|m| m.role == "user").unwrap_or(0);
    let recent = recent_tool_results(messages);
    let mut images = Vec::new();
    for (i, m) in messages.iter().enumerate().skip(start) {
        images.extend(m.images.clone());
        match m.role.as_str() {
            "user" => blocks.push(format!("[USER REQUEST]\n{}", m.text)),
            "assistant" if !m.tool_calls.is_empty() => {
                blocks.push(format!("[ASSISTANT]\n{}", m.tool_calls.iter().map(render_tool_call).collect::<Vec<_>>().join("\n")))
            }
            "tool" => blocks.push(render_tool_result(m, recent.contains(&i))),
            _ => {}
        }
    }
    if let Some(t) = tools {
        blocks.extend(active_objective(messages));
        blocks.push(output_rules(t));
    }
    (mask_financial_numbers(&join_blocks(blocks)), images)
}

/// Masks card numbers (4-4-4-4) and Thai bank account numbers (3-1-5-1), keeping the first and last group.
pub fn mask_financial_numbers(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    'scan: while i < chars.len() {
        let at_boundary = i == 0 || !chars[i - 1].is_ascii_alphanumeric();
        if at_boundary && chars[i].is_ascii_digit() {
            for (groups, need_sep) in [(&[4usize, 4, 4, 4][..], false), (&[3, 1, 5, 1][..], true)] {
                if let Some(end) = match_groups(&chars, i, groups, need_sep) {
                    let digits: Vec<char> = chars[i..end].iter().copied().filter(char::is_ascii_digit).collect();
                    let last = groups[groups.len() - 1];
                    let mut parts: Vec<String> = vec![digits[..groups[0]].iter().collect()];
                    parts.extend(groups[1..groups.len() - 1].iter().map(|g| "X".repeat(*g)));
                    parts.push(digits[digits.len() - last..].iter().collect());
                    out.push_str(&parts.join("-"));
                    i = end;
                    continue 'scan;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// End index when `chars[start..]` is digit groups of these sizes, joined by '-' or ' ' (or nothing,
/// unless `need_sep`: a bare 10-digit run is more often a phone number than an account), and not
/// followed by another digit or letter.
fn match_groups(chars: &[char], start: usize, groups: &[usize], need_sep: bool) -> Option<usize> {
    let mut i = start;
    for (gi, g) in groups.iter().enumerate() {
        if gi > 0 {
            if i < chars.len() && (chars[i] == '-' || chars[i] == ' ') {
                i += 1;
            } else if need_sep {
                return None;
            }
        }
        for _ in 0..*g {
            if i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            } else {
                return None;
            }
        }
    }
    if i < chars.len() && chars[i].is_ascii_alphanumeric() {
        return None;
    }
    Some(i)
}

/// Streaming Lookahead State Machine separating CoT, normal content, and <tool_call> XML tags
pub struct StreamInterceptor {
    tools_enabled: bool,
    buf: String,
    inside_tool: bool,
    pub full_clean_text: String,
    pub full_reasoning: String,
    pub tool_calls: Vec<ParsedToolCall>,
}

impl StreamInterceptor {
    pub fn new(tools_enabled: bool) -> Self {
        Self {
            tools_enabled,
            buf: String::new(),
            inside_tool: false,
            full_clean_text: String::new(),
            full_reasoning: String::new(),
            tool_calls: Vec::new(),
        }
    }

    pub fn push_chunk(&mut self, thought_delta: Option<&str>, text_delta: &str) -> (Option<String>, Option<String>) {
        let emitted_thought = thought_delta.filter(|s| !s.is_empty()).map(|s| {
            self.full_reasoning.push_str(s);
            s.to_string()
        });

        if text_delta.is_empty() {
            return (emitted_thought, None);
        }

        if !self.tools_enabled {
            self.full_clean_text.push_str(text_delta);
            return (emitted_thought, Some(text_delta.to_string()));
        }

        self.buf.push_str(text_delta);
        let mut emitted_text = String::new();
        const OPEN_TAG: &str = "<tool_call>";
        const CLOSE_TAG: &str = "</tool_call>";

        loop {
            if !self.inside_tool {
                if let Some(idx) = self.buf.find(OPEN_TAG) {
                    let before = &self.buf[..idx];
                    emitted_text.push_str(before);
                    self.full_clean_text.push_str(before);
                    self.buf.drain(..idx + OPEN_TAG.len());
                    self.inside_tool = true;
                } else {
                    let safe_len = safe_prefix_flush_len(&self.buf, OPEN_TAG);
                    if safe_len > 0 {
                        let chunk = &self.buf[..safe_len];
                        emitted_text.push_str(chunk);
                        self.full_clean_text.push_str(chunk);
                        self.buf.drain(..safe_len);
                    }
                    break;
                }
            } else if let Some(end_idx) = self.buf.find(CLOSE_TAG) {
                let raw_json = self.buf[..end_idx].trim().to_string();
                self.buf.drain(..end_idx + CLOSE_TAG.len());
                self.inside_tool = false;
                if let Some(tc) = try_parse_tool_json(&raw_json) {
                    self.tool_calls.push(tc);
                }
            } else {
                break;
            }
        }

        let text_out = if emitted_text.is_empty() { None } else { Some(emitted_text) };
        (emitted_thought, text_out)
    }

    pub fn finalize(&mut self, final_full_text: &str) -> Option<String> {
        // Fallback pass in case Gemini wrapped <tool_call> inside markdown code fences
        let normalized = normalize_tool_tags(final_full_text);
        if self.tools_enabled && self.tool_calls.is_empty() && normalized.contains("<tool_call>") {
            let stripped = strip_fenced_tool_calls(&normalized, &mut self.tool_calls);
            self.full_clean_text = stripped;
            return None;
        }
        if !self.inside_tool && !self.buf.is_empty() {
            let tail = std::mem::take(&mut self.buf);
            self.full_clean_text.push_str(&tail);
            return Some(tail);
        }
        None
    }
}

fn safe_prefix_flush_len(buf: &str, tag: &str) -> usize {
    let max_check = tag.len().saturating_sub(1).min(buf.len());
    for k in (1..=max_check).rev() {
        let start = buf.len() - k;
        if buf.is_char_boundary(start) && tag.starts_with(&buf[start..]) {
            return start;
        }
    }
    buf.len()
}

/// Other spellings models use for the call tags, mapped to `<tool_call>` / `</tool_call>`.
fn normalize_tool_tags(text: &str) -> String {
    let mut out = text.to_string();
    for (from, to) in [
        ("<<<TOOL_CALL>>>", "<tool_call>"),
        ("<<<END_TOOL_CALL>>>", "</tool_call>"),
        ("<<</TOOL_CALL>>>", "</tool_call>"),
        ("<TOOL_CALL>", "<tool_call>"),
        ("</TOOL_CALL>", "</tool_call>"),
    ] {
        out = out.replace(from, to);
    }
    out
}

fn try_parse_tool_json(raw: &str) -> Option<ParsedToolCall> {
    let raw = raw.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    let parsed: Value = serde_json::from_str(raw).ok()?;
    // {"function":{"name":..,"arguments":..}} is accepted as well as {"name":..,"arguments":..}
    let parsed = match parsed.get("name") {
        Some(_) => parsed,
        None => parsed.get("function")?.clone(),
    };
    let name = parsed.get("name")?.as_str()?.to_string();
    let args = match parsed.get("arguments") {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => "{}".to_string(),
    };
    let mut id_bytes = [0u8; 6];
    rand::thread_rng().fill_bytes(&mut id_bytes);
    Some(ParsedToolCall {
        id: format!("call_{}", hex::encode(id_bytes)),
        name,
        arguments_json: args,
    })
}

fn strip_fenced_tool_calls(text: &str, out: &mut Vec<ParsedToolCall>) -> String {
    let mut remaining = text.to_string();
    while let Some(start) = remaining.find("<tool_call>") {
        if let Some(rel_end) = remaining[start..].find("</tool_call>") {
            let end = start + rel_end;
            let inner = &remaining[start + 11..end];
            if let Some(tc) = try_parse_tool_json(inner.trim()) {
                out.push(tc);
            }
            remaining.replace_range(start..end + 12, "");
        } else {
            break;
        }
    }
    remaining.replace("```xml", "").replace("```json", "").replace("```", "").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CALL: &str = r#"<tool_call>{"name":"get_weather","arguments":{"city":"Bangkok"}}</tool_call>"#;

    /// Feed chunks, finalize with the full text, and return (emitted text, interceptor).
    fn run(tools: bool, chunks: &[&str]) -> (String, StreamInterceptor) {
        let mut it = StreamInterceptor::new(tools);
        let mut emitted = String::new();
        for c in chunks {
            if let (_, Some(t)) = it.push_chunk(None, c) {
                emitted.push_str(&t);
            }
        }
        let full: String = chunks.concat();
        if let Some(tail) = it.finalize(&full) {
            emitted.push_str(&tail);
        }
        (emitted, it)
    }

    fn assert_single_weather_call(it: &StreamInterceptor) {
        assert_eq!(it.tool_calls.len(), 1, "calls: {:?}", it.tool_calls);
        let tc = &it.tool_calls[0];
        assert_eq!(tc.name, "get_weather");
        let args: Value = serde_json::from_str(&tc.arguments_json).unwrap();
        assert_eq!(args, json!({"city": "Bangkok"}));
        assert!(tc.id.starts_with("call_"));
    }

    #[test]
    fn plain_text_passes_through() {
        let (out, it) = run(true, &["Hello ", "wor", "ld <b>x</b> 1 < 2"]);
        assert_eq!(out, "Hello world <b>x</b> 1 < 2");
        assert_eq!(it.full_clean_text, out);
        assert!(it.tool_calls.is_empty());
    }

    #[test]
    fn tool_call_in_one_chunk() {
        let text = format!("Let me check. {CALL}");
        let (out, it) = run(true, &[&text]);
        assert_eq!(out, "Let me check. ");
        assert_eq!(it.full_clean_text, "Let me check. ");
        assert_single_weather_call(&it);
    }

    #[test]
    fn tool_call_split_at_every_byte_position() {
        let full = format!("before {CALL} after");
        for i in 0..=full.len() {
            let (a, b) = full.split_at(i);
            let (out, it) = run(true, &[a, b]);
            assert!(!out.contains("<tool_call") && !out.contains("</tool_call"), "split {i}: {out:?}");
            assert_eq!(out, "before  after", "split {i}");
            assert_eq!(it.full_clean_text, "before  after", "split {i}");
            assert_single_weather_call(&it);
        }
    }

    #[test]
    fn tool_call_fed_one_char_at_a_time() {
        let full = format!("x {CALL} y");
        let chunks: Vec<String> = full.chars().map(String::from).collect();
        let refs: Vec<&str> = chunks.iter().map(String::as_str).collect();
        let (out, it) = run(true, &refs);
        assert_eq!(out, "x  y");
        assert_single_weather_call(&it);
    }

    #[test]
    fn multibyte_text_around_tool_call() {
        let full = format!("สวัสดี {CALL} จบ");
        let boundaries: Vec<usize> = full.char_indices().map(|(i, _)| i).chain([full.len()]).collect();
        for i in boundaries {
            let (a, b) = full.split_at(i);
            let (out, it) = run(true, &[a, b]);
            assert_eq!(out, "สวัสดี  จบ", "split {i}");
            assert_single_weather_call(&it);
        }
    }

    #[test]
    fn multiple_tool_calls_keep_order_and_unique_ids() {
        let text = concat!(
            r#"<tool_call>{"name":"a","arguments":{"n":1}}</tool_call>"#,
            "mid",
            r#"<tool_call>{"name":"b","arguments":"{\"n\":2}"}</tool_call>"#,
        );
        let (out, it) = run(true, &[&text[..20], &text[20..70], &text[70..]]);
        assert_eq!(out, "mid");
        assert_eq!(it.tool_calls.len(), 2);
        assert_eq!(it.tool_calls[0].name, "a");
        assert_eq!(it.tool_calls[1].name, "b");
        assert_eq!(it.tool_calls[1].arguments_json, r#"{"n":2}"#, "string arguments are passed through verbatim");
        assert_ne!(it.tool_calls[0].id, it.tool_calls[1].id);
    }

    #[test]
    fn missing_arguments_default_to_empty_object() {
        let (_, it) = run(true, &[r#"<tool_call>{"name":"ping"}</tool_call>"#]);
        assert_eq!(it.tool_calls[0].arguments_json, "{}");
    }

    #[test]
    fn malformed_json_in_tags_is_dropped_without_leaking_tags() {
        for body in ["{not json", "[1,2]", r#"{"arguments":{}}"#, r#"{"name":5}"#, ""] {
            let text = format!("a<tool_call>{body}</tool_call>b");
            let (out, it) = run(true, &[&text]);
            assert!(it.tool_calls.is_empty(), "{body:?}");
            assert!(!it.full_clean_text.contains("tool_call"), "{body:?}: {:?}", it.full_clean_text);
            assert!(!out.contains("tool_call"), "{body:?}");
        }
    }

    #[test]
    fn tools_disabled_passes_tags_through_as_text() {
        let text = format!("hi {CALL}");
        let (out, it) = run(false, &[&text[..5], &text[5..]]);
        assert_eq!(out, text);
        assert_eq!(it.full_clean_text, text);
        assert!(it.tool_calls.is_empty());
    }

    #[test]
    fn fenced_tool_call_is_still_parsed() {
        let text = format!("```xml\n{CALL}\n```");
        let (_, it) = run(true, &[&text]);
        assert_single_weather_call(&it);
    }

    #[test]
    fn reasoning_is_kept_separate_from_content() {
        let mut it = StreamInterceptor::new(true);
        let (cot, txt) = it.push_chunk(Some("thinking..."), "");
        assert_eq!(cot.as_deref(), Some("thinking..."));
        assert_eq!(txt, None);
        let (cot, txt) = it.push_chunk(Some(" more"), "answer");
        assert_eq!(cot.as_deref(), Some(" more"));
        assert_eq!(txt.as_deref(), Some("answer"));
        let (cot, _) = it.push_chunk(Some(""), "!");
        assert_eq!(cot, None, "empty thought delta is not emitted");
        assert_eq!(it.full_reasoning, "thinking... more");
        assert_eq!(it.full_clean_text, "answer!");
    }

    #[test]
    fn reasoning_with_tools_disabled() {
        let mut it = StreamInterceptor::new(false);
        let (cot, txt) = it.push_chunk(Some("t"), "x");
        assert_eq!((cot.as_deref(), txt.as_deref()), (Some("t"), Some("x")));
        assert_eq!(it.full_reasoning, "t");
    }

    // ---- render_turn_prompt ----

    fn msg(role: &str, text: &str) -> NormalizedMessage {
        NormalizedMessage {
            role: role.into(),
            text: text.into(),
            images: vec![],
            tool_calls: vec![],
            tool_call_id: None,
            tool_name: None,
        }
    }

    fn tool_msg(id: &str, text: &str) -> NormalizedMessage {
        NormalizedMessage { tool_call_id: Some(id.into()), ..msg("tool", text) }
    }

    fn tools() -> Vec<Value> {
        vec![json!({"type":"function","function":{"name":"get_weather","description":"d","parameters":{"type":"object"}}})]
    }

    #[test]
    fn new_chat_single_user_is_bare_text() {
        let (p, imgs) = render_turn_prompt(&[msg("user", "hello")], None, true, false);
        assert_eq!(p, "hello");
        assert!(imgs.is_empty());
    }

    #[test]
    fn new_chat_renders_full_history_with_labels() {
        let mut asst = msg("assistant", "calling");
        asst.tool_calls.push(ParsedToolCall { id: "c1".into(), name: "f".into(), arguments_json: "{\"a\":1}".into() });
        let msgs = vec![msg("system", "be nice"), msg("user", "q1"), asst, tool_msg("c1", "res"), msg("user", "q2")];
        let (p, _) = render_turn_prompt(&msgs, None, true, false);
        assert!(p.starts_with("[SYSTEM INSTRUCTIONS]\nbe nice"));
        assert!(p.contains("[USER]\nq1"));
        assert!(p.contains("[ASSISTANT]\ncalling\n<tool_call>{\"name\":\"f\",\"arguments\":{\"a\":1}}</tool_call>"));
        assert!(p.contains("[TOOL_RESULT id=\"c1\"]\nres"));
        assert!(p.ends_with("[USER]\nq2"));
    }

    #[test]
    fn continuation_contains_only_the_newest_user_message() {
        let msgs = vec![msg("system", "sys"), msg("user", "old q"), msg("assistant", "old a"), msg("user", "new q")];
        let (p, _) = render_turn_prompt(&msgs, None, false, false);
        assert_eq!(p, "new q");
    }

    #[test]
    fn continuation_after_tool_call_sends_only_tool_result() {
        let msgs = vec![msg("user", "q"), msg("assistant", ""), tool_msg("c1", "42")];
        let (p, _) = render_turn_prompt(&msgs, None, false, false);
        assert_eq!(p, "[TOOL_RESULT id=\"c1\"]\n42");
    }

    #[test]
    fn continuation_includes_all_parallel_tool_results_after_last_assistant() {
        let msgs = vec![msg("user", "q"), msg("assistant", ""), tool_msg("c1", "r1"), tool_msg("c2", "r2")];
        let (p, _) = render_turn_prompt(&msgs, None, false, false);
        assert!(p.contains("id=\"c1\"") && p.contains("id=\"c2\""));
        assert!(!p.contains("[USER]") && !p.contains("\nq"));
    }

    #[test]
    fn images_collected_from_rendered_messages_only() {
        let mut old = msg("user", "old");
        old.images.push("data:old".into());
        let mut new = msg("user", "new");
        new.images.push("data:new".into());
        let msgs = vec![old, msg("assistant", "a"), new];
        let (_, imgs) = render_turn_prompt(&msgs, None, false, false);
        assert_eq!(imgs, vec!["data:new".to_string()]);
        let (p, imgs) = render_turn_prompt(&msgs, None, true, false);
        assert_eq!(imgs, vec!["data:new".to_string()]);
        assert!(p.contains("old\n(earlier image not re-sent)"), "{p}");
    }

    #[test]
    fn anthropic_tool_result_array_joins_text_and_takes_images() {
        let msgs = vec![json!({"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":[
            {"type":"text","text":"a"},
            {"type":"image","source":{"type":"base64","media_type":"image/png","data":"QUJD"}},
            {"type":"text","text":"b"}]}]})];
        let out = parse_anthropic_messages(None, &msgs);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].role, "tool");
        assert_eq!(out[0].text, "a\nb");
        assert_eq!(out[0].images, vec!["data:image/png;base64,QUJD".to_string()]);
    }

    #[test]
    fn url_images_are_kept_and_detected() {
        let msgs = vec![json!({"role":"user","content":[{"type":"image","source":{"type":"url","url":"https://x/y.png"}}]})];
        let out = parse_anthropic_messages(None, &msgs);
        assert_eq!(out[0].images, vec!["https://x/y.png".to_string()]);
        assert!(!is_base64_image("https://x/y.png"));
        assert!(is_base64_image("data:image/png;base64,QUJD"));
    }

    #[test]
    fn tools_prompt_opens_with_role_and_protocol_and_ends_with_rules() {
        let t = tools();
        let m = [msg("user", "hi")];
        let (p, _) = render_turn_prompt(&m, Some(&t), true, true);
        assert!(p.starts_with("[ROLE]"));
        let (role, proto, user, rules) = (p.find("[ROLE]").unwrap(), p.find("[AGENT_TOOL_PROTOCOL]").unwrap(), p.find("[USER]\nhi").unwrap(), p.find("[OUTPUT_RULES]").unwrap());
        assert!(role < proto && proto < user && user < rules, "{p}");
        assert!(p.contains("get_weather"));
        assert!(p.ends_with("[/OUTPUT_RULES]"));

        let (p, _) = render_turn_prompt(&m, None, true, true);
        assert_eq!(p, "hi", "no tools");
        let (p, _) = render_turn_prompt(&m, Some(&[]), true, true);
        assert_eq!(p, "hi", "empty tools");
    }

    #[test]
    fn continuation_with_same_tools_only_adds_the_rules() {
        let t = tools();
        let msgs = vec![msg("user", "a"), msg("assistant", "b"), msg("user", "c")];
        let (p, _) = render_turn_prompt(&msgs, Some(&t), false, false);
        assert!(p.starts_with("c\n\n[OUTPUT_RULES]"), "{p}");
        let (p, _) = render_turn_prompt(&msgs, Some(&t), false, true);
        assert!(p.starts_with("[ROLE]") && p.contains("[AGENT_TOOL_PROTOCOL]") && p.ends_with("[/OUTPUT_RULES]"));
    }

    #[test]
    fn long_tool_descriptions_are_cut() {
        let t = vec![json!({"type":"function","function":{"name":"x","description":"word ".repeat(400),"parameters":{}}})];
        let block = build_tool_protocol_block(&t);
        assert!(block.len() < 900 + 600, "{}", block.len());
        assert!(block.contains("…"));
    }

    #[test]
    fn old_tool_results_are_cut_and_recent_ones_kept() {
        let big = "x".repeat(1000);
        let mut msgs = vec![msg("user", "q")];
        for i in 0..10 {
            msgs.push(NormalizedMessage { tool_calls: vec![ParsedToolCall { id: format!("c{i}"), name: "f".into(), arguments_json: "{}".into() }], ..msg("assistant", "") });
            msgs.push(tool_msg(&format!("c{i}"), &big));
        }
        let (p, _) = render_turn_prompt(&msgs, None, true, false);
        assert_eq!(p.matches("[... tool output cut: 1000 chars ...]").count(), 2, "only the 2 oldest of 10 are cut");
        assert_eq!(prune_tool_result(&"y".repeat(30_000), true).chars().count() < 16_000, true);
    }

    #[test]
    fn turn_ending_on_tool_results_restates_the_request() {
        let t = tools();
        let msgs = vec![msg("user", "draw a cat"), msg("assistant", ""), tool_msg("c1", "ok")];
        let (p, _) = render_turn_prompt(&msgs, Some(&t), false, false);
        assert!(p.contains("[ACTIVE OBJECTIVE]\nThe user request being served: \"draw a cat\""));
        let (p, _) = render_turn_prompt(&[msg("user", "hi")], Some(&t), true, true);
        assert!(!p.contains("ACTIVE OBJECTIVE"));
    }

    #[test]
    fn active_objective_skips_the_openmausbot_context_block() {
        let t = tools();
        let note = "Context from OpenMausBot updated since this conversation started; it replaces any earlier copy:

Computers: a desktop with a terminal

draw a cat";
        let msgs = vec![msg("user", note), msg("assistant", ""), tool_msg("c1", "ok")];
        let (p, _) = render_turn_prompt(&msgs, Some(&t), false, false);
        assert!(p.contains("The user request being served: \"draw a cat\""), "{p}");
    }

    #[test]
    fn recovery_prompt_keeps_only_the_current_request_and_masks_numbers() {
        let t = tools();
        let msgs = vec![
            msg("system", "persona"),
            msg("user", "old question"),
            msg("assistant", "old answer"),
            msg("user", "pay card 4111 1111 1111 1234 to account 123-4-56789-0, call 0812345678"),
        ];
        let (p, _) = render_recovery_prompt(&msgs, Some(&t));
        assert!(p.starts_with("[ROLE]") && p.contains("[SYSTEM INSTRUCTIONS]\npersona") && p.ends_with("[/OUTPUT_RULES]"));
        assert!(!p.contains("old question") && !p.contains("old answer"));
        assert!(p.contains("[USER REQUEST]\npay card 4111-XXXX-XXXX-1234 to account 123-X-XXXXX-0, call 0812345678"), "{p}");
    }

    #[test]
    fn masking_leaves_other_numbers_alone() {
        assert_eq!(mask_financial_numbers("ts 1794652052501 id 4111111111111234x"), "ts 1794652052501 id 4111111111111234x");
        assert_eq!(mask_financial_numbers("4111111111111234"), "4111-XXXX-XXXX-1234");
    }

    fn bridged_tools() -> Vec<Value> {
        let f = |name: &str, desc: &str| json!({"type":"function","function":{"name":name,"description":desc,"parameters":{"type":"object"}}});
        vec![
            f("agents_post_to_thread", "post"),
            f("agents_find_tool", "Find an OpenMausBot tool that is not in your tool list. Tools reached this way: trigger, generate_image, publish_page, … and 12 more."),
            f("agents_use_tool", "Run one OpenMausBot tool"),
        ]
    }

    #[test]
    fn bridged_tools_get_a_hint_naming_hidden_tools_and_the_image_call() {
        let rules = output_rules(&bridged_tools());
        assert!(rules.contains("HIDDEN TOOLS: these tools are not in the schema above but you can run them: trigger, generate_image, publish_page, … and 12 more."));
        assert!(rules.contains(r#"{"name":"agents_use_tool","arguments":{"name":"generate_image","arguments":{"prompt":"#));
        assert!(rules.ends_with("[/OUTPUT_RULES]"));
        assert!(!build_tool_protocol_block(&bridged_tools()).contains("HIDDEN TOOLS"));

        let mut no_image = bridged_tools();
        no_image[1]["function"]["description"] = json!("Find. Tools reached this way: trigger.");
        let hint = hidden_tools_hint(&no_image).unwrap();
        assert!(hint.contains("run them: trigger.") && !hint.contains("generate_image"));
    }

    #[test]
    fn no_hint_without_the_bridge() {
        assert!(hidden_tools_hint(&tools()).is_none());
        let without_use = bridged_tools()[..2].to_vec();
        assert!(hidden_tools_hint(&without_use).is_none());
        assert!(!output_rules(&tools()).contains("HIDDEN TOOLS"));
    }

    #[test]
    fn other_tag_spellings_and_function_wrapper_are_parsed() {
        let (_, it) = run(true, &["<<<TOOL_CALL>>>{\"name\":\"get_weather\",\"arguments\":{}}<<<END_TOOL_CALL>>>"]);
        assert_eq!(it.tool_calls.len(), 1);
        assert!(!it.full_clean_text.contains("TOOL_CALL"));
        let (_, it) = run(true, &["<tool_call>{\"function\":{\"name\":\"f\",\"arguments\":\"{\\\"a\\\":1}\"}}</tool_call>"]);
        assert_eq!(it.tool_calls[0].name, "f");
        assert_eq!(it.tool_calls[0].arguments_json, "{\"a\":1}");
    }

    #[test]
    fn anthropic_tool_schema_uses_input_schema() {
        let t = vec![json!({"name":"calc","description":"d","input_schema":{"type":"object"}})];
        let block = build_tool_protocol_block(&t);
        assert!(block.contains("\"name\":\"calc\"") && block.contains("\"parameters\":{\"type\":\"object\"}"));
    }

    // ---- message parsers ----

    #[test]
    fn openai_parser_handles_parts_tool_calls_and_tool_role() {
        let raw = vec![
            json!({"role":"user","content":[{"type":"text","text":"a"},{"type":"image_url","image_url":{"url":"data:x"}},{"type":"text","text":"b"}]}),
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"c1","function":{"name":"f","arguments":"{\"k\":1}"}},{"id":"c2","function":{"name":"","arguments":"{}"}}]}),
            json!({"role":"tool","tool_call_id":"c1","content":"ok"}),
        ];
        let m = parse_openai_messages(&raw);
        assert_eq!(m.len(), 3);
        assert_eq!(m[0].text, "a\nb");
        assert_eq!(m[0].images, vec!["data:x"]);
        assert_eq!(m[1].tool_calls.len(), 1, "nameless tool call dropped");
        assert_eq!(m[1].tool_calls[0].arguments_json, "{\"k\":1}");
        assert_eq!(m[2].tool_call_id.as_deref(), Some("c1"));
    }

    #[test]
    fn anthropic_parser_splits_tool_results_and_system() {
        let sys = json!([{"type":"text","text":"s1"},{"type":"text","text":"s2"}]);
        let raw = vec![
            json!({"role":"user","content":"hi"}),
            json!({"role":"assistant","content":[{"type":"text","text":"t"},{"type":"tool_use","id":"tu1","name":"f","input":{"a":1}}]}),
            json!({"role":"user","content":[{"type":"tool_result","tool_use_id":"tu1","content":"res"}]}),
        ];
        let m = parse_anthropic_messages(Some(&sys), &raw);
        let roles: Vec<&str> = m.iter().map(|x| x.role.as_str()).collect();
        assert_eq!(roles, ["system", "user", "assistant", "tool"]);
        assert_eq!(m[0].text, "s1\ns2");
        assert_eq!(m[2].tool_calls[0].name, "f");
        assert_eq!(m[3].tool_call_id.as_deref(), Some("tu1"));
        assert_eq!(m[3].text, "res");
    }
}
