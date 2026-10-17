//! Meeting-notes settings, stored as the nested `meetings` object in
//! `settings_store.json`. Secrets (Linear key, provider keys) never live here:
//! they are in the Keychain through [`crate::secrets`].

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct MeetingSettings {
    /// Master switch. Turning it on is what requests calendar, system-audio
    /// and notification permissions (never during onboarding).
    pub enabled: bool,
    /// Transcription model id used for meetings (batch, word timestamps).
    /// `None` = the default meeting model (Parakeet v3).
    pub meeting_model: Option<String>,
    /// Days to keep raw meeting audio: 3, 7 or 30. `None` = forever.
    /// Deleting audio never deletes voiceprints.
    pub audio_retention_days: Option<u32>,
    /// Post "Take notes for <event>?" at a calendar event's start time.
    pub calendar_notifications: bool,
    /// Minutes between "still recording" reminders.
    pub reminder_interval_minutes: u32,
    /// Name the mic track (always the user) is labelled with.
    pub user_display_name: String,
    /// The user's email, used to match "me" to a Linear user.
    pub user_email: Option<String>,
    /// Cosine similarity at or above which a speaker cluster is matched to a
    /// known person automatically. Below it the match is shown as "Confirm?".
    pub speaker_match_threshold: f32,
    /// Cosine similarity below which a match is not suggested at all.
    pub speaker_suggest_threshold: f32,
    /// LLM provider used for meeting summaries (an id from
    /// `post_process_providers`, or [`crate::meetings::settings::BUILTIN_LLM_PROVIDER_ID`]).
    /// Per feature: dictation post-processing keeps its own provider.
    pub llm_provider_id: String,
    /// Model per provider for meeting summaries (provider id -> model id).
    pub llm_models: std::collections::HashMap<String, String>,
    /// Remembered Linear defaults between meetings.
    pub linear_default_team_id: Option<String>,
    pub linear_default_project_id: Option<String>,
}

/// Provider id of the built-in llama.cpp runtime (the `guidy-llm` helper).
pub const BUILTIN_LLM_PROVIDER_ID: &str = "guidy_local";

pub const RETENTION_CHOICES: [Option<u32>; 4] = [Some(3), Some(7), Some(30), None];

impl Default for MeetingSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            meeting_model: None,
            audio_retention_days: Some(3),
            calendar_notifications: true,
            reminder_interval_minutes: 10,
            user_display_name: String::new(),
            user_email: None,
            speaker_match_threshold: 0.65,
            speaker_suggest_threshold: 0.45,
            llm_provider_id: BUILTIN_LLM_PROVIDER_ID.to_string(),
            llm_models: Default::default(),
            linear_default_team_id: None,
            linear_default_project_id: None,
        }
    }
}

/// Manual `Debug` so logs never contain the user's name or email.
impl std::fmt::Debug for MeetingSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redact = |empty: bool| if empty { "<empty>" } else { "<redacted>" };
        f.debug_struct("MeetingSettings")
            .field("enabled", &self.enabled)
            .field("meeting_model", &self.meeting_model)
            .field("audio_retention_days", &self.audio_retention_days)
            .field("calendar_notifications", &self.calendar_notifications)
            .field("reminder_interval_minutes", &self.reminder_interval_minutes)
            .field(
                "user_display_name",
                &redact(self.user_display_name.is_empty()),
            )
            .field(
                "user_email",
                &self.user_email.as_ref().map(|_| "<redacted>"),
            )
            .field("speaker_match_threshold", &self.speaker_match_threshold)
            .field("speaker_suggest_threshold", &self.speaker_suggest_threshold)
            .field("llm_provider_id", &self.llm_provider_id)
            .field("llm_models", &self.llm_models)
            .field("linear_default_team_id", &self.linear_default_team_id)
            .field("linear_default_project_id", &self.linear_default_project_id)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_the_users_identity() {
        let s = MeetingSettings {
            user_display_name: "Imran Secret".into(),
            user_email: Some("imran@secret.example".into()),
            ..Default::default()
        };
        let dbg = format!("{s:?}");
        assert!(!dbg.contains("Imran Secret"), "{dbg}");
        assert!(!dbg.contains("secret.example"), "{dbg}");
        assert!(dbg.contains("<redacted>"));
        assert!(dbg.contains("speaker_match_threshold"));
    }
}
