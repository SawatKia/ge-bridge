use crate::state::{AppState, DomStreamEvent};
use crate::stream::{parse_anthropic_messages, StreamInterceptor};
use axum::{extract::{State}, http::{HeaderMap, StatusCode}, response::{sse::{Event, KeepAlive, Sse}, IntoResponse, Response}, Json};
use serde_json::{json, Value};
use super::{check_auth, json_error, reject_non_base64_images};
use std::convert::Infallible;
use std::sync::{atomic::Ordering, Arc};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use super::supervisor::{commit_session, dispatch_browser_turn};

/// Native Anthropic `/v1/messages` handler (Claude Code & Anthropic SDK harnesses)
pub(super) async fn anthropic_messages(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    if !check_auth(&headers, &state.token) {
        return json_error(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    let raw_msgs = body.get("messages").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let messages = parse_anthropic_messages(body.get("system"), &raw_msgs);
    if let Some(r) = reject_non_base64_images(&messages) {
        return r;
    }
    let tools = body.get("tools").and_then(|v| v.as_array()).cloned();
    let req_model = body.get("model").and_then(|v| v.as_str());
    let is_stream = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);

    let mut job = match dispatch_browser_turn(&state, messages, tools, req_model, None).await {
        Ok(j) => j,
        Err(err) => return json_error(StatusCode::BAD_GATEWAY, &err),
    };

    let msg_id = format!("msg_{}", &job.job_id[4..]);
    let model_name = job.target_model.clone();

    if !is_stream {
        let mut interceptor = StreamInterceptor::new(job.has_tools);
        let mut reported_model = None;
        while let Some(ev) = job.rx.recv().await {
            match ev {
                DomStreamEvent::Chunk { thought_delta, text_delta } => {
                    interceptor.push_chunk(thought_delta.as_deref(), &text_delta);
                }
                DomStreamEvent::Done { full_text, active_model } => {
                    interceptor.finalize(&full_text);
                    reported_model = active_model;
                    break;
                }
                DomStreamEvent::Error(err) => {
                    return json_error(StatusCode::BAD_GATEWAY, &err);
                }
            }
        }
        let _ = reported_model;
        commit_session(
            &state,
            job.messages,
            &interceptor.full_clean_text,
            &interceptor.tool_calls,
            &model_name,
            job.tools_hash,
            job.tab_id.load(Ordering::Relaxed),
        );

        let mut content_blocks = Vec::new();
        if !interceptor.full_reasoning.is_empty() {
            content_blocks.push(json!({ "type": "thinking", "thinking": interceptor.full_reasoning }));
        }
        if !interceptor.full_clean_text.is_empty() {
            content_blocks.push(json!({ "type": "text", "text": interceptor.full_clean_text }));
        }
        for tc in &interceptor.tool_calls {
            let input_val: Value = serde_json::from_str(&tc.arguments_json).unwrap_or(json!({}));
            content_blocks.push(json!({
                "type": "tool_use",
                "id": tc.id,
                "name": tc.name,
                "input": input_val
            }));
        }

        return Json(json!({
            "id": msg_id,
            "type": "message",
            "role": "assistant",
            "model": model_name,
            "content": content_blocks,
            "stop_reason": if interceptor.tool_calls.is_empty() { "end_turn" } else { "tool_use" },
            "usage": { "input_tokens": 0, "output_tokens": 0 }
        }))
        .into_response();
    }

    // Anthropic SSE Streaming: thinking and text each get their own content block, opened when their
    // first delta arrives; tool_use blocks follow at the end.
    let (sse_tx, sse_rx) = mpsc::unbounded_channel::<Result<Event, Infallible>>();
    let state_clone = state.clone();

    tokio::spawn(async move {
        let mut interceptor = StreamInterceptor::new(job.has_tools);
        let send = |name: &str, data: Value| {
            let _ = sse_tx.send(Ok(Event::default().event(name).data(data.to_string())));
        };
        // (index, kind) of the open block; `next` is the next free index.
        let mut open: Option<(usize, &'static str)> = None;
        let mut next = 0usize;
        let switch_to = |kind: &'static str, open: &mut Option<(usize, &'static str)>, next: &mut usize| -> usize {
            if let Some((idx, k)) = *open {
                if k == kind {
                    return idx;
                }
                send("content_block_stop", json!({ "type": "content_block_stop", "index": idx }));
            }
            let idx = *next;
            *next += 1;
            let block = if kind == "thinking" { json!({ "type": "thinking", "thinking": "" }) } else { json!({ "type": "text", "text": "" }) };
            send("content_block_start", json!({ "type": "content_block_start", "index": idx, "content_block": block }));
            *open = Some((idx, kind));
            idx
        };

        send(
            "message_start",
            json!({
                "type": "message_start",
                "message": {
                    "id": msg_id, "type": "message", "role": "assistant", "model": model_name,
                    "content": [], "stop_reason": Value::Null, "usage": { "input_tokens": 0, "output_tokens": 0 }
                }
            }),
        );

        while let Some(ev) = job.rx.recv().await {
            match ev {
                DomStreamEvent::Chunk { thought_delta, text_delta } => {
                    let (cot, txt) = interceptor.push_chunk(thought_delta.as_deref(), &text_delta);
                    if let Some(r) = cot {
                        let idx = switch_to("thinking", &mut open, &mut next);
                        send("content_block_delta", json!({ "type": "content_block_delta", "index": idx, "delta": { "type": "thinking_delta", "thinking": r } }));
                    }
                    if let Some(t) = txt {
                        let idx = switch_to("text", &mut open, &mut next);
                        send("content_block_delta", json!({ "type": "content_block_delta", "index": idx, "delta": { "type": "text_delta", "text": t } }));
                    }
                }
                DomStreamEvent::Done { full_text, .. } => {
                    if let Some(tail) = interceptor.finalize(&full_text) {
                        let idx = switch_to("text", &mut open, &mut next);
                        send("content_block_delta", json!({ "type": "content_block_delta", "index": idx, "delta": { "type": "text_delta", "text": tail } }));
                    }
                    if open.is_none() && interceptor.tool_calls.is_empty() {
                        switch_to("text", &mut open, &mut next);
                    }
                    if let Some((idx, _)) = open.take() {
                        send("content_block_stop", json!({ "type": "content_block_stop", "index": idx }));
                    }

                    for tc in &interceptor.tool_calls {
                        let blk_idx = next;
                        next += 1;
                        let input_val: Value = serde_json::from_str(&tc.arguments_json).unwrap_or(json!({}));
                        send(
                            "content_block_start",
                            json!({
                                "type": "content_block_start", "index": blk_idx,
                                "content_block": { "type": "tool_use", "id": tc.id, "name": tc.name, "input": {} }
                            }),
                        );
                        send(
                            "content_block_delta",
                            json!({
                                "type": "content_block_delta", "index": blk_idx,
                                "delta": { "type": "input_json_delta", "partial_json": input_val.to_string() }
                            }),
                        );
                        send("content_block_stop", json!({ "type": "content_block_stop", "index": blk_idx }));
                    }

                    commit_session(
                        &state_clone,
                        job.messages,
                        &interceptor.full_clean_text,
                        &interceptor.tool_calls,
                        &model_name,
                        job.tools_hash,
                        job.tab_id.load(Ordering::Relaxed),
                    );

                    let stop = if interceptor.tool_calls.is_empty() { "end_turn" } else { "tool_use" };
                    send("message_delta", json!({ "type": "message_delta", "delta": { "stop_reason": stop }, "usage": { "output_tokens": 0 } }));
                    send("message_stop", json!({ "type": "message_stop" }));
                    break;
                }
                DomStreamEvent::Error(err) => {
                    send("error", json!({ "type": "error", "error": { "type": "api_error", "message": err } }));
                    break;
                }
            }
        }
    });

    Sse::new(UnboundedReceiverStream::new(sse_rx)).keep_alive(KeepAlive::default()).into_response()
}
