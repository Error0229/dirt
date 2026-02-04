//! Global hotkey registration and handling

use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyManager,
};

/// Default hotkey: Ctrl+Alt+N (Windows/Linux) or Cmd+Option+N (macOS)
/// N for "Note" - a quick way to capture a thought
pub fn default_hotkey() -> HotKey {
    #[cfg(target_os = "macos")]
    let modifiers = Modifiers::META | Modifiers::ALT;
    #[cfg(not(target_os = "macos"))]
    let modifiers = Modifiers::CONTROL | Modifiers::ALT;

    HotKey::new(Some(modifiers), Code::KeyN)
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
        tracing::info!("Registered global hotkey: Ctrl+Alt+N");

        Ok(Self {
            _manager: manager,
            hotkey,
        })
    }
}
