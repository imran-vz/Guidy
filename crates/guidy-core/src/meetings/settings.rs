//! Meeting-notes settings, stored as the nested `meetings` object in
//! `settings_store.json`. Secrets (Linear key, provider keys) never live here:
//! they are in the Keychain through [`crate::secrets`].

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
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
