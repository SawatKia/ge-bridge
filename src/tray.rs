use crate::integrations;
use crate::notify::{self, Notice};
use crate::state::{AppState, TraySignal};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
    Icon, TrayIconBuilder,
};

pub fn run_tray_loop(state: Arc<AppState>) {
    let (tx, mut rx) = mpsc::unbounded_channel::<TraySignal>();
    *state.tray_tx.lock() = Some(tx);

    let menu = build_tray_menu(&state);
    let icon = app_icon();

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("GeBridge v2 — Gemini Enterprise Local AI Server")
        .with_icon(icon)
        .build()
        .expect("Failed to create Windows System Tray icon");

    let menu_rx = MenuEvent::receiver();

    loop {
        pump_os_events();

        // Handle dynamic menu/icon updates when Extension syncs models or connects
        let mut need_rebuild = false;
        while rx.try_recv().is_ok() {
            need_rebuild = true;
        }
        if need_rebuild {
            let _ = tray.set_menu(Some(Box::new(build_tray_menu(&state))));
        }

        // Handle System-Tray clicks
        while let Ok(ev) = menu_rx.try_recv() {
            let id = ev.id.0.as_str();
            if id == "quit" {
                std::process::exit(0);
            } else if id == "open_dashboard" {
                let port = state.prefs.read().port;
                let _ = open::that(format!("http://127.0.0.1:{port}"));
            } else if id == "open_logs" {
                let dir = crate::state::logs_dir();
                let _ = std::fs::create_dir_all(&dir);
                let _ = open::that(dir);
            } else if id == "open_gemini" {
                let url = state
                    .prefs
                    .read()
                    .gemini_home_url
                    .clone()
                    .unwrap_or_else(|| "https://vertexaisearch.cloud.google/".to_string());
                let _ = open::that(url);
            } else if id == "refresh_models" {
                state.request_models_sync(true, "tray");
            } else if id == "calibrate_ui" {
                let _ = state.send_to_extension(&json!({ "type": "START_CALIBRATION" }));
            } else if id == "link_openmausbot" {
                let prefs = state.prefs.read().clone();
                let notice = match integrations::link_openmausbot(
                    prefs.port,
                    &state.token,
                    &prefs.default_model,
                    prefs.enable_tools_default,
                ) {
                    Ok(linked) => {
                        state.log(
                            "info",
                            format!("Linked OpenMausBot config at {} (image generation: {}, bots moved to the ACP engine: {:?})", linked.path.display(), linked.image_gen, linked.moved_bots),
                        );
                        notify::openmausbot_linked(linked.image_gen, linked.moved_bots)
                    }
                    Err(err) => {
                        state.log("error", format!("Linking OpenMausBot failed: {err}"));
                        notify::openmausbot_failed(&err)
                    }
                };
                show(&state, &notice);
            } else if id == "copy_openai_env" {
                let prefs = state.prefs.read().clone();
                let copied = integrations::copy_to_clipboard(&format!(
                    "OPENAI_BASE_URL=http://127.0.0.1:{}/v1\nOPENAI_API_KEY={}",
                    prefs.port, state.token
                ));
                show(&state, &if copied { notify::openai_env_copied(&prefs.default_model) } else { notify::clipboard_failed() });
            } else if id == "copy_claude_env" {
                let port = state.prefs.read().port;
                let copied = integrations::copy_to_clipboard(&format!(
                    "ANTHROPIC_BASE_URL=http://127.0.0.1:{port}\nANTHROPIC_AUTH_TOKEN={}",
                    state.token
                ));
                show(&state, &if copied { notify::claude_env_copied() } else { notify::clipboard_failed() });
            } else if id == "copy_token" {
                let copied = integrations::copy_to_clipboard(&state.token);
                show(&state, &if copied { notify::token_copied() } else { notify::clipboard_failed() });
            } else if id == "toggle_autostart" {
                let next = !integrations::is_autostart_enabled();
                let _ = integrations::set_autostart(next);
                let _ = tray.set_menu(Some(Box::new(build_tray_menu(&state))));
            } else if let Some(model_id) = id.strip_prefix("model:") {
                {
                    let mut prefs = state.prefs.write();
                    prefs.default_model = model_id.to_string();
                    prefs.save();
                }
                state.broadcast_dashboard();
                let _ = tray.set_menu(Some(Box::new(build_tray_menu(&state))));
            } else if let Some(eff) = id.strip_prefix("effort:") {
                {
                    let mut prefs = state.prefs.write();
                    prefs.reasoning_effort = eff.to_string();
                    prefs.save();
                }
                state.broadcast_dashboard();
                let _ = tray.set_menu(Some(Box::new(build_tray_menu(&state))));
            }
        }

        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Must run on the tray thread: the balloon goes on the icon this thread created.
fn show(state: &AppState, notice: &Notice) {
    if !notify::show(notice) {
        state.log("warn", format!("Could not show the notification \"{}\"", notice.title));
    }
}

fn build_tray_menu(state: &AppState) -> Menu {
    let menu = Menu::new();
    let prefs = state.prefs.read().clone();
    let ext_ok = state.ext_connected();
    let mut tabs: Vec<crate::state::TabState> = state.tabs.read().values().cloned().collect();
    tabs.sort_by_key(|t| (t.is_api_lane(), t.tab_id));
    let tab_count = tabs.len();
    let browser_count = tabs.iter().filter(|t| !t.is_api_lane()).count();

    let status_label = if ext_ok && tab_count > 0 {
        format!("🟢 Ready — {browser_count} browser tab(s), {} API lane(s)", tab_count - browser_count)
    } else if ext_ok {
        "🟡 Extension Connected — Open Gemini Tab".to_string()
    } else {
        "🟠 Waiting for Browser Extension...".to_string()
    };

    let _ = menu.append(&MenuItem::with_id("status", status_label, false, None));
    for (i, t) in tabs.iter().enumerate() {
        let _ = menu.append(&MenuItem::with_id(format!("tab_info:{i}"), format!("    {}", t.label()), false, None));
    }
    let _ = menu.append(&PredefinedMenuItem::separator());

    // Dynamic Default Model Submenu
    let model_sub = Submenu::new(format!("Default Model ({})", prefs.default_model), true);
    for m in state.models.read().iter().filter(|m| m.available) {
        let checked = m.id == prefs.default_model;
        let item = CheckMenuItem::with_id(
            format!("model:{}", m.id),
            format!("{} ({})", m.display_name, m.id),
            true,
            checked,
            None,
        );
        let _ = model_sub.append(&item);
    }
    let _ = model_sub.append(&PredefinedMenuItem::separator());
    let _ = model_sub.append(&MenuItem::with_id("refresh_models", "🔄 Refresh Available Models", true, None));
    let _ = menu.append(&model_sub);

    // Reasoning Effort Submenu
    let effort_sub = Submenu::new(format!("Reasoning Effort ({})", prefs.reasoning_effort), true);
    for eff in ["auto", "low", "medium", "high", "max"] {
        let item = CheckMenuItem::with_id(
            format!("effort:{eff}"),
            eff.to_uppercase(),
            true,
            prefs.reasoning_effort == eff,
            None,
        );
        let _ = effort_sub.append(&item);
    }
    let _ = menu.append(&effort_sub);

    // 1-Click Harness Integrations Submenu
    let int_sub = Submenu::new("Quick Integrations", true);
    let _ = int_sub.append(&MenuItem::with_id(
        "link_openmausbot",
        "⚡ Auto-Configure OpenMausBot",
        true,
        None,
    ));
    let _ = int_sub.append(&MenuItem::with_id(
        "copy_openai_env",
        "📋 Copy Hermes / Cline / OpenAI Env",
        true,
        None,
    ));
    let _ = int_sub.append(&MenuItem::with_id(
        "copy_claude_env",
        "📋 Copy Claude Code Anthropic Env",
        true,
        None,
    ));
    let _ = int_sub.append(&MenuItem::with_id("copy_token", "🔑 Copy Pairing Token", true, None));
    let _ = menu.append(&int_sub);

    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&MenuItem::with_id(
        "open_dashboard",
        format!("🌐 Open Control Center (127.0.0.1:{})", prefs.port),
        true,
        None,
    ));
    let _ = menu.append(&MenuItem::with_id("open_logs", "📄 Open Logs Folder", true, None));
    let _ = menu.append(&MenuItem::with_id(
        "open_gemini",
        "🌐 Open Gemini Enterprise",
        true,
        None,
    ));
    let _ = menu.append(&MenuItem::with_id(
        "calibrate_ui",
        "🎯 Visual Calibrate Browser Selectors",
        true,
        None,
    ));
    let _ = menu.append(&CheckMenuItem::with_id(
        "toggle_autostart",
        "Start with Windows",
        true,
        integrations::is_autostart_enabled(),
        None,
    ));
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&MenuItem::with_id("quit", "Quit GeBridge", true, None));

    menu
}

/// Loads the icon embedded by build.rs (resource ordinal 1).
fn app_icon() -> Icon {
    Icon::from_resource(1, Some((32, 32))).expect("embedded app icon (resource 1)")
}

#[cfg(windows)]
fn pump_os_events() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_QUIT,
    };
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, std::mem::zeroed(), 0, 0, PM_REMOVE) != 0 {
            if msg.message == WM_QUIT {
                std::process::exit(0);
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(not(windows))]
fn pump_os_events() {}
