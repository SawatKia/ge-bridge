mod anthropic;
mod openai;
mod supervisor;

use anthropic::anthropic_messages;
use openai::{openai_chat_completions, openai_image_generations, openai_list_models};
use crate::integrations;
use crate::state::{AppState, DomStreamEvent, ModelInfo, TabState};
use crate::stream::{is_base64_image, NormalizedMessage};
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
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    convert::Infallible,
    sync::Arc,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

const EXTENSION_ID: &str = "lkbfapcoeagmekmncfbdgiefneohalej";
const FAVICON: &[u8] = include_bytes!("../../extension/icons/icon32.png");
const DASHBOARD_TEMPLATE: &str = include_str!("../dashboard.html");

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
        .layer(axum::extract::DefaultBodyLimit::max(100 * 1024 * 1024))
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

const ALLOWED_IMAGE_MIMES: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/gif"];
const IMAGE_MAX_BYTES: usize = 10 * 1024 * 1024;

/// Why an image is unusable, or None when it is a base64 data URL of an allowed type and size.
fn image_problem(u: &str) -> Option<String> {
    if !is_base64_image(u) {
        return Some("Image URLs are not supported; send the image as base64 (a data: URL or a base64 source)".into());
    }
    let (head, payload) = u.strip_prefix("data:")?.split_once(',')?;
    let mime = head.split(';').next().unwrap_or("").to_ascii_lowercase();
    if !ALLOWED_IMAGE_MIMES.contains(&mime.as_str()) {
        return Some(format!("Unsupported image type {mime}; allowed: {}", ALLOWED_IMAGE_MIMES.join(", ")));
    }
    let decoded = payload.len() * 3 / 4;
    (decoded > IMAGE_MAX_BYTES).then(|| format!("Image is too large ({decoded} bytes decoded); the limit is {IMAGE_MAX_BYTES} bytes (10 MB)"))
}

/// A 400 when any image is not a base64 data URL (the extension would silently skip it), has a
/// type outside the allowed list, or decodes to more than 10 MB.
fn reject_non_base64_images(messages: &[NormalizedMessage]) -> Option<Response> {
    messages
        .iter()
        .flat_map(|m| &m.images)
        .find_map(|u| image_problem(u))
        .map(|msg| json_error(StatusCode::BAD_REQUEST, &msg))
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
    fn image_rules() {
        let img = |mime: &str, n: usize| format!("data:{mime};base64,{}", "A".repeat(n));
        for m in ["image/png", "image/jpeg", "image/webp", "image/gif"] {
            assert!(image_problem(&img(m, 8)).is_none(), "{m}");
        }
        let svg = image_problem(&img("image/svg+xml", 8)).unwrap();
        assert!(svg.contains("image/svg+xml") && svg.contains("image/png"), "{svg}");
        // 13981014 base64 chars decode to exactly 10 MiB; one more char is over.
        assert!(image_problem(&img("image/png", 13_981_014)).is_none());
        assert!(image_problem(&img("image/png", 13_981_015)).unwrap().contains("too large"));
        assert!(image_problem("https://example.com/a.png").unwrap().contains("not supported"));
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
