//! A Windows-style "Alt bar": the menu bar stays hidden until Alt is pressed
//! and released on its own (Alt used as a modifier, e.g. Alt+Tab or a
//! shortcut, does not toggle it), hides again on Esc or after a choice, and
//! can be pinned open.

use egui::{Context, Event, Key};

#[derive(Debug, Default, Clone)]
pub struct AltMenuBar {
    revealed: bool,
    alt_down: bool,
    /// Something else happened while Alt was held: it was a modifier.
    alt_used: bool,
}

impl AltMenuBar {
    /// Feed this frame's input; returns whether the bar should be drawn.
    pub fn update(&mut self, ctx: &Context, always: bool) -> bool {
        let (alt, other, escape) = ctx.input(|i| {
            // The Alt keys themselves arrive as key events too; only other
            // keys, clicks and typing mean Alt was used as a modifier.
            let other = i.events.iter().any(|e| match e {
                Event::Key { key, .. } => !matches!(key, Key::AltLeft | Key::AltRight),
                Event::PointerButton { .. } | Event::Text(_) => true,
                _ => false,
            });
            (i.modifiers.alt, other, i.key_pressed(Key::Escape))
        });
        self.step(alt, other, escape);
        self.revealed || always
    }

    /// The state machine behind [`update`](Self::update).
    pub fn step(&mut self, alt: bool, other_input: bool, escape: bool) {
        if alt {
            if !self.alt_down {
                self.alt_down = true;
                self.alt_used = false;
            }
            if other_input {
                self.alt_used = true;
            }
        } else if self.alt_down {
            self.alt_down = false;
            if !self.alt_used {
                self.revealed = !self.revealed;
            }
        }
        if escape {
            self.revealed = false;
        }
    }

    /// Hide again (call after a menu item was chosen).
    pub fn close(&mut self) {
        self.revealed = false;
    }

    pub fn is_revealed(&self) -> bool {
        self.revealed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tap_alt_toggles() {
        let mut b = AltMenuBar::default();
        b.step(true, false, false);
        assert!(!b.is_revealed(), "only on release");
        b.step(false, false, false);
        assert!(b.is_revealed());
        b.step(true, false, false);
        b.step(false, false, false);
        assert!(!b.is_revealed());
    }

    #[test]
    fn alt_as_modifier_does_not_toggle() {
        let mut b = AltMenuBar::default();
        b.step(true, false, false);
        b.step(true, true, false); // Alt+something
        b.step(false, false, false);
        assert!(!b.is_revealed());
    }

    #[test]
    fn escape_and_close_hide() {
        let mut b = AltMenuBar::default();
        b.step(true, false, false);
        b.step(false, false, false);
        b.step(false, false, true);
        assert!(!b.is_revealed());
        b.step(true, false, false);
        b.step(false, false, false);
        b.close();
        assert!(!b.is_revealed());
    }
}
