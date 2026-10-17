//! [`MeetingsManager`]: the one facade the app (UI, tray, CLI) calls for
//! meeting notes. It is registered with `app_handle.manage(Arc<MeetingsManager>)`
//! at startup.
//!
//! Each section below is owned by one workstream (docs/MEETING_NOTES_ARCH.md).
//! Signatures are the contract the UI codes against: change them only together
//! with the UI. Until a section is implemented its methods return
//! [`not_ready`].

#![allow(unused_variables)]

use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::host::AppHandle;
use crate::meetings::types::*;

/// Who a speaker is being assigned to.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum PersonRef {
    Existing(PersonId),
    New { display_name: String, email: Option<String> },
}

/// A suggestion in the Speaker N -> name picker.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct NameSuggestion {
    pub display_name: String,
    pub email: Option<String>,
    /// Set when the suggestion is an existing person.
    pub person_id: Option<PersonId>,
    pub source: SuggestionSource,
    /// Voice similarity to this speaker, when known.
    pub similarity: Option<f32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionSource {
    /// From the linked calendar event's attendees (listed first).
    Attendee,
    /// From the People list.
    KnownPerson,
}

/// Result of turning meeting notes on (permission prompts).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct PermissionReport {
    pub calendar_granted: bool,
    pub system_audio_granted: bool,
    pub microphone_granted: bool,
    pub notifications_granted: bool,
}

/// Outcome of creating one approved ticket.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TicketResult {
    pub action_item_id: ActionItemId,
    /// `Ok(identifier)` e.g. "ENG-123", or the error message.
    pub result: std::result::Result<String, String>,
    /// True when the issue already existed and nothing new was created.
    pub already_existed: bool,
}

pub fn not_ready(what: &str) -> anyhow::Error {
    anyhow::anyhow!("meeting notes: {what} is not implemented yet")
}

pub struct MeetingsManager {
    app: AppHandle,
    // Owners add their state here (store, recorder, processing jobs, ...).
}

impl MeetingsManager {
    pub fn new(app: &AppHandle) -> Result<Arc<Self>> {
        Ok(Arc::new(Self { app: app.clone() }))
    }

    pub fn app(&self) -> &AppHandle {
        &self.app
    }

    // ===== Enablement & permissions (owner: native/B) ======================

    /// Request calendar, system-audio, mic and notification permissions. Called
    /// the first time the user turns meeting notes on.
    pub fn request_permissions(&self) -> Result<PermissionReport> {
        Err(not_ready("permissions"))
    }

    pub fn permission_status(&self) -> PermissionReport {
        PermissionReport::default()
    }

    // ===== Calendar (owner: native/B) =======================================

    /// Events from now-1h to now+24h that have a meeting link.
    pub fn upcoming_events(&self) -> Result<Vec<CalendarEvent>> {
        Err(not_ready("calendar"))
    }

    // ===== Recording (owner: capture/C) =====================================

    pub fn recorder_state(&self) -> RecorderState {
        RecorderState::Idle
    }

    /// Start recording. `calendar_event_id: None` links to whichever calendar
    /// event is happening now, if there is one.
    pub fn start_recording(&self, calendar_event_id: Option<String>) -> Result<MeetingId> {
        Err(not_ready("recording"))
    }

    /// Stop recording, finalize the track files, and queue processing.
    pub fn stop_recording(&self) -> Result<Option<MeetingId>> {
        Err(not_ready("recording"))
    }

    /// Decode a stretch of one track for playback (16 kHz mono f32).
    pub fn segment_audio(
        &self,
        meeting_id: MeetingId,
        track: Track,
        t0_ms: i64,
        t1_ms: i64,
    ) -> Result<Vec<f32>> {
        Err(not_ready("audio playback"))
    }

    /// Delete raw audio past the retention period. Never touches voiceprints.
    pub fn apply_retention(&self) -> Result<usize> {
        Err(not_ready("retention"))
    }

    // ===== Meetings & people data (owner: store/A) ==========================

    pub fn list_meetings(&self) -> Result<Vec<Meeting>> {
        Err(not_ready("meetings list"))
    }

    pub fn get_meeting(&self, id: MeetingId) -> Result<Option<Meeting>> {
        Err(not_ready("meetings"))
    }

    pub fn rename_meeting(&self, id: MeetingId, title: &str) -> Result<()> {
        Err(not_ready("meetings"))
    }

    /// Deletes the meeting, its transcript and audio. Voiceprints stay.
    pub fn delete_meeting(&self, id: MeetingId) -> Result<()> {
        Err(not_ready("meetings"))
    }

    pub fn segments(&self, id: MeetingId) -> Result<Vec<Segment>> {
        Err(not_ready("segments"))
    }

    pub fn speakers(&self, id: MeetingId) -> Result<Vec<MeetingSpeaker>> {
        Err(not_ready("speakers"))
    }

    pub fn list_people(&self) -> Result<Vec<Person>> {
        Err(not_ready("people"))
    }

    pub fn rename_person(&self, id: PersonId, display_name: &str, email: Option<&str>) -> Result<()> {
        Err(not_ready("people"))
    }

    /// Move everything (voiceprints, speaker links, action-item owners) from
    /// `from` into `into`, then delete `from`.
    pub fn merge_people(&self, from: PersonId, into: PersonId) -> Result<()> {
        Err(not_ready("people"))
    }

    /// Deletes the person and their voiceprints.
    pub fn delete_person(&self, id: PersonId) -> Result<()> {
        Err(not_ready("people"))
    }

    // ===== Speakers & processing (owner: processing/D) ======================

    /// Queue (or resume) processing. Progress arrives as
    /// [`events::PROCESSING_PROGRESS`].
    pub fn process_meeting(&self, id: MeetingId) -> Result<()> {
        Err(not_ready("processing"))
    }

    pub fn cancel_processing(&self, id: MeetingId) -> Result<()> {
        Err(not_ready("processing"))
    }

    pub fn processing_progress(&self, id: MeetingId) -> Option<ProcessingProgress> {
        None
    }

    /// Attendees first, then known people (by voice similarity).
    pub fn speaker_suggestions(&self, speaker_id: SpeakerId) -> Result<Vec<NameSuggestion>> {
        Err(not_ready("speaker naming"))
    }

    /// Name a speaker. Stores that speaker's voiceprints under the person and
    /// marks the speaker confirmed. Returns the person id.
    pub fn assign_speaker(&self, speaker_id: SpeakerId, person: PersonRef) -> Result<PersonId> {
        Err(not_ready("speaker naming"))
    }

    /// Accept an uncertain automatic match ("Confirm?").
    pub fn confirm_speaker(&self, speaker_id: SpeakerId) -> Result<()> {
        Err(not_ready("speaker naming"))
    }

    // ===== Summaries & action items (owner: llm/E) ==========================

    /// Summarize with the meetings provider/model from settings, or the given
    /// `(provider_id, model)` override. Replaces the stored summary and
    /// action items that have no Linear issue yet.
    pub fn summarize(
        &self,
        id: MeetingId,
        provider_override: Option<(String, String)>,
    ) -> Result<StoredSummary> {
        Err(not_ready("summaries"))
    }

    pub fn summary(&self, id: MeetingId) -> Result<Option<StoredSummary>> {
        Err(not_ready("summaries"))
    }

    pub fn update_summary(&self, summary: &StoredSummary) -> Result<()> {
        Err(not_ready("summaries"))
    }

    pub fn action_items(&self, id: MeetingId) -> Result<Vec<ActionItem>> {
        Err(not_ready("action items"))
    }

    pub fn update_action_item(&self, item: &ActionItem) -> Result<()> {
        Err(not_ready("action items"))
    }

    pub fn add_action_item(&self, meeting_id: MeetingId, text: &str) -> Result<ActionItem> {
        Err(not_ready("action items"))
    }

    pub fn delete_action_item(&self, id: ActionItemId) -> Result<()> {
        Err(not_ready("action items"))
    }

    // ===== Linear (owner: linear/F) =========================================

    pub fn set_linear_api_key(&self, key: &str) -> Result<()> {
        Err(not_ready("Linear"))
    }

    pub fn has_linear_api_key(&self) -> bool {
        false
    }

    /// Returns the authenticated user's name.
    pub fn test_linear(&self) -> Result<String> {
        Err(not_ready("Linear"))
    }

    pub fn linear_teams(&self) -> Result<Vec<crate::linear::Team>> {
        Err(not_ready("Linear"))
    }

    pub fn linear_projects(&self, team_id: &str) -> Result<Vec<crate::linear::Project>> {
        Err(not_ready("Linear"))
    }

    pub fn linear_users(&self) -> Result<Vec<crate::linear::User>> {
        Err(not_ready("Linear"))
    }

    pub fn linear_labels(&self, team_id: &str) -> Result<Vec<crate::linear::Label>> {
        Err(not_ready("Linear"))
    }

    /// Fill each action item's `ticket` draft (title, description with quote
    /// and meeting link, remembered team/project, assignee matched by email,
    /// else the user) where it is still empty. Returns the updated items.
    pub fn draft_tickets(&self, meeting_id: MeetingId) -> Result<Vec<ActionItem>> {
        Err(not_ready("Linear"))
    }

    /// Create Linear issues for the given approved items. Items that already
    /// have an issue are skipped (`already_existed`), so retries never
    /// duplicate.
    pub fn create_tickets(&self, ids: &[ActionItemId]) -> Result<Vec<TicketResult>> {
        Err(not_ready("Linear"))
    }
}
