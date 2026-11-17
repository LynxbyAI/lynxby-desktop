//! The message box, as a value.
//!
//! Kept out of the view so the editing rules (the ones a person notices when
//! they are wrong) are tested without a window.

/// The deployment rejects anything longer, so the composer stops first.
pub const MAX_MESSAGE_CHARS: usize = 4000;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Composer {
    text: String,
}

impl Composer {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    pub fn len(&self) -> usize {
        self.text.chars().count()
    }

    pub fn is_full(&self) -> bool {
        self.len() >= MAX_MESSAGE_CHARS
    }

    pub fn can_send(&self) -> bool {
        !self.is_empty() && !self.is_full()
    }

    /// Ignores control characters, which arrive as keystrokes but are not text.
    pub fn insert(&mut self, value: &str) {
        for character in value.chars() {
            if character.is_control() || self.is_full() {
                continue;
            }

            self.text.push(character);
        }
    }

    /// One character, never half of one: `pop` keeps the string valid UTF-8.
    pub fn backspace(&mut self) {
        self.text.pop();
    }

    /// Empties the box and returns what was in it, trimmed. Empty when there
    /// was nothing worth sending.
    pub fn take(&mut self) -> String {
        let message = self.text.trim().to_string();
        self.text.clear();

        message
    }

    pub fn clear(&mut self) {
        self.text.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_empty_and_cannot_send() {
        let composer = Composer::default();

        assert!(composer.is_empty());
        assert!(!composer.can_send());
    }

    #[test]
    fn typing_makes_it_sendable() {
        let mut composer = Composer::default();
        composer.insert("Hello");

        assert_eq!(composer.text(), "Hello");
        assert!(composer.can_send());
    }

    #[test]
    fn whitespace_alone_is_not_a_message() {
        let mut composer = Composer::default();
        composer.insert("   \n\t ");

        assert!(composer.is_empty());
        assert!(!composer.can_send());
    }

    #[test]
    fn backspace_removes_one_character() {
        let mut composer = Composer::default();
        composer.insert("abc");
        composer.backspace();

        assert_eq!(composer.text(), "ab");
    }

    #[test]
    fn backspace_on_empty_does_nothing() {
        let mut composer = Composer::default();
        composer.backspace();

        assert_eq!(composer.text(), "");
    }

    #[test]
    fn backspace_keeps_multibyte_characters_whole() {
        let mut composer = Composer::default();
        composer.insert("café");
        composer.backspace();

        assert_eq!(composer.text(), "caf");
    }

    #[test]
    fn control_characters_are_not_text() {
        let mut composer = Composer::default();
        composer.insert("a\u{0}b\u{7}");

        assert_eq!(composer.text(), "ab");
    }

    #[test]
    fn take_returns_the_trimmed_message_and_empties_the_box() {
        let mut composer = Composer::default();
        composer.insert("  hello  ");

        assert_eq!(composer.take(), "hello");
        assert!(composer.is_empty());
    }

    #[test]
    fn take_on_whitespace_returns_nothing() {
        let mut composer = Composer::default();
        composer.insert("  ");

        assert_eq!(composer.take(), "");
    }

    #[test]
    fn it_stops_at_the_deployment_limit() {
        let mut composer = Composer::default();
        composer.insert(&"a".repeat(MAX_MESSAGE_CHARS + 50));

        assert_eq!(composer.len(), MAX_MESSAGE_CHARS);
        assert!(composer.is_full());
        assert!(!composer.can_send());
    }

    #[test]
    fn it_becomes_sendable_again_after_backspacing_below_the_limit() {
        let mut composer = Composer::default();
        composer.insert(&"a".repeat(MAX_MESSAGE_CHARS));
        composer.backspace();

        assert!(composer.can_send());
    }
}
