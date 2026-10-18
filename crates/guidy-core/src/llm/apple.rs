//! [`AppleProvider`]: Apple Intelligence's on-device model as an
//! [`LlmProvider`] (macOS 26+, Apple silicon). Its context is about 4k
//! tokens, so meeting transcripts are summarized in small chunks.

use anyhow::Result;

use super::{ChatMessage, ChatOptions, LlmProvider, Role};
use crate::settings::{APPLE_INTELLIGENCE_DEFAULT_MODEL_ID, APPLE_INTELLIGENCE_PROVIDER_ID};

/// Apple's on-device model context window (prompt + output).
pub const APPLE_CONTEXT_TOKENS: usize = 4096;

pub struct AppleProvider {
    _private: (),
}

impl AppleProvider {
    /// Fails when Apple Intelligence isn't available on this Mac.
    pub fn new() -> Result<Self> {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            if !crate::apple_intelligence::check_apple_intelligence_availability() {
                anyhow::bail!(
                    "Apple Intelligence is not available. Turn it on in System Settings, or pick another model."
                );
            }
            Ok(Self { _private: () })
        }
        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        {
            anyhow::bail!("Apple Intelligence is only available on Apple silicon Macs")
        }
    }
}

/// Split messages into the bridge's (system prompt, user content). Later
/// turns are labelled so a retry conversation still reads correctly.
pub fn flatten_messages(messages: &[ChatMessage]) -> (String, String) {
    let system: Vec<&str> = messages
        .iter()
        .filter(|m| m.role == Role::System)
        .map(|m| m.content.as_str())
        .collect();
    let turns: Vec<&ChatMessage> = messages.iter().filter(|m| m.role != Role::System).collect();
    let user = if turns.len() == 1 {
        turns[0].content.clone()
    } else {
        turns
            .iter()
            .map(|m| match m.role {
                Role::Assistant => format!("Your previous reply:\n{}", m.content),
                _ => m.content.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    (system.join("\n\n"), user)
}

impl LlmProvider for AppleProvider {
    fn name(&self) -> String {
        format!("{APPLE_INTELLIGENCE_PROVIDER_ID}/{APPLE_INTELLIGENCE_DEFAULT_MODEL_ID}")
    }

    fn context_tokens(&self) -> usize {
        APPLE_CONTEXT_TOKENS
    }

    fn chat(&self, messages: &[ChatMessage], opts: &ChatOptions) -> Result<String> {
        if opts.is_cancelled() {
            anyhow::bail!(super::CANCELLED);
        }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            let (system, user) = flatten_messages(messages);
            let max_tokens = opts.max_tokens.unwrap_or(0) as i32;
            crate::apple_intelligence::process_text_with_system_prompt(&system, &user, max_tokens)
                .map(|s| super::strip_think(&s).to_string())
                .map_err(|e| anyhow::anyhow!("Apple Intelligence: {e}"))
        }
        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        {
            let _ = messages;
            anyhow::bail!("Apple Intelligence is only available on Apple silicon Macs")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_keeps_system_separate_and_labels_retries() {
        let (system, user) = flatten_messages(&[
            ChatMessage::system("rules"),
            ChatMessage::user("transcript"),
        ]);
        assert_eq!(system, "rules");
        assert_eq!(user, "transcript");

        let (_, user) = flatten_messages(&[
            ChatMessage::system("rules"),
            ChatMessage::user("transcript"),
            ChatMessage::assistant("bad"),
            ChatMessage::user("fix it"),
        ]);
        assert!(user.starts_with("transcript"));
        assert!(user.contains("Your previous reply:\nbad"));
        assert!(user.ends_with("fix it"));
    }
}
