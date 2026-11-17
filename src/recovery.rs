//! Spotting a turn worth one retry: Gemini refusing at the start of its answer, or a policy block.
//! The retry itself (fresh chat, shortened prompt from `stream::render_recovery_prompt`) runs in the server.

/// Answer text held back before it reaches the client, so a refusal can still be replaced.
/// Agent turns get a longer window: Gemini's "I am a text assistant" preamble can start with a heading.
pub const GATE_CHARS_TOOLS: usize = 300;
pub const GATE_CHARS_CHAT: usize = 120;

/// Policy refusals, in any turn.
const POLICY_REFUSALS: &[&str] = &[
    "ขออภัย ไม่สามารถตอบคำถามนี้ได้",
    "ขออภัยครับ ผมไม่สามารถ",
    "ขออภัยค่ะ ดิฉันไม่สามารถ",
    "sorry, i could not respond to this request",
    "i can't help with that",
    "i cannot help with that",
    "i'm not able to help with that",
];

/// "I am only a chat model" refusals: wrong only when the agent gave tools that can do the job.
const CAPABILITY_REFUSALS: &[&str] = &[
    "ไม่สามารถเรนเดอร์",
    "ไม่สามารถสร้างภาพ",
    "ไม่สามารถสร้างรูป",
    "ไม่สามารถวาด",
    "ให้บริการในรูปแบบข้อความ",
    "ไม่สามารถเข้าถึงคอมพิวเตอร์",
    "ไม่สามารถเข้าถึงไฟล์",
    "i can't generate images",
    "i cannot generate images",
    "i can't create images",
    "i cannot create images",
    "i'm unable to generate images",
    "i am unable to generate images",
    "as a text-based",
    "i'm a text-based",
    "i am a text-based",
    "i don't have the ability to",
    "i do not have the ability to",
    "i can't access your computer",
    "i cannot access your computer",
    "i can't run commands",
    "i cannot run commands",
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Refusal {
    /// A content-policy refusal.
    Policy,
    /// "I am only a chat model" about images: an image tool can do it.
    Image,
    /// "I am only a chat model" about anything else.
    Capability,
    /// The user asked for a picture and the answer started without calling the image tool (Gemini
    /// often writes a design brief and a prompt instead).
    SkippedImageTool,
}

impl Refusal {
    pub fn label(self) -> &'static str {
        match self {
            Refusal::Policy => "refused (policy)",
            Refusal::Image => "said it cannot make images",
            Refusal::Capability => "said it cannot use tools",
            Refusal::SkippedImageTool => "answered a picture request without calling the image tool",
        }
    }
}

const NEGATIONS: &[&str] = &[
    "ไม่สามารถ", "ไม่รองรับ", "ทำไม่ได้",
    "can't", "cannot", "can not", "unable to", "not able to", "don't have the ability", "do not have the ability", "not supported",
];
/// Thai words that contain ภาพ/รูป without meaning a picture ("format", "overview", ...).
const NOT_PICTURE_WORDS: &[&str] = &["รูปแบบ", "ภาพรวม", "รูปธรรม", "สภาพ", "ภาพลักษณ์"];
const PICTURE_WORDS: &[&str] = &["ภาพ", "รูป", "วาด", "image", "picture", "photo", "draw", "illustration"];

/// Whether `text` asks for, or talks about, a picture.
pub fn mentions_picture(text: &str) -> bool {
    let mut t = text.to_lowercase();
    for w in NOT_PICTURE_WORDS {
        t = t.replace(w, " ");
    }
    PICTURE_WORDS.iter().any(|w| t.contains(w))
}

/// Picture-making words for `asks_for_new_picture`, beyond `PICTURE_WORDS`.
const MADE_THINGS: &[&str] = &["สติกเกอร์", "sticker", "โลโก้", "logo", "อวาตาร์", "avatar", "ไอคอน", "icon", "โปสเตอร์", "poster", "wallpaper", "วอลเปเปอร์"];
const MAKE_VERBS: &[&str] = &[
    "สร้าง", "วาด", "ทำ", "ขอ", "เจน", "ออกแบบ",
    "generate", "create", "make", "draw", "render", "paint", "design", "produce", "give me", "show me",
];
/// Thai words that contain a make-verb without meaning "make" ("why", "work", "do it again" is fine).
const NOT_MAKE_WORDS: &[&str] = &["ทำไม", "ทำงาน", "ขอบคุณ", "ขออภัย", "ขอโทษ"];

/// Whether the user's request asks for a new picture to be made ("สร้างภาพสติกเกอร์…", "draw a cat").
/// Questions about pictures without a make-verb ("รูปนี้คืออะไร") do not count.
pub fn asks_for_new_picture(request: &str) -> bool {
    let mut t = request.to_lowercase();
    for w in NOT_PICTURE_WORDS.iter().chain(NOT_MAKE_WORDS) {
        t = t.replace(w, " ");
    }
    let thing = PICTURE_WORDS.iter().chain(MADE_THINGS).any(|w| t.contains(w));
    thing && MAKE_VERBS.iter().any(|v| t.contains(v))
}

/// Which refusal `text` (the start of an answer) is, if any. A text that already holds a tool call is
/// never a refusal.
pub fn refusal_kind(text: &str, has_tools: bool) -> Option<Refusal> {
    let lower = text.to_lowercase();
    if lower.contains("<tool_call") || lower.contains("tool_call>") {
        return None;
    }
    if POLICY_REFUSALS.iter().any(|p| lower.contains(p)) {
        return Some(Refusal::Policy);
    }
    if !has_tools {
        return None;
    }
    let negated = NEGATIONS.iter().any(|n| lower.contains(n));
    if negated && mentions_picture(&lower) {
        return Some(Refusal::Image);
    }
    if CAPABILITY_REFUSALS.iter().any(|p| lower.contains(p)) {
        return Some(Refusal::Capability);
    }
    None
}

/// Notes OpenMausBot writes into user messages that are not the user's words: the context block it puts
/// in front of a message (`withContextNote` in its prompt-split driver) and its automatic retry notes.
const OMB_CONTEXT_PREFIXES: &[&str] = &[
    "Context from OpenMausBot updated since this conversation started",
    "The OpenMausBot context notes from earlier in this conversation have been cleared",
];
const OMB_META_PREFIX: &str = "[From OpenMausBot";
const OMB_TIME_LINE: &str = "Current time when this message was sent:";

/// The user's own words in a user message: an OpenMausBot context block in front is dropped (the block's
/// end is unknown, so the request is taken to be the last paragraph). The second value is the full
/// message when a block was dropped, for callers that want to show it with a warning.
pub fn user_request_text(text: &str) -> (&str, Option<&str>) {
    let full = text.trim();
    // Passages OpenMausBot recalls from earlier chats sit in front of the message; they are not the request.
    let t = after_recalled_passages(full).trim();
    if t.len() != full.len() {
        let (req, _) = user_request_text(t);
        return (req, Some(full));
    }
    // OpenMausBot's ACP prompts carry the bot persona and instructions in front of the user's words, ending with
    // this line; a persona that talks about images must not make "สร้างไฟล์ hello.txt" a picture request.
    if let Some(at) = t.rfind(OMB_TIME_LINE) {
        let after = t[at..].find('\n').map(|i| t[at + i..].trim()).unwrap_or("");
        if !after.is_empty() {
            return (after, Some(t));
        }
    }
    if !OMB_CONTEXT_PREFIXES.iter().any(|p| t.starts_with(p)) {
        return (t, None);
    }
    let last = t.rsplit("\n\n").map(str::trim).find(|p| !p.is_empty()).unwrap_or("");
    let is_prefix_only = OMB_CONTEXT_PREFIXES.iter().any(|p| last.starts_with(p));
    (if is_prefix_only { "" } else { last }, Some(t))
}

const OMB_RECALL_END: &str = "[end of recalled passages";

/// The part of an OpenMausBot message after its recalled-passages block (the whole text when it has none).
pub fn after_recalled_passages(text: &str) -> &str {
    match text.rfind(OMB_RECALL_END) {
        Some(at) => text[at..].find('\n').map_or("", |i| &text[at + i..]),
        None => text,
    }
}

fn is_omb_meta(text: &str) -> bool {
    text.trim_start().starts_with(OMB_META_PREFIX)
}

/// "16:9" or "9:16" only when the request names an orientation; "portrait lens", "wide-angle" and
/// "mountain landscape" are about the subject, not the frame.
pub fn aspect_for_request(text: &str) -> &'static str {
    let t = text.to_lowercase();
    const WIDE: &[&str] = &[
        "16:9", "แนวนอน", "widescreen", "wide screen", "banner",
        "landscape orientation", "landscape format", "landscape mode", "landscape aspect", "horizontal image", "horizontal picture",
    ];
    const TALL: &[&str] = &[
        "9:16", "แนวตั้ง", "สตอรี่", "vertical image", "vertical picture", "vertical format", "vertical orientation",
        "portrait orientation", "portrait format", "portrait mode", "portrait aspect", "phone wallpaper", "instagram story", "tiktok",
    ];
    if WIDE.iter().any(|w| t.contains(w)) {
        "16:9"
    } else if TALL.iter().any(|w| t.contains(w)) {
        "9:16"
    } else {
        "1:1"
    }
}

/// Earlier conversation given to the prompt writer.
const WRITER_CONTEXT_MAX_CHARS: usize = 3_000;
const WRITER_REQUEST_MAX_CHARS: usize = 4_000;

/// A plain text task Gemini does not refuse: turn the user's picture request into one English image
/// prompt. `turns` is (role, text), oldest first; the last user turn is the request.
pub fn prompt_writer_prompt(turns: &[(&str, &str)]) -> String {
    let convo: Vec<(&str, &str)> = turns
        .iter()
        .filter(|(r, t)| (*r == "user" || *r == "assistant") && !t.trim().is_empty() && !is_omb_meta(t))
        .copied()
        .collect();
    let last_user = convo.iter().rposition(|(r, _)| *r == "user");
    let (request, full) = last_user.map_or(("", None), |i| user_request_text(convo[i].1));

    let mut earlier: Vec<String> = Vec::new();
    let mut used = 0;
    for (role, text) in convo[..last_user.unwrap_or(convo.len())].iter().rev().take(6) {
        let own = if *role == "user" { user_request_text(text).0 } else { text.trim() };
        if own.is_empty() {
            continue;
        }
        let piece: String = own.chars().take(1_200).collect();
        used += piece.chars().count();
        if used > WRITER_CONTEXT_MAX_CHARS && !earlier.is_empty() {
            break;
        }
        earlier.push(format!("{}: {piece}", if *role == "user" { "User" } else { "Assistant" }));
    }
    earlier.reverse();

    let mut out = String::from(
        "Write one prompt for an AI image generator, in English, for the picture in the user's request below. \
         Describe the subject, setting, style, lighting and composition concretely, in under 120 words. \
         If the request already is a detailed English image prompt, output it unchanged, without leading words such as \"generate\" or \"create\". \
         Use the earlier conversation only to understand what the request refers to. \
         Output only the prompt text: no title, no quotes, no explanation, no options.\n\n",
    );
    out.push_str("Request:\n");
    out.push_str(&request.chars().take(WRITER_REQUEST_MAX_CHARS).collect::<String>());
    if full.is_some() && request.is_empty() {
        out.push_str("(the user's message held only an OpenMausBot context block)");
    }
    if !earlier.is_empty() {
        out.push_str("\n\nEarlier conversation:\n");
        out.push_str(&earlier.join("\n"));
    }
    out
}

/// The prompt writer's answer as a bare prompt: no fences, quotes, labels or extra paragraphs.
pub fn clean_image_prompt(text: &str) -> Option<String> {
    let t = text.trim().trim_start_matches("```text").trim_start_matches("```").trim_end_matches("```").trim();
    let first = t.split("

").map(str::trim).find(|p| !p.is_empty())?;
    let first = first.split_once(':').filter(|(label, _)| label.to_lowercase().contains("prompt") && label.len() < 30).map_or(first, |(_, rest)| rest.trim());
    let cleaned = first.trim_matches(|c| c == '"' || c == '“' || c == '”' || c == '*' || c == '`').trim();
    (!cleaned.is_empty()).then(|| cleaned.chars().take(1_500).collect())
}

/// Errors that a fresh chat with a shorter prompt can get past: content-policy and bad-request blocks.
/// Auth, rate limits, timeouts and missing tabs are not retried here.
pub fn is_recoverable_error(err: &str) -> bool {
    let e = err.to_lowercase();
    const NOT: &[&str] = &["\"code\": 401", "\"code\": 403", "\"code\": 429", "http 401", "http 403", "http 429", "timed out", "timeout", "no gemini tab", "extension",
        // A malformed request from ge-bridge itself: a retry would fail the same way.
        "unknown name", "invalid json payload", "cannot find field"];
    if NOT.iter().any(|p| e.contains(p)) {
        return false;
    }
    const YES: &[&str] = &[
        "chat_rai",
        "rai_violation",
        "responsible ai",
        "content policy",
        "policy violation",
        "safety",
        "blocked",
        "\"code\": 400",
        "\"code\": 422",
        "http 400",
        "http 422",
        "invalid_argument",
    ];
    YES.iter().any(|p| e.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_refusal_seen_in_openmausbot_is_caught_only_when_tools_exist() {
        let seen = "## 🖼️ ขีดความสามารถในการสร้างภาพของ Gemini Enterprise\n\nGemini Enterprise เป็นระบบสนทนาอัจฉริยะที่ให้บริการในรูปแบบข้อความ จึงไม่สามารถเรนเดอร์";
        assert_eq!(refusal_kind(seen, true), Some(Refusal::Image));
        assert_eq!(refusal_kind(seen, false), None);
        assert_eq!(refusal_kind("I can't generate images, but here is a prompt", true), Some(Refusal::Image));
        // Seen live after the prompt rewrite: "files" between the negation and "picture".
        let live = "ขณะนี้ Gemini Enterprise ยังไม่สามารถสร้างไฟล์รูปภาพออกมาโดยตรงได้ เนื่องจากระบบยังไม่รองรับเครื่องมือสร้างภาพ";
        assert_eq!(refusal_kind(live, true), Some(Refusal::Image));
        assert_eq!(refusal_kind("I can't run commands on your machine", true), Some(Refusal::Capability));
        assert_eq!(refusal_kind("ไม่สามารถเปิดไฟล์ในรูปแบบนี้ได้", true), None, "รูปแบบ is format, not picture");
    }

    #[test]
    fn policy_refusals_count_in_any_turn_and_tool_calls_never_count() {
        assert_eq!(refusal_kind("ขออภัย ไม่สามารถตอบคำถามนี้ได้", false), Some(Refusal::Policy));
        assert_eq!(refusal_kind("Sorry, I could not respond to this request.", false), Some(Refusal::Policy));
        assert_eq!(refusal_kind("ไม่สามารถสร้างภาพได้ <tool_call>{\"name\":\"x\"}", true), None);
        assert_eq!(refusal_kind("Here is the summary you asked for.", true), None);
    }

    #[test]
    fn only_policy_and_bad_request_errors_are_retried() {
        assert!(is_recoverable_error("{\n  \"error\": {\n    \"code\": 400,\n    \"message\": \"Request contains an invalid argument.\""));
        assert!(is_recoverable_error("Gemini answered with CHAT_RAI_VIOLATION"));
        assert!(!is_recoverable_error("{\n  \"error\": {\n    \"code\": 401,\n    \"message\": \"Request had invalid authentication credentials.\""));
        assert!(!is_recoverable_error("Turn timed out after 300 s"));
        assert!(!is_recoverable_error("No Gemini tab is open"));
        assert!(!is_recoverable_error("{\"error\": {\"code\": 400, \"message\": \"Invalid JSON payload received. Unknown name \\\"x\\\": Cannot find field.\"}}"));
    }

    #[test]
    fn picture_requests_aspect_and_prompt_writing() {
        assert!(mentions_picture("ลองทำรูปออกมา"));
        assert!(!mentions_picture("สรุปภาพรวมของโปรเจกต์ในรูปแบบตาราง"));
        assert_eq!(aspect_for_request("ทำภาพแนวนอนให้หน่อย"), "16:9");
        assert_eq!(aspect_for_request("a poster in portrait orientation"), "9:16");
        assert_eq!(aspect_for_request("draw a cat"), "1:1");

        let p = prompt_writer_prompt(&[("system", "persona"), ("user", "สร้างรูปแมวส้ม"), ("assistant", "ทำไม่ได้"), ("user", "ลองทำรูปออกมา")]);
        assert!(p.contains("Request:
ลองทำรูปออกมา

Earlier conversation:
User: สร้างรูปแมวส้ม
Assistant: ทำไม่ได้"), "{p}");
        assert!(!p.contains("persona"));
        let retry = prompt_writer_prompt(&[("user", "draw a fox"), ("user", "[From OpenMausBot · retry · information only]")]);
        assert!(retry.contains("Request:
draw a fox"), "OpenMausBot retry notes are not the request: {retry}");

        assert_eq!(clean_image_prompt("Prompt: \"An orange cat asleep on a sofa\"

Want more?").as_deref(), Some("An orange cat asleep on a sofa"));
        assert_eq!(clean_image_prompt("```
A red apple
```").as_deref(), Some("A red apple"));
        assert_eq!(clean_image_prompt("  "), None);
    }

    const OMB_NOTE: &str = "Context from OpenMausBot updated since this conversation started; it replaces any earlier copy:

Computers: this Windows desktop with a web browser and a file terminal.
Time: 17:50

";
    const IDOL: &str = "generate the Ultra-realistic photo of a cute petite Japanese female idol, sweet smile, soft cinematic studio lighting, shot on 85mm portrait lens, 8k resolution";

    #[test]
    fn repro_writer_ignores_the_openmausbot_context_note() {
        let msg = format!("{OMB_NOTE}{IDOL}");
        let p = prompt_writer_prompt(&[("system", "persona"), ("user", &msg)]);
        assert!(p.contains(IDOL), "{p}");
        assert!(!p.contains("web browser") && !p.contains("file terminal"), "context note leaked into the writer prompt:
{p}");
    }

    #[test]
    fn user_request_text_drops_recalled_passages() {
        let t = "[1] chat \"x\": but now some characters shows as unreadable\n[end of recalled passages — the message follows]\n\nwhat is in this image?";
        assert_eq!(user_request_text(t), ("what is in this image?", Some(t)));
        assert_eq!(after_recalled_passages("no marker"), "no marker");
    }

    #[test]
    fn user_request_text_drops_the_context_block() {
        assert_eq!(user_request_text(&format!("{OMB_NOTE}{IDOL}")), (IDOL, Some(format!("{OMB_NOTE}{IDOL}").trim())));
        assert_eq!(user_request_text("  just this  "), ("just this", None));
        // Known limit: a message that is only a context block (no user text) cannot be told apart from
        // block + one paragraph, so its last paragraph comes back. OpenMausBot sends that only for
        // attachment-only messages.
        assert!(user_request_text(OMB_NOTE).0.ends_with("Time: 17:50"));
    }

    #[test]
    fn repro_portrait_lens_is_not_a_tall_picture() {
        assert_eq!(aspect_for_request(IDOL), "1:1");
        assert_eq!(aspect_for_request("a wide-angle shot of a mountain landscape"), "1:1");
    }

    #[test]
    fn picture_requests_are_told_apart_from_talk_about_pictures() {
        // The request from the failing OpenMausBot turn (typo included).
        assert!(asks_for_new_picture("สร้างภาพสติกเกอร์โดยตัวละครเป็ฯสาวญี่ปุ่นผิวขาว ตัวเล็กน่ารัก"));
        assert!(asks_for_new_picture("สร้างภาพตัวอยา่งภาพเดียว"));
        assert!(asks_for_new_picture("วาดแมวส้มให้หน่อย"));
        assert!(asks_for_new_picture("Generate the ultra-realistic photo of an idol"));
        assert!(asks_for_new_picture("make me a logo for my cafe"));
        assert!(!asks_for_new_picture("ทำไมรูปภาพไม่ขึ้น"), "why-question");
        assert!(!asks_for_new_picture("รูปนี้คืออะไร"), "no make-verb");
        assert!(!asks_for_new_picture("สร้างภาพรวมของโปรเจกต์"), "overview, not a picture");
        assert!(!asks_for_new_picture("ขอบคุณสำหรับรูป"), "thanks");
        assert!(!asks_for_new_picture("summarize this document"));
        // 2026-11-16: an ACP prompt whose persona talks about sticker images, asking for a text file.
        let acp = "You are Artist, a personal bot in OpenMausBot. About: Generates AI sticker images.\n\n\
                   Current time when this message was sent: Monday, 2026-11-16 15:26 Asia/Bangkok.\n\nสร้างไฟล์ hello.txt";
        assert!(asks_for_new_picture(acp), "the whole text looks like a picture request");
        assert_eq!(user_request_text(acp).0, "สร้างไฟล์ hello.txt");
        assert!(!asks_for_new_picture(user_request_text(acp).0), "the user's own words do not");
    }
}
