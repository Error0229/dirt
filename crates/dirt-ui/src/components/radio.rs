//! What a group of exclusive choices needs from the keyboard: the arrow
//! keys move the choice, and focus moves with it. The segmented control
//! and the accent dots are both such groups.

use std::rc::Rc;

use dioxus::prelude::*;

/// The choice an arrow key moves to from `index`, of `count`. Left and Up
/// go back, Right and Down go on, and both wrap; Home and End go to the
/// ends.
pub(super) const fn step(key: &Key, index: usize, count: usize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    match key {
        Key::ArrowRight | Key::ArrowDown => Some((index + 1) % count),
        Key::ArrowLeft | Key::ArrowUp => Some((index + count - 1) % count),
        Key::Home => Some(0),
        Key::End => Some(count - 1),
        _ => None,
    }
}

/// The buttons of one group, kept so that focus can follow the choice.
#[derive(Clone, Copy)]
pub(super) struct Buttons(Signal<Vec<Option<Rc<MountedData>>>>);

impl Buttons {
    /// Takes down the button at `index` as it is put on the page.
    pub(super) fn mounted(mut self, index: usize, button: Rc<MountedData>) {
        let mut buttons = self.0.write();
        if buttons.len() <= index {
            buttons.resize(index + 1, None);
        }
        buttons[index] = Some(button);
    }

    /// Moves focus to the button at `index`.
    pub(super) fn focus(self, index: usize) {
        let button = self.0.peek().get(index).cloned().flatten();
        if let Some(button) = button {
            spawn(async move {
                // A button that has gone cannot take focus, and need not.
                let _ = button.set_focus(true).await;
            });
        }
    }
}

/// The buttons of the group this component renders.
pub(super) fn use_buttons() -> Buttons {
    Buttons(use_signal(Vec::new))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arrows_move_and_wrap() {
        assert_eq!(step(&Key::ArrowRight, 0, 3), Some(1));
        assert_eq!(step(&Key::ArrowRight, 2, 3), Some(0));
        assert_eq!(step(&Key::ArrowLeft, 0, 3), Some(2));
        assert_eq!(step(&Key::ArrowUp, 1, 3), Some(0));
        assert_eq!(step(&Key::Home, 2, 3), Some(0));
        assert_eq!(step(&Key::End, 0, 3), Some(2));
        assert_eq!(step(&Key::Enter, 0, 3), None);
        assert_eq!(step(&Key::ArrowRight, 0, 0), None);
    }
}
