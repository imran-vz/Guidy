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
    /// The user's email addresses (personal, work, ...). An attendee with
    /// any of them is the user.
    pub user_emails: Vec<UserEmail>,
    /// Cosine similarity at or above which a speaker cluster is matched to a
    /// known person automatically. Below it the match is shown as "Confirm?".
    pub speaker_match_threshold: f32,
    /// Cosine similarity below which a match is not suggested at all.
    pub speaker_suggest_threshold: f32,
    /// LLM provider used for meeting summaries (an id from
    /// `post_process_providers`, or [`crate::meetings::settings::BUILTIN_LLM_PROVIDER_ID`]).
    /// Per feature: dictation post-processing keeps its own provider.
    pub llm_provider_id: String,
    /// The cloud provider last chosen for summaries, restored when the user
    /// switches back from an on-device model. Empty: none chosen yet.
    pub llm_cloud_provider_id: String,
    /// Model per provider for meeting summaries (provider id -> model id).
    pub llm_models: std::collections::HashMap<String, String>,
    /// Remembered Linear defaults between meetings.
    pub linear_default_team_id: Option<String>,
    pub linear_default_project_id: Option<String>,
}

/// One of the user's email addresses, with a free-text label ("Work").
#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct UserEmail {
    pub label: String,
    pub email: String,
}

/// Manual `Debug` so logs never contain the address.
impl std::fmt::Debug for UserEmail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

impl UserEmail {
    /// Whether `email` is this address (trimmed, case-insensitive).
    pub fn matches(&self, email: &str) -> bool {
        let email = email.trim();
        !email.is_empty() && self.email.trim().eq_ignore_ascii_case(email)
    }
}

impl MeetingSettings {
    /// Whether `email` is one of the user's addresses (case-insensitive).
    pub fn is_user_email(&self, email: &str) -> bool {
        self.user_emails.iter().any(|u| u.matches(email))
    }
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
            user_emails: Vec::new(),
            speaker_match_threshold: 0.65,
            speaker_suggest_threshold: 0.45,
            llm_provider_id: BUILTIN_LLM_PROVIDER_ID.to_string(),
            llm_cloud_provider_id: String::new(),
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
            .field("user_emails", &self.user_emails)
            .field("speaker_match_threshold", &self.speaker_match_threshold)
            .field("speaker_suggest_threshold", &self.speaker_suggest_threshold)
            .field("llm_provider_id", &self.llm_provider_id)
            .field("llm_cloud_provider_id", &self.llm_cloud_provider_id)
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
            user_emails: vec![UserEmail {
                label: "Work".into(),
                email: "imran@secret.example".into(),
            }],
            ..Default::default()
        };
        let dbg = format!("{s:?}");
        assert!(!dbg.contains("Imran Secret"), "{dbg}");
        assert!(!dbg.contains("secret.example"), "{dbg}");
        assert!(dbg.contains("<redacted>"));
        assert!(dbg.contains("speaker_match_threshold"));
    }

    #[test]
    fn user_email_match_ignores_case_and_blanks() {
        let s = MeetingSettings {
            user_emails: vec![
                UserEmail {
                    label: "Personal".into(),
                    email: "me@home.example".into(),
                },
                UserEmail {
                    label: "Work".into(),
                    email: "Me@Work.example".into(),
                },
            ],
            ..Default::default()
        };
        assert!(s.is_user_email(" me@work.EXAMPLE "));
        assert!(s.is_user_email("me@home.example"));
        assert!(!s.is_user_email("other@work.example"));
        assert!(!s.is_user_email(""));
    }
}
