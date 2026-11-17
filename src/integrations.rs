pub fn copy_to_clipboard(text: &str) -> bool {
    arboard::Clipboard::new().and_then(|mut cb| cb.set_text(text.to_string())).is_ok()
}

#[cfg(windows)]
pub fn is_autostart_enabled() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
        .and_then(|k| k.get_value::<String, _>("GeBridge"))
        .is_ok()
}

#[cfg(not(windows))]
pub fn is_autostart_enabled() -> bool {
    false
}

#[cfg(windows)]
pub fn set_autostart(enable: bool) -> Result<(), String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
        .map_err(|e| e.to_string())?;
    if enable {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        key.set_value("GeBridge", &format!("\"{}\"", exe.display()))
            .map_err(|e| e.to_string())?;
    } else {
        let _ = key.delete_value("GeBridge");
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn set_autostart(_enable: bool) -> Result<(), String> {
    Ok(())
}
