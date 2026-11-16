//! Desktop notifications for tray actions: what just happened and what to do next.
//! On Windows this is a balloon on ge-bridge's own tray icon, which Windows 10/11 shows as a toast.

pub struct Notice {
    pub title: String,
    pub body: String,
    pub error: bool,
}

impl Notice {
    fn ok(title: &str, body: String) -> Self {
        Self { title: title.into(), body, error: false }
    }
    fn err(title: &str, body: String) -> Self {
        Self { title: title.into(), body, error: true }
    }
}

/// A Windows 10 toast shows four lines of body text, about 37 characters each (proportional font, so approximate), and cuts the rest.
pub const TOAST_LINES: usize = 4;
pub const TOAST_LINE_CHARS: usize = 37;

/// Lines `body` takes in a toast, wrapping at spaces the way Windows does.
pub fn toast_lines(body: &str) -> usize {
    body.split('\n')
        .map(|para| {
            let (mut lines, mut width) = (1, 0);
            for w in para.split(' ').map(|word| word.chars().count()) {
                if width == 0 {
                    width = w;
                } else if width + 1 + w <= TOAST_LINE_CHARS {
                    width += 1 + w;
                } else {
                    lines += 1;
                    width = w;
                }
                while width > TOAST_LINE_CHARS {
                    lines += 1;
                    width -= TOAST_LINE_CHARS;
                }
            }
            lines
        })
        .sum()
}

pub fn openmausbot_linked(image_gen: bool, moved_bots: Option<usize>) -> Notice {
    let images = if image_gen { "images on" } else { "image provider kept" };
    match moved_bots {
        Some(n) => Notice::ok("OpenMausBot configured", format!("ACP engine set, {images}, {n} bots moved.\nNext: start OpenMausBot.")),
        None => Notice::ok("OpenMausBot configured", format!("ACP engine set, {images}. Bots not moved.\nNext: quit it, click again.")),
    }
}

pub fn openmausbot_failed(err: &str) -> Notice {
    let err: String = err.chars().take(TOAST_LINE_CHARS * 2 - 4).collect();
    Notice::err("OpenMausBot setup failed", format!("{err}\nCheck the .openmausbot folder is writable, then retry."))
}

pub fn openai_env_copied(model: &str) -> Notice {
    Notice::ok(
        "OpenAI env copied",
        format!("Next: paste OPENAI_BASE_URL and OPENAI_API_KEY into Hermes, Cline or your tool. Model: {model}."),
    )
}

pub fn claude_env_copied() -> Notice {
    Notice::ok(
        "Claude Code env copied",
        "Next: paste into your shell or the \"env\" block of Claude Code's settings.json, then start claude.".into(),
    )
}

pub fn token_copied() -> Notice {
    Notice::ok(
        "Pairing token copied",
        "Next: paste it into the GeBridge extension popup, or use it as the API key in your tool. Keep it private.".into(),
    )
}

pub fn clipboard_failed() -> Notice {
    Notice::err("Could not copy to the clipboard", "Another app may be holding the clipboard. Try again.".into())
}

/// Shows `n` as a balloon on the tray icon created on the calling thread. Returns false when there is
/// no such icon or Windows refused; callers just log it.
#[cfg(windows)]
pub fn show(n: &Notice) -> bool {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::Shell::{Shell_NotifyIconW, NIF_INFO, NIIF_ERROR, NIIF_INFO, NIM_MODIFY, NOTIFYICONDATAW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, GetClassNameW};

    // tray-icon's hidden window class; its icon id is internal, so try the first few.
    const TRAY_CLASS: &str = "tray_icon_app";
    const MAX_ICON_ID: u32 = 16;

    unsafe extern "system" fn find(hwnd: HWND, out: LPARAM) -> BOOL {
        let mut buf = [0u16; 64];
        let len = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if len > 0 && String::from_utf16_lossy(&buf[..len as usize]) == TRAY_CLASS {
            *(out as *mut HWND) = hwnd;
            return 0;
        }
        1
    }

    fn fill<const N: usize>(dst: &mut [u16; N], s: &str) {
        let wide: Vec<u16> = s.encode_utf16().take(N - 1).collect();
        dst[..wide.len()].copy_from_slice(&wide);
    }

    unsafe {
        let mut hwnd: HWND = std::ptr::null_mut();
        EnumThreadWindows(GetCurrentThreadId(), Some(find), &mut hwnd as *mut HWND as LPARAM);
        if hwnd.is_null() {
            return false;
        }
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uFlags = NIF_INFO;
        nid.dwInfoFlags = if n.error { NIIF_ERROR } else { NIIF_INFO };
        fill(&mut nid.szInfoTitle, &n.title);
        fill(&mut nid.szInfo, &n.body);
        (1..=MAX_ICON_ID).any(|id| {
            nid.uID = id;
            Shell_NotifyIconW(NIM_MODIFY, &nid) != 0
        })
    }
}

#[cfg(not(windows))]
pub fn show(_n: &Notice) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_say_what_happened_and_what_to_do_next_within_windows_limits() {
        let all = [
            openmausbot_linked(true, Some(21)),
            openmausbot_linked(false, None),
            openmausbot_failed("Access is denied. (os error 5)"),
            openai_env_copied("gemini-3.5-flash"),
            claude_env_copied(),
            token_copied(),
            clipboard_failed(),
        ];
        for n in &all {
            assert!(n.title.encode_utf16().count() < 64, "title fits szInfoTitle: {}", n.title);
            assert!(toast_lines(&n.body) <= TOAST_LINES, "body fits a toast ({} lines): {}", toast_lines(&n.body), n.body);
            assert!(n.error || n.body.contains("Next:"), "success says what to do next: {}", n.body);
        }
        assert!(all[0].body.contains("21 bots moved") && all[0].body.contains("images on"));
        assert!(all[1].body.contains("image provider kept") && all[1].body.contains("quit it"));
        assert!(toast_lines(&openmausbot_failed(&"word ".repeat(100)).body) <= TOAST_LINES);
        assert!(toast_lines(&openmausbot_failed(&"x".repeat(500)).body) <= TOAST_LINES);
        assert!(all[2].error && all[2].body.starts_with("Access is denied"));
        assert!(all[3].body.contains("gemini-3.5-flash"));
    }

    #[test]
    fn toast_line_count_matches_what_windows_showed() {
        // Seen on screen: 4 lines shown and "for a bot." cut off, so 5 lines.
        assert_eq!(
            toast_lines("Model gemini-4-argon, image generation on.\nNext: restart OpenMausBot, pick \"Gemini Enterprise (GeBridge v2)\" for a bot."),
            5
        );
        // Seen on screen: exactly 3 lines.
        assert_eq!(toast_lines("Next: paste it into the GeBridge extension popup, or use it as the API key in your tool. Keep it private."), 3);
    }
}
