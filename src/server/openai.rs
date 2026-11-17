use crate::state::{now_secs, AppState, DomStreamEvent};
use crate::stream::{parse_openai_messages, StreamInterceptor};
use axum::{extract::{Query, State}, http::{HeaderMap, StatusCode}, response::{sse::{Event, KeepAlive, Sse}, IntoResponse, Response}, Json};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{json, Value};
use super::{check_auth, check_origin, json_error, reject_non_base64_images};
use std::convert::Infallible;
use std::sync::{atomic::Ordering, Arc};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use super::supervisor::{commit_session, dispatch_browser_turn};

#[derive(Deserialize)]
pub(super) struct ModelsQuery {
    refresh: Option<bool>,
}

/// OpenAI-compatible `POST /v1/images/generations` (OpenMausBot's "custom" image provider and others).
pub(super) async fn openai_image_generations(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    let reject = |status: StatusCode, msg: &str| {
        state.log("warn", format!("Image request rejected ({}): {msg}", status.as_u16()));
        json_error(status, msg)
    };
    if !check_origin(&headers, state.prefs.read().port) {
        return reject(StatusCode::FORBIDDEN, "Origin rejected");
    }
    if !check_auth(&headers, &state.token) {
        return reject(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    let parsed: crate::images::ImageRequest = match serde_json::from_value(body) {
        Ok(r) => r,
        Err(e) => return reject(StatusCode::BAD_REQUEST, &format!("Invalid request: {e}")),
    };
    let req = match crate::images::validate(&parsed) {
        Ok(r) => r,
        Err(e) => return reject(StatusCode::BAD_REQUEST, &e),
    };

    let mut id_bytes = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut id_bytes);
    let job_id = format!("img_{}", hex::encode(id_bytes));
    let (tx, rx) = tokio::sync::oneshot::channel();
    state.pending_images.lock().insert(job_id.clone(), tx);
    let cmd = json!({ "type": "RUN_IMAGE", "jobId": job_id, "prompt": req.prompt, "aspect": req.aspect });
    if let Err(err) = state.send_to_extension(&cmd) {
        state.pending_images.lock().remove(&job_id);
        return reject(StatusCode::SERVICE_UNAVAILABLE, &err);
    }
    state.log("info", format!("Dispatched {job_id} [image, aspect={}]", req.aspect.unwrap_or("auto")));

    let outcome = tokio::time::timeout(std::time::Duration::from_secs(crate::images::IMAGE_TIMEOUT_SECS), rx).await;
    state.pending_images.lock().remove(&job_id);
    match outcome {
        Ok(Ok(Ok(img))) => {
            state.log("info", format!("{job_id} done ({}, {} base64 chars)", img.mime, img.b64.len()));
            {
                let mut recent = state.recent_images.lock();
                if recent.len() >= crate::bot_images::RECENT_IMAGES_MAX {
                    recent.pop_front();
                }
                recent.push_back(crate::bot_images::RecentImage { prompt: req.prompt.clone(), mime: img.mime.clone(), b64: img.b64.clone(), at: now_secs() });
            }
            (
                StatusCode::OK,
                [("Access-Control-Allow-Origin", "*")],
                Json(crate::images::response_body(&img, req.as_url, now_secs())),
            )
                .into_response()
        }
        Ok(Ok(Err(err))) => {
            state.log("warn", format!("{job_id} failed: {err}"));
            json_error(StatusCode::BAD_GATEWAY, &format!("Gemini image generation failed: {err}"))
        }
        Ok(Err(_)) => json_error(StatusCode::BAD_GATEWAY, "Extension disconnected before the image was ready"),
        Err(_) => {
            state.log("warn", format!("{job_id} timed out"));
            json_error(StatusCode::GATEWAY_TIMEOUT, "Image generation timed out")
        }
    }
}

/// Standard OpenAI `GET /v1/models` returning live discovered Gemini Enterprise models
pub(super) async fn openai_list_models(
    headers: HeaderMap,
    Query(q): Query<ModelsQuery>,
    State(state): State<Arc<AppState>>,
) -> Response {
    if !check_origin(&headers, state.prefs.read().port) {
        return json_error(StatusCode::FORBIDDEN, "Origin rejected");
    }
    if !check_auth(&headers, &state.token) {
        return json_error(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    // Harnesses may ask for ?refresh=true on every picker open; that only rescans a stale (24 h) cache
    if q.refresh == Some(true) && state.request_models_sync(false, "GET /v1/models?refresh=true") {
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    }

    let models: Vec<Value> = state
        .models
        .read()
        .iter()
        .filter(|m| m.available)
        .map(|m| {
            json!({
                "id": m.id,
                "object": "model",
                "created": 1760000000,
                "owned_by": "gemini-enterprise-bridge",
                // OpenMausBot (and OpenRouter-style clients) label the picker entry with `name`
                "name": m.display_name,
                "metadata": {
                    "display_name": m.display_name,
                    "supports_thinking": m.supports_thinking,
                    "reasoning_efforts": m.efforts
                }
            })
        })
        .collect();

    (
        StatusCode::OK,
        [("Access-Control-Allow-Origin", "*")],
        Json(json!({ "object": "list", "data": models })),
    )
        .into_response()
}

/// Primary OpenAI `/v1/chat/completions` handler (OpenMausBot, Hermes, Cursor, Cline)
pub(super) async fn openai_chat_completions(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    if !check_origin(&headers, state.prefs.read().port) {
        return json_error(StatusCode::FORBIDDEN, "Origin rejected");
    }
    if !check_auth(&headers, &state.token) {
        return json_error(StatusCode::UNAUTHORIZED, "Unauthorized");
    }

    let raw_msgs = body.get("messages").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let messages = parse_openai_messages(&raw_msgs);
    if let Some(r) = reject_non_base64_images(&messages) {
        return r;
    }
    let tools = body.get("tools").and_then(|v| v.as_array()).cloned();
    let req_model = body.get("model").and_then(|v| v.as_str());
    let req_effort = body.get("reasoning_effort").and_then(|v| v.as_str());
    let is_stream = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);

    let mut job = match dispatch_browser_turn(&state, messages, tools, req_model, req_effort).await {
        Ok(j) => j,
        Err(err) => return json_error(StatusCode::BAD_GATEWAY, &err),
    };

    let cmpl_id = format!("chatcmpl_{}", &job.job_id[4..]);
    let created = now_secs();
    let model_name = job.target_model.clone();

    if !is_stream {
        let mut interceptor = StreamInterceptor::new(job.has_tools);
        let mut final_err = None;
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
                DomStreamEvent::Error(e) => {
                    final_err = Some(e);
                    break;
                }
            }
        }
        let _ = reported_model;

        if let Some(err) = final_err {
            return json_error(StatusCode::BAD_GATEWAY, &err);
        }

        commit_session(
            &state,
            job.messages,
            &interceptor.full_clean_text,
            &interceptor.tool_calls,
            &model_name,
            job.tools_hash,
            job.tab_id.load(Ordering::Relaxed),
        );

        let tc_json: Vec<Value> = interceptor
            .tool_calls
            .iter()
            .map(|tc| {
                json!({
                    "id": tc.id,
                    "type": "function",
                    "function": { "name": tc.name, "arguments": tc.arguments_json }
                })
            })
            .collect();

        let mut msg_obj = json!({
            "role": "assistant",
            "content": if interceptor.full_clean_text.is_empty() && !tc_json.is_empty() {
                Value::Null
            } else {
                Value::String(interceptor.full_clean_text)
            }
        });
        if !interceptor.full_reasoning.is_empty() {
            msg_obj["reasoning_content"] = Value::String(interceptor.full_reasoning);
        }
        if !tc_json.is_empty() {
            msg_obj["tool_calls"] = Value::Array(tc_json.clone());
        }

        return (
            StatusCode::OK,
            [("Access-Control-Allow-Origin", "*")],
            Json(json!({
                "id": cmpl_id,
                "object": "chat.completion",
                "created": created,
                "model": model_name,
                "choices": [{
                    "index": 0,
                    "message": msg_obj,
                    "finish_reason": if tc_json.is_empty() { "stop" } else { "tool_calls" }
                }]
            })),
        )
            .into_response();
    }

    // Real-time OpenAI SSE Streaming
    let (sse_tx, sse_rx) = mpsc::unbounded_channel::<Result<Event, Infallible>>();
    let state_clone = state.clone();

    tokio::spawn(async move {
        let mut interceptor = StreamInterceptor::new(job.has_tools);
        let mut reported_model = None;

        let role_chunk = json!({
            "id": cmpl_id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model_name,
            "choices": [{ "index": 0, "delta": { "role": "assistant" }, "finish_reason": Value::Null }]
        });
        let _ = sse_tx.send(Ok(Event::default().data(role_chunk.to_string())));

        while let Some(ev) = job.rx.recv().await {
            match ev {
                DomStreamEvent::Chunk { thought_delta, text_delta } => {
                    let (cot, txt) = interceptor.push_chunk(thought_delta.as_deref(), &text_delta);
                    if let Some(r) = cot {
                        let c = json!({
                            "id": cmpl_id,
                            "object": "chat.completion.chunk",
                            "created": created,
                            "model": model_name,
                            "choices": [{ "index": 0, "delta": { "reasoning_content": r }, "finish_reason": Value::Null }]
                        });
                        let _ = sse_tx.send(Ok(Event::default().data(c.to_string())));
                    }
                    if let Some(t) = txt {
                        let c = json!({
                            "id": cmpl_id,
                            "object": "chat.completion.chunk",
                            "created": created,
                            "model": model_name,
                            "choices": [{ "index": 0, "delta": { "content": t }, "finish_reason": Value::Null }]
                        });
                        let _ = sse_tx.send(Ok(Event::default().data(c.to_string())));
                    }
                }
                DomStreamEvent::Done { full_text, active_model } => {
                    reported_model = active_model;
                    if let Some(tail) = interceptor.finalize(&full_text) {
                        let c = json!({
                            "id": cmpl_id,
                            "object": "chat.completion.chunk",
                            "created": created,
                            "model": model_name,
                            "choices": [{ "index": 0, "delta": { "content": tail }, "finish_reason": Value::Null }]
                        });
                        let _ = sse_tx.send(Ok(Event::default().data(c.to_string())));
                    }

                    for (idx, tc) in interceptor.tool_calls.iter().enumerate() {
                        let c = json!({
                            "id": cmpl_id,
                            "object": "chat.completion.chunk",
                            "created": created,
                            "model": model_name,
                            "choices": [{
                                "index": 0,
                                "delta": {
                                    "tool_calls": [{
                                        "index": idx,
                                        "id": tc.id,
                                        "type": "function",
                                        "function": { "name": tc.name, "arguments": tc.arguments_json }
                                    }]
                                },
                                "finish_reason": Value::Null
                            }]
                        });
                        let _ = sse_tx.send(Ok(Event::default().data(c.to_string())));
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

                    let done_chunk = json!({
                        "id": cmpl_id,
                        "object": "chat.completion.chunk",
                        "created": created,
                        "model": model_name,
                        "choices": [{
                            "index": 0,
                            "delta": {},
                            "finish_reason": if interceptor.tool_calls.is_empty() { "stop" } else { "tool_calls" }
                        }]
                    });
                    let _ = sse_tx.send(Ok(Event::default().data(done_chunk.to_string())));
                    let _ = sse_tx.send(Ok(Event::default().data("[DONE]")));
                    break;
                }
                DomStreamEvent::Error(err) => {
                    let e = json!({ "error": { "message": err } });
                    let _ = sse_tx.send(Ok(Event::default().data(e.to_string())));
                    let _ = sse_tx.send(Ok(Event::default().data("[DONE]")));
                    break;
                }
            }
        }
        let _ = reported_model;
    });

    (
        [("Access-Control-Allow-Origin", "*")],
        Sse::new(UnboundedReceiverStream::new(sse_rx)).keep_alive(KeepAlive::default()),
    )
        .into_response()
}
