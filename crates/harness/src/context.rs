use super::{Message, Role};

/// Keeps the system prompt and the user instruction under a character budget.
///
/// The harness is provider-agnostic, so it budgets characters rather than
/// tokens. A provider adapter that knows its tokenizer may pass a lower
/// character budget to stay inside the model window.
pub struct ContextBudget {
    max_chars: usize,
}

impl ContextBudget {
    pub fn new(max_chars: usize) -> Self {
        Self { max_chars }
    }

    pub fn chars(messages: &[Message]) -> usize {
        messages.iter().map(|message| message.content.chars().count()).sum()
    }

    /// Drop the oldest assistant/tool messages until the budget is met.
    /// System and user messages are never removed. Returns the count removed.
    pub fn trim(&self, messages: &mut Vec<Message>) -> usize {
        let mut removed = 0;
        while Self::chars(messages) > self.max_chars {
            let Some(index) = messages
                .iter()
                .position(|message| message.role == Role::Assistant || message.role == Role::Tool)
            else {
                break;
            };
            messages.remove(index);
            removed += 1;
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Message;

    fn conversation(count: usize) -> Vec<Message> {
        let mut messages = vec![Message::system("system rules"), Message::user("the instruction")];
        for index in 0..count {
            messages.push(Message::assistant(format!("assistant {index} {}", "x".repeat(50))));
            messages.push(Message::tool(format!("tool {index} {}", "y".repeat(50))));
        }
        messages
    }

    #[test]
    fn trimming_keeps_the_system_prompt_and_user_instruction() {
        let mut messages = conversation(20);
        let removed = ContextBudget::new(400).trim(&mut messages);
        assert!(removed > 0);
        assert_eq!(messages[0].content, "system rules");
        assert_eq!(messages[1].content, "the instruction");
        assert!(ContextBudget::chars(&messages) <= 400);
    }

    #[test]
    fn trimming_stops_when_only_protected_messages_remain() {
        let mut messages = vec![Message::system("a".repeat(300)), Message::user("b".repeat(300))];
        let removed = ContextBudget::new(10).trim(&mut messages);
        assert_eq!(removed, 0);
        assert_eq!(messages.len(), 2);
    }
}
