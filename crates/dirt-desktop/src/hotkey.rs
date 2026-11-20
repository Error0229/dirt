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
}

impl HotkeyManager {
    /// Create and register the global hotkey
    pub fn new() -> Result<Self, global_hotkey::Error> {
        let manager = GlobalHotKeyManager::new()?;
        let hotkey = default_hotkey();

        manager.register(hotkey)?;
        tracing::info!("Registered global hotkey: Ctrl+Alt+N");

        Ok(Self { _manager: manager })
    }
}

/// One press of the hotkey at a time, from the press to its letting go:
/// whether it is held long enough to talk (interface design 6.18).
///
/// The press, the letting go and the press's deadline each change it under
/// one lock, and say what to ask of quick capture while they hold it. So a
/// deadline that comes as the keys are let go either talks before the
/// letting go is heard, which then ends the talking, or finds the keys up
/// and does nothing: it can never start talking after the letting go.
#[derive(Debug, Default)]
pub struct Hold {
    down: bool,
    /// Counts presses, so that a deadline is told from the press before.
    presses: u64,
    talking: bool,
}

impl Hold {
    pub const fn new() -> Self {
        Self {
            down: false,
            presses: 0,
            talking: false,
        }
    }

    /// The keys went down. Gives the press's number, for its deadline, or
    /// `None` for the system repeating a key held down, which is not
    /// another press.
    pub const fn pressed(&mut self) -> Option<u64> {
        if self.down {
            return None;
        }
        self.down = true;
        self.talking = false;
        self.presses += 1;
        Some(self.presses)
    }

    /// Press `press` has been held as long as a tap can be. Says whether
    /// quick capture listens: only if that press is still down.
    pub const fn held(&mut self, press: u64) -> bool {
        if !self.down || self.presses != press || self.talking {
            return false;
        }
        self.talking = true;
        true
    }

    /// The keys were let go. Says whether what was said is kept: only if
    /// the press was held long enough to talk.
    pub const fn released(&mut self) -> bool {
        let kept = self.down && self.talking;
        self.down = false;
        self.talking = false;
        kept
    }
}

#[cfg(test)]
mod tests {
    use super::Hold;

    /// A tap opens quick capture and nothing more; a hold talks, and its
    /// letting go keeps what was said.
    #[test]
    fn a_tap_only_opens_and_a_hold_talks_until_let_go() {
        let mut hold = Hold::new();
        let tap = hold.pressed().expect("a press");
        assert!(!hold.released(), "a tap keeps nothing");
        assert!(
            !hold.held(tap),
            "a tap's deadline comes after it was let go"
        );

        let press = hold.pressed().expect("a press");
        assert!(hold.held(press));
        assert!(hold.released(), "letting go of a hold keeps what was said");
    }

    /// The system's repeat of a key held down is not another press.
    #[test]
    fn a_repeat_is_not_a_press() {
        let mut hold = Hold::new();
        let press = hold.pressed().expect("a press");
        assert_eq!(hold.pressed(), None);
        assert!(hold.held(press));
        assert!(!hold.held(press), "a press talks once");
        assert!(hold.released());
    }

    /// A deadline that comes after the letting go starts nothing, so the
    /// microphone is never left listening with the keys up (#303).
    #[test]
    fn a_deadline_after_the_letting_go_does_nothing() {
        let mut hold = Hold::new();
        let press = hold.pressed().expect("a press");
        assert!(!hold.released());
        assert!(!hold.held(press));
        // And a later letting go has nothing to end.
        assert!(!hold.released());
    }

    /// The deadline of a press already let go does not make the next
    /// press talk before its own deadline.
    #[test]
    fn an_old_deadline_does_not_count_for_a_new_press() {
        let mut hold = Hold::new();
        let first = hold.pressed().expect("a press");
        assert!(!hold.released());
        let second = hold.pressed().expect("a press");
        assert!(!hold.held(first));
        assert!(hold.held(second));
        assert!(hold.released());
    }
}
