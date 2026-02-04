//! Global hotkey registration and handling

use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyManager,
};

/// Default hotkey: Ctrl+Shift+D (Windows/Linux) or Cmd+Shift+D (macOS)
pub fn default_hotkey() -> HotKey {
    #[cfg(target_os = "macos")]
    let modifiers = Modifiers::META | Modifiers::SHIFT;
    #[cfg(not(target_os = "macos"))]
    let modifiers = Modifiers::CONTROL | Modifiers::SHIFT;

    HotKey::new(Some(modifiers), Code::KeyD)
}

/// Manages global hotkey registration
pub struct HotkeyManager {
    _manager: GlobalHotKeyManager,
    #[allow(dead_code)]
    pub hotkey: HotKey,
}

impl HotkeyManager {
    /// Create and register the global hotkey
    pub fn new() -> Result<Self, global_hotkey::Error> {
        let manager = GlobalHotKeyManager::new()?;
        let hotkey = default_hotkey();

        manager.register(hotkey)?;
        tracing::info!("Registered global hotkey: Ctrl+Shift+D");

        Ok(Self {
            _manager: manager,
            hotkey,
        })
    }
}
