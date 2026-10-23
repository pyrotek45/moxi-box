use crate::domain::{ChatMessage, MessageRole};

#[derive(Default)]
pub(super) struct PromptHistory {
    entries: Vec<String>,
    cursor: Option<usize>,
    draft: String,
}

impl PromptHistory {
    pub fn from_messages(messages: &[ChatMessage]) -> Self {
        let mut history = Self::default();
        for message in messages.iter().filter(|m| m.role == MessageRole::User) {
            history.record(&message.content);
        }
        history
    }
    pub fn record(&mut self, text: &str) {
        if !text.trim().is_empty() && self.entries.last().is_none_or(|entry| entry != text) {
            self.entries.push(text.to_owned());
        }
        self.cursor = None;
        self.draft.clear();
    }
    pub fn browsing(&self) -> bool {
        self.cursor.is_some()
    }
    pub fn edited(&mut self) {
        self.cursor = None;
        self.draft.clear();
    }
    pub fn navigate(&mut self, text: &mut String, older: bool) {
        if self.entries.is_empty() {
            return;
        }
        if older {
            let index = match self.cursor {
                None => {
                    self.draft = text.clone();
                    self.entries.len() - 1
                }
                Some(index) => index.saturating_sub(1),
            };
            self.cursor = Some(index);
            *text = self.entries[index].clone();
        } else if let Some(index) = self.cursor {
            if index + 1 == self.entries.len() {
                *text = std::mem::take(&mut self.draft);
                self.cursor = None;
            } else {
                self.cursor = Some(index + 1);
                *text = self.entries[index + 1].clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_recall_prompts_and_restore_unsent_draft() {
        let mut history = PromptHistory::default();
        history.record("first");
        history.record("second\nwith two lines");
        let mut text = "unfinished draft".to_owned();
        history.navigate(&mut text, true);
        assert_eq!(text, "second\nwith two lines");
        history.navigate(&mut text, true);
        history.navigate(&mut text, true);
        assert_eq!(text, "first");
        history.navigate(&mut text, false);
        history.navigate(&mut text, false);
        assert_eq!(text, "unfinished draft");
        assert!(!history.browsing());
        history.navigate(&mut text, false);
        assert_eq!(text, "unfinished draft");
    }
}
