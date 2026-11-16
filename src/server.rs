use crate::integrations;
use crate::state::{hash_json, now_secs, AppState, DomStreamEvent, ModelInfo, SessionEntry, TabState};
use crate::stream::{
    parse_anthropic_messages, parse_openai_messages, render_turn_prompt, NormalizedMessage,
    ParsedToolCall, StreamInterceptor,
};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::{HeaderMap, Method, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc,
    },
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

const EXTENSION_ID: &str = "lkbfapcoeagmekmncfbdgiefneohalej";
const FAVICON: &[u8] = include_bytes!("../extension/icons/icon32.png");
const DASHBOARD_TEMPLATE: &str = include_str!("dashboard.html");

pub async fn run_http_server(state: Arc<AppState>) -> Result<(), String> {
    let port = state.prefs.read().port;
    let app = Router::new()
        .route("/", get(serve_dashboard))
        .route("/favicon.ico", get(serve_favicon))
        .route("/api/health", get(api_health))
        .route("/api/pair", post(api_pair))
        .route("/ws/extension", get(ws_extension_upgrade))
        .route("/api/events", get(api_dashboard_sse))
        .route("/api/status", get(api_status))
        .route("/api/prefs", post(api_update_prefs))
        .route("/api/refresh-models", post(api_refresh_models))
        .route("/api/reset-sessions", post(api_reset_sessions))
        .route("/api/link-openmausbot", post(api_link_openmausbot))
        .route("/v1/models", get(openai_list_models))
        .route("/models", get(openai_list_models))
        .route("/v1/chat/completions", post(openai_chat_completions))
        .route("/chat/completions", post(openai_chat_completions))
        .route("/v1/messages", post(anthropic_messages))
        .route("/v1/images/generations", post(openai_image_generations))
        .route("/images/generations", post(openai_image_generations))
        .fallback(cors_preflight_or_404)
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|e| format!("Failed to bind 127.0.0.1:{port}: {e}"))?;

    state.log("info", format!("GeBridge v2 listening on http://127.0.0.1:{port}"));
    axum::serve(listener, app).await.map_err(|e| e.to_string())
}

fn check_origin(headers: &HeaderMap, port: u16) -> bool {
    let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) else {
        return true;
    };
    origin == format!("http://127.0.0.1:{port}")
        || origin == format!("http://localhost:{port}")
        || origin.starts_with("chrome-extension://")
        || origin.starts_with("extension://")
}

async fn serve_favicon() -> Response {
    ([("Content-Type", "image/png")], FAVICON).into_response()
}

async fn api_health(State(state): State<Arc<AppState>>) -> Response {
    let st = state.status_json();
    (
        [("Access-Control-Allow-Origin", "*")],
        Json(json!({
            "ok": true,
            "name": "ge-bridge",
            "version": env!("CARGO_PKG_VERSION"),
            "extensionConnected": st["extensionConnected"],
            "tabs": st["tabs"].as_array().map(|a| a.len()).unwrap_or(0),
            "geminiTargetSet": st["geminiTargetSet"],
        })),
    )
        .into_response()
}

async fn api_pair(headers: HeaderMap, State(state): State<Arc<AppState>>) -> Response {
    let origin = headers.get("origin").and_then(|v| v.to_str().ok());
    if origin == Some(format!("chrome-extension://{EXTENSION_ID}").as_str()) {
        state.log("info", "Pair request from the GeBridge extension: allowed.");
        (
            [("Access-Control-Allow-Origin", "*")],
            Json(json!({ "token": state.token })),
        )
            .into_response()
    } else {
        state.log("warn", "Pair request denied: origin is not the GeBridge extension.");
        json_error(StatusCode::FORBIDDEN, "Pairing is only available to the GeBridge extension")
    }
}

fn check_auth(headers: &HeaderMap, token: &str) -> bool {
    if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if auth.strip_prefix("Bearer ").map(str::trim) == Some(token) {
            return true;
        }
    }
    if let Some(xkey) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        if xkey.trim() == token {
            return true;
        }
    }
    false
}

fn json_error(status: StatusCode, msg: &str) -> Response {
    (
        status,
        [("Access-Control-Allow-Origin", "*")],
        Json(json!({ "error": { "message": msg, "type": "bridge_error" } })),
    )
        .into_response()
}

async fn cors_preflight_or_404(method: Method, headers: HeaderMap, State(state): State<Arc<AppState>>) -> Response {
    if !check_origin(&headers, state.prefs.read().port) {
        return json_error(StatusCode::FORBIDDEN, "Origin rejected");
    }
    if method == Method::OPTIONS {
        return (
            StatusCode::NO_CONTENT,
            [
                ("Access-Control-Allow-Origin", "*"),
                ("Access-Control-Allow-Headers", "Authorization, X-Api-Key, Anthropic-Version, Content-Type"),
                ("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
            ],
            "",
        )
            .into_response();
    }
    json_error(StatusCode::NOT_FOUND, "Endpoint not found")
}

async fn serve_dashboard(headers: HeaderMap, State(state): State<Arc<AppState>>) -> Response {
    if let Some(site) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        if site != "none" && site != "same-origin" {
            return json_error(StatusCode::FORBIDDEN, "Direct local navigation required");
        }
    }
    Html(DASHBOARD_TEMPLATE.replace("__INJECT_BRIDGE_TOKEN__", &state.token)).into_response()
}

#[derive(Deserialize)]
struct WsQuery {
    token: Option<String>,
}

async fn ws_extension_upgrade(
    ws: WebSocketUpgrade,
    Query(q): Query<WsQuery>,
    State(state): State<Arc<AppState>>,
) -> Response {
    if q.token.as_deref() != Some(&state.token) {
        return json_error(StatusCode::UNAUTHORIZED, "Invalid extension pairing token");
    }
    ws.on_upgrade(move |socket| handle_extension_ws(socket, state))
}

async fn handle_extension_ws(socket: WebSocket, state: Arc<AppState>) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    let conn_id = state.ext_link.write().attach(tx.clone());
    state.log("info", "Chromium Extension connected over full-duplex WebSocket.");
    state.notify_tray();

    // Passive model read on connect, only when the cached list is older than 24 h
    if !crate::state::models_cache_fresh(state.models_age_secs()) {
        let _ = tx.send(json!({ "type": "REQUEST_MODELS_SYNC" }).to_string());
    }

    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_tx.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = ws_rx.next().await {
        let Message::Text(raw) = msg else { continue };
        let Ok(payload) = serde_json::from_str::<Value>(&raw) else { continue };
        match payload.get("type").and_then(|v| v.as_str()).unwrap_or("") {
            "TABS_SYNC" => {
                if !state.ext_link.read().is_current(conn_id) {
                    continue;
                }
                if let Some(arr) = payload.get("tabs").and_then(|v| v.as_array()) {
                    let mut next_tabs = HashMap::new();
                    let old_tabs = state.tabs.read().clone();
                    for item in arr {
                        let Some(id) = item.get("tabId").and_then(|v| v.as_i64()) else { continue };
                        let prev_busy = old_tabs.get(&id).map(|t| t.busy).unwrap_or(false);
                        next_tabs.insert(
                            id,
                            TabState {
                                tab_id: id,
                                url: item.get("url").and_then(|v| v.as_str()).unwrap_or("").into(),
                                title: item.get("title").and_then(|v| v.as_str()).unwrap_or("Gemini Enterprise").into(),
                                active_model: item.get("activeModel").and_then(|v| v.as_str()).map(String::from),
                                calibrated: item.get("calibrated").and_then(|v| v.as_bool()).unwrap_or(false),
                                busy: prev_busy,
                            },
                        );
                    }
                    *state.tabs.write() = next_tabs;
                    state.tab_notify.notify_waiters();
                    state.notify_tray();
                    state.broadcast_dashboard();
                }
            }
            "EXT_LOG" => {
                for line in crate::logfile::ext_log_lines(&payload, crate::state::now_millis()) {
                    state.ext_log.append_line(&line);
                }
            }
            "MODELS_SYNC" => {
                if let Some(Ok(models)) = payload
                    .get("models")
                    .cloned()
                    .map(serde_json::from_value::<Vec<ModelInfo>>)
                {
                    let count = models.iter().filter(|m| m.available).count();
                    state.update_models(models);
                    state.log("info", format!("Synced {count} available models from Gemini Enterprise UI."));
                }
            }
            "GEMINI_TARGET" => {
                let url = payload.get("homeUrl").and_then(|v| v.as_str()).unwrap_or("");
                match crate::state::validate_gemini_home_url(url) {
                    Some(host) => {
                        let host = host.to_string();
                        let mut prefs = state.prefs.write();
                        if prefs.gemini_home_url.as_deref() != Some(url) {
                            prefs.gemini_home_url = Some(url.to_string());
                            prefs.save();
                            drop(prefs);
                            state.log("info", format!("Gemini Enterprise target saved (host {host})."));
                            state.notify_tray();
                            state.broadcast_dashboard();
                        }
                    }
                    None => state.log("warn", "Rejected GEMINI_TARGET: not a valid Gemini Enterprise home URL."),
                }
            }
            "DOM_DELTA" => {
                if let Some(job_id) = payload.get("jobId").and_then(|v| v.as_str()) {
                    let thought = payload.get("thoughtDelta").and_then(|v| v.as_str()).map(String::from);
                    let text = payload.get("textDelta").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    if let Some(job_tx) = state.pending_jobs.lock().get(job_id) {
                        let _ = job_tx.send(DomStreamEvent::Chunk {
                            thought_delta: thought,
                            text_delta: text,
                        });
                    }
                }
            }
            "IMAGE_DONE" => {
                if let Some(job_id) = payload.get("jobId").and_then(|v| v.as_str()) {
                    if let Some(tx) = state.pending_images.lock().remove(job_id) {
                        let _ = tx.send(crate::images::parse_image_done(&payload));
                    }
                }
            }
            "TURN_DONE" => {
                if let Some(job_id) = payload.get("jobId").and_then(|v| v.as_str()) {
                    if let Some(job_tx) = state.pending_jobs.lock().remove(job_id) {
                        if let Some(err) = payload.get("error").and_then(|v| v.as_str()) {
                            let _ = job_tx.send(DomStreamEvent::Error(err.to_string()));
                        } else {
                            let full_text = payload.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let active_model = payload.get("activeModel").and_then(|v| v.as_str()).map(String::from);
                            let _ = job_tx.send(DomStreamEvent::Done { full_text, active_model });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    writer.abort();
    if !state.ext_link.write().detach(conn_id) {
        // A newer connection replaced this one; leave its link and tabs alone.
        state.log("info", "Stale extension WebSocket closed (newer connection is live).");
        return;
    }
    state.tabs.write().clear();
    state.log("warn", "Extension WebSocket disconnected.");
    state.notify_tray();
    state.broadcast_dashboard();
}

async fn api_dashboard_sse(headers: HeaderMap, Query(q): Query<WsQuery>, State(state): State<Arc<AppState>>) -> Response {
    if !check_auth(&headers, &state.token) && q.token.as_deref() != Some(&state.token) {
        return json_error(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    let (tx, rx) = mpsc::unbounded_channel::<String>();
    let init = json!({
        "type": "state",
        "status": state.status_json(),
        "logs": *state.logs.lock()
    })
    .to_string();
    let _ = tx.send(init);
    state.ui_events_tx.lock().push(tx);

    let stream = tokio_stream::StreamExt::map(UnboundedReceiverStream::new(rx), |msg| {
        Ok::<Event, Infallible>(Event::default().data(msg))
    });
    Sse::new(stream).keep_alive(KeepAlive::default()).into_response()
}

async fn api_status(headers: HeaderMap, State(state): State<Arc<AppState>>) -> Response {
    if !check_auth(&headers, &state.token) {
        return json_error(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    Json(state.status_json()).into_response()
}

async fn api_update_prefs(headers: HeaderMap, State(state): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    if !check_auth(&headers, &state.token) {
        return json_error(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    {
        let mut prefs = state.prefs.write();
        if let Some(m) = body.get("defaultModel").and_then(|v| v.as_str()) {
            prefs.default_model = m.to_string();
        }
        if let Some(e) = body.get("reasoningEffort").and_then(|v| v.as_str()) {
            prefs.reasoning_effort = e.to_string();
        }
        prefs.save();
    }
    state.notify_tray();
    state.broadcast_dashboard();
    Json(json!({ "ok": true, "status": state.status_json() })).into_response()
}

async fn api_refresh_models(headers: HeaderMap, State(state): State<Arc<AppState>>) -> Response {
    if !check_auth(&headers, &state.token) {
        return json_error(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    let sent = state.request_models_sync(true, "dashboard/CLI");
    Json(json!({ "ok": true, "sent": sent })).into_response()
}

async fn api_reset_sessions(headers: HeaderMap, State(state): State<Arc<AppState>>) -> Response {
    if !check_auth(&headers, &state.token) {
        return json_error(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    state.sessions.lock().clear();
    state.log("info", "Cleared LRU SessionStore; next turn starts a clean thread.");
    Json(json!({ "ok": true })).into_response()
}

async fn api_link_openmausbot(headers: HeaderMap, State(state): State<Arc<AppState>>) -> Response {
    if !check_auth(&headers, &state.token) {
        return json_error(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    let prefs = state.prefs.read().clone();
    match integrations::link_openmausbot(prefs.port, &state.token, &prefs.default_model, prefs.enable_tools_default) {
        Ok(linked) => {
            state.log("info", format!("Auto-linked OpenMausBot at {} (image generation: {})", linked.path.display(), linked.image_gen));
            Json(json!({ "ok": true, "path": linked.path.display().to_string(), "imageGen": linked.image_gen, "movedBots": linked.moved_bots })).into_response()
        }
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &err),
    }
}

#[derive(Deserialize)]
struct ModelsQuery {
    refresh: Option<bool>,
}

/// OpenAI-compatible `POST /v1/images/generations` (OpenMausBot's "custom" image provider and others).
async fn openai_image_generations(
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
async fn openai_list_models(
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

struct PreparedJob {
    job_id: String,
    /// The tab serving the turn; a recovery retry may move it to another tab.
    tab_id: Arc<AtomicI64>,
    target_model: String,
    rx: mpsc::UnboundedReceiver<DomStreamEvent>,
    messages: Vec<NormalizedMessage>,
    tools_hash: Option<String>,
    has_tools: bool,
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

async fn dispatch_browser_turn(
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
        if matches!(trigger, Some(Refusal::Image | Refusal::SkippedImageTool)) && !retried {
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

fn commit_session(
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

/// Primary OpenAI `/v1/chat/completions` handler (OpenMausBot, Hermes, Cursor, Cline)
async fn openai_chat_completions(
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

/// Native Anthropic `/v1/messages` handler (Claude Code & Anthropic SDK harnesses)
async fn anthropic_messages(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    if !check_auth(&headers, &state.token) {
        return json_error(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    let raw_msgs = body.get("messages").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let messages = parse_anthropic_messages(body.get("system"), &raw_msgs);
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn hdrs(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(*k, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn origin_table() {
        let port = 8787;
        let cases: &[(Option<&str>, bool)] = &[
            (None, true),
            (Some("http://127.0.0.1:8787"), true),
            (Some("http://localhost:8787"), true),
            (Some("chrome-extension://abcdefghijklmnop"), true),
            (Some("extension://abc"), true),
            (Some("http://127.0.0.1:9999"), false),
            (Some("http://localhost:9999"), false),
            (Some("https://127.0.0.1:8787"), false),
            (Some("http://127.0.0.1:8787.evil.com"), false),
            (Some("http://evil.com"), false),
            (Some("https://vertexaisearch.cloud.google"), false),
            (Some("null"), false),
            (Some(""), false),
        ];
        for (origin, want) in cases {
            let h = match origin {
                Some(o) => hdrs(&[("origin", o)]),
                None => HeaderMap::new(),
            };
            assert_eq!(check_origin(&h, port), *want, "origin {origin:?}");
        }
    }

    #[test]
    fn origin_follows_configured_port() {
        let h = hdrs(&[("origin", "http://127.0.0.1:18787")]);
        assert!(check_origin(&h, 18787));
        assert!(!check_origin(&h, 8787));
    }

    #[test]
    fn auth_table() {
        let tok = "gemb_secret";
        let cases: &[(&[(&'static str, &str)], bool)] = &[
            (&[], false),
            (&[("authorization", "Bearer gemb_secret")], true),
            (&[("authorization", "Bearer  gemb_secret ")], true),
            (&[("authorization", "Bearer wrong")], false),
            (&[("authorization", "gemb_secret")], false),
            (&[("authorization", "Basic gemb_secret")], false),
            (&[("authorization", "bearer gemb_secret")], false),
            (&[("x-api-key", "gemb_secret")], true),
            (&[("x-api-key", " gemb_secret ")], true),
            (&[("x-api-key", "wrong")], false),
            (&[("authorization", "Bearer wrong"), ("x-api-key", "gemb_secret")], true),
            (&[("authorization", "Bearer gemb_secret"), ("x-api-key", "wrong")], true),
            (&[("authorization", "Bearer wrong"), ("x-api-key", "wrong")], false),
        ];
        for (pairs, want) in cases {
            assert_eq!(check_auth(&hdrs(pairs), tok), *want, "{pairs:?}");
        }
    }
}
