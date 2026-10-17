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
use crate::meetings::store::MeetingsStore;
use crate::meetings::types::*;

/// Who a speaker is being assigned to.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum PersonRef {
    Existing(PersonId),
    New {
        display_name: String,
        email: Option<String>,
    },
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
    /// The encrypted `meetings.db` (owner: store/A).
    store: Arc<MeetingsStore>,
    /// `<app_data_dir>/meetings`.
    meetings_dir: std::path::PathBuf,
    /// Recorder + scheduler (owner: capture/C).
    capture: crate::meetings::recorder::CaptureState,
    /// Linear key, cached lists and the create guard (owner: linear/F).
    linear: crate::linear::service::LinearService,
    /// Processing queue and jobs (owner: processing/D).
    processing: crate::meetings::processing::Processor,
    /// Meetings with a `create_tickets` call running (count per meeting).
    /// `summarize` refuses to start or to store while its meeting is here.
    ticket_creations: parking_lot::Mutex<std::collections::HashMap<MeetingId, usize>>,
    // Owners add their state here (store, recorder, processing jobs, ...).
}

impl MeetingsManager {
    pub fn new(app: &AppHandle) -> Result<Arc<Self>> {
        let data_dir =
            crate::portable::app_data_dir(app).map_err(|e| anyhow::anyhow!("app data dir: {e}"))?;
        let key = crate::secrets::meetings_data_key()?;
        let store = Arc::new(MeetingsStore::open(
            &data_dir.join(crate::meetings::store::MEETINGS_DB_FILE),
            &key,
        )?);
        let meetings_dir = data_dir.join("meetings");
        let processing =
            crate::meetings::processing::Processor::new(app, store.clone(), meetings_dir.clone());
        Ok(Arc::new(Self {
            app: app.clone(),
            store,
            meetings_dir,
            capture: Default::default(),
            linear: Default::default(),
            processing,
            ticket_creations: Default::default(),
        }))
    }

    pub fn app(&self) -> &AppHandle {
        &self.app
    }

    /// The meetings database, for the other layers (recorder, processing,
    /// summaries, Linear).
    pub fn store(&self) -> &Arc<MeetingsStore> {
        &self.store
    }

    /// `<app_data_dir>/meetings`.
    pub fn meetings_dir(&self) -> &std::path::Path {
        &self.meetings_dir
    }

    /// `<app_data_dir>/meetings/audio`: `Meeting::*_audio_path` are relative
    /// to this.
    pub fn audio_dir(&self) -> std::path::PathBuf {
        self.meetings_dir.join("audio")
    }

    fn emit_meeting_changed(&self, id: MeetingId) {
        if let Err(e) = self.app.emit(events::MEETING_CHANGED, id) {
            log::warn!("emit {}: {e}", events::MEETING_CHANGED);
        }
    }

    fn emit_people_changed(&self) {
        if let Err(e) = self.app.emit(events::PEOPLE_CHANGED, ()) {
            log::warn!("emit {}: {e}", events::PEOPLE_CHANGED);
        }
    }

    // ===== Enablement & permissions (owner: native/B) ======================

    /// Request calendar, system-audio, mic and notification permissions. Called
    /// the first time the user turns meeting notes on.
    ///
    /// Blocks until every prompt is answered: call it off the UI thread. Each
    /// permission is asked independently; the report says which were granted.
    pub fn request_permissions(&self) -> Result<PermissionReport> {
        crate::meetings::native::request_permissions()
    }

    /// Current grants, without prompting.
    pub fn permission_status(&self) -> PermissionReport {
        crate::meetings::native::permission_status()
    }

    // ===== Calendar (owner: native/B) =======================================

    /// Events from now-1h to now+24h that have a meeting link.
    pub fn upcoming_events(&self) -> Result<Vec<CalendarEvent>> {
        crate::meetings::native::upcoming_events()
    }

    // ===== Recording (owner: capture/C) =====================================

    pub fn recorder_state(&self) -> RecorderState {
        self.capture
            .recorder
            .get()
            .map(|r| r.state())
            .unwrap_or_default()
    }

    /// The recording session state machine (created on first use).
    pub fn recorder(&self) -> &Arc<crate::meetings::recorder::MeetingRecorder> {
        self.capture.recorder.get_or_init(|| {
            Arc::new(crate::meetings::recorder::adapters::app_recorder(
                &self.app,
                self.store.clone(),
                self.audio_dir(),
            ))
        })
    }

    /// Start recording. `calendar_event_id: None` links to whichever calendar
    /// event is happening now, if there is one.
    pub fn start_recording(&self, calendar_event_id: Option<String>) -> Result<MeetingId> {
        if !crate::meetings::is_supported() {
            anyhow::bail!("meeting notes need macOS 14.2 or later");
        }
        let id = self
            .recorder()
            .start(calendar_event_id, crate::meetings::recorder::now_ms())?;
        // Reminders need the scheduler even if calendar prompts are off.
        self.sync_scheduler();
        Ok(id)
    }

    /// Stop recording, finalize the track files, and queue processing.
    pub fn stop_recording(&self) -> Result<Option<MeetingId>> {
        let Some(recorder) = self.capture.recorder.get() else {
            return Ok(None);
        };
        let stopped = recorder.stop(crate::meetings::recorder::now_ms())?;
        if let Some(id) = stopped {
            if let Err(e) = self.process_meeting(id) {
                log::info!("meetings: processing of meeting {id} not queued: {e:#}");
            }
        }
        self.sync_scheduler();
        Ok(stopped)
    }

    /// Decode a stretch of one track for playback (16 kHz mono f32).
    pub fn segment_audio(
        &self,
        meeting_id: MeetingId,
        track: Track,
        t0_ms: i64,
        t1_ms: i64,
    ) -> Result<Vec<f32>> {
        self.recorder()
            .segment_audio(meeting_id, track, t0_ms, t1_ms)
    }

    /// Delete raw audio past the retention period. Never touches voiceprints.
    pub fn apply_retention(&self) -> Result<usize> {
        let processing = &self.processing;
        let n = self
            .recorder()
            .apply_retention(crate::meetings::recorder::now_ms(), &|id| {
                processing.is_busy(id)
            })?;
        if n > 0 {
            log::info!("meetings: retention deleted {n} audio file(s)");
        }
        Ok(n)
    }

    /// Run the reminder / calendar-prompt scheduler when meeting notes are
    /// supported and enabled (or a recording needs reminders); stop it
    /// otherwise. Call after the `meetings` settings change.
    pub fn sync_scheduler(&self) {
        let enabled = crate::settings::get_settings(&self.app).meetings.enabled;
        let want = crate::meetings::is_supported()
            && (enabled || self.recorder_state() != RecorderState::Idle);
        let mut scheduler = self.capture.scheduler.lock();
        match (want, scheduler.is_some()) {
            (true, false) => {
                match crate::meetings::recorder::Scheduler::start(self.recorder().clone()) {
                    Ok(s) => *scheduler = Some(s),
                    Err(e) => log::error!("meetings: scheduler failed to start: {e:#}"),
                }
            }
            (false, true) => *scheduler = None,
            _ => {}
        }
    }

    /// A response to one of our notifications ("Stop", "Keep going",
    /// "Start").
    pub fn handle_notification(&self, notification_id: &str, action_id: &str) {
        use crate::meetings::recorder::{parse_notification, NotificationCommand};
        match parse_notification(notification_id, action_id) {
            NotificationCommand::Stop(id) => {
                if self.recorder().current_meeting_id() == Some(id) {
                    if let Err(e) = self.stop_recording() {
                        log::error!("meetings: stop from notification failed: {e:#}");
                    }
                }
            }
            NotificationCommand::KeepGoing(id) => self
                .recorder()
                .snooze(id, crate::meetings::recorder::now_ms()),
            NotificationCommand::Start(event_id) => {
                if let Err(e) = self.start_recording(Some(event_id)) {
                    log::error!("meetings: start from notification failed: {e:#}");
                }
            }
            NotificationCommand::Ignore => {}
        }
    }

    /// Startup work: route notification responses, start the scheduler,
    /// recover recordings interrupted by a crash, then apply retention now
    /// and once a day on a background thread.
    pub fn start_background(self: &Arc<Self>) {
        crate::meetings::recorder::adapters::install_notification_handler(&self.app);
        self.sync_scheduler();
        let this = Arc::downgrade(self);
        let spawned = std::thread::Builder::new()
            .name("meetings-maintenance".into())
            .spawn(move || {
                if let Some(m) = this.upgrade() {
                    // Recordings cut short by a crash are processed like a
                    // normal stop. If queueing fails (e.g. no meeting model)
                    // they stay `Recorded` and the UI offers "Process".
                    match m.recorder().recover_interrupted() {
                        Ok(ids) => {
                            for id in ids {
                                if let Err(e) = m.process_meeting(id) {
                                    log::info!(
                                        "meetings: recovered meeting {id} not queued: {e:#}"
                                    );
                                }
                            }
                        }
                        Err(e) => log::error!("meetings: crash recovery failed: {e:#}"),
                    }
                    if let Err(e) = m.resume_interrupted_processing() {
                        log::error!("meetings: resuming processing failed: {e:#}");
                    }
                }
                loop {
                    match this.upgrade() {
                        Some(m) => {
                            if let Err(e) = m.apply_retention() {
                                log::error!("meetings: retention failed: {e:#}");
                            }
                        }
                        None => return,
                    }
                    std::thread::sleep(std::time::Duration::from_secs(24 * 60 * 60));
                }
            });
        if let Err(e) = spawned {
            log::error!("meetings: maintenance thread failed to start: {e}");
        }
    }

    // ===== Meetings & people data (owner: store/A) ==========================

    pub fn list_meetings(&self) -> Result<Vec<Meeting>> {
        self.store.list_meetings()
    }

    pub fn get_meeting(&self, id: MeetingId) -> Result<Option<Meeting>> {
        self.store.get_meeting(id)
    }

    pub fn rename_meeting(&self, id: MeetingId, title: &str) -> Result<()> {
        let title = title.trim();
        if title.is_empty() {
            anyhow::bail!("a meeting title can't be empty");
        }
        self.store.set_meeting_title(id, title)?;
        self.emit_meeting_changed(id);
        Ok(())
    }

    /// Deletes the meeting, its transcript and audio. Voiceprints stay.
    pub fn delete_meeting(&self, id: MeetingId) -> Result<()> {
        let Some(meeting) = self.store.get_meeting(id)? else {
            return Ok(());
        };
        if meeting.status == MeetingStatus::Recording {
            anyhow::bail!("stop the recording before deleting the meeting");
        }
        // Cancels a running job and removes the per-segment voiceprint
        // sidecar: voiceprints are kept only for people the user named (D11).
        if let Err(e) = self.forget_processing(id) {
            log::warn!("meetings: clearing processing files for {id} failed: {e:#}");
        }
        self.store.delete_meeting(id)?;
        let audio_dir = self.audio_dir();
        for rel in [&meeting.mic_audio_path, &meeting.system_audio_path]
            .into_iter()
            .flatten()
        {
            delete_audio_file(&audio_dir, rel);
        }
        self.emit_meeting_changed(id);
        Ok(())
    }

    pub fn segments(&self, id: MeetingId) -> Result<Vec<Segment>> {
        self.store.segments(id)
    }

    pub fn speakers(&self, id: MeetingId) -> Result<Vec<MeetingSpeaker>> {
        self.store.speakers(id)
    }

    pub fn list_people(&self) -> Result<Vec<Person>> {
        self.store.list_people()
    }

    pub fn rename_person(
        &self,
        id: PersonId,
        display_name: &str,
        email: Option<&str>,
    ) -> Result<()> {
        let display_name = display_name.trim();
        if display_name.is_empty() {
            anyhow::bail!("a person's name can't be empty");
        }
        let email = email.map(str::trim).filter(|e| !e.is_empty());
        self.store.update_person(id, display_name, email)?;
        self.emit_people_changed();
        Ok(())
    }

    /// Move everything (voiceprints, speaker links, action-item owners) from
    /// `from` into `into`, then delete `from`.
    pub fn merge_people(&self, from: PersonId, into: PersonId) -> Result<()> {
        self.store.merge_people(from, into)?;
        self.emit_people_changed();
        Ok(())
    }

    /// Deletes the person and their voiceprints, including the speaker
    /// centroids linked to them and their per-segment voiceprints in the
    /// meetings' embeddings sidecars (found through the segments' speakers).
    pub fn delete_person(&self, id: PersonId) -> Result<()> {
        let segments = self.store.person_segments(id)?;
        self.store.delete_person(id)?;
        if !segments.is_empty() {
            match crate::secrets::meetings_data_key() {
                Ok(key) => {
                    for (meeting_id, ids) in &segments {
                        let ids: std::collections::HashSet<i64> = ids.iter().copied().collect();
                        if let Err(e) =
                            crate::meetings::processing::sidecar::drop_segment_embeddings(
                                &self.meetings_dir,
                                *meeting_id,
                                &key,
                                &ids,
                            )
                        {
                            log::warn!(
                                "meetings: dropping voiceprints from meeting {meeting_id}: {e:#}"
                            );
                        }
                    }
                }
                Err(e) => log::warn!("meetings: voiceprint sidecars not cleaned: {e:#}"),
            }
            for meeting_id in segments.keys() {
                self.emit_meeting_changed(*meeting_id);
            }
        }
        self.emit_people_changed();
        Ok(())
    }

    // ===== Speakers & processing (owner: processing/D) ======================

    /// Queue (or resume) processing. Progress arrives as
    /// [`events::PROCESSING_PROGRESS`]. One meeting processes at a time; the
    /// rest wait in a queue. A processed meeting is processed again from
    /// scratch (naming is redone by voice matching).
    pub fn process_meeting(&self, id: MeetingId) -> Result<()> {
        self.processing.enqueue(id)
    }

    /// Stop (or unqueue) processing. Completed stages are kept, so
    /// [`Self::process_meeting`] resumes; the meeting goes back to `Recorded`.
    pub fn cancel_processing(&self, id: MeetingId) -> Result<()> {
        self.processing.cancel(id)
    }

    /// The latest progress of a queued or running meeting.
    pub fn processing_progress(&self, id: MeetingId) -> Option<ProcessingProgress> {
        self.processing.progress(id)
    }

    /// Queue every meeting a crash left mid-processing. Call once at startup,
    /// after the manager is registered. Returns how many were queued.
    pub fn resume_interrupted_processing(&self) -> Result<usize> {
        self.processing.resume_interrupted()
    }

    /// The speaker-embedding model's state (it downloads on first use).
    pub fn speaker_model_status(&self) -> crate::meetings::processing::SpeakerModelStatus {
        crate::meetings::processing::speaker_model_status(&self.app)
    }

    /// Start downloading the speaker-embedding model in the background.
    /// Status arrives as [`crate::meetings::processing::SPEAKER_MODEL_STATUS_EVENT`].
    pub fn download_speaker_model(&self) {
        crate::meetings::processing::start_speaker_model_download(&self.app)
    }

    /// Attendees first, then known people (by voice similarity).
    pub fn speaker_suggestions(&self, speaker_id: SpeakerId) -> Result<Vec<NameSuggestion>> {
        let user_email = crate::settings::get_settings(&self.app).meetings.user_email;
        crate::meetings::processing::naming::suggestions(
            &self.store,
            speaker_id,
            user_email.as_deref(),
        )
    }

    /// Name a speaker. Stores that speaker's voiceprints under the person and
    /// marks the speaker confirmed. Returns the person id.
    pub fn assign_speaker(&self, speaker_id: SpeakerId, person: PersonRef) -> Result<PersonId> {
        let key = crate::secrets::meetings_data_key()?;
        let person_id = crate::meetings::processing::naming::assign(
            &self.store,
            &self.meetings_dir,
            &key,
            speaker_id,
            person,
        )?;
        self.emit_speaker_changed(speaker_id);
        Ok(person_id)
    }

    /// Accept an uncertain automatic match ("Confirm?").
    pub fn confirm_speaker(&self, speaker_id: SpeakerId) -> Result<()> {
        let key = crate::secrets::meetings_data_key()?;
        crate::meetings::processing::naming::confirm(
            &self.store,
            &self.meetings_dir,
            &key,
            speaker_id,
        )?;
        self.emit_speaker_changed(speaker_id);
        Ok(())
    }

    /// Forget a deleted meeting's processing state: cancel/unqueue it and
    /// delete its checkpoint and per-segment voiceprint sidecars (which hold
    /// voiceprints of people nobody named). `delete_meeting` should call it.
    ///
    /// Waits (up to 10 s) for a running job to stop first, so it can't write
    /// a sidecar after they are deleted; a job that outlives the wait finds
    /// the meeting gone and removes its own files when it finishes.
    pub fn forget_processing(&self, id: MeetingId) -> Result<()> {
        if !self
            .processing
            .cancel_and_wait(id, std::time::Duration::from_secs(10))?
        {
            log::warn!("meetings: processing of {id} did not stop in time");
        }
        crate::meetings::processing::delete_sidecars(&self.meetings_dir, id)
    }

    fn creating_tickets_for(&self, meeting_id: MeetingId) -> bool {
        self.ticket_creations.lock().contains_key(&meeting_id)
    }

    fn emit_speaker_changed(&self, speaker_id: SpeakerId) {
        if let Ok(Some(s)) = self.store.get_speaker(speaker_id) {
            let _ = self.app.emit(events::MEETING_CHANGED, s.meeting_id);
        }
        let _ = self.app.emit(events::PEOPLE_CHANGED, ());
    }

    // ===== Summaries & action items (owner: llm/E) ==========================

    /// Summarize with the meetings provider/model from settings, or the given
    /// `(provider_id, model)` override. Replaces the stored summary and
    /// action items that have no Linear issue yet.
    ///
    /// Blocking (minutes with a local model): call it off the UI thread.
    /// Progress arrives as [`crate::meetings::summarize::events::SUMMARY_PROGRESS`];
    /// [`Self::cancel_summarize`] stops it.
    pub fn summarize(
        &self,
        id: MeetingId,
        provider_override: Option<(String, String)>,
    ) -> Result<StoredSummary> {
        use crate::meetings::summarize as sum;

        let meeting = self
            .store
            .get_meeting(id)?
            .ok_or_else(|| anyhow::anyhow!("meeting {id} not found"))?;
        if self.creating_tickets_for(id) {
            anyhow::bail!(SUMMARIZE_DURING_CREATE);
        }
        let settings = crate::settings::get_settings(&self.app);
        let provider = crate::llm::provider_for_feature(
            &self.app,
            &settings,
            crate::llm::Feature::Meetings,
            provider_override,
        )?;
        let job = sum::begin_job(id)?;
        let segments = self.store.segments(id)?;
        let speakers = self.store.speakers(id)?;
        let people = self.store.list_people()?;
        let title = summary_title(&meeting).to_string();
        let input = sum::SummarizeInput {
            meeting_id: id,
            meeting_title: Some(title.as_str()),
            segments: &segments,
            speakers: &speakers,
            people: &people,
            user_display_name: &settings.meetings.user_display_name,
        };
        let opts = crate::llm::ChatOptions {
            cancel: Some(job.cancel.clone()),
            ..Default::default()
        };
        let app = self.app.clone();
        let _ = app.emit(
            sum::events::SUMMARY_PROGRESS,
            sum::SummaryProgress {
                meeting_id: id,
                done: 0,
                total: 0,
            },
        );
        log::info!("Summarizing meeting {id} with {}", provider.name());
        let output = sum::summarize_meeting(provider.as_ref(), &input, &opts, &|done, total| {
            let _ = app.emit(
                sum::events::SUMMARY_PROGRESS,
                sum::SummaryProgress {
                    meeting_id: id,
                    done,
                    total,
                },
            );
        })?;
        let stored = {
            // Held while storing: `create_tickets` claims its meetings under
            // this lock, so the two can't interleave.
            let creating = self.ticket_creations.lock();
            if creating.contains_key(&id) {
                anyhow::bail!(SUMMARIZE_DURING_CREATE);
            }
            sum::persist(
                &self.store,
                id,
                &output,
                &provider.name(),
                &self.linear.in_flight_ids(),
            )?
        };
        drop(job);
        self.emit_meeting_changed(id);
        Ok(stored)
    }

    /// Stop a running [`Self::summarize`] (it returns a "cancelled" error and
    /// stores nothing). Returns false when none is running.
    pub fn cancel_summarize(&self, id: MeetingId) -> bool {
        crate::meetings::summarize::cancel_job(id)
    }

    pub fn is_summarizing(&self, id: MeetingId) -> bool {
        crate::meetings::summarize::is_running(id)
    }

    pub fn summary(&self, id: MeetingId) -> Result<Option<StoredSummary>> {
        self.store.summary(id)
    }

    /// Save user edits to a summary (keeps `generated_by`/`generated_at`).
    pub fn update_summary(&self, summary: &StoredSummary) -> Result<()> {
        self.store.put_summary(summary)?;
        self.emit_meeting_changed(summary.meeting_id);
        Ok(())
    }

    pub fn action_items(&self, id: MeetingId) -> Result<Vec<ActionItem>> {
        self.store.action_items(id)
    }

    /// Save the user's edits to an action item: its fields (text, owner,
    /// due, quote, timestamp) always; its ticket draft and state only while
    /// the Linear issue doesn't exist (a stale copy can't unlink or reopen a
    /// created ticket; `Created` itself can't be set here). The Linear link
    /// fields of `item` are ignored. Edited items survive a regenerated
    /// summary.
    pub fn update_action_item(&self, item: &ActionItem) -> Result<()> {
        use crate::meetings::store::TicketEditError;
        let current = self
            .store
            .get_action_item(item.id)?
            .ok_or_else(|| anyhow::anyhow!("action item {} not found", item.id))?;
        self.store.update_action_item(item)?;
        let created =
            current.ticket_state == TicketState::Created || current.linear_issue_id.is_some();
        if !created {
            let frozen = |r: Result<()>| -> Result<bool> {
                match r {
                    Ok(()) => Ok(true),
                    // Created meanwhile: the issue wins, silently.
                    Err(e) if e.downcast_ref::<TicketEditError>().is_some() => Ok(false),
                    Err(e) => Err(e),
                }
            };
            let mut changed = false;
            if item.ticket != current.ticket {
                changed |= frozen(self.store.update_ticket_draft(item.id, &item.ticket))?;
            }
            if item.ticket_state != current.ticket_state
                && item.ticket_state != TicketState::Created
            {
                changed |= frozen(self.store.set_ticket_state(item.id, item.ticket_state))?;
            }
            if changed {
                self.store.mark_action_item_user_edited(item.id)?;
            }
        }
        self.emit_meeting_changed(current.meeting_id);
        Ok(())
    }

    /// Add a manual action item (no source quote; owner unset).
    pub fn add_action_item(&self, meeting_id: MeetingId, text: &str) -> Result<ActionItem> {
        let text = text.trim();
        if text.is_empty() {
            anyhow::bail!("an action item needs some text");
        }
        let item = self.store.insert_manual_action_item(&ActionItem {
            id: 0,
            meeting_id,
            text: text.to_string(),
            owner_person_id: None,
            owner_name: None,
            owner_is_user: false,
            due: None,
            source_quote: String::new(),
            source_t_ms: None,
            ticket: TicketDraft::default(),
            ticket_state: TicketState::Draft,
            linear_issue_id: None,
            linear_issue_identifier: None,
            linear_issue_url: None,
        })?;
        self.emit_meeting_changed(meeting_id);
        Ok(item)
    }

    /// Delete an action item (an explicit user action; regenerating the
    /// summary never deletes items that have a Linear issue).
    pub fn delete_action_item(&self, id: ActionItemId) -> Result<()> {
        let meeting_id = self.store.get_action_item(id)?.map(|i| i.meeting_id);
        self.store.delete_action_item(id)?;
        if let Some(m) = meeting_id {
            self.emit_meeting_changed(m);
        }
        Ok(())
    }

    // ===== Linear (owner: linear/F) =========================================

    /// Store the personal API key in the Keychain. An empty key deletes it.
    pub fn set_linear_api_key(&self, key: &str) -> Result<()> {
        self.linear.set_api_key(key)
    }

    pub fn has_linear_api_key(&self) -> bool {
        self.linear.has_api_key()
    }

    /// Returns the authenticated user's name.
    pub fn test_linear(&self) -> Result<String> {
        Ok(crate::host::async_runtime::block_on(self.linear.test())?.name)
    }

    pub fn linear_teams(&self) -> Result<Vec<crate::linear::Team>> {
        crate::host::async_runtime::block_on(self.linear.teams())
    }

    pub fn linear_projects(&self, team_id: &str) -> Result<Vec<crate::linear::Project>> {
        crate::host::async_runtime::block_on(self.linear.projects(team_id))
    }

    pub fn linear_users(&self) -> Result<Vec<crate::linear::User>> {
        crate::host::async_runtime::block_on(self.linear.users())
    }

    pub fn linear_labels(&self, team_id: &str) -> Result<Vec<crate::linear::Label>> {
        crate::host::async_runtime::block_on(self.linear.labels(team_id))
    }

    /// Fill each action item's `ticket` draft (title, description with quote
    /// and meeting link, remembered team/project, assignee matched by email,
    /// else the user) where it is still empty. Returns the updated items.
    pub fn draft_tickets(&self, meeting_id: MeetingId) -> Result<Vec<ActionItem>> {
        let m = crate::settings::get_settings(&self.app).meetings;
        let defaults = crate::linear::service::LinearDefaults {
            team_id: m.linear_default_team_id,
            project_id: m.linear_default_project_id,
        };
        let items = crate::host::async_runtime::block_on(self.linear.draft_tickets(
            self.store.as_ref(),
            meeting_id,
            &defaults,
        ))?;
        self.emit_meeting_changed(meeting_id);
        Ok(items)
    }

    /// Create Linear issues for the given approved items. Items that already
    /// have an issue are skipped (`already_existed`), so retries never
    /// duplicate. The team/project used become the remembered defaults.
    ///
    /// While it runs, [`Self::summarize`] refuses to run for the meetings
    /// involved.
    pub fn create_tickets(&self, ids: &[ActionItemId]) -> Result<Vec<TicketResult>> {
        let mut meetings: Vec<MeetingId> = Vec::new();
        for id in ids {
            if let Some(item) = self.store.get_action_item(*id)? {
                if !meetings.contains(&item.meeting_id) {
                    meetings.push(item.meeting_id);
                }
            }
        }
        let outcome = {
            let _claim = TicketCreationClaim::new(&self.ticket_creations, &meetings);
            crate::host::async_runtime::block_on(
                self.linear.create_tickets(self.store.as_ref(), ids),
            )
        };
        for m in &meetings {
            self.emit_meeting_changed(*m);
        }
        let outcome = outcome?;
        if let Some((team, project)) = outcome.used_team_project {
            let mut settings = crate::settings::get_settings(&self.app);
            let m = &mut settings.meetings;
            if m.linear_default_team_id.as_deref() != Some(team.as_str())
                || m.linear_default_project_id != project
            {
                m.linear_default_team_id = Some(team);
                m.linear_default_project_id = project;
                crate::settings::write_settings(&self.app, settings);
            }
        }
        Ok(outcome.results)
    }
}

/// The title the summarizer sees: the meeting's current title (the user may
/// have renamed it), never the linked calendar event's snapshot.
fn summary_title(meeting: &Meeting) -> &str {
    &meeting.title
}

const SUMMARIZE_DURING_CREATE: &str =
    "Linear tickets are being created for this meeting. Summarize again when that finishes.";

/// Marks meetings as having a `create_tickets` call running, until dropped.
struct TicketCreationClaim<'a> {
    map: &'a parking_lot::Mutex<std::collections::HashMap<MeetingId, usize>>,
    meetings: Vec<MeetingId>,
}

impl<'a> TicketCreationClaim<'a> {
    fn new(
        map: &'a parking_lot::Mutex<std::collections::HashMap<MeetingId, usize>>,
        meetings: &[MeetingId],
    ) -> Self {
        let mut m = map.lock();
        for id in meetings {
            *m.entry(*id).or_default() += 1;
        }
        Self {
            map,
            meetings: meetings.to_vec(),
        }
    }
}

impl Drop for TicketCreationClaim<'_> {
    fn drop(&mut self) {
        let mut m = self.map.lock();
        for id in &self.meetings {
            if let Some(n) = m.get_mut(id) {
                *n -= 1;
                if *n == 0 {
                    m.remove(id);
                }
            }
        }
    }
}

/// Remove one meeting audio file. `rel` must stay inside `audio_dir` (no
/// absolute paths or `..`), so a corrupt row can't delete anything else.
fn delete_audio_file(audio_dir: &std::path::Path, rel: &str) {
    use std::path::Component;
    let rel_path = std::path::Path::new(rel);
    if rel.is_empty()
        || !rel_path
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
    {
        log::warn!("Not deleting meeting audio with unexpected path {rel:?}");
        return;
    }
    let path = audio_dir.join(rel_path);
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log::warn!("Failed to delete meeting audio {}: {e}", path.display()),
    }
}

#[cfg(test)]
mod store_section_tests {
    use super::*;
    use crate::host::testing::TestApp;

    fn meeting(mic: Option<&str>) -> Meeting {
        Meeting {
            id: 0,
            title: "Sync".into(),
            calendar_event_id: None,
            calendar_event: None,
            started_at: 1,
            ended_at: Some(2),
            mic_audio_path: mic.map(Into::into),
            system_audio_path: None,
            status: MeetingStatus::Processed,
            error: None,
            processing_stage: None,
        }
    }

    #[test]
    fn delete_meeting_removes_rows_and_audio_and_emits() {
        let t = TestApp::new();
        let mgr = MeetingsManager::new(&t.app).unwrap();
        std::fs::create_dir_all(mgr.audio_dir()).unwrap();
        let audio = mgr.audio_dir().join("1-mic.opus.enc");
        std::fs::write(&audio, b"x").unwrap();
        let outside = mgr.meetings_dir().join("keep.txt");
        std::fs::write(&outside, b"x").unwrap();

        let id = mgr
            .store()
            .insert_meeting(&meeting(Some("1-mic.opus.enc")))
            .unwrap();
        let evil = mgr
            .store()
            .insert_meeting(&meeting(Some("../keep.txt")))
            .unwrap();
        mgr.rename_meeting(id, "  New name ").unwrap();
        assert_eq!(mgr.get_meeting(id).unwrap().unwrap().title, "New name");

        mgr.delete_meeting(id).unwrap();
        mgr.delete_meeting(evil).unwrap();
        assert!(mgr.get_meeting(id).unwrap().is_none());
        assert!(!audio.exists());
        assert!(outside.exists(), "path traversal must not delete files");
        // Deleting a missing meeting is a no-op.
        mgr.delete_meeting(id).unwrap();

        let seen = t.events.lock();
        assert!(seen.iter().any(|e| e.name == events::MEETING_CHANGED));
    }

    #[test]
    fn people_methods_go_through_the_store() {
        let t = TestApp::new();
        let mgr = MeetingsManager::new(&t.app).unwrap();
        let a = mgr.store().insert_person("Ann", None).unwrap();
        let b = mgr.store().insert_person("Annie", Some("a@x.io")).unwrap();
        mgr.rename_person(a.id, "Ann Lee", Some(" ")).unwrap();
        mgr.merge_people(b.id, a.id).unwrap();
        let people = mgr.list_people().unwrap();
        assert_eq!(people.len(), 1);
        assert_eq!(people[0].display_name, "Ann Lee");
        assert_eq!(people[0].email.as_deref(), Some("a@x.io"));
        mgr.delete_person(a.id).unwrap();
        assert!(mgr.list_people().unwrap().is_empty());
        assert!(t
            .events
            .lock()
            .iter()
            .any(|e| e.name == events::PEOPLE_CHANGED));
    }

    fn item_in(mgr: &MeetingsManager, meeting_id: MeetingId, text: &str) -> ActionItem {
        mgr.store()
            .insert_action_item(&ActionItem {
                id: 0,
                meeting_id,
                text: text.into(),
                owner_person_id: None,
                owner_name: None,
                owner_is_user: true,
                due: None,
                source_quote: "I'll do it".into(),
                source_t_ms: Some(0),
                ticket: TicketDraft {
                    title: text.into(),
                    team_id: Some("t".into()),
                    ..Default::default()
                },
                ticket_state: TicketState::Draft,
                linear_issue_id: None,
                linear_issue_identifier: None,
                linear_issue_url: None,
            })
            .unwrap()
    }

    fn changed_events(t: &TestApp) -> usize {
        t.events
            .lock()
            .iter()
            .filter(|e| e.name == events::MEETING_CHANGED)
            .count()
    }

    #[test]
    fn update_action_item_applies_ticket_only_until_created() {
        let t = TestApp::new();
        let mgr = MeetingsManager::new(&t.app).unwrap();
        let m = mgr.store().insert_meeting(&meeting(None)).unwrap();
        let a = item_in(&mgr, m, "first");

        // Before creation: fields, ticket and state are all applied.
        let before = changed_events(&t);
        let mut e = a.clone();
        e.text = "first, edited".into();
        e.ticket.title = "Better title".into();
        e.ticket_state = TicketState::Approved;
        mgr.update_action_item(&e).unwrap();
        let got = mgr.store().get_action_item(a.id).unwrap().unwrap();
        assert_eq!(got.text, "first, edited");
        assert_eq!(got.ticket.title, "Better title");
        assert_eq!(got.ticket_state, TicketState::Approved);
        assert_eq!(
            mgr.store().action_item_flags(a.id).unwrap(),
            Some((false, true))
        );
        assert!(changed_events(&t) > before);

        // Created meanwhile; the UI saves its stale copy.
        mgr.store()
            .set_linear_issue(a.id, "iss", "ENG-1", "u")
            .unwrap();
        let mut stale = e.clone();
        stale.text = "after".into();
        stale.ticket.title = "Stale".into();
        stale.ticket_state = TicketState::Draft;
        stale.linear_issue_id = None;
        mgr.update_action_item(&stale).unwrap();
        let got = mgr.store().get_action_item(a.id).unwrap().unwrap();
        assert_eq!(got.text, "after");
        assert_eq!(got.ticket.title, "Better title");
        assert_eq!(got.ticket_state, TicketState::Created);
        assert_eq!(got.linear_issue_id.as_deref(), Some("iss"));
    }

    #[test]
    fn manual_items_survive_a_regenerated_summary_and_mutations_emit() {
        let t = TestApp::new();
        let mgr = MeetingsManager::new(&t.app).unwrap();
        let m = mgr.store().insert_meeting(&meeting(None)).unwrap();
        let before = changed_events(&t);
        let manual = mgr.add_action_item(m, "Book the room").unwrap();
        assert_eq!(changed_events(&t), before + 1);
        assert_eq!(
            mgr.store().action_item_flags(manual.id).unwrap(),
            Some((true, false))
        );
        let generated = item_in(&mgr, m, "generated");
        let out = crate::meetings::summarize::SummarizeOutput {
            summary: Default::default(),
            items: vec![],
        };
        crate::meetings::summarize::persist(mgr.store(), m, &out, "x/y", &[]).unwrap();
        let left: Vec<_> = mgr
            .action_items(m)
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect();
        assert_eq!(left, vec![manual.id]);
        assert!(mgr.store().get_action_item(generated.id).unwrap().is_none());

        // draft_tickets (no Linear key: drafts from defaults) emits too.
        let before = changed_events(&t);
        mgr.draft_tickets(m).unwrap();
        assert_eq!(changed_events(&t), before + 1);
        mgr.delete_action_item(manual.id).unwrap();
        assert_eq!(changed_events(&t), before + 2);
    }

    #[test]
    fn summarize_is_refused_while_tickets_are_created() {
        let t = TestApp::new();
        let mgr = MeetingsManager::new(&t.app).unwrap();
        let m = mgr.store().insert_meeting(&meeting(None)).unwrap();
        let claim = TicketCreationClaim::new(&mgr.ticket_creations, &[m, m]);
        let e = mgr.summarize(m, None).unwrap_err();
        assert!(e.to_string().contains("being created"), "{e}");
        drop(claim);
        assert!(!mgr.creating_tickets_for(m));
    }

    #[test]
    fn summaries_use_the_current_meeting_title() {
        let mut m = meeting(None);
        m.title = "Renamed by the user".into();
        m.calendar_event = Some(CalendarEvent {
            id: "e".into(),
            title: "Old calendar title".into(),
            start_at: 0,
            end_at: 1,
            meeting_url: None,
            attendees: vec![],
        });
        assert_eq!(summary_title(&m), "Renamed by the user");
    }

    #[test]
    fn deleting_a_person_drops_their_sidecar_voiceprints() {
        use crate::meetings::processing::sidecar::{self, SegmentEmbedding};
        let t = TestApp::new();
        let mgr = MeetingsManager::new(&t.app).unwrap();
        let m = mgr.store().insert_meeting(&meeting(None)).unwrap();
        let ann = mgr.store().insert_person("Ann", None).unwrap();
        let sp = |id, person| MeetingSpeaker {
            id,
            meeting_id: m,
            cluster_label: format!("Speaker {id}"),
            person_id: person,
            match_confidence: None,
            confirmed: true,
            is_user: false,
        };
        let seg = |t0, speaker| Segment {
            id: 0,
            meeting_id: m,
            track: Track::System,
            t0_ms: t0,
            t1_ms: t0 + 1000,
            text: "hi".into(),
            words: vec![],
            speaker_id: Some(speaker),
        };
        let (speakers, segs) = mgr
            .store()
            .replace_transcript(
                m,
                &[sp(1, Some(ann.id)), sp(2, None)],
                &[seg(0, 1), seg(2000, 2)],
            )
            .unwrap();
        mgr.store()
            .set_speaker_embedding(speakers[0].id, Some(&[1.0]))
            .unwrap();
        let key = crate::secrets::meetings_data_key().unwrap();
        let e = |id| SegmentEmbedding {
            segment_id: id,
            duration_ms: 1000,
            embedding: vec![1.0],
        };
        sidecar::write_embeddings(mgr.meetings_dir(), m, &key, &[e(segs[0].id), e(segs[1].id)])
            .unwrap();

        mgr.delete_person(ann.id).unwrap();
        let left = sidecar::read_embeddings(mgr.meetings_dir(), m, &key)
            .unwrap()
            .unwrap();
        assert_eq!(left, vec![e(segs[1].id)]);
        assert_eq!(mgr.store().speaker_embedding(speakers[0].id).unwrap(), None);
    }
}
