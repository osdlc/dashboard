//! A small text field: typing, moving the caret, delete and paste. The view
//! keeps one keyboard focus and sends keys to the active field. There is no
//! selection or IME composition.

use gpui::KeyDownEvent;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextField {
    pub text: String,
    /// Byte offset, always on a character boundary.
    pub caret: usize,
}

impl TextField {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let caret = text.len();
        Self { text, caret }
    }

    pub fn insert(&mut self, text: &str) {
        self.text.insert_str(self.caret, text);
        self.caret += text.len();
    }

    fn before(&self) -> usize {
        self.text[..self.caret]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn after(&self) -> usize {
        self.text[self.caret..]
            .chars()
            .next()
            .map_or(self.caret, |c| self.caret + c.len_utf8())
    }

    /// Applies a key. Returns true when the field used it. `multiline` fields
    /// take Enter as a new line.
    pub fn key(&mut self, event: &KeyDownEvent, multiline: bool) -> bool {
        let keystroke = &event.keystroke;
        match keystroke.key.as_str() {
            "backspace" => {
                let at = self.before();
                self.text.replace_range(at..self.caret, "");
                self.caret = at;
            }
            "delete" => {
                let to = self.after();
                self.text.replace_range(self.caret..to, "");
            }
            "left" => self.caret = self.before(),
            "right" => self.caret = self.after(),
            "home" => self.caret = 0,
            "end" => self.caret = self.text.len(),
            "enter" if multiline => self.insert("\n"),
            "enter" | "tab" | "escape" | "up" | "down" | "pageup" | "pagedown" => return false,
            _ => {
                let modifiers = &keystroke.modifiers;
                if modifiers.platform || modifiers.control {
                    return false;
                }
                let Some(text) = &keystroke.key_char else {
                    return false;
                };
                self.insert(&text.clone());
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_at_the_caret_on_character_boundaries() {
        let mut field = TextField::new("aé");
        assert_eq!(field.caret, 3);
        field.caret = field.before();
        assert_eq!(field.caret, 1);
        field.insert("z");
        assert_eq!(field.text, "azé");
        assert_eq!(field.after(), 4);
    }
}
