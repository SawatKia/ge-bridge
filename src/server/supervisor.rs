use crate::state::{hash_json, now_secs, AppState, DomStreamEvent, SessionEntry};
use crate::stream::{render_turn_prompt, NormalizedMessage, ParsedToolCall};
use rand::RngCore;
use serde_json::{json, Value};
use std::sync::{atomic::{AtomicI64, Ordering}, Arc};
use tokio::sync::mpsc;

pub(super) struct PreparedJob {
    pub(super) job_id: String,
    /// The tab serving the turn; a recovery retry may move it to another tab.
    pub(super) tab_id: Arc<AtomicI64>,
    pub(super) target_model: String,
    pub(super) rx: mpsc::UnboundedReceiver<DomStreamEvent>,
    pub(super) messages: Vec<NormalizedMessage>,
    pub(super) tools_hash: Option<String>,
    pub(super) has_tools: bool,
}

/// Sends one `RUN_TURN` to the extension. Returns (job id, tab id, event stream).
#[allow(clippy::too_many_arguments)]
async fn send_job(
    state: &Arc<AppState>,
    prompt: String,
    images: Vec<String>,
    new_chat: bool,
    target_model: &str,
    switch_model: bool,
    target_effort: Option<&str>,
    preferred_tab: Option<i64>,
) -> Result<(String, i64, mpsc::UnboundedReceiver<DomStreamEvent>), String> {
    let tab_id = state.acquire_tab(preferred_tab).await?;
    let mut id_bytes = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut id_bytes);
    let job_id = format!("job_{}", hex::encode(id_bytes));
    let (tx, rx) = mpsc::unbounded_channel();
    state.pending_jobs.lock().insert(job_id.clone(), tx);

    let cmd = json!({
        "type": "RUN_TURN",
        "jobId": job_id,
        "tabId": tab_id,
        "prompt": prompt,
        "images": images,
        "newChat": new_chat,
        "targetModel": if switch_model || new_chat { Some(target_model) } else { None },
        "reasoningEffort": target_effort
    });
    if let Err(err) = state.send_to_extension(&cmd) {
        state.pending_jobs.lock().remove(&job_id);
        state.release_tab(tab_id, None);
        return Err(err);
    }
    state.log("info", format!("Dispatched {job_id} -> tab {tab_id} [model={target_model}, new_chat={new_chat}]"));
    Ok((job_id, tab_id, rx))
}

pub(super) async fn dispatch_browser_turn(
    state: &Arc<AppState>,
    messages: Vec<NormalizedMessage>,
    tools: Option<Vec<Value>>,
    req_model: Option<&str>,
    req_effort: Option<&str>,
) -> Result<PreparedJob, String> {
    let (target_model, target_effort) = state.resolve_model_and_effort(req_model, req_effort)?;
    let prefix_hash = if messages.len() > 1 {
        Some(hash_json(&messages[..messages.len() - 1]))
    } else {
        None
    };
    let tools_hash = tools.as_ref().filter(|t| !t.is_empty()).map(hash_json);

    let matched = prefix_hash.as_ref().and_then(|ph| {
        state
            .sessions
            .lock()
            .iter()
            .find(|s| &s.last_full_hash == ph)
            .cloned()
    });

    let (new_chat, preferred_tab, inject_tools, switch_model) = match &matched {
        Some(sess) if sess.active_model == target_model => (
            false,
            sess.preferred_tab_id,
            tools_hash != sess.tools_hash,
            false,
        ),
        Some(sess) => (
            false, // Switch model picker in-place on the existing tab when supported
            sess.preferred_tab_id,
            tools_hash != sess.tools_hash,
            true,
        ),
        None => (true, None, tools_hash.is_some(), true),
    };

    let (prompt, images) = render_turn_prompt(&messages, tools.as_deref(), new_chat, inject_tools);
    let prompt_chars = prompt.chars().count();
    let (job_id, tab_id, inner_rx) = send_job(
        state,
        prompt,
        images,
        new_chat,
        &target_model,
        switch_model,
        target_effort.as_deref(),
        preferred_tab,
    )
    .await?;

    if let Some(t) = tools.as_deref().filter(|t| !t.is_empty()) {
        let hidden = if crate::stream::hidden_tools_hint(t).is_some() { ", hidden-tools hint added" } else { "" };
        state.log(
            "debug",
            format!("{job_id}: {} tools, injected={inject_tools}{hidden}, prompt {prompt_chars} chars", t.len()),
        );
    }

    let has_tools = tools_hash.is_some();
    let tab = Arc::new(AtomicI64::new(tab_id));
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let plan = RetryPlan { messages: messages.clone(), tools, target_model: target_model.clone(), effort: target_effort };
    tokio::spawn(supervise_turn(state.clone(), plan, job_id.clone(), inner_rx, tab.clone(), out_tx, has_tools));

    Ok(PreparedJob {
        job_id,
        tab_id: tab,
        target_model,
        rx: out_rx,
        messages,
        has_tools,
        tools_hash,
    })
}

/// What a recovery retry needs to rebuild the turn.
struct RetryPlan {
    messages: Vec<NormalizedMessage>,
    tools: Option<Vec<Value>>,
    target_model: String,
    effort: Option<String>,
}

/// Sits between the extension and the request handler for one turn:
/// - holds back the first `GATE_CHARS_*` of answer text; if that start is a refusal (or the turn fails with
///   a policy/bad-request error), it retries once in a new Gemini chat with `render_recovery_prompt`;
/// - forwards everything else as it arrives (thoughts are never held);
/// - releases the tab when the turn ends and logs time to first thought/text.
async fn supervise_turn(
    state: Arc<AppState>,
    plan: RetryPlan,
    mut job_id: String,
    mut rx: mpsc::UnboundedReceiver<DomStreamEvent>,
    tab: Arc<AtomicI64>,
    out: mpsc::UnboundedSender<DomStreamEvent>,
    has_tools: bool,
) {
    use crate::recovery::{is_recoverable_error, refusal_kind, Refusal, GATE_CHARS_CHAT, GATE_CHARS_TOOLS};
    let gate = if has_tools { GATE_CHARS_TOOLS } else { GATE_CHARS_CHAT };
    // The user just asked for a picture and the agent can make one: an answer that starts without a
    // tool call is a brief or a prompt instead of the picture. Only on the user's own turn, never after
    // tool results (that would make another picture after every image).
    let picture_turn = plan.messages.last().is_some_and(|m| m.role == "user")
        && crate::recovery::asks_for_new_picture(crate::recovery::user_request_text(&plan.messages.last().unwrap().text).0)
        && plan.tools.as_deref().and_then(crate::stream::image_tool).is_some();
    // The turn that carries an image tool's result: save that image into the bot's folder and show it.
    let image_prompt = crate::bot_images::image_call_prompt(&plan.messages);
    let judge = |text: &str| -> Option<Refusal> {
        refusal_kind(text, has_tools).or_else(|| {
            let has_call = text.contains("<tool") || text.contains("tool_call") || text.contains("TOOL_CALL");
            (picture_turn && !has_call).then_some(Refusal::SkippedImageTool)
        })
    };
    let mut retried = false;
    let mut gating = true;
    let mut held = String::new();
    let mut started = std::time::Instant::now();
    let (mut first_thought_ms, mut first_text_ms) = (None::<u128>, None::<u128>);

    loop {
        // Some(refusal), or None for a policy/bad-request block.
        let trigger: Option<Refusal> = match rx.recv().await {
            None => {
                state.release_tab(tab.load(Ordering::Relaxed), None);
                let _ = out.send(DomStreamEvent::Error("The extension ended the turn without an answer.".into()));
                return;
            }
            Some(DomStreamEvent::Chunk { thought_delta, text_delta }) => {
                if let Some(t) = thought_delta.filter(|t| !t.is_empty()) {
                    first_thought_ms.get_or_insert_with(|| started.elapsed().as_millis());
                    let _ = out.send(DomStreamEvent::Chunk { thought_delta: Some(t), text_delta: String::new() });
                }
                if text_delta.is_empty() {
                    continue;
                }
                first_text_ms.get_or_insert_with(|| started.elapsed().as_millis());
                if !gating {
                    let _ = out.send(DomStreamEvent::Chunk { thought_delta: None, text_delta });
                    continue;
                }
                held.push_str(&text_delta);
                if held.chars().count() < gate {
                    continue;
                }
                match judge(&held).filter(|_| !retried) {
                    Some(kind) => Some(kind),
                    None => {
                        gating = false;
                        let _ = out.send(DomStreamEvent::Chunk { thought_delta: None, text_delta: std::mem::take(&mut held) });
                        continue;
                    }
                }
            }
            Some(DomStreamEvent::Done { full_text, active_model }) => {
                let answer = if full_text.is_empty() { held.clone() } else { full_text.clone() };
                match judge(&answer).filter(|_| gating && !retried) {
                    Some(kind) => Some(kind),
                    None => {
                        if gating && !held.is_empty() {
                            let _ = out.send(DomStreamEvent::Chunk { thought_delta: None, text_delta: std::mem::take(&mut held) });
                        }
                        state.log(
                            "debug",
                            format!(
                                "{job_id}: first thought {} ms, first text {} ms, done {} ms{}",
                                first_thought_ms.map_or("-".into(), |v| v.to_string()),
                                first_text_ms.map_or("-".into(), |v| v.to_string()),
                                started.elapsed().as_millis(),
                                if retried { " (after recovery retry)" } else { "" }
                            ),
                        );
                        state.release_tab(tab.load(Ordering::Relaxed), active_model.clone());
                        let mut full_text = full_text;
                        if let Some(note) = image_prompt.as_deref().and_then(|p| save_image_for_bot(&state, &plan.messages, p, &job_id)) {
                            let _ = out.send(DomStreamEvent::Chunk { thought_delta: None, text_delta: note.clone() });
                            full_text.push_str(&note);
                        }
                        let _ = out.send(DomStreamEvent::Done { full_text, active_model });
                        return;
                    }
                }
            }
            Some(DomStreamEvent::Error(err)) => {
                if gating && !retried && is_recoverable_error(&err) {
                    None
                } else {
                    state.release_tab(tab.load(Ordering::Relaxed), None);
                    let msg = if retried && is_recoverable_error(&err) {
                        format!("Gemini blocked this request again after ge-bridge retried it once in a new chat with a shorter prompt. Rephrase it or start a new conversation. ({err})")
                    } else {
                        err
                    };
                    let _ = out.send(DomStreamEvent::Error(msg));
                    return;
                }
            }
        };

        let reason = trigger.map_or("was blocked", Refusal::label);
        state.release_tab(tab.load(Ordering::Relaxed), None);

        // Image rescue: Gemini Enterprise insists it cannot draw, but the agent has an image tool. Have
        // Gemini write only the image prompt (a text task it does) and answer with the tool call ourselves.
        // Only on the user's own turn without a picture of theirs: after a tool result it would make another
        // picture after every image, and "I can't see the image" about an attached one is not a drawing refusal.
        let user_turn = plan.messages.last().is_some_and(|m| m.role == "user" && m.images.is_empty() && !crate::recovery::after_recalled_passages(&m.text).contains("<attached-image"));
        if matches!(trigger, Some(Refusal::Image | Refusal::SkippedImageTool)) && !retried && user_turn {
            if let Some(tool) = plan.tools.as_deref().and_then(crate::stream::image_tool) {
                state.log("warn", format!("{job_id}: Gemini {reason}; asking it for an image prompt and calling the agent's image tool instead"));
                match rescue_image(&state, &plan, &tool).await {
                    Ok(call) => {
                        state.log("info", format!("{job_id}: image rescue produced a tool call ({} chars)", call.len()));
                        let _ = out.send(DomStreamEvent::Chunk { thought_delta: None, text_delta: call.clone() });
                        let _ = out.send(DomStreamEvent::Done { full_text: call, active_model: None });
                        return;
                    }
                    Err(e) => state.log("warn", format!("{job_id}: image rescue failed ({e}); falling back to a normal retry")),
                }
            }
        }

        // Recovery retry: one per turn.
        retried = true;
        gating = true;
        held.clear();
        state.log("warn", format!("{job_id}: Gemini {reason}; retrying once in a new chat with a shortened prompt"));
        let (prompt, images) = crate::stream::render_recovery_prompt(&plan.messages, plan.tools.as_deref());
        match send_job(&state, prompt, images, true, &plan.target_model, true, plan.effort.as_deref(), None).await {
            Ok((new_id, new_tab, new_rx)) => {
                state.log("info", format!("{job_id}: recovery retry is {new_id}"));
                job_id = new_id;
                tab.store(new_tab, Ordering::Relaxed);
                rx = new_rx;
                started = std::time::Instant::now();
                first_thought_ms = None;
                first_text_ms = None;
            }
            Err(e) => {
                let _ = out.send(DomStreamEvent::Error(e));
                return;
            }
        }
    }
}

/// Saves the image ge-bridge made for `prompt` into the asking bot's working folder (OpenMausBot's
/// `bots.json`), or into Pictures\GeBridge when the bot is unknown, and returns the text to add to the
/// reply (preview and link). None when no matching image is waiting.
fn save_image_for_bot(state: &AppState, messages: &[NormalizedMessage], prompt: &str, job_id: &str) -> Option<String> {
    use crate::bot_images::{folder_for, pick_image, reply_note, save};
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

/// One plain-text Gemini turn in a new chat that writes an image prompt for the conversation; returns the
/// agent's image tool call built from it.
async fn rescue_image(state: &Arc<AppState>, plan: &RetryPlan, tool: &crate::stream::ImageTool) -> Result<String, String> {
    use crate::recovery::{aspect_for_request, clean_image_prompt, prompt_writer_prompt, refusal_kind};
    let turns: Vec<(&str, &str)> = plan.messages.iter().map(|m| (m.role.as_str(), m.text.as_str())).collect();
    let (job_id, tab_id, mut rx) =
        send_job(state, prompt_writer_prompt(&turns), vec![], true, &plan.target_model, true, plan.effort.as_deref(), None).await?;
    let mut text = String::new();
    let mut model = None;
    let outcome = loop {
        match rx.recv().await {
            Some(DomStreamEvent::Chunk { text_delta, .. }) => text.push_str(&text_delta),
            Some(DomStreamEvent::Done { full_text, active_model }) => {
                model = active_model;
                if !full_text.is_empty() {
                    text = full_text;
                }
                break Ok(());
            }
            Some(DomStreamEvent::Error(e)) => break Err(e),
            None => break Err("the extension ended the turn".to_string()),
        }
    };
    state.release_tab(tab_id, model);
    outcome?;
    if refusal_kind(&text, false).is_some() {
        return Err(format!("{job_id} refused to write a prompt"));
    }
    let prompt = clean_image_prompt(&text).ok_or_else(|| format!("{job_id} wrote no prompt"))?;
    let request = plan.messages.iter().rev().find(|m| m.role == "user").map_or("", |m| m.text.as_str());
    let request = crate::recovery::user_request_text(request).0;
    state.log("debug", format!("{job_id}: image prompt {} chars, aspect from the request only", prompt.chars().count()));
    Ok(crate::stream::image_tool_call(tool, &prompt, aspect_for_request(request)))
}

pub(super) fn commit_session(
    state: &AppState,
    mut messages: Vec<NormalizedMessage>,
    clean_text: &str,
    tool_calls: &[ParsedToolCall],
    model: &str,
    tools_hash: Option<String>,
    tab_id: i64,
) {
    messages.push(NormalizedMessage {
        role: "assistant".into(),
        text: clean_text.to_string(),
        images: vec![],
        tool_calls: tool_calls.to_vec(),
        tool_call_id: None,
        tool_name: None,
    });
    let next_hash = hash_json(&messages);
    let mut sessions = state.sessions.lock();
    if sessions.len() >= 32 {
        sessions.pop_front();
    }
    sessions.push_back(SessionEntry {
        last_full_hash: next_hash,
        active_model: model.to_string(),
        tools_hash,
        preferred_tab_id: Some(tab_id),
        updated_at: now_secs(),
    });
}
